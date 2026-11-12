use std::{
    collections::BTreeMap,
    fs, io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
};

use openrust::{
    config::{ConfigError, Environment, Value},
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    foundation::{
        Application, BootstrapError, bootstrap::LoadEnvironmentVariables, configuration::Routing,
        http::Kernel,
    },
    http::{Body, Request, StatusCode},
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;

#[path = "../routes/web.rs"]
mod web;

#[path = "../config/mod.rs"]
mod config;

struct TestRoot(PathBuf);

#[test]
fn artisan_installs_optional_routes_without_overwriting_or_duplication() {
    let root = TestRoot::new("install-routes");
    fs::write(root.0.join("Cargo.toml"), "").unwrap();
    fs::create_dir(root.0.join("routes")).unwrap();
    fs::create_dir(root.0.join("bootstrap")).unwrap();
    let modules = root.0.join("routes/mod.rs");
    let bootstrap = root.0.join("bootstrap/app.rs");
    fs::write(&modules, "pub(crate) mod console;\npub(crate) mod web;\n").unwrap();
    fs::write(&bootstrap, "custom bootstrap\n").unwrap();
    let run = |command: &str| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", command])
            .env("APP_BASE_PATH", &root.0)
            .output()
            .unwrap()
    };
    let result = run("install:api");
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("manually"));
    assert!(!root.0.join("routes/api.rs").exists());
    assert!(!fs::read_to_string(&modules).unwrap().contains("mod api;"));

    fs::write(&bootstrap, include_str!("../bootstrap/app.rs")).unwrap();
    for (command, module, registration) in [
        ("install:api", "api", ".api(routes::api::register)"),
        (
            "install:broadcasting",
            "channels",
            ".channels(routes::channels::register)",
        ),
        ("install:grpc", "grpc", ".withGrpc(routes::grpc::register)"),
    ] {
        let result = run(command);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let routes = root.0.join(format!("routes/{module}.rs"));
        assert!(fs::read_to_string(&routes).unwrap().contains("fn register"));
        fs::write(&routes, "// Existing application routes\n").unwrap();
        assert!(run(command).status.success());
        assert_eq!(
            fs::read_to_string(&routes).unwrap(),
            "// Existing application routes\n"
        );
        assert_eq!(
            fs::read_to_string(&modules)
                .unwrap()
                .matches(&format!("mod {module};"))
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(&bootstrap)
                .unwrap()
                .matches(registration)
                .count(),
            1
        );
    }
}

