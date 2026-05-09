use std::fs;
use std::path::Path;

fn main() {
    // Path to the eBPF binary relative to this build script
    let ebpf_path = "../target/bpfel-unknown-none/release/mikrom-agent-ebpf";
    let ebpf_dir = "../target/bpfel-unknown-none/release";

    // If the file doesn't exist, create an empty one to satisfy include_bytes!
    // This is mainly for CI/linting where the eBPF program might not have been built yet.
    if !Path::new(ebpf_path).exists() {
        println!(
            "cargo:warning=eBPF binary not found at {}, creating dummy file for compilation",
            ebpf_path
        );
        let _ = fs::create_dir_all(ebpf_dir);
        let _ = fs::write(ebpf_path, []);
    }

    // Re-run if the eBPF binary changes (if it exists)
    println!("cargo:rerun-if-changed={}", ebpf_path);
}
