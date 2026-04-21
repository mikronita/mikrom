use mikrom_agent::server::AgentServer;
use mikrom_proto::scheduler::{
    AppStatusRequest, AppStatusResponse, CancelRequest, CancelResponse, DeleteAppRequest,
    DeleteAppResponse, DeployRequest, DeployResponse, GetLogsRequest,
    GetLogsResponse as ProtoGetLogsResponse, ListAppsRequest, ListAppsResponse, PauseRequest,
    PauseResponse, RegisterWorkerRequest, RegisterWorkerResponse, ReportMetricsRequest,
    ReportMetricsResponse, ResumeRequest, ResumeResponse,
    scheduler_service_server::{SchedulerService, SchedulerServiceServer},
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, transport::Server};

struct MockScheduler {
    tx: mpsc::Sender<String>,
    // Usamos un contador atómico para decidir cuándo fallar
    heartbeat_count: Arc<std::sync::atomic::AtomicUsize>,
}

#[tonic::async_trait]
impl SchedulerService for MockScheduler {
    async fn register_worker(
        &self,
        _request: Request<RegisterWorkerRequest>,
    ) -> Result<Response<RegisterWorkerResponse>, Status> {
        let _ = self.tx.send("register".to_string()).await;
        Ok(Response::new(RegisterWorkerResponse {
            success: true,
            message: "OK".to_string(),
        }))
    }

    async fn report_metrics(
        &self,
        _request: Request<ReportMetricsRequest>,
    ) -> Result<Response<ReportMetricsResponse>, Status> {
        let count = self
            .heartbeat_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(format!("metrics_{}", count)).await;

        // El segundo heartbeat fallará (success = false) para forzar re-registro
        if count == 1 {
            Ok(Response::new(ReportMetricsResponse { success: false }))
        } else {
            Ok(Response::new(ReportMetricsResponse { success: true }))
        }
    }

    // Métodos no usados en este test
    async fn deploy_app(
        &self,
        _: Request<DeployRequest>,
    ) -> Result<Response<DeployResponse>, Status> {
        unimplemented!()
    }
    async fn list_apps(
        &self,
        _: Request<ListAppsRequest>,
    ) -> Result<Response<ListAppsResponse>, Status> {
        unimplemented!()
    }

    async fn list_workers(
        &self,
        _: Request<mikrom_proto::scheduler::ListWorkersRequest>,
    ) -> Result<Response<mikrom_proto::scheduler::ListWorkersResponse>, Status> {
        unimplemented!()
    }
    async fn get_app_status(
        &self,
        _: Request<AppStatusRequest>,
    ) -> Result<Response<AppStatusResponse>, Status> {
        unimplemented!()
    }
    async fn delete_app(
        &self,
        _: Request<DeleteAppRequest>,
    ) -> Result<Response<DeleteAppResponse>, Status> {
        unimplemented!()
    }
    async fn cancel_app(
        &self,
        _: Request<CancelRequest>,
    ) -> Result<Response<CancelResponse>, Status> {
        unimplemented!()
    }
    async fn pause_app(&self, _: Request<PauseRequest>) -> Result<Response<PauseResponse>, Status> {
        unimplemented!()
    }
    async fn resume_app(
        &self,
        _: Request<ResumeRequest>,
    ) -> Result<Response<ResumeResponse>, Status> {
        unimplemented!()
    }

    type GetAppLogsStream =
        tokio_stream::wrappers::ReceiverStream<Result<ProtoGetLogsResponse, Status>>;
    async fn get_app_logs(
        &self,
        _: Request<GetLogsRequest>,
    ) -> Result<Response<Self::GetAppLogsStream>, Status> {
        unimplemented!()
    }
}

#[tokio::test]
async fn test_agent_re_registers_when_scheduler_rejects_metrics() {
    let (tx, mut rx) = mpsc::channel(100);
    let heartbeat_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    // 1. Iniciar Mock Scheduler
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let listener = std::net::TcpListener::bind(addr).unwrap();
    let actual_addr = listener.local_addr().unwrap();
    drop(listener);

    let mock_scheduler = MockScheduler {
        tx,
        heartbeat_count,
    };
    tokio::spawn(async move {
        Server::builder()
            .add_service(SchedulerServiceServer::new(mock_scheduler))
            .serve(actual_addr)
            .await
            .unwrap();
    });

    // 2. Iniciar Agente
    let agent_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let agent = AgentServer::with_scheduler_addr(
        "test-host".to_string(),
        "test-node".to_string(),
        "127.0.0.1".to_string(),
        "10.0.0.1/8".to_string(),
        format!("http://{}", actual_addr),
    );

    // Ejecutamos el agente en background
    let agent_handle = tokio::spawn(async move {
        agent.serve(agent_addr, false).await.unwrap();
    });

    // 3. Verificar secuencia de eventos
    // Esperamos primer registro
    assert_eq!(
        rx.recv().await.expect("Should receive register"),
        "register"
    );

    // Esperamos primer heartbeat (exitoso)
    assert_eq!(
        rx.recv().await.expect("Should receive metrics_0"),
        "metrics_0"
    );

    // Esperamos segundo heartbeat (fallido, devolverá success=false)
    assert_eq!(
        rx.recv().await.expect("Should receive metrics_1"),
        "metrics_1"
    );

    // ¡Aquí está la magia! El agente debería re-registrarse
    // Ponemos un timeout razonable porque el bucle de métricas espera 5s
    let next_event = tokio::time::timeout(tokio::time::Duration::from_secs(10), rx.recv()).await;
    assert_eq!(
        next_event
            .expect("Timeout waiting for re-registration")
            .expect("Should receive register"),
        "register"
    );

    // Y debería volver a enviar métricas
    assert_eq!(
        rx.recv().await.expect("Should receive metrics_2"),
        "metrics_2"
    );

    agent_handle.abort();
}
