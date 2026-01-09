use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let proto_dir = PathBuf::from("proto");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=proto/");

    // Compile the proto files from the local proto/ directory
    tonic_build::configure()
        .build_server(true)
        .build_client(false)
        .file_descriptor_set_path(out_dir.join("envoy_ratelimit_descriptor.bin"))
        .compile_protos(
            &[proto_dir.join("envoy/service/ratelimit/v3/rls.proto")],
            &[&proto_dir],
        )?;

    Ok(())
}
