use mikrom_proto::scheduler::{
    AppConfig, DatabaseStatusRequest, DeleteDatabaseRequest, DeployDatabaseRequest, HypervisorType,
    WorkloadType,
};
use mikrom_proto::subjects;
use prost::Message;
use std::collections::HashMap;

#[test]
fn shared_subjects_include_database_topics() {
    assert_eq!(
        subjects::SCHEDULER_DEPLOY_DATABASE,
        "mikrom.scheduler.database.deploy"
    );
    assert_eq!(
        subjects::SCHEDULER_LIST_DATABASES,
        "mikrom.scheduler.database.list"
    );
    assert_eq!(
        subjects::SCHEDULER_GET_DATABASE_STATUS,
        "mikrom.scheduler.database.status"
    );
    assert_eq!(
        subjects::SCHEDULER_DELETE_DATABASE,
        "mikrom.scheduler.database.delete"
    );
}

#[test]
fn deploy_database_request_roundtrip_keeps_workload_type() {
    let req = DeployDatabaseRequest {
        database_id: "db-1".to_string(),
        database_name: "orders".to_string(),
        rootfs_image: "local:/opt/neon".to_string(),
        tenant_id: "tenant-1".to_string(),
        deployment_id: "dep-1".to_string(),
        vpc_ipv6_prefix: "fd00:abcd::".to_string(),
        config: Some(AppConfig {
            vcpus: 2,
            memory_mib: 1024,
            disk_mib: 4096,
            port: 5432,
            env: HashMap::from([("max_connections".to_string(), "200".to_string())]),
            volumes: vec![],
            health_check_path: "/".to_string(),
            ipv6_address: "fd00:abcd::1".to_string(),
            ipv6_gateway: "fe80::1".to_string(),
            hypervisor: HypervisorType::HypertypeCloudHypervisor as i32,
            workload_type: WorkloadType::Database as i32,
        }),
    };

    let mut bytes = Vec::new();
    req.encode(&mut bytes).unwrap();
    let decoded = DeployDatabaseRequest::decode(&bytes[..]).unwrap();
    assert_eq!(decoded.database_id, "db-1");
    assert_eq!(
        decoded.config.unwrap().workload_type,
        WorkloadType::Database as i32
    );
}

#[test]
fn database_status_and_delete_requests_roundtrip() {
    let status = DatabaseStatusRequest {
        job_id: "job-1".to_string(),
        tenant_id: "tenant-1".to_string(),
    };
    let delete = DeleteDatabaseRequest {
        job_id: "job-1".to_string(),
        tenant_id: "tenant-1".to_string(),
    };

    let status_bytes = status.encode_to_vec();
    let delete_bytes = delete.encode_to_vec();

    let decoded_status = DatabaseStatusRequest::decode(&status_bytes[..]).unwrap();
    let decoded_delete = DeleteDatabaseRequest::decode(&delete_bytes[..]).unwrap();

    assert_eq!(decoded_status.job_id, "job-1");
    assert_eq!(decoded_status.tenant_id, "tenant-1");
    assert_eq!(decoded_delete.job_id, "job-1");
    assert_eq!(decoded_delete.tenant_id, "tenant-1");
}
