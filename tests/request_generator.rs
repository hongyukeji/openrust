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
            "request-generator-{}-{}-{}",
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
fn request_command_generates_nested_types_and_requires_an_explicit_name() {
    let root = Root::new();
    let help = root.run(&["make:request", "--help"]);
    assert!(help.contains("<name>") && help.contains("--force"));
    for args in [
        vec!["make:request"],
        vec!["-n", "make:request"],
        vec!["make:request", "--no-interaction"],
    ] {
        assert_eq!(root.artisan(&args).status.code(), Some(1));
    }
    assert!(!root.path("app").exists());
    for (name, path, class) in [
        (
            "StoreArticleRequest",
            "app/http/requests/store_article_request.rs",
            "StoreArticleRequest",
        ),
        (
            "Admin/StoreUserRequest",
            "app/http/requests/admin/store_user_request.rs",
            "StoreUserRequest",
        ),
        (
            "Admin\\StoreTokenRequest",
            "app/http/requests/admin/store_token_request.rs",
            "StoreTokenRequest",
        ),
        (
            "App/Http/Requests/UpdateArticleRequest",
            "app/http/requests/update_article_request.rs",
            "UpdateArticleRequest",
        ),
    ] {
        root.run(&["make:request", name]);
        let code = fs::read_to_string(root.path(path)).unwrap();
        assert!(code.contains(&format!("impl foundation::http::FormRequest for {class}")));
        assert!(code.contains("AuthorizationError::Denied"));
        assert!(!code.contains("{{class}}"));
    }
    assert!(!root.path("app/http/requests/mod.rs").exists());
}
#[test]
fn request_generator_preserves_existing_files_and_uses_published_overrides() {
    let root = Root::new();
    root.run(&["stub:publish"]);
    let stub = root.path("stubs/request.rs.stub");
    assert!(stub.is_file());
    let customized = fs::read_to_string(&stub).unwrap() + "\n// 自定义请求模板\n";
    fs::write(&stub, &customized).unwrap();
    root.run(&["make:request", "StoreRequest"]);
    let path = root.path("app/http/requests/store_request.rs");
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("自定义请求模板")
    );
    fs::write(&path, "用户代码").unwrap();
    let out = root.artisan(&["make:request", "StoreRequest"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("已存在"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "用户代码");
    root.run(&["make:request", "StoreRequest", "-f"]);
    assert!(fs::read_to_string(path).unwrap().contains("自定义请求模板"));
    root.run(&["stub:publish"]);
    assert_eq!(fs::read_to_string(&stub).unwrap(), customized);
    root.run(&["stub:publish", "--force"]);
    assert!(!fs::read_to_string(stub).unwrap().contains("自定义请求模板"));
}
#[test]
fn invalid_names_and_directory_modules_do_not_overwrite_application_files() {
    let root = Root::new();
    for name in [
        "../Outside",
        "Admin/../Outside",
        "lowercase",
        "App/Http/Requests/Crate",
    ] {
        assert_eq!(
            root.artisan(&["make:request", name]).status.code(),
            Some(1),
            "{name}"
        );
    }
    let path = root.path("app/http/requests/store_request/mod.rs");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "用户模块").unwrap();
    assert_eq!(
        root.artisan(&["make:request", "StoreRequest", "--force"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "用户模块");
    assert!(!root.path("app/http/requests/store_request.rs").exists());
}
