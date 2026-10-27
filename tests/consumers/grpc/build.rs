fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/echo.proto");
    println!("cargo:rerun-if-changed=proto/legacy/echo.proto");
    let descriptor_path =
        std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("echo_descriptor.bin");
    let mut config = tonic_prost_build::Config::new();
    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .file_descriptor_set_path(descriptor_path)
        .compile_with_config(config, &["proto/echo.proto"], &["proto"])?;
    let legacy_out = std::path::PathBuf::from(std::env::var("OUT_DIR")?).join("legacy");
    std::fs::create_dir_all(&legacy_out)?;
    let mut legacy_config = tonic_prost_build::Config::new();
    legacy_config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);
    tonic_prost_build::configure()
        .out_dir(legacy_out)
        .compile_with_config(
            legacy_config,
            &["proto/legacy/echo.proto"],
            &["proto/legacy"],
        )?;
    Ok(())
}