#[test]
fn artisan_generates_commands_and_preserves_existing_source() {
    let root = TestRoot::new("make-command");
    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "make:command"])
            .args(arguments)
            .env("APP_BASE_PATH", &root.0)
            .output()
            .unwrap()
    };
    let path = root.0.join("app/console/commands/reports/send_report.rs");
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(&["Reports/SendReport"]));
        let second = scope.spawn(|| run(&["Reports/SendReport.rs"]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        results
            .iter()
            .filter(|result| result.status.success())
            .count(),
        1
    );
    let original = fs::read_to_string(&path).unwrap();
    assert!(original.contains("pub struct SendReport"));
    assert!(original.contains(r#""app:send-report""#));
    assert_eq!(run(&["Reports/SendReport"]).status.code(), Some(1));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    assert!(
        run(&["Reports/SendReport", "-f", "--command=reports:send"])
            .status
            .success()
    );
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains(r#""reports:send""#)
    );

    for arguments in [
        &["../Escape"][..],
        &["Mod/Escape"],
        &["Self"],
        &["Escape", "--command=app:bad name"],
    ] {
        assert_eq!(run(arguments).status.code(), Some(1), "{arguments:?}");
    }
    assert!(!root.0.join("app/console/commands/escape.rs").exists());

    let stubs = root.0.join("stubs");
    fs::create_dir(&stubs).unwrap();
    let stub = stubs.join("console.rs.stub");
    fs::write(&stub, b"// {{class}} - {{command}}\n").unwrap();
    assert!(run(&["CustomStub"]).status.success());
    assert_eq!(
        fs::read_to_string(root.0.join("app/console/commands/custom_stub.rs")).unwrap(),
        "// CustomStub - app:custom-stub\n"
    );
    fs::write(&stub, [0xff]).unwrap();
    assert_eq!(run(&["BrokenStub"]).status.code(), Some(1));
    assert!(!root.0.join("app/console/commands/broken_stub.rs").exists());
}

#[test]
fn artisan_generates_isolated_feature_and_unit_tests() {
    let root = TestRoot::new("make-test");
    let other = TestRoot::new("make-test-other");
    let run = |base: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .args(["artisan", "make:test"])
            .args(args)
            .env("APP_BASE_PATH", base)
            .output()
            .unwrap()
    };
    let commands = [&["Reports/ReportTest"][..], &["Reports/ReportTest.rs"]];
    let generated = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(&root.0, commands[0]));
        let second = scope.spawn(|| run(&root.0, commands[1]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        generated
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    let feature_path = "tests/feature/reports/report_test.rs";
    let original = fs::read_to_string(root.0.join(feature_path)).unwrap();
    assert!(original.contains("TestApp::create"));
    assert!(original.contains("fn test_report_test()"));
    assert_eq!(run(&root.0, commands[0]).status.code(), Some(1));
    assert_eq!(
        fs::read_to_string(root.0.join(feature_path)).unwrap(),
        original
    );
    assert!(!other.0.join(feature_path).exists());

    assert!(run(&other.0, &["ReportTest", "-u"]).status.success());
    let unit = other.0.join("tests/unit/report_test.rs");
    let source = fs::read_to_string(&unit).unwrap();
    assert!(source.contains("#[test]"));
    assert!(!source.contains("TestApp"));
    assert!(!root.0.join("tests/unit").exists());

    fs::create_dir(other.0.join("stubs")).unwrap();
    fs::write(other.0.join("stubs/test.unit.rs.stub"), "// {{test}}\n").unwrap();
    assert!(
        run(&other.0, &["ReportTest", "--unit", "-f"])
            .status
            .success()
    );
    assert_eq!(fs::read_to_string(&unit).unwrap(), "// test_report_test\n");
    fs::write(other.0.join("stubs/test.unit.rs.stub"), [0xff]).unwrap();
    assert_eq!(
        run(&other.0, &["ReportTest", "--unit", "--force"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(fs::read_to_string(&unit).unwrap(), "// test_report_test\n");

    let harness = other.0.join("tests/unit/main.rs");
    fs::write(&harness, "// existing Cargo target\n").unwrap();
    for args in [
        &["../Escape"][..],
        &["Main", "--unit", "--force"],
        &["Self"],
    ] {
        assert_eq!(run(&other.0, args).status.code(), Some(1));
    }
    assert_eq!(
        fs::read_to_string(harness).unwrap(),
        "// existing Cargo target\n"
    );
}

#[test]
fn artisan_generates_matching_command_tests_and_reports_partial_failures() {
    let root = TestRoot::new("make-matching-test");
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &root.0)
            .output()
            .unwrap()
    };
    let command = root.0.join("app/console/commands/reports/send_report.rs");
    let test = root
        .0
        .join("tests/feature/console/commands/reports/send_report_test.rs");
    let name = "Reports/SendReport.rs";
    let matching = "Console/Commands/Reports/SendReportTest";

    let stubs = root.0.join("stubs");
    fs::create_dir(&stubs).unwrap();
    let stub = stubs.join("test.rs.stub");
    fs::write(&stub, [0xff]).unwrap();
    let partial = run(&["make:command", name, "--test"]);
    assert_eq!(partial.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&partial.stderr).contains(matching));
    assert!(!test.exists());
    let original = fs::read_to_string(&command).unwrap();
    assert!(original.contains(r#""app:send-report""#));

    fs::remove_file(&stub).unwrap();
    assert!(run(&["make:test", matching]).status.success());
    assert_eq!(fs::read_to_string(&command).unwrap(), original);
    assert!(
        fs::read_to_string(&test)
            .unwrap()
            .contains("test_send_report_test")
    );
    fs::write(&test, "// existing test\n").unwrap();
    assert_eq!(
        run(&["make:command", name, "--test"]).status.code(),
        Some(1)
    );
    assert_eq!(fs::read_to_string(&test).unwrap(), "// existing test\n");

    assert!(
        run(&[
            "make:command",
            name,
            "--test",
            "--force",
            "--command=reports:send"
        ])
        .status
        .success()
    );
    assert!(
        fs::read_to_string(&command)
            .unwrap()
            .contains(r#""reports:send""#)
    );
    assert!(
        fs::read_to_string(&test)
            .unwrap()
            .contains("test_send_report_test")
    );

    let existing = "Console/Commands/ArchiveTest";
    assert!(run(&["make:test", existing]).status.success());
    let existing = root
        .0
        .join("tests/feature/console/commands/archive_test.rs");
    fs::write(&existing, "// keep the test\n").unwrap();
    assert_eq!(
        run(&["make:command", "Archive", "--test"]).status.code(),
        Some(1)
    );
    assert!(root.0.join("app/console/commands/archive.rs").is_file());
    assert_eq!(fs::read_to_string(existing).unwrap(), "// keep the test\n");
}

#[test]
fn artisan_runs_real_cargo_tests_with_forwarded_arguments_and_exit_status() {
    let root = TestRoot::new("artisan-test");
    fs::create_dir(root.0.join("src")).unwrap();
    fs::write(
        root.0.join("Cargo.toml"),
        "[package]\nname = \"artisan-test-probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        root.0.join("src/lib.rs"),
        r#"
#[test]
fn probe() { println!("stdout marker"); eprintln!("stderr marker"); }

#[test]
fn probe_fails() { panic!("test failure marker"); }
"#,
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &root.0)
            .env("CARGO_TARGET_DIR", root.0.join("target"))
            .env("CARGO_NET_OFFLINE", "true")
            .output()
            .unwrap()
    };

    let help = run(&["test", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("CARGO_ARGS"));
    assert!(!root.0.join("target").exists());

    let success = run(&["test", "--lib", "probe", "--", "--exact", "--nocapture"]);
    assert!(success.status.success(), "{:?}", success);
    assert!(String::from_utf8_lossy(&success.stdout).contains("stdout marker"));
    assert!(String::from_utf8_lossy(&success.stderr).contains("stderr marker"));
    let failure = run(&["test", "--", "--lib", "probe_fails", "--", "--exact"]);
    assert_eq!(failure.status.code(), Some(101), "{:?}", failure);
    assert!(String::from_utf8_lossy(&failure.stdout).contains("test failure marker"));

    let silent = run(&["--silent", "test", "--lib", "probe_fails", "--", "--exact"]);
    assert_eq!(silent.status.code(), Some(101));
    assert!(silent.stdout.is_empty() && silent.stderr.is_empty());
}

#[test]
fn artisan_publishes_stubs_used_by_source_generators() {
    let root = TestRoot::new("publish-stubs");
    let other = TestRoot::new("publish-stubs-other");
    let run = |base: &Path, args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", base)
            .output()
            .unwrap()
    };
    let stubs = root.0.join("stubs");
    let console = stubs.join("console.rs.stub");
    fs::write(&stubs, "blocked").unwrap();
    assert_eq!(run(&root.0, &["stub:publish"]).status.code(), Some(1));
    fs::remove_file(&stubs).unwrap();
    fs::create_dir(&stubs).unwrap();
    fs::write(&console, "// custom\n").unwrap();

    assert!(
        run(&root.0, &["stub:publish", "--existing", "--force"])
            .status
            .success()
    );
    assert_eq!(fs::read_dir(&stubs).unwrap().count(), 1);
    let original = fs::read_to_string(&console).unwrap();
    assert!(original.contains("{{class}}"));
    fs::write(&console, "// preserved\n").unwrap();

    let outcomes = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(&root.0, &["stub:publish"]));
        let second = scope.spawn(|| run(&root.0, &["stub:publish"]));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert!(outcomes.iter().all(|output| output.status.success()));
    assert_eq!(fs::read_dir(&stubs).unwrap().count(), 11);
    assert_eq!(fs::read_to_string(&console).unwrap(), "// preserved\n");
    assert!(!other.0.join("stubs").exists());
    assert!(run(&root.0, &["stub:publish", "--force"]).status.success());
    assert_eq!(fs::read_to_string(&console).unwrap(), original);

    for entry in fs::read_dir(&stubs).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        let source = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("// used {name}\n{source}")).unwrap();
    }
    for (stub, arguments, path) in [
        (
            "console",
            &["make:command", "Example"][..],
            "app/console/commands/example.rs",
        ),
        ("model", &["make:model", "Flight"], "app/models/flight.rs"),
        (
            "policy",
            &["make:policy", "FlightPolicy"],
            "app/policies/flight_policy.rs",
        ),
        (
            "factory",
            &["make:factory", "FlightFactory"],
            "database/factories/flight_factory.rs",
        ),
        (
            "seeder",
            &["make:seeder", "FlightSeeder"],
            "database/seeders/flight_seeder.rs",
        ),
        (
            "test",
            &["make:test", "FlightTest"],
            "tests/feature/flight_test.rs",
        ),
        (
            "test.unit",
            &["make:test", "FlightTest", "--unit"],
            "tests/unit/flight_test.rs",
        ),
    ] {
        let result = run(&root.0, arguments);
        assert!(result.status.success(), "{arguments:?}: {result:?}");
        assert!(
            fs::read_to_string(root.0.join(path))
                .unwrap()
                .starts_with(&format!("// used {stub}.rs.stub"))
        );
    }
    for (stub, name) in [
        ("migration", "manual_change"),
        ("migration.create", "create_flights_table"),
        ("migration.update", "add_gate_to_flights_table"),
    ] {
        let result = run(&root.0, &["make:migration", name]);
        assert!(result.status.success(), "{result:?}");
        let path = fs::read_dir(root.0.join("database/migrations"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .ends_with(&format!("_{name}.rs"))
            })
            .unwrap();
        assert!(
            fs::read_to_string(path)
                .unwrap()
                .starts_with(&format!("// used {stub}.rs.stub"))
        );
    }
}

#[tokio::test]
async fn artisan_publishes_templates_modified_by_application_listeners() {
    use openrust::{
        contracts::events::{Event, Listener},
        events::{
            Delivery, EventContext, EventFuture, EventResult, ListenerRegistry, ListenerResponse,
        },
        foundation::events::{DiscoverEvents, PublishingStubs},
        testing::TestApp,
    };

    struct Customize;
    impl Listener for Customize {
        fn handle<'a>(
            &'a self,
            event: &'a dyn Event,
            _: &'a mut EventContext<'_>,
        ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
            Box::pin(async move {
                let stubs = event.downcast_ref::<PublishingStubs>().unwrap();
                assert!(stubs.remove("model.rs.stub").is_some());
                assert!(stubs.insert("../escape.stub", "invalid").is_err());
                stubs
                    .insert("example.rs.stub", "// application template\n")
                    .unwrap();
                stubs.add(file!(), "from-file.rs.stub").unwrap();
                Ok(None)
            })
        }
    }
    fn register(registry: &mut ListenerRegistry) -> EventResult<()> {
        registry.listen(
            "foundation.publishing_stubs",
            Arc::new(Customize),
            Delivery::Sync,
        )
    }

    let mut app =
        TestApp::create(|builder| builder.withEvents(DiscoverEvents::new(&[register]))).unwrap();
    app.artisan(["stub:publish"]).await.assertSuccessful();
    assert!(!app.root().join("stubs/model.rs.stub").exists());
    assert!(!app.root().join("escape.stub").exists());
    assert_eq!(
        fs::read(app.root().join("stubs/from-file.rs.stub")).unwrap(),
        fs::read(file!()).unwrap()
    );
    assert_eq!(
        fs::read_to_string(app.root().join("stubs/example.rs.stub")).unwrap(),
        "// application template\n"
    );
}

