use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rule-generator-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
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
        let out = self.artisan(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }
    fn path(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rule_command_generates_regular_and_implicit_types() {
    let root = Root::new();
    let help = root.run(&["make:rule", "--help"]);
    assert!(help.contains("<name>") && help.contains("--force") && help.contains("--implicit"));
    for args in [
        vec!["make:rule"],
        vec!["-n", "make:rule"],
        vec!["make:rule", "--no-interaction"],
    ] {
        assert_eq!(root.artisan(&args).status.code(), Some(1));
    }
    assert!(!root.path("app").exists());
    for (name, path, class) in [
        ("Uppercase", "app/rules/uppercase.rs", "Uppercase"),
        (
            "Admin/AdminRule",
            "app/rules/admin/admin_rule.rs",
            "AdminRule",
        ),
        (
            "Admin\\TokenRule",
            "app/rules/admin/token_rule.rs",
            "TokenRule",
        ),
        (
            "App/Rules/QualifiedRule",
            "app/rules/qualified_rule.rs",
            "QualifiedRule",
        ),
    ] {
        root.run(&["make:rule", name]);
        let code = fs::read_to_string(root.path(path)).unwrap();
        assert!(code.contains(&format!(
            "impl contracts::validation::ValidationRule for {class}"
        )));
        assert!(!code.contains("fn is_implicit"));
        assert!(!code.contains("{{class}}"));
    }
    root.run(&["make:rule", "ImplicitRule", "-i"]);
    assert!(
        fs::read_to_string(root.path("app/rules/implicit_rule.rs"))
            .unwrap()
            .contains("fn is_implicit")
    );
    assert!(!root.path("app/rules/mod.rs").exists());
}
#[test]
fn rule_generator_preserves_files_and_distinguishes_published_stubs() {
    let root = Root::new();
    root.run(&["stub:publish"]);
    let stub = root.path("stubs/rule.rs.stub");
    assert!(stub.is_file());
    let customized = fs::read_to_string(&stub).unwrap() + "\n// 自定义规则模板\n";
    fs::write(&stub, &customized).unwrap();
    let implicit_stub = root.path("stubs/rule.implicit.rs.stub");
    let text = fs::read_to_string(&implicit_stub).unwrap() + "\n// 隐式专属模板\n";
    fs::write(&implicit_stub, text).unwrap();
    root.run(&["make:rule", "CustomImplicit", "--implicit"]);
    let generated = fs::read_to_string(root.path("app/rules/custom_implicit.rs")).unwrap();
    assert!(generated.contains("隐式专属模板"));
    assert!(!generated.contains("自定义规则模板"));
    root.run(&["make:rule", "UpperRule"]);
    let path = root.path("app/rules/upper_rule.rs");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("自定义规则模板")
    );
    fs::write(&path, "用户代码").unwrap();
    let out = root.artisan(&["make:rule", "UpperRule"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("已存在"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "用户代码");
    root.run(&["make:rule", "UpperRule", "-f"]);
    assert!(fs::read_to_string(path).unwrap().contains("自定义规则模板"));
    root.run(&["stub:publish"]);
    assert_eq!(fs::read_to_string(&stub).unwrap(), customized);
    root.run(&["stub:publish", "--force"]);
    assert!(!fs::read_to_string(stub).unwrap().contains("自定义规则模板"));
}
#[test]
fn invalid_names_and_directory_modules_do_not_overwrite_application_files() {
    let root = Root::new();
    for name in [
        "../Outside",
        "Admin/../Outside",
        "lowercase",
        "App/Rules/Crate",
    ] {
        assert_eq!(
            root.artisan(&["make:rule", name]).status.code(),
            Some(1),
            "{name}"
        );
    }
    let path = root.path("app/rules/upper_rule/mod.rs");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "用户模块").unwrap();
    assert_eq!(
        root.artisan(&["make:rule", "UpperRule", "--force"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "用户模块");
    assert!(!root.path("app/rules/upper_rule.rs").exists());
}
