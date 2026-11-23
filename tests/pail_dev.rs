#![cfg(feature = "pail")]

use openrust::{
    console::{ConsoleFuture, Output},
    container::{Callback, ServiceKey},
    contracts::{
        container::Container as _, foundation::Application as _, support::DeferrableProvider,
    },
    foundation::{Application, DevCommands},
    support::{DeferredProvider, ProviderResult, ServiceProvider, facades::Artisan},
};
use openrust_pail::PailServiceProvider;
use serde_json::Value;
use std::{
    any::type_name,
    ffi::OsString,
    path::PathBuf,
    process::Command,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[path = "support/vendor/pail_dev.rs"]
mod package;
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "pail-dev-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join(".env"), "APP_ENV=production\n").unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
async fn list(app: &Application, arguments: &[&str]) -> (i32, Vec<Value>) {
    let args: Vec<OsString> = arguments.iter().map(Into::into).collect();
    app.call(
        Callback::new(&[], |_| async {
            let mut output = Output::default();
            let status = Artisan::call("dev:list", &args, &mut output).await.unwrap();
            Ok((status, serde_json::from_str(&output.stdout).unwrap()))
        }),
        Default::default(),
    )
    .await
    .unwrap()
}
fn names(processes: &[Value]) -> Vec<&str> {
    processes
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect()
}
struct Early(Arc<Mutex<Vec<String>>>);
impl ServiceProvider for Early {
    fn register<'a>(&'a mut self, app: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert!(!app.provider_is_loaded(type_name::<Self>()));
            *self.0.lock().unwrap() = DevCommands::commands()?
                .into_iter()
                .map(|v| v.name)
                .collect();
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert!(app.provider_is_loaded(type_name::<Self>()));
            assert!(app.provider_is_loaded(type_name::<PailServiceProvider>()));
            Ok(())
        })
    }
}
#[tokio::test]
async fn defaults_use_loaded_provider_identity_and_do_not_freeze_early_absence() {
    let root = Root::new();
    let other = Root::new();
    let early = Arc::new(Mutex::new(Vec::new()));
    let app = Application::configure(root.0.clone())
        .with_providers(vec![
            Box::new(Early(early.clone())),
            Box::new(PailServiceProvider),
        ])
        .create()
        .unwrap();
    let other = Application::configure(other.0.clone()).create().unwrap();
    assert!(!app.provider_is_loaded(type_name::<PailServiceProvider>()));
    let ((status, entries), (_, absent)) = tokio::join!(list(&app, &[]), list(&other, &[]));
    assert_eq!(status, 0);
    assert_eq!(names(&entries), ["server", "queue", "logs"]);
    assert_eq!(*early.lock().unwrap(), ["server", "queue"]);
    assert_eq!(names(&absent), ["server", "queue"]);
    assert!(app.provider_is_loaded(type_name::<PailServiceProvider>()));
    assert!(!other.provider_is_loaded(type_name::<PailServiceProvider>()));
    assert_eq!(entries[2]["command"], "./artisan pail --timeout=0");
    assert_eq!(entries[2]["priority"], 0);
    assert_eq!(list(&app, &["--only-vendor"]).await, (1, vec![]));
    assert_eq!(list(&app, &["--except-vendor"]).await, (0, entries.clone()));
    assert_eq!(
        list(&app, &["--filter=logs"]).await,
        (0, vec![entries[2].clone()])
    );
    assert!(
        !root.0.join("storage/pail").exists(),
        "listing must not start Pail"
    );
}
struct Configure(&'static str);
impl ServiceProvider for Configure {
    fn boot<'a>(&'a mut self, _: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            match self.0 {
                "without-default" => DevCommands::without_default_commands()?,
                "without-vendor" => DevCommands::without_vendor_commands()?,
                "except" => DevCommands::except(&["logs"])?,
                "only" => DevCommands::only(&["logs"])?,
                "order" => DevCommands::order(&["logs", "server"])?,
                "vendor" => package::register()?,
                "user" => {
                    DevCommands::artisan("pail --timeout=7", Some("logs"))?.pink();
                    package::register()?;
                }
                _ => unreachable!(),
            }
            Ok(())
        })
    }
}
#[tokio::test]
async fn logs_obey_default_vendor_user_priorities_and_shared_filters() {
    for mode in [
        "without-default",
        "without-vendor",
        "except",
        "only",
        "order",
        "vendor",
        "user",
    ] {
        let root = Root::new();
        let app = Application::configure(root.0.clone())
            .with_providers(vec![
                Box::new(PailServiceProvider),
                Box::new(Configure(mode)),
            ])
            .create()
            .unwrap();
        let (status, entries) = list(&app, &[]).await;
        assert_eq!(status, 0, "{mode}");
        match mode {
            "without-default" => assert!(entries.is_empty()),
            "without-vendor" => assert_eq!(names(&entries), ["server", "queue", "logs"]),
            "except" => assert_eq!(names(&entries), ["server", "queue"]),
            "only" => assert_eq!(names(&entries), ["logs"]),
            "order" => assert_eq!(names(&entries), ["logs", "server", "queue"]),
            "vendor" => {
                assert_eq!(entries[2]["command"], "echo package logs");
                assert_eq!(entries[2]["priority"], 1);
            }
            "user" => {
                assert_eq!(entries[2]["command"], "./artisan pail --timeout=7");
                assert_eq!(entries[2]["priority"], 2);
                assert_eq!(entries[2]["color"], "#fb7185");
            }
            _ => unreachable!(),
        }
    }
}
struct Lazy {
    fail: Arc<AtomicBool>,
    boot_fail: bool,
}
impl DeferrableProvider for Lazy {
    fn provides() -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<u32>(), ServiceKey::typed::<u64>()]
    }
}
impl ServiceProvider for Lazy {
    fn name(&self) -> &'static str {
        "diagnostic-only"
    }
    fn is_deferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<u32>(), ServiceKey::typed::<u64>()]
    }
    fn register<'a>(&'a mut self, app: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert!(!app.provider_is_loaded(type_name::<Self>()));
            if self.fail.swap(false, Ordering::SeqCst) {
                return Err(std::io::Error::other("registration failed").into());
            }
            app.container().instance(Arc::new(7u32))?;
            app.container().instance(Arc::new(7u64))?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert!(app.provider_is_loaded(type_name::<Self>()));
            assert!(!app.provider_is_loaded("diagnostic-only"));
            if self.boot_fail {
                return Err(std::io::Error::other("boot failed").into());
            }
            Ok(())
        })
    }
}
#[tokio::test]
async fn loaded_provider_marks_successful_registration_before_boot_and_survives_boot_failure() {
    for (factory, boot_fail) in [(false, false), (false, true), (true, false), (true, true)] {
        let root = Root::new();
        let fail = Arc::new(AtomicBool::new(true));
        let provider: Box<dyn ServiceProvider> = if factory {
            Box::new(DeferredProvider::new(move || Lazy {
                fail: fail.clone(),
                boot_fail,
            }))
        } else {
            Box::new(Lazy { fail, boot_fail })
        };
        let mut app = Application::configure(root.0.clone())
            .with_providers(vec![provider])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        assert!(!app.provider_is_loaded(type_name::<Lazy>()));
        assert!(app.make::<u32>().await.is_err());
        assert!(!app.provider_is_loaded(type_name::<Lazy>()));
        // A failed consumed key does not retrigger registration; use an unconsumed key.
        let result = app.make::<u64>().await;
        if boot_fail {
            assert!(result.is_err());
        } else {
            assert_eq!(*result.unwrap(), 7);
        }
        assert!(app.provider_is_loaded(type_name::<Lazy>()));
    }
}
#[test]
fn actual_binary_discovery_preserves_default_priority_and_environment_gates() {
    let root = Root::new();
    let run = |env: &str, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "dev:list", "--json"])
            .args(args)
            .env("APP_BASE_PATH", &root.0)
            .env("APP_ENV", env)
            .env_remove("PAIL_TESTS")
            .env_remove("VAPOR_SSM_PATH")
            .env_remove("APP_CONFIG_CACHE")
            .env_remove("APP_PACKAGES_CACHE")
            .output()
            .unwrap()
    };
    for env in ["production", "testing"] {
        let result = run(env, &[]);
        assert!(result.status.success(), "{:?}", result);
        let entries: Vec<Value> = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(names(&entries), ["server", "queue", "logs"]);
        assert_eq!(entries[2]["priority"], 0);
        assert!(!root.0.join("storage/pail").exists());
        let result = run(env, &["--only-vendor"]);
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(result.stdout, b"[]\n");
    }
}