#[test]
fn artisan_generates_and_registers_providers_with_concurrent_retries() {
    let root = TestRoot::new("make-provider");
    let registry = root.0.join("bootstrap/providers.rs");
    let modules = root.0.join("app/providers/mod.rs");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::create_dir_all(modules.parent().unwrap()).unwrap();
    fs::write(root.0.join("Cargo.toml"), "[package]\n").unwrap();
    fs::write(&modules, include_str!("../app/providers/mod.rs")).unwrap();
    fs::write(&registry, "// custom registry\n").unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .arg("artisan")
            .args(args)
            .env("APP_BASE_PATH", &root.0)
            .output()
            .unwrap()
    };

    let args = ["make:provider", "Reports/ReportServiceProvider.rs"];
    let source = root
        .0
        .join("app/providers/reports/report_service_provider.rs");
    let rejected = run(&args);
    assert_eq!(rejected.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("manually"));
    assert!(!source.exists());

    fs::write(&registry, include_str!("../bootstrap/providers.rs")).unwrap();
    let results = std::thread::scope(|scope| {
        let first = scope.spawn(|| run(&args));
        let second = scope.spawn(|| run(&args));
        [first.join().unwrap(), second.join().unwrap()]
    });
    assert_eq!(
        results
            .iter()
            .filter(|output| output.status.success())
            .count(),
        1
    );
    let original = fs::read_to_string(&source).unwrap();
    assert!(original.contains("fn register") && original.contains("fn boot"));
    let registry_text = fs::read_to_string(&registry).unwrap();
    let entry =
        "Box::new(crate::app::providers::reports::report_service_provider::ReportServiceProvider),";
    assert_eq!(registry_text.matches(entry).count(), 1);
    assert!(
        fs::read_to_string(&modules)
            .unwrap()
            .contains("pub(crate) mod reports;")
    );
    assert!(
        fs::read_to_string(source.parent().unwrap().join("mod.rs"))
            .unwrap()
            .contains("pub(crate) mod report_service_provider;")
    );

    fs::write(&registry, registry_text.replace(entry, "")).unwrap();
    assert!(run(&args).status.success());
    assert_eq!(fs::read_to_string(&source).unwrap(), original);
    assert_eq!(
        fs::read_to_string(&registry)
            .unwrap()
            .matches(entry)
            .count(),
        1
    );

    let multiline = "Box::new(\n            crate::app::providers::reports::report_service_provider::ReportServiceProvider,\n        ),";
    fs::write(
        &registry,
        fs::read_to_string(&registry)
            .unwrap()
            .replace(entry, multiline),
    )
    .unwrap();
    let formatted = fs::read_to_string(&registry).unwrap();

    assert!(run(&["stub:publish"]).status.success());
    let stub = root.0.join("stubs/provider.rs.stub");
    let template = fs::read_to_string(&stub).unwrap();
    fs::write(&stub, format!("// customized\n{template}")).unwrap();
    assert!(
        run(&["make:provider", "Reports/ReportServiceProvider", "-f"])
            .status
            .success()
    );
    assert!(
        fs::read_to_string(&source)
            .unwrap()
            .starts_with("// customized\n")
    );
    assert_eq!(fs::read_to_string(&registry).unwrap(), formatted);
}

