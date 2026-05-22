use mikrom_agent::hypervisor::VmHypervisor;
use mikrom_proto::id::{AppId, VmId};
use std::collections::HashMap;
use std::sync::Arc;

fn qemu_config() -> mikrom_agent::qemu::QemuConfig {
    mikrom_agent::qemu::QemuConfig {
        binary: "/bin/sleep".into(),
        kernel_path: "/dev/null".into(),
        rootfs_path: "/dev/null".into(),
        base_rootfs_path: "/dev/null".into(),
        data_dir: std::env::temp_dir().join(format!("integ-test-qemu-{}", std::process::id())),
        qmp_timeout_secs: 1,
        extra_args: vec!["3600".into()],
        kernel_url: None,
        rootfs_url: None,
        image_cache_dir: std::env::temp_dir().join("integ-test-cache"),
        virtiofsd_binary: String::new(),
        virtiofsd_socket_dir: std::env::temp_dir().join("integ-test-virtiofsd"),
        virtiofsd_shares: Vec::new(),
    }
}

fn fc_config() -> mikrom_agent::firecracker::FirecrackerConfig {
    mikrom_agent::firecracker::FirecrackerConfig::stub()
}

fn default_vm_config() -> mikrom_agent::hypervisor::types::VmConfig {
    mikrom_agent::hypervisor::types::VmConfig::default()
}

fn new_vm_id() -> VmId {
    VmId::new()
}

fn new_app_id() -> AppId {
    AppId::new()
}

#[tokio::test]
async fn test_both_hypervisors_can_run_vms() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("integ-agent".into(), qemu_config()).await;

    let vm_fc = new_vm_id();
    let vm_qemu = new_vm_id();
    let app_id = new_app_id();

    fc.start_vm(vm_fc, app_id, "img".into(), default_vm_config())
        .await
        .expect("Firecracker start_vm should succeed");

    qemu.start_vm(vm_qemu, app_id, "img".into(), default_vm_config())
        .await
        .expect("QEMU start_vm should succeed");

    let fc_info = fc.get_vm_info(&vm_fc).await;
    assert!(fc_info.is_some(), "FC VM should exist");
    assert_eq!(
        fc_info.unwrap().status,
        mikrom_agent::hypervisor::types::VmStatus::Running
    );

    let qemu_info = qemu.get_vm_info(&vm_qemu).await;
    assert!(qemu_info.is_some(), "QEMU VM should exist");
    assert_eq!(
        qemu_info.unwrap().status,
        mikrom_agent::hypervisor::types::VmStatus::Running
    );

    // Each VM only visible on its own hypervisor
    assert!(
        fc.get_vm_info(&vm_qemu).await.is_none(),
        "FC should not see QEMU VM"
    );
    assert!(
        qemu.get_vm_info(&vm_fc).await.is_none(),
        "QEMU should not see FC VM"
    );

    fc.stop_vm(&vm_fc).await.expect("FC stop should succeed");
    qemu.stop_vm(&vm_qemu)
        .await
        .expect("QEMU stop should succeed");
}

#[tokio::test]
async fn test_stop_on_one_does_not_affect_other() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("integ-agent".into(), qemu_config()).await;

    let vm_fc = new_vm_id();
    let vm_qemu = new_vm_id();
    let app_id = new_app_id();

    fc.start_vm(vm_fc, app_id, "img".into(), default_vm_config())
        .await
        .expect("FC start");
    qemu.start_vm(vm_qemu, app_id, "img".into(), default_vm_config())
        .await
        .expect("QEMU start");

    fc.stop_vm(&vm_fc).await.expect("FC stop");
    assert_eq!(
        fc.get_vm_info(&vm_fc).await.unwrap().status,
        mikrom_agent::hypervisor::types::VmStatus::Stopped
    );
    assert_eq!(
        qemu.get_vm_info(&vm_qemu).await.unwrap().status,
        mikrom_agent::hypervisor::types::VmStatus::Running,
        "QEMU VM should still be running"
    );

    qemu.stop_vm(&vm_qemu).await.ok();
}

#[tokio::test]
async fn test_each_hypervisor_lists_only_its_own_vms() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("integ-agent".into(), qemu_config()).await;

    let app_id = new_app_id();
    let fc_vm1 = new_vm_id();
    let fc_vm2 = new_vm_id();
    let qemu_vm = new_vm_id();

    fc.start_vm(fc_vm1, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();
    fc.start_vm(fc_vm2, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();
    qemu.start_vm(qemu_vm, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();

    let fc_vms = fc.get_all_vms().await;
    let qemu_vms = qemu.get_all_vms().await;

    assert_eq!(fc_vms.len(), 2);
    assert_eq!(qemu_vms.len(), 1);

    let fc_ids: Vec<_> = fc_vms.iter().map(|v| v.vm_id).collect();
    assert!(fc_ids.contains(&fc_vm1));
    assert!(fc_ids.contains(&fc_vm2));
    assert!(!fc_ids.contains(&qemu_vm));

    fc.stop_vm(&fc_vm1).await.ok();
    fc.stop_vm(&fc_vm2).await.ok();
    qemu.stop_vm(&qemu_vm).await.ok();
}

#[tokio::test]
async fn test_hypervisor_types_are_distinct() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("integ-agent".into(), qemu_config()).await;

    assert_eq!(
        fc.hypervisor_type(),
        mikrom_agent::hypervisor::HypervisorType::Firecracker
    );
    assert_eq!(
        qemu.hypervisor_type(),
        mikrom_agent::hypervisor::HypervisorType::QemuMicrovm
    );
}

#[tokio::test]
async fn test_combined_get_all_vms_from_map() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("integ-agent".into(), qemu_config()).await;

    let mut hvs: HashMap<_, Arc<dyn mikrom_agent::hypervisor::VmHypervisor>> = HashMap::new();
    hvs.insert(
        mikrom_agent::hypervisor::HypervisorType::Firecracker,
        Arc::new(fc),
    );
    let qemu_arc = Arc::new(qemu);
    hvs.insert(
        mikrom_agent::hypervisor::HypervisorType::QemuMicrovm,
        qemu_arc.clone(),
    );

    let app_id = new_app_id();
    let fc_vm = new_vm_id();
    let qemu_vm = new_vm_id();

    hvs[&mikrom_agent::hypervisor::HypervisorType::Firecracker]
        .start_vm(fc_vm, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();
    qemu_arc
        .start_vm(qemu_vm, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();

    // Collect VMs from all hypervisors
    let mut all_vms = Vec::new();
    for hv in hvs.values() {
        all_vms.extend(hv.get_all_vms().await);
    }
    let ids: Vec<_> = all_vms.iter().map(|v| v.vm_id).collect();
    assert!(ids.contains(&fc_vm), "FC VM should be in combined list");
    assert!(ids.contains(&qemu_vm), "QEMU VM should be in combined list");

    hvs[&mikrom_agent::hypervisor::HypervisorType::Firecracker]
        .stop_vm(&fc_vm)
        .await
        .ok();
    qemu_arc.stop_vm(&qemu_vm).await.ok();
}
