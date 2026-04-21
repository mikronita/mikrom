use mikrom_proto::scheduler::ListWorkersRequest;
use mikrom_proto::scheduler::scheduler_service_client::SchedulerServiceClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = SchedulerServiceClient::connect("http://127.0.0.1:5002").await?;
    let response = client.list_workers(ListWorkersRequest {}).await?;

    for worker in response.into_inner().workers {
        println!(
            "Worker: {} | Hostname: {} | IP: {} | Port: {} | Bridge: {}",
            worker.host_id, worker.hostname, worker.ip_address, worker.agent_port, worker.bridge_ip
        );
    }

    Ok(())
}