impl TestRoot {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("openrust-startup-{}-{name}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn process_entry_selects_a_kernel_before_bootstrap() {
    let root = TestRoot::new("process-entry");
    let binary = env!("CARGO_BIN_EXE_main");
    let run = |arguments: &[&str]| {
        Command::new(binary)
            .current_dir(&root.0)
            .env("APP_BASE_PATH", &root.0)
            .args(arguments)
            .output()
            .unwrap()
    };
    let unknown = ["unknown", "artisan", "list"];
    let output = run(&unknown);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let name = std::path::Path::new(binary).file_name().unwrap();
    let expected = format!("Usage: {} [artisan [COMMAND]]", name.to_string_lossy());
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim_end(), expected);

    fs::write(root.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let output = run(&unknown);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim_end(), expected);
    for arguments in [&[][..], &["artisan", "list"][..]] {
        let output = run(arguments);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.starts_with("Application startup failed: configuration failed"));
        assert!(!error.contains("Usage:"));
    }
}

#[test]
fn compiled_binaries_use_the_declared_root_independent_of_working_directory() {
    let working = TestRoot::new("foreign-cwd");
    fs::write(working.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let installed = TestRoot::new("installed-app");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_main"));
    let relocated = installed.0.join(binary.file_name().unwrap());
    fs::copy(binary, &relocated).unwrap();

    let run = |executable: &std::path::Path, root: Option<&std::path::Path>| {
        let mut command = Command::new(executable);
        command.env_remove("APP_BASE_PATH");
        if let Some(root) = root {
            command.env("APP_BASE_PATH", root);
        }
        command
            .current_dir(&working.0)
            .args(["artisan", "list"])
            .output()
            .unwrap()
    };
    assert!(run(binary, None).status.success());
    assert!(run(&relocated, Some(&installed.0)).status.success());

    fs::write(installed.0.join(".env"), "APP_KEY=\"unclosed\n").unwrap();
    let output = run(&relocated, None);
    if cfg!(feature = "standalone") {
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("configuration failed"));
    } else {
        assert!(output.status.success());
    }
    let output = run(&relocated, Some(&installed.0));
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("configuration failed"));

    let output = run(&relocated, Some(std::path::Path::new("relative")));
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("expected an absolute application root")
    );
}