struct CancelledRegistration;
impl ServiceProvider for CancelledRegistration {
    fn is_deferred(&self) -> bool {
        true
    }
    fn provides(&self) -> Vec<ServiceKey> {
        vec![ServiceKey::typed::<String>()]
    }
    fn register<'a>(&'a mut self, _: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(std::future::pending())
    }
}
struct RegistrationFailure;
impl ServiceProvider for RegistrationFailure {
    fn register<'a>(&'a mut self, _: &'a Application) -> ConsoleFuture<'a, ProviderResult> {
        Box::pin(async { Err(std::io::Error::other("eager register failed").into()) })
    }
}
#[tokio::test]
async fn failed_eager_and_cancelled_deferred_registrations_are_not_marked_loaded() {
    let root = Root::new();
    let mut app = Application::configure(root.0.clone())
        .with_providers(vec![
            Box::new(PailServiceProvider),
            Box::new(RegistrationFailure),
        ])
        .create()
        .unwrap();
    assert!(app.boot().await.is_err());
    assert!(app.provider_is_loaded(type_name::<PailServiceProvider>()));
    assert!(!app.provider_is_loaded(type_name::<RegistrationFailure>()));
    let other = Root::new();
    let mut app = Application::configure(other.0.clone())
        .with_providers(vec![Box::new(CancelledRegistration)])
        .create()
        .unwrap();
    app.boot().await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), app.make::<String>())
            .await
            .is_err()
    );
    assert!(!app.provider_is_loaded(type_name::<CancelledRegistration>()));
}
