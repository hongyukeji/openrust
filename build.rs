//! 接入根应用的编译期 Provider 包清单。

#[path = "tools/cli/templates/bootstrap/package_discovery_build.rs"]
mod package_discovery;

fn main() -> std::io::Result<()> {
    package_discovery::compile()
}