struct First(Arc<Mutex<Vec<&'static str>>>);
struct Second(Arc<Mutex<Vec<&'static str>>>);

impl ServiceProvider for First {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            assert_eq!(
                app.config().unwrap().get("app.name"),
                Some(json!("test app"))
            );
            app.container().instance(self.0.clone()).unwrap();
            self.0.lock().unwrap().push("first.register");
            Ok(())
        })
    }

    fn boot(&mut self, app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        let service = app.container().make::<str>();
        Box::pin(async move {
            assert_eq!(&*service?, "service from second");
            self.0.lock().unwrap().push("first.boot");
            Ok(())
        })
    }
}

impl ServiceProvider for Second {
    fn register<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .instance::<str>(Arc::from("service from second"))
                .unwrap();
            self.0.lock().unwrap().push("second.register");
            Ok(())
        })
    }

    fn boot(&mut self, _app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        Box::pin(async move {
            self.0.lock().unwrap().push("second.boot");
            Ok(())
        })
    }
}

#[tokio::test]
async fn all_providers_register_before_boot_and_success_is_idempotent() {
    let root = TestRoot::new("providers");
    let events = Arc::new(Mutex::new(Vec::new()));
    let middleware_events = events.clone();
    let application = Application::configure(root.0.clone())
        .withConfig(|_, _| Ok(BTreeMap::from([("app".into(), json!({"name":"test app"}))])))
        .withProviders(vec![
            Box::new(First(events.clone())),
            Box::new(Second(events.clone())),
            Box::new(Second(events.clone())),
        ])
        .withMiddleware(move |_| middleware_events.lock().unwrap().push("middleware"))
        .withRouting(Routing::new(|app| {
            assert_eq!(
                &*app.container().make::<str>().unwrap(),
                "service from second"
            );
            app.container()
                .make::<Mutex<Vec<&'static str>>>()
                .unwrap()
                .lock()
                .unwrap()
                .push("routes");
            Ok(openrust::routing::Router::new())
        }))
        .create()
        .unwrap();
    assert_eq!(application.base_path(), root.0.canonicalize().unwrap());
    assert!(!application.is_booted());
    assert!(events.lock().unwrap().is_empty());
    assert!(
        application
            .container()
            .make::<dyn openrust::contracts::config::Repository>()
            .is_err()
    );
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();
    kernel.bootstrap().await.unwrap();
    assert!(kernel.get_application().is_booted());
    let current = kernel.get_application().config().unwrap();
    assert_eq!(current.get("app.name"), Some(json!("test app")));
    current.set("app.extra", Value::Null).unwrap();
    assert!(kernel.get_application().config().unwrap().has("app.extra"));
    assert_eq!(
        *events.lock().unwrap(),
        [
            "middleware",
            "first.register",
            "second.register",
            "first.boot",
            "second.boot",
            "routes"
        ]
    );
}

#[tokio::test]
async fn grpc_registration_runs_after_providers_and_only_once() {
    let root = TestRoot::new("grpc-registration");
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut application = Application::configure(root.0.clone())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "app".into(),
                json!({"name": "test app"}),
            )]))
        })
        .withProviders(vec![
            Box::new(First(events.clone())),
            Box::new(Second(events.clone())),
        ])
        .withGrpc(|app, _services| {
            let events = app.container().make::<Mutex<Vec<&'static str>>>()?;
            events.lock().unwrap().push("grpc.register");
            Ok(())
        })
        .create()
        .unwrap();
    application.boot().await.unwrap();
    application.boot().await.unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [
            "first.register",
            "second.register",
            "first.boot",
            "second.boot",
            "grpc.register"
        ]
    );
}

struct FailingProvider;

impl ServiceProvider for FailingProvider {
    fn register<'a>(
        &'a mut self,
        _app: &'a Application,
    ) -> openrust::http::HttpFuture<'a, ProviderResult> {
        Box::pin(async move { Ok(()) })
    }

    fn boot(&mut self, _app: &Application) -> openrust::http::HttpFuture<'_, ProviderResult> {
        Box::pin(async { Err(io::Error::other("private probe").into()) })
    }
}

fn web_config(
    base: &std::path::Path,
    environment: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut config = config::load(base, environment)?;
    config.get_mut("app").unwrap()["key"] = "a".repeat(32).into();
    config.get_mut("session").unwrap()["driver"] = "array".into();
    Ok(config)
}

fn listener_config(
    base: &std::path::Path,
    _: &Environment,
) -> Result<BTreeMap<String, Value>, ConfigError> {
    let port: u16 = fs::read_to_string(base.join("port"))
        .unwrap()
        .parse()
        .unwrap();
    Ok(BTreeMap::from([(
        "app".into(),
        json!({"host":"127.0.0.1", "port":port}),
    )]))
}

#[tokio::test]
async fn start_bootstraps_through_the_http_kernel() {
    let root = TestRoot::new("start-before-boot");
    let application = Application::configure(root.0.clone())
        .withProviders(vec![Box::new(FailingProvider)])
        .create()
        .unwrap();

    let error = application.start().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::ProviderBootFailed { .. })
    ));
}

#[tokio::test]
async fn failed_provider_prevents_listening_and_is_not_retried() {
    let root = TestRoot::new("provider-failure");
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    fs::write(
        root.0.join("port"),
        occupied.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let application = Application::configure(root.0.clone())
        .withConfig(listener_config)
        .withProviders(vec![Box::new(FailingProvider)])
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);

    // An occupied port would fail first if the real listener bound before bootstrap.
    let error = kernel.listen().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::ProviderBootFailed { .. })
    ));
    assert!(error.to_string().contains("FailingProvider"));
    assert!(!error.to_string().contains("private probe"));
    assert!(!kernel.get_application().is_booted());
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::PreviousBootFailed)
    ));
    let (parts, _) = http_request().into_parts();
    let mut request = Request::from_parts(parts, Body::empty());
    assert_eq!(
        kernel.handle(&mut request).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
}

