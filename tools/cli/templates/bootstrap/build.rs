//! 生成应用的编译期装配入口。

#[path = "bootstrap/package_discovery_build.rs"]
mod package_discovery;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    package_discovery::compile()?;
{{grpc_build}}    Ok(())
}
