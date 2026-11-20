use std::{fs, path::Path};

fn check_providers(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            check_providers(&path);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let source = fs::read_to_string(&path).unwrap();
            for identifier in source.split(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
                assert!(
                    !identifier.starts_with("openrust_"),
                    "{} references extension {identifier}; package setup belongs in its own provider, application examples in fixtures",
                    path.display()
                );
            }
            assert!(
                !source.contains("cfg(feature"),
                "{} contains feature-gated application wiring",
                path.display()
            );
        }
    }
}

#[test]
fn default_application_providers_do_not_preinstall_extensions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    check_providers(&root.join("app/providers"));
    let bootstrap = fs::read_to_string(root.join("bootstrap/providers.rs")).unwrap();
    assert!(!bootstrap.contains("openrust_"));
    assert!(!bootstrap.contains("cfg(feature"));
}