fn http_request() -> axum::extract::Request {
    axum::extract::Request::new(Body::empty())
}

#[tokio::test]
async fn welcome_route_reads_the_named_view_from_the_application_root() {
    let root = TestRoot::new("named-view");
    let views = root.0.join("resources/views");
    let custom_views = root.0.join("custom-views");
    fs::create_dir_all(&views).unwrap();
    fs::create_dir_all(&custom_views).unwrap();
    let welcome = views.join("welcome.html");
    let custom_welcome = custom_views.join("welcome.html");
    fs::write(&welcome, "default welcome").unwrap();
    fs::write(&custom_welcome, "custom welcome").unwrap();

    let application = Application::configure(root.0.clone())
        .withConfig(|base, env| {
            let mut config = web_config(base, env)?;
            config.get_mut("view").unwrap()["paths"] = json!(["custom-views", "resources/views"]);
            Ok(config)
        })
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    let (parts, body) = http_request().into_parts();
    let (_, body) = kernel
        .handle(&mut Request::from_parts(parts, body))
        .await
        .into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "custom welcome"
    );
    fs::remove_file(custom_welcome).unwrap();

    for html in ["<h1>Welcome</h1>", "<h1>Updated</h1>"] {
        fs::write(&welcome, html).unwrap();
        let (parts, body) = http_request().into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let (parts, body) = response.into_parts();
        assert_eq!(parts.headers["content-type"], "text/html; charset=utf-8");
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), html);
    }

    fs::remove_file(welcome).unwrap();
    let (parts, body) = http_request().into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn public_mount_serves_files_after_application_routes() {
    use openrust::{
        contracts::routing::Registrar as _,
        http::{HttpError, HttpFuture, Method, Response},
        routing::Router,
        support::facades::Route,
        view::view,
    };

    fn not_found(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async { Err(HttpError::NotFound) })
    }

    let root = TestRoot::new("static-mount");
    let public = root.0.join("public");
    let public_storage = root.0.join("storage/app/public");
    let private_storage = root.0.join("storage/app/private");
    fs::create_dir_all(public.join("css")).unwrap();
    fs::create_dir_all(&public_storage).unwrap();
    fs::create_dir_all(&private_storage).unwrap();
    fs::write(public_storage.join("avatar.png"), "public avatar").unwrap();
    fs::create_dir_all(public.join("storage")).unwrap();
    fs::write(public.join("storage/avatar.png"), "website avatar").unwrap();
    fs::write(public_storage.join("protected.txt"), "disk mount shadow").unwrap();
    fs::write(private_storage.join("secret.txt"), "private file").unwrap();
    fs::create_dir_all(root.0.join("resources/views")).unwrap();
    fs::write(
        root.0.join("resources/views/welcome.html"),
        "application home",
    )
    .unwrap();
    fs::write(root.0.join("outside.txt"), "outside the mount").unwrap();
    for (path, body) in [
        ("logo.png", "image"),
        ("css/app.css", "body{}"),
        ("css/index.html", "directory index"),
        ("index.html", "static home"),
        ("protected.txt", "static shadow"),
        ("post-only.txt", "static shadow"),
        ("你好.txt", "encoded name"),
    ] {
        fs::write(public.join(path), body).unwrap();
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("../outside.txt", public.join("external.txt")).unwrap();
        std::os::unix::fs::symlink("../private/secret.txt", public_storage.join("linked.txt"))
            .unwrap();
    }

    let application = Application::configure(root.0.clone())
        .withConfig(web_config)
        .withRouting(
            Routing::new(|_| {
                let mut router = Router::new();
                router.add_route(&[Method::POST], "/post-only.txt", Arc::new(not_found))?;
                Ok(router)
            })
            .web(|| {
                Route::staticDisk("public")?;
                Route::get("/", || view("welcome"))?;
                Route::staticPublic()?;
                Route::get("/protected.txt", not_found)?;
                Route::get("/storage/protected.txt", not_found)?;
                Route::r#static("/assets", "./public/css")
            })
            .health("/up"),
        )
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    let config = kernel.get_application().config().unwrap();
    let app_url = config.get("app.url").unwrap();
    let disk_url = format!(
        "{}/storage",
        app_url.as_str().unwrap().trim_end_matches('/')
    );
    assert_eq!(
        config.get("filesystems.disks.public.url"),
        Some(json!(disk_url))
    );

    for (method, uri, body, mime) in [
        (
            Method::GET,
            "/",
            "application home",
            "text/html; charset=utf-8",
        ),
        (Method::GET, "/logo.png", "image", "image/png"),
        (
            Method::GET,
            "/storage/avatar.png",
            "public avatar",
            "image/png",
        ),
        (Method::GET, "/css/app.css?version=1", "body{}", "text/css"),
        (Method::HEAD, "/css/app.css", "", "text/css"),
        (Method::GET, "/assets/app.css", "body{}", "text/css"),
        (
            Method::GET,
            "/%E4%BD%A0%E5%A5%BD.txt",
            "encoded name",
            "text/plain",
        ),
    ] {
        let (parts, request_body) = axum::http::Request::builder()
            .method(method.clone())
            .uri(uri)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel
            .handle(&mut Request::from_parts(parts, request_body))
            .await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        let (parts, response_body) = response.into_parts();
        assert_eq!(parts.headers["content-type"], mime, "{uri}");
        if method == Method::HEAD {
            assert_eq!(parts.headers["content-length"], "6");
        }
        assert_eq!(
            axum::body::to_bytes(response_body, 1024).await.unwrap(),
            body,
            "{uri}"
        );
    }

    for (method, uri, status) in [
        (Method::GET, "/up", StatusCode::OK),
        (Method::GET, "/css", StatusCode::NOT_FOUND),
        (Method::GET, "/css/", StatusCode::NOT_FOUND),
        (Method::GET, "/logo.png/", StatusCode::NOT_FOUND),
        (Method::GET, "/public/logo.png", StatusCode::NOT_FOUND),
        (Method::GET, "/avatar.png", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/secret.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/linked.txt", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/storage/%2e%2e/private/secret.txt",
            StatusCode::NOT_FOUND,
        ),
        (Method::GET, "/missing.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/protected.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/storage/protected.txt", StatusCode::NOT_FOUND),
        (
            Method::GET,
            "/post-only.txt",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (Method::GET, "/assets-other/app.css", StatusCode::NOT_FOUND),
        (Method::GET, "/../outside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/%2e%2e/outside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/..%5coutside.txt", StatusCode::NOT_FOUND),
        (Method::GET, "/external.txt", StatusCode::NOT_FOUND),
        (Method::POST, "/logo.png", StatusCode::NOT_FOUND),
    ] {
        let (parts, body) = axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), status, "{uri}");
        if status == StatusCode::METHOD_NOT_ALLOWED {
            assert_eq!(response.headers()["allow"], "POST");
        }
    }

    let application = Application::configure(root.0.clone())
        .withConfig(web_config)
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut reversed = Kernel::new(application);
    reversed.bootstrap().await.unwrap();
    let (parts, body) = axum::http::Request::get("/storage/avatar.png")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = reversed.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let (_, body) = response.into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "website avatar"
    );

    let app = Application::configure(root.0.clone()).create().unwrap();
    let mut unmounted = Kernel::new(app);
    unmounted.bootstrap().await.unwrap();
    let (parts, body) = axum::http::Request::get("/logo.png")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = unmounted
        .handle(&mut Request::from_parts(parts, body))
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_mount_revalidates_get_and_head_using_file_modification_time() {
    use std::time::{Duration, UNIX_EPOCH};

    let root = TestRoot::new("static-revalidation");
    let public = root.0.join("public");
    fs::create_dir(&public).unwrap();
    let path = public.join("app.css");
    let set_file = |body: &str, millis: u64| {
        fs::write(&path, body).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_millis(millis)),
            )
            .unwrap();
    };
    set_file("body{}", 1_700_000_000_123);
    let application = Application::configure(root.0.clone())
        .withConfig(web_config)
        .withRouting(Routing::default().web(web::register))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    kernel.bootstrap().await.unwrap();

    const MODIFIED: &str = "Tue, 14 Nov 2023 22:13:20 GMT";
    for (method, since, tag, status, content) in [
        ("GET", None, None, StatusCode::OK, "body{}"),
        ("GET", Some(MODIFIED), None, StatusCode::NOT_MODIFIED, ""),
        ("HEAD", Some(MODIFIED), None, StatusCode::NOT_MODIFIED, ""),
        ("GET", Some("invalid"), None, StatusCode::OK, "body{}"),
        (
            "GET",
            Some(MODIFIED),
            Some("\"other\""),
            StatusCode::OK,
            "body{}",
        ),
        ("GET", None, Some("*"), StatusCode::NOT_MODIFIED, ""),
    ] {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri("/app.css");
        if let Some(since) = since {
            request = request.header("if-modified-since", since);
        }
        if let Some(tag) = tag {
            request = request.header("if-none-match", tag);
        }
        let (parts, body) = request.body(Body::empty()).unwrap().into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        assert_eq!(response.status(), status, "{method} {since:?} {tag:?}");
        assert_eq!(response.headers()["last-modified"], MODIFIED);
        if status == StatusCode::NOT_MODIFIED {
            assert!(!response.headers().contains_key("content-length"));
            assert!(!response.headers().contains_key("content-type"));
        }
        let (_, body) = response.into_parts();
        assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), content);
    }

    set_file("body{color:red}", 1_700_000_002_123);
    let (parts, body) = axum::http::Request::get("/app.css")
        .header("if-modified-since", MODIFIED)
        .body(Body::empty())
        .unwrap()
        .into_parts();
    let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["last-modified"],
        "Tue, 14 Nov 2023 22:13:22 GMT"
    );
    let (_, body) = response.into_parts();
    assert_eq!(
        axum::body::to_bytes(body, 1024).await.unwrap(),
        "body{color:red}"
    );
}

