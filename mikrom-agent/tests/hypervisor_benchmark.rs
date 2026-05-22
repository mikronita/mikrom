use mikrom_agent::hypervisor::VmHypervisor;
use mikrom_proto::id::{AppId, VmId};
use std::time::Instant;

fn qemu_config() -> mikrom_agent::qemu::QemuConfig {
    mikrom_agent::qemu::QemuConfig {
        binary: "/bin/sleep".into(),
        kernel_path: "/dev/null".into(),
        rootfs_path: "/dev/null".into(),
        base_rootfs_path: "/dev/null".into(),
        data_dir: std::env::temp_dir().join(format!("bench-qemu-{}", std::process::id())),
        qmp_timeout_secs: 1,
        extra_args: vec!["3600".into()],
        kernel_url: None,
        rootfs_url: None,
        image_cache_dir: std::env::temp_dir().join("bench-cache"),
        virtiofsd_binary: String::new(),
        virtiofsd_socket_dir: std::env::temp_dir().join("bench-virtiofsd"),
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
async fn benchmark_start_stop() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("bench-agent".into(), qemu_config()).await;

    let n = 5;

    // --- Firecracker start ---
    let fc_start = Instant::now();
    let fc_vms: Vec<VmId> = (0..n).map(|_| new_vm_id()).collect();
    for &vm in &fc_vms {
        fc.start_vm(vm, new_app_id(), "img".into(), default_vm_config())
            .await
            .expect("FC start_vm");
    }
    let fc_start_elapsed = fc_start.elapsed();

    // --- QEMU start ---
    let qemu_start = Instant::now();
    let qemu_vms: Vec<VmId> = (0..n).map(|_| new_vm_id()).collect();
    for &vm in &qemu_vms {
        qemu.start_vm(vm, new_app_id(), "img".into(), default_vm_config())
            .await
            .expect("QEMU start_vm");
    }
    let qemu_start_elapsed = qemu_start.elapsed();

    // --- Firecracker stop ---
    let fc_stop = Instant::now();
    for &vm in &fc_vms {
        fc.stop_vm(&vm).await.expect("FC stop_vm");
    }
    let fc_stop_elapsed = fc_stop.elapsed();

    // --- QEMU stop ---
    let qemu_stop = Instant::now();
    for &vm in &qemu_vms {
        qemu.stop_vm(&vm).await.expect("QEMU stop_vm");
    }
    let qemu_stop_elapsed = qemu_stop.elapsed();

    let fc_start_avg = fc_start_elapsed / n;
    let qemu_start_avg = qemu_start_elapsed / n;

    println!();
    println!("=== Hypervisor benchmark ({n} iterations) ===");
    println!();
    println!("  Start:");
    println!("    Firecracker  total={fc_start_elapsed:?}  avg={fc_start_avg:?}/vm");
    println!("    QEMU         total={qemu_start_elapsed:?}  avg={qemu_start_avg:?}/vm");
    println!();
    println!("  Stop:");
    println!("    Firecracker  total={fc_stop_elapsed:?}");
    println!("    QEMU         total={qemu_stop_elapsed:?}");
    println!();
    println!(
        "  Ratio (QEMU/FC start avg): {:.2}x",
        qemu_start_avg.as_secs_f64() / fc_start_avg.as_secs_f64().max(1e-9)
    );
}

#[tokio::test]
async fn benchmark_get_all_vms() {
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("bench-agent".into(), qemu_config()).await;

    let n = 10;
    let app_id = new_app_id();

    let fc_vms: Vec<VmId> = (0..n).map(|_| new_vm_id()).collect();
    let qemu_vms: Vec<VmId> = (0..n).map(|_| new_vm_id()).collect();

    for &vm in &fc_vms {
        fc.start_vm(vm, app_id, "img".into(), default_vm_config())
            .await
            .unwrap();
    }
    for &vm in &qemu_vms {
        qemu.start_vm(vm, app_id, "img".into(), default_vm_config())
            .await
            .unwrap();
    }

    let fc_list_start = Instant::now();
    let fc_vms_out = fc.get_all_vms().await;
    let fc_list_elapsed = fc_list_start.elapsed();

    let qemu_list_start = Instant::now();
    let qemu_vms_out = qemu.get_all_vms().await;
    let qemu_list_elapsed = qemu_list_start.elapsed();

    println!();
    println!("=== get_all_vms benchmark ({n} VMs each) ===");
    println!();
    println!(
        "  Firecracker  {fc_list_elapsed:?}  ({} VMs)",
        fc_vms_out.len()
    );
    println!(
        "  QEMU         {qemu_list_elapsed:?}  ({} VMs)",
        qemu_vms_out.len()
    );

    for &vm in &fc_vms {
        fc.stop_vm(&vm).await.ok();
    }
    for &vm in &qemu_vms {
        qemu.stop_vm(&vm).await.ok();
    }
}

#[tokio::test]
async fn benchmark_pause_resume_qemu() {
    // Only QEMU is benchmarked for pause/resume:
    // Firecracker stub mode doesn't manage real processes, so pause_vm
    // on the stub would fail with VmNotFound.  With real Firecracker
    // (fc binary + API socket) the pause path goes through the API socket
    // and is comparable to QEMU's QMP-based pause.
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("bench-agent".into(), qemu_config()).await;

    let vm_qemu = new_vm_id();
    let app_id = new_app_id();

    qemu.start_vm(vm_qemu, app_id, "img".into(), default_vm_config())
        .await
        .unwrap();

    let pause = Instant::now();
    qemu.pause_vm(&vm_qemu).await.expect("QEMU pause");
    let pause_elapsed = pause.elapsed();

    let resume = Instant::now();
    qemu.resume_vm(&vm_qemu).await.expect("QEMU resume");
    let resume_elapsed = resume.elapsed();

    println!();
    println!("=== QEMU Pause/Resume benchmark ===");
    println!();
    println!("  Pause   {pause_elapsed:?}");
    println!("  Resume  {resume_elapsed:?}");

    qemu.stop_vm(&vm_qemu).await.ok();
}

#[tokio::test]
async fn benchmark_start_single_latency() {
    // Measure single VM start latency across multiple runs for statistics
    let fc = mikrom_agent::firecracker::FirecrackerManager::with_config(fc_config());
    let qemu =
        mikrom_agent::qemu::QemuManager::with_config("bench-agent".into(), qemu_config()).await;

    let runs = 5;
    let mut fc_times = Vec::with_capacity(runs);
    let mut qemu_times = Vec::with_capacity(runs);

    for _ in 0..runs {
        let vm = new_vm_id();
        let api = new_app_id();

        let start = Instant::now();
        fc.start_vm(vm, api, "img".into(), default_vm_config())
            .await
            .unwrap();
        fc_times.push(start.elapsed());
        fc.stop_vm(&vm).await.ok();
    }

    for _ in 0..runs {
        let vm = new_vm_id();
        let api = new_app_id();

        let start = Instant::now();
        qemu.start_vm(vm, api, "img".into(), default_vm_config())
            .await
            .unwrap();
        qemu_times.push(start.elapsed());
        qemu.stop_vm(&vm).await.ok();
    }

    fn stats(label: &str, times: &[std::time::Duration]) {
        let n = times.len();
        let sum: std::time::Duration = times.iter().sum();
        let avg = sum / n as u32;
        let min = *times.iter().min().unwrap();
        let max = *times.iter().max().unwrap();
        let variance: f64 = times
            .iter()
            .map(|t| {
                let diff = t.as_secs_f64() - avg.as_secs_f64();
                diff * diff
            })
            .sum::<f64>()
            / n as f64;
        println!(
            "  {label:15}  avg={avg:?}  min={min:?}  max={max:?}  stddev={:.2?}",
            (variance.sqrt() * 1_000_000_000.0) as u64
        );
    }

    println!();
    println!("=== Start latency ({runs} runs) ===");
    println!();
    stats("Firecracker", &fc_times);
    stats("QEMU", &qemu_times);
}
