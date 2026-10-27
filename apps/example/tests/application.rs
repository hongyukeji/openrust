fn manifest_declares_package_name(manifest: &str, package_name: &str) -> bool {
    for line in manifest.lines() {
        if line == format!("name = \"{package_name}\"") {
            return true;
        }
    }
    false
}

#[test]
fn application_package_is_generated() {
    let manifest = include_str!("../Cargo.toml");
    let package_name = env!("CARGO_PKG_NAME");
    assert!(manifest_declares_package_name(manifest, package_name));
}