#[tokio::test]
async fn public_mounts_follow_the_application_and_disk_configuration() {
    use openrust::support::facades::Route;

    let root = TestRoot::new("configured-mounts");
    for (directory, contents) in [
        ("assets", "application assets"),
        ("uploads", "public disk"),
        ("override", "explicit application path"),
        ("public", "old default"),
        ("storage/app/public", "old disk default"),
    ] {
        let directory = root.0.join(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("probe.txt"), contents).unwrap();
    }
    let environment = root.0.join(".env");
    let settings = "APP_URL=http://localhost:8000\nPUBLIC_PATH=assets\nPUBLIC_URL_PREFIX=/assets\nPUBLIC_STORAGE_ROOT=uploads\n";
    let application = |register: fn() -> Result<(), openrust::routing::RoutingError>| {
        Application::configure(root.0.clone())
            .withConfig(web_config)
            .withRouting(Routing::default().web(register))
            .create()
            .unwrap()
    };

    async fn check(kernel: &Kernel, path: &str, expected: Option<&str>) {
        let (parts, body) = axum::http::Request::get(path)
            .body(Body::empty())
            .unwrap()
            .into_parts();
        let response = kernel.handle(&mut Request::from_parts(parts, body)).await;
        if let Some(expected) = expected {
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let (_, body) = response.into_parts();
            assert_eq!(axum::body::to_bytes(body, 1024).await.unwrap(), expected);
        } else {
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }
    }

    fs::write(
        &environment,
        format!(
            "{}PUBLIC_STORAGE_URL=https://LOCALHOST:443/storage/tmp/\n",
            settings.replace("http://localhost:8000", "https://localhost")
        ),
    )
    .unwrap();
    let mut kernel = Kernel::new(application(Route::staticPublic));
    kernel.bootstrap().await.unwrap();
    check(&kernel, "/assets/probe.txt", Some("application assets")).await;
    check(&kernel, "/storage/tmp/probe.txt", None).await;
    check(&kernel, "/storage/probe.txt", None).await;

    let mut kernel = Kernel::new(application(web::register));
    kernel.bootstrap().await.unwrap();
    check(&kernel, "/assets/probe.txt", Some("application assets")).await;
    check(&kernel, "/storage/tmp/probe.txt", Some("public disk")).await;
    check(&kernel, "/storage/tmp-other/probe.txt", None).await;
    check(&kernel, "/probe.txt", None).await;
    check(&kernel, "/storage/probe.txt", None).await;

    let cdn = format!("{settings}PUBLIC_STORAGE_URL=https://cdn.example/media\n");
    fs::write(&environment, &cdn).unwrap();
    let mut kernel = Kernel::new(application(web::register));
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::RoutingFailed { .. })
    ));

    fs::write(
        &environment,
        format!("{settings}PUBLIC_STORAGE_URL=/local-media\n"),
    )
    .unwrap();
    let mut app = application(web::register);
    app.usePublicPath("override");
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    check(
        &kernel,
        "/assets/probe.txt",
        Some("explicit application path"),
    )
    .await;
    check(&kernel, "/local-media/probe.txt", Some("public disk")).await;
    check(&kernel, "/storage/probe.txt", None).await;

    fs::write(&environment, "APP_PORT=9081\nPUBLIC_STORAGE_ROOT=uploads\n").unwrap();
    let mut kernel = Kernel::new(application(web::register));
    kernel.bootstrap().await.unwrap();
    let config = kernel.get_application().config().unwrap();
    assert_eq!(config.get("app.port"), Some(json!(9081)));
    assert_eq!(config.get("app.url"), Some(json!("http://localhost:9081")));
    assert_eq!(
        config.get("filesystems.disks.public.url"),
        Some(json!("http://localhost:9081/storage"))
    );
    check(&kernel, "/storage/probe.txt", Some("public disk")).await;
}

