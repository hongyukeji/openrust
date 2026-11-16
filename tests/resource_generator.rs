use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

struct AppRoot(PathBuf);

impl AppRoot {
    fn new() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "openrust-resource-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }

    fn artisan(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &self.0)
            .output()
            .unwrap()
    }

    fn run(&self, args: &[&str]) -> String {
        let output = self.artisan(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }
}

impl Drop for AppRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn artisan_generates_resources_and_selects_the_collection_stub_first() {
    let root = AppRoot::new();
    let help = root.run(&["make:resource", "--help"]);
    for flag in ["--force", "--json-api", "--collection"] {
        assert!(help.contains(flag), "{help}");
    }
    for (args, path, expected) in [
        (
            &["UserResource"][..],
            "user_resource.rs",
            "JsonResource for UserResource",
        ),
        (
            &["Admin/UserResource", "-j"],
            "admin/user_resource.rs",
            "JsonApiResource for UserResource",
        ),
        (
            &["Admin\\UserCollection", "--json-api"],
            "admin/user_collection.rs",
            "pub type UserCollection",
        ),
        (
            &["UserList", "-c", "-j"],
            "user_list.rs",
            "pub type UserList",
        ),
        (
            &["App/Http/Resources/OrderResource", "--json-api"],
            "order_resource.rs",
            "JsonApiResource for OrderResource",
        ),
    ] {
        let mut command = vec!["make:resource"];
        command.extend(args);
        assert!(root.run(&command).contains("created successfully"));
        let source = fs::read_to_string(root.0.join("app/http/resources").join(path)).unwrap();
        assert!(source.contains(expected), "{source}");
    }
    assert!(!root.0.join("app/http/resources/mod.rs").exists());
    let rejected = root.artisan(&["make:resource", "Admin/../UnsafeResource"]);
    assert!(!rejected.status.success());
    assert!(
        !root
            .0
            .join("app/http/resources/unsafe_resource.rs")
            .exists()
    );
}

#[test]
fn artisan_publishes_and_uses_resource_templates_and_preserves_existing_source() {
    let root = AppRoot::new();
    root.run(&["stub:publish"]);
    for name in ["resource", "resource-json-api", "resource-collection"] {
        let path = root.0.join("stubs").join(format!("{name}.rs.stub"));
        let source = fs::read_to_string(&path).unwrap();
        assert!(source.contains("{{class}}"));
        fs::write(&path, format!("// {name}\n{source}")).unwrap();
    }
    for (resource, flag, stub, path) in [
        ("InvoiceResource", None, "resource", "invoice_resource.rs"),
        (
            "InvoiceDocument",
            Some("--json-api"),
            "resource-json-api",
            "invoice_document.rs",
        ),
        (
            "InvoiceCollection",
            Some("--collection"),
            "resource-collection",
            "invoice_collection.rs",
        ),
    ] {
        let mut args = vec!["make:resource", resource];
        args.extend(flag);
        root.run(&args);
        let path = root.0.join("app/http/resources").join(path);
        let original = fs::read_to_string(&path).unwrap();
        assert!(original.starts_with(&format!("// {stub}\n")));
        fs::write(&path, "// application-owned\n").unwrap();

        let duplicate = root.artisan(&args);
        assert_eq!(duplicate.status.code(), Some(0));
        assert!(String::from_utf8_lossy(&duplicate.stderr).contains("already exists"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "// application-owned\n");
        args.push("-f");
        root.run(&args);
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }
    let directory_module = root.0.join("app/http/resources/account_resource/mod.rs");
    fs::create_dir_all(directory_module.parent().unwrap()).unwrap();
    fs::write(&directory_module, "// directory module\n").unwrap();
    assert!(
        !root
            .artisan(&["make:resource", "AccountResource", "-f"])
            .status
            .success()
    );
    assert!(
        !root
            .0
            .join("app/http/resources/account_resource.rs")
            .exists()
    );
    assert_eq!(
        fs::read_to_string(directory_module).unwrap(),
        "// directory module\n"
    );
}
