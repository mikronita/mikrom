use mikrom_proto::subjects;

#[test]
fn test_vm_logs_subject() {
    let vm_id = "test-vm-123";
    let subject = subjects::vm_logs(vm_id);
    assert_eq!(subject, "mikrom.logs.test-vm-123");
}

#[test]
fn test_builder_status_subject() {
    let build_id = "build-abc";
    let subject = subjects::builder_status(build_id);
    assert_eq!(subject, "mikrom.builder.build-abc.status");
}

#[test]
fn test_constants_not_empty() {
    assert!(!subjects::ROUTER_CONFIG_UPDATED.is_empty());
    assert!(!subjects::SCHEDULER_JOB_UPDATES.is_empty());
}