#[test]
fn environment_file_selection_and_redaction() {
    let root = TestRoot::new("environment");
    fs::write(root.0.join(".env"), "OPENRUST_TEST_PROBE=default\n").unwrap();
    fs::write(root.0.join(".env.test"), "OPENRUST_TEST_PROBE=selected\n").unwrap();
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, Some("test")).unwrap();
    assert_eq!(environment.get("OPENRUST_TEST_PROBE"), Some("selected"));
    let environment = LoadEnvironmentVariables::bootstrap(&root.0, Some("missing")).unwrap();
    assert_eq!(environment.get("OPENRUST_TEST_PROBE"), Some("default"));
    assert!(matches!(
        LoadEnvironmentVariables::bootstrap(&root.0, Some("../test")),
        Err(ConfigError::InvalidEnvironmentName)
    ));
    fs::write(root.0.join(".env.test"), "APP_KEY=\"private-probe\n").unwrap();
    let error = LoadEnvironmentVariables::bootstrap(&root.0, Some("test"))
        .err()
        .unwrap();
    assert!(matches!(error, ConfigError::InvalidEnvironmentFile { .. }));
    assert!(!error.to_string().contains("private-probe"));
}

#[tokio::test]
async fn invalid_routes_fail_bootstrap_before_the_listener_opens() {
    let root = TestRoot::new("invalid-route");
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    fs::write(
        root.0.join("port"),
        occupied.local_addr().unwrap().port().to_string(),
    )
    .unwrap();
    let application = Application::configure(root.0.clone())
        .withConfig(listener_config)
        .withRouting(Routing::new(|_| {
            use openrust::{contracts::routing::Registrar as _, routing::Router};
            let mut router = Router::new();
            router.add_route(&[], "/", Arc::new(unreachable_handler))?;
            Ok(router)
        }))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);

    let error = kernel.listen().await.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<BootstrapError>(),
        Some(BootstrapError::RoutingFailed { .. })
    ));
    assert!(matches!(
        kernel.bootstrap().await,
        Err(BootstrapError::PreviousBootFailed)
    ));
}

fn unreachable_handler(
    _: &mut Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    unreachable!("invalid route must not be dispatched")
}
