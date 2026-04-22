fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .out_dir(std::path::Path::new("src"))
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "proto/scheduler.proto",
                "proto/agent.proto",
                "proto/builder.proto",
            ],
            &["proto/"],
        )?;

    println!("cargo:rerun-if-changed=proto/scheduler.proto");
    println!("cargo:rerun-if-changed=proto/agent.proto");
    println!("cargo:rerun-if-changed=proto/builder.proto");

    Ok(())
}
