use std::path::{Path, PathBuf};

fn ensure_protoc() -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Ok(protoc) = std::env::var("PROTOC") {
        let path = PathBuf::from(protoc);
        if path.exists() {
            return Ok(path);
        }
    }

    if let Ok(output) = std::process::Command::new("protoc")
        .arg("--version")
        .output()
        && output.status.success()
    {
        return Ok(PathBuf::from("protoc"));
    }

    let cache_dir = Path::new("/tmp/opencode/protoc");
    let bin_dir = cache_dir.join("bin");
    let protoc_path = bin_dir.join("protoc");
    if protoc_path.exists() {
        let shim_dir = Path::new("/tmp/opencode/bin");
        std::fs::create_dir_all(shim_dir)?;
        let shim_path = shim_dir.join("protoc");
        if !shim_path.exists() {
            std::fs::write(
                &shim_path,
                format!("#!/bin/sh\nexec '{}' \"$@\"\n", protoc_path.display()),
            )?;
            let mut perms = std::fs::metadata(&shim_path)?.permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                perms.set_mode(0o755);
                std::fs::set_permissions(&shim_path, perms)?;
            }
        }
        let include_dir = cache_dir.join("include");
        println!("cargo:rustc-env=PROTOC={}", protoc_path.display());
        println!("cargo:rustc-env=PROTOC_INCLUDE={}", include_dir.display());
        return Ok(protoc_path);
    }

    std::fs::create_dir_all(&bin_dir)?;

    let version = "26.1";
    let archive_name = format!("protoc-{version}-linux-x86_64.zip");
    let archive_path = cache_dir.join(&archive_name);
    if !archive_path.exists() {
        let url = format!(
            "https://github.com/protocolbuffers/protobuf/releases/download/v{version}/{archive_name}"
        );
        let status = std::process::Command::new("curl")
            .args(["-L", "--fail", "--silent", "--show-error", "-o"])
            .arg(&archive_path)
            .arg(url)
            .status()?;
        if !status.success() {
            return Err("failed to download protoc".into());
        }
    }

    let unzip_status = std::process::Command::new("unzip")
        .args([
            "-o",
            archive_path.to_str().ok_or("invalid archive path")?,
            "-d",
        ])
        .arg(cache_dir)
        .status()?;
    if !unzip_status.success() {
        return Err("failed to extract protoc".into());
    }

    if !protoc_path.exists() {
        return Err("protoc binary not found after extraction".into());
    }

    let shim_dir = Path::new("/tmp/opencode/bin");
    std::fs::create_dir_all(shim_dir)?;
    let shim_path = shim_dir.join("protoc");
    std::fs::write(
        &shim_path,
        format!("#!/bin/sh\nexec '{}' \"$@\"\n", protoc_path.display()),
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&shim_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&shim_path, perms)?;
    }

    let include_dir = cache_dir.join("include");
    println!("cargo:rustc-env=PROTOC={}", protoc_path.display());
    println!("cargo:rustc-env=PROTOC_INCLUDE={}", include_dir.display());
    Ok(protoc_path)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let protoc = ensure_protoc()?;
    let protoc_dir = protoc
        .parent()
        .ok_or("protoc path missing parent directory")?;
    println!(
        "cargo:rustc-env=PATH=/tmp/opencode/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    );
    let _ = protoc_dir;
    tonic_prost_build::configure()
        .out_dir(std::path::Path::new("src"))
        .build_server(false)
        .build_client(false)
        .compile_protos(
            &[
                "proto/scheduler.proto",
                "proto/agent.proto",
                "proto/builder.proto",
                "proto/router.proto",
            ],
            &["proto/"],
        )?;

    println!("cargo:rerun-if-changed=proto/scheduler.proto");
    println!("cargo:rerun-if-changed=proto/agent.proto");
    println!("cargo:rerun-if-changed=proto/builder.proto");
    println!("cargo:rerun-if-changed=proto/router.proto");

    Ok(())
}
