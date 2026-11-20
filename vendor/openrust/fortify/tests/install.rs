use openrust::{
    console::{Application as Artisan, Output},
    contracts::console::Kernel as _,
    foundation::{Application, console::Kernel},
};
use openrust_fortify::FortifyResourcesServiceProvider;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Self(std::env::temp_dir().join(format!(
            "fortify-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        for (path, source) in [
            ("Cargo.toml", include_str!("../../../../Cargo.toml")),
            ("app/mod.rs", include_str!("../../../../app/mod.rs")),
            (
                "app/providers/mod.rs",
                include_str!("../../../../app/providers/mod.rs"),
            ),
            (
                "app/models/user.rs",
                include_str!("../../../../app/models/user.rs"),
            ),
            (
                "bootstrap/providers.rs",
                include_str!("../../../../bootstrap/providers.rs"),
            ),
            ("config/mod.rs", include_str!("../../../../config/mod.rs")),
        ] {
            let target = root.0.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, source).unwrap();
        }
        root
    }
    fn kernel(&self) -> Kernel {
        let app = Application::configure(self.0.clone())
            .withProviders(vec![Box::new(FortifyResourcesServiceProvider)])
            .create()
            .unwrap();
        Kernel::new(app, Artisan::new())
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.0.join(path)).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn installer_publishes_registers_and_preserves_application_edits_on_retry() {
    let root = Root::new();
    let mut kernel = root.kernel();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(&["fortify:install".into(), "--help".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert!(
        output
            .stdout
            .contains("Install all of the Fortify resources")
    );
    for _ in 0..2 {
        let mut output = Output::default();
        assert_eq!(
            kernel
                .handle(&["fortify:install".into()], &mut output)
                .await,
            0,
            "{}",
            output.stderr
        );
        assert!(
            output
                .stdout
                .contains("Fortify scaffolding installed successfully.")
        );
    }
    assert_eq!(
        root.read("bootstrap/providers.rs")
            .matches("Box::new(openrust_fortify::FortifyServiceProvider)")
            .count(),
        1
    );
    assert!(root.read("config/mod.rs").contains("fortify::options()"));
    assert!(
        root.read("app/models/user.rs")
            .contains("two_factor_recovery_codes")
    );
    let migration_count = std::fs::read_dir(root.0.join("database/migrations"))
        .unwrap()
        .count();
    assert_eq!(migration_count, 2);
    let path = root.0.join("app/actions/fortify/create_new_user.rs");
    let customized = format!(
        "{}\n// application customization\n",
        root.read("app/actions/fortify/create_new_user.rs")
    );
    std::fs::write(&path, &customized).unwrap();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), customized);
    assert_eq!(
        std::fs::read_dir(root.0.join("database/migrations"))
            .unwrap()
            .count(),
        2
    );
    assert!(!root.0.join("target").exists());
}

#[tokio::test]
async fn installer_rejects_unknown_source_layout_before_publishing() {
    let root = Root::new();
    std::fs::write(
        root.0.join("bootstrap/providers.rs"),
        "fn providers() { custom_registry() }\n",
    )
    .unwrap();
    let previous = root.read("app/mod.rs");
    let mut output = Output::default();
    assert_ne!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0
    );
    assert!(
        output.stderr.contains("Register Fortify manually"),
        "{}",
        output.stderr
    );
    assert!(!root.0.join("config/fortify.rs").exists());
    assert_eq!(root.read("app/mod.rs"), previous);
}

#[tokio::test]
async fn installer_preserves_source_line_endings_and_existing_action_modules() {
    let root = Root::new();
    for relative in [
        "app/mod.rs",
        "app/providers/mod.rs",
        "app/models/user.rs",
        "bootstrap/providers.rs",
        "config/mod.rs",
    ] {
        let path = root.0.join(relative);
        let source = std::fs::read_to_string(&path)
            .unwrap()
            .replace('\n', "\r\n");
        std::fs::write(path, source).unwrap();
    }
    std::fs::create_dir_all(root.0.join("app/actions")).unwrap();
    std::fs::write(
        root.0.join("app/actions/mod.rs"),
        "// existing application actions\r\n",
    )
    .unwrap();
    let mut output = Output::default();
    assert_eq!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    for relative in [
        "app/mod.rs",
        "app/actions/mod.rs",
        "app/providers/mod.rs",
        "app/models/user.rs",
        "bootstrap/providers.rs",
        "config/mod.rs",
    ] {
        let source = root.read(relative);
        assert!(source.contains("\r\n"));
        assert!(!source.replace("\r\n", "").contains('\n'), "{relative}");
    }
    assert!(
        root.read("app/actions/mod.rs")
            .starts_with("// existing application actions\r\n")
    );
}
