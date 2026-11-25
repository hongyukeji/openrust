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

#[test]
fn default_application_does_not_ship_optional_package_configuration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = fs::read_to_string(root.join("config/mod.rs")).unwrap();
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let features = manifest
        .split("[features]")
        .nth(1)
        .unwrap()
        .split("[dependencies]")
        .next()
        .unwrap();
    for line in features
        .lines()
        .filter(|line| line.contains("dep:openrust-"))
    {
        let name = line.split('=').next().unwrap().trim();
        assert!(
            !root.join(format!("config/{name}.rs")).exists(),
            "optional {name} configuration belongs to its package until explicitly published"
        );
        let module = name.replace('-', "_");
        assert!(
            !root.join(format!("config/{module}.rs")).exists(),
            "optional {name} configuration belongs to its package until explicitly published"
        );
        assert!(
            !source.contains(&format!("mod {module};")),
            "default loader preinstalls {name}"
        );
    }
    for directory in ["app", "routes"] {
        check_providers(&root.join(directory));
    }
}
