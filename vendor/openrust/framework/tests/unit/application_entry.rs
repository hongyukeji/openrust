use super::console::ConsoleInvocation;
use super::prelude::*;

#[test]
fn prelude_json_is_a_response() {
    let response = Json("ok").into_response();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/json");
}

#[test]
fn console_environment_argument_is_removed_before_command_dispatch() {
    let parse = |args: &[&str]| {
        ConsoleInvocation::parse(args.iter().map(|arg| (*arg).to_owned()), "serve").into_parts()
    };
    assert_eq!(parse(&[]), ("serve".into(), Vec::new(), None));
    assert_eq!(
        ConsoleInvocation::parse(["--env=local".to_owned()], "start").into_parts(),
        ("start".into(), Vec::new(), Some("local".into()))
    );
    assert_eq!(
        parse(&["--env", "local", "config:cache"]),
        ("config:cache".into(), Vec::new(), Some("local".into()))
    );
    assert_eq!(
        parse(&["help", "--env=staging", "detail"]),
        ("help".into(), vec!["detail".into()], Some("staging".into()))
    );
    assert_eq!(
        parse(&["--envelope=mail", "--env"]),
        ("--envelope=mail".into(), Vec::new(), None)
    );
    assert_eq!(
        parse(&["-n", "--env=production", "migrate", "down"]),
        (
            "migrate".into(),
            vec!["down".into(), "--no-interaction".into()],
            Some("production".into()),
        )
    );
}

#[test]
fn console_invocation_preserves_global_and_unknown_command_boundaries() {
    let parse =
        |args: &[&str]| ConsoleInvocation::parse(args.iter().map(|arg| (*arg).to_owned()), "list");
    let help = parse(&["--env=staging", "--help", "build", "--env=ignored"]);
    assert_eq!(help.name(), "help");
    assert_eq!(help.arguments(), ["build"]);
    assert_eq!(help.environment(), Some("staging"));

    let build = parse(&["-n", "--env", "local", "build", "--platform=linux/arm64"]);
    assert_eq!(build.name(), "build");
    assert_eq!(build.arguments(), ["--platform=linux/arm64"]);
    assert_eq!(build.environment(), Some("local"));

    let unknown = parse(&[
        "custom:entry",
        "--unknown",
        "-n",
        "--env",
        "testing",
        "value",
    ]);
    assert_eq!(unknown.name(), "custom:entry");
    assert_eq!(unknown.arguments(), ["--unknown", "-n", "value"]);
    assert_eq!(unknown.environment(), Some("testing"));

    let key = parse(&["--no-interaction", "key:generate", "--force"]);
    assert_eq!(key.arguments(), ["--force", "--no-interaction"]);
    assert_eq!(parse(&[]).name(), "list");
}

#[test]
fn artisan_prefix_changes_the_default_only_at_the_command_boundary() {
    let parse = |args: &[&str]| {
        ConsoleInvocation::parse(args.iter().map(|arg| (*arg).to_owned()), "start").into_parts()
    };
    assert_eq!(parse(&[]), ("start".into(), Vec::new(), None));
    assert_eq!(parse(&["artisan"]), ("list".into(), Vec::new(), None));
    assert_eq!(
        parse(&["--env", "artisan"]),
        ("start".into(), Vec::new(), Some("artisan".into()))
    );
    assert_eq!(
        parse(&["--env=artisan"]),
        ("start".into(), Vec::new(), Some("artisan".into()))
    );
    assert_eq!(
        parse(&["--env=staging", "artisan", "--help", "migrate"]),
        (
            "help".into(),
            vec!["migrate".into()],
            Some("staging".into())
        )
    );
    assert_eq!(
        parse(&[
            "artisan",
            "migrate",
            "--env",
            "production",
            "--force",
            "--no-interaction"
        ]),
        (
            "migrate".into(),
            vec!["--force".into(), "--no-interaction".into()],
            Some("production".into()),
        )
    );
    assert_eq!(
        parse(&["-n", "artisan", "migrate", "--database", "artisan"]),
        (
            "migrate".into(),
            vec![
                "--database".into(),
                "artisan".into(),
                "--no-interaction".into()
            ],
            None,
        )
    );
    assert_eq!(
        parse(&["help", "artisan"]),
        ("help".into(), vec!["artisan".into()], None)
    );
    assert_eq!(
        parse(&["artisan", "artisan", "literal"]),
        ("artisan".into(), vec!["literal".into()], None)
    );
}

struct DotenvFixture;

impl ApplicationDefinition for DotenvFixture {
    type Config = NoConfig;

    fn default_providers() -> crate::support::DefaultProviders {
        crate::support::DefaultProviders::empty()
    }
}

#[test]
#[expect(
    clippy::result_large_err,
    reason = "figment::Jail 要求回调返回其固定错误类型"
)]
fn application_exposes_the_dotenv_path_selected_by_bootstrap() {
    let _environment = crate::lock_test_env();
    figment::Jail::expect_with(|jail| {
        jail.clear_env();
        jail.create_dir("bootstrap")?;
        jail.create_dir("bootstrap/cache")?;
        jail.create_file(".env", "APP_NAME=default\n")?;
        jail.create_file(".env.build", "APP_NAME=selected\n")?;
        let error = |error: crate::Error| figment::Error::from(error.to_string());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| figment::Error::from(error.to_string()))?;
        runtime.block_on(async {
            let app =
                crate::lifecycle::bootstrap_for_console::<DotenvFixture>("build", Some("build"))
                    .await
                    .map_err(error)?;
            assert_eq!(app.config().app.name, "selected");
            assert_eq!(
                app.dotenv_path().and_then(std::path::Path::file_name),
                Some(std::ffi::OsStr::new(".env.build"))
            );
            app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await
                .map_err(error)?;

            let runtime = crate::foundation::ProviderBuilder::new()
                .start(figment::Figment::new())
                .await
                .map_err(error)?;
            let app = Application::from_provider_runtime(runtime).map_err(error)?;
            assert!(app.dotenv_path().is_none());
            app.shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await
                .map_err(error)
        })
    });
}
