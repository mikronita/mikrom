#[cfg(test)]
mod tests {
    use mikrom_proto::scheduler::{
        DeployRequest, RegisterWorkerRequest, ReportMetricsRequest,
        scheduler_service_server::SchedulerService,
    };
    use mikrom_scheduler::server::SchedulerServer;
    use std::collections::HashMap;
    use tonic::Request;

    #[tokio::test]
    async fn test_deploy_selects_only_one_worker() {
        let server = SchedulerServer::new(None).unwrap();

        // Register Worker 1
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "host-1".to_string(),
                hostname: "node-1".to_string(),
                ip_address: "10.0.0.1".to_string(),
                agent_port: 5003,
                bridge_ip: "10.0.0.1/8".to_string(),
            }))
            .await
            .unwrap();

        // Register Worker 2
        server
            .register_worker(Request::new(RegisterWorkerRequest {
                host_id: "host-2".to_string(),
                hostname: "node-2".to_string(),
                ip_address: "10.0.0.2".to_string(),
                agent_port: 5003,
                bridge_ip: "10.0.0.1/8".to_string(),
            }))
            .await
            .unwrap();

        // Send metrics for both so they are "available"
        let metrics = |id: &str| {
            Request::new(ReportMetricsRequest {
                host_id: id.to_string(),
                cpu_usage: 0.1,
                ram_used_bytes: 0,
                ram_total_bytes: 8 * 1024 * 1024 * 1024,
                disk_used_bytes: 0,
                disk_total_bytes: 100 * 1024 * 1024 * 1024,
                apps_count: 0,
                timestamp: 0,
                load_avg_1: 0.0,
                load_avg_5: 0.0,
                load_avg_15: 0.0,
                vms: HashMap::new(),
            })
        };

        server.report_metrics(metrics("host-1")).await.unwrap();
        server.report_metrics(metrics("host-2")).await.unwrap();

        // Now deploy
        let deploy_req = DeployRequest {
            app_id: "test-app".to_string(),
            app_name: "test-app".to_string(),
            image: "nginx".to_string(),
            config: None,
            user_id: "user-1".to_string(),
        };

        // Note: forward_deploy_to_agent will fail in tests because there is no real agent
        // but we want to see if it even gets there and what host it picks.
        let resp = server
            .deploy_app(Request::new(deploy_req))
            .await
            .unwrap()
            .into_inner();

        println!("Deployed to host: {}", resp.host_id);
        assert!(!resp.host_id.is_empty());

        // Check job state in scheduler
        let job = server.scheduler().get_job(&resp.job_id).unwrap();
        assert!(job.host_id.is_some());
    }
}
