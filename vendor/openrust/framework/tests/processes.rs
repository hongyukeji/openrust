use openrust::process::{Command, Error, Factory};
use std::time::Duration;

#[tokio::test]
async fn fake_configuration_and_results_are_isolated() {
    let left = Factory::new();
    let right = Factory::new();
    left.fakeCommand("tool *", Factory::result("left", "stderr", 3));
    right.fakeCommand("tool *", Factory::result("right", "", 0));
    left.preventStrayProcesses(true);
    right.preventStrayProcesses(true);

    let left_run = left
        .command(["tool", "value with spaces"])
        .env([("MODE", "test")])
        .removeEnv("HOME")
        .input(b"payload")
        .run();
    let right_run = right.run(["tool", "value with spaces"]);
    let (left_result, right_result) = tokio::join!(left_run, right_run);
    let left_result = left_result.unwrap();
    assert_eq!(left_result.exitCode(), 3);
    assert_eq!(
        left_result.command(),
        &Command::from(["tool", "value with spaces"])
    );
    assert_eq!(left_result.errorOutput(), b"stderr");
    assert!(matches!(left_result.throw(), Err(Error::Failed(_))));
    assert_eq!(right_result.unwrap().output(), b"right");
    left.assertRan(|process| {
        process
            .environment
            .get(std::ffi::OsStr::new("MODE"))
            .is_some_and(|mode| mode.as_ref().is_some_and(|mode| mode == "test"))
            && process.environment.get(std::ffi::OsStr::new("HOME")) == Some(&None)
            && process.input == b"payload"
    });
    right.assertRanTimes(|_| true, 1);
    assert!(matches!(
        left.run(["other"]).await,
        Err(Error::StrayProcess)
    ));
    left.assertDidntRun(|process| process.command == Command::from(["other"]));
}

#[tokio::test]
async fn uses_registered_fakes_and_keeps_the_recording_across_reconfiguration() {
    let factory = Factory::new();
    factory.preventStrayProcesses(true);
    factory.fakeSequence(
        "example",
        [
            Ok(Factory::result("first", "", 0)),
            Ok(Factory::result("second", "", 1)),
        ],
    );
    assert_eq!(factory.run("example").await.unwrap().output(), b"first");
    assert_eq!(factory.run("example").await.unwrap().exitCode(), 1);
    assert!(matches!(
        factory.run("example").await,
        Err(Error::SequenceExhausted)
    ));
    factory.fake(|_| Some(Ok(Factory::result("replaced", "", 0))));
    assert_eq!(factory.run("example").await.unwrap().output(), b"replaced");
    factory.assertRanTimes(|process| process.command == Command::from("example"), 4);
}

#[tokio::test]
async fn artisan_resolves_process_fakes_from_each_application() {
    use openrust::{
        console::{
            Application as Artisan, Command as ArtisanCommand, CommandDefinition, ConsoleError,
            ConsoleFuture, Input, Output,
        },
        contracts::{container::Container as _, foundation::Application as _},
        foundation::Application,
        support::{ProviderResult, ServiceProvider, facades::Process},
    };

    struct Stub(u8);
    impl ServiceProvider for Stub {
        fn boot<'a>(
            &'a mut self,
            app: &'a Application,
        ) -> openrust::http::HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                let factory = app.container().make::<Factory>()?;
                factory.fakeCommand("example", Factory::result("", "", i32::from(self.0)));
                factory.preventStrayProcesses(true);
                Ok(())
            })
        }
    }

    struct Probe(CommandDefinition);
    impl ArtisanCommand for Probe {
        fn definition(&self) -> &CommandDefinition {
            &self.0
        }

        fn handle<'a>(
            &'a self,
            _: &'a Application,
            _: &'a Input,
            _: &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
            Box::pin(async {
                let result = Process::run("example")
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                Ok(result.exitCode())
            })
        }
    }

    fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
        artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
            "process:probe",
        )))))
    }

    async fn invoke(code: u8) -> i32 {
        Application::configure(std::env::temp_dir())
            .withProviders(vec![Box::new(Stub(code))])
            .withCommands(register)
            .create()
            .unwrap()
            .handleCommand(&["process:probe".into()])
            .await
            .unwrap()
    }

    let (left, right) = tokio::join!(invoke(4), invoke(7));
    assert_eq!((left, right), (4, 7));
}

#[cfg(unix)]
mod unix {
    use super::*;
    use openrust::process::{OutputStream, Signal};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::SystemTime,
    };

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "openrust-process-{}-{}",
                std::process::id(),
                fastrand::u64(..)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn runs_shell_and_literal_arguments_with_cwd_env_and_input() {
        let factory = Factory::new();
        let directory = TestDirectory::new();
        let result = factory
            .command(r#"printf '%s\n' "$MODE"; pwd -P; cat; printf 'warning' >&2; exit 7"#)
            .path(&directory.0)
            .env([("MODE", "test")])
            .input(b"from stdin")
            .run()
            .await
            .unwrap();
        assert_eq!(
            result.output(),
            format!("test\n{}\nfrom stdin", directory.0.display()).as_bytes()
        );
        assert_eq!(result.errorOutput(), b"warning");
        assert_eq!(result.exitCode(), 7);
        assert!(result.seeInOutput("from stdin"));
        assert!(result.seeInErrorOutput("warning"));
        assert!(result.throwIf(false).is_ok());

        let literal = factory
            .run(["/usr/bin/printf", "%s", "literal $(exit 9); a b"])
            .await
            .unwrap();
        assert_eq!(literal.output(), b"literal $(exit 9); a b");
        let quiet = factory
            .command("printf out; printf err >&2")
            .quietly()
            .run()
            .await
            .unwrap();
        assert!(quiet.output().is_empty() && quiet.errorOutput().is_empty());
        assert!(
            matches!(factory.run(["/missing-openrust-test-executable"]).await, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
        );
    }

    #[tokio::test]
    async fn streams_output_once_and_returns_the_complete_result() {
        let factory = Factory::new();
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let captured = chunks.clone();
        let mut process = factory
            .command("printf first; sleep 0.1; printf second; printf problem >&2; sleep 0.1")
            .startWithOutput(move |stream, bytes| {
                captured.lock().unwrap().push((stream, bytes.to_vec()))
            })
            .unwrap();
        assert!(process.id().is_some());
        let first = wait_for_output(&mut process, b"first").await;
        assert!(first.starts_with(b"first"));
        let result = process.wait().await.unwrap();
        assert_eq!(result.output(), b"firstsecond");
        assert_eq!(result.errorOutput(), b"problem");
        let chunks = chunks.lock().unwrap();
        let stdout: Vec<u8> = chunks
            .iter()
            .filter(|(stream, _)| *stream == OutputStream::Stdout)
            .flat_map(|(_, bytes)| bytes.iter().copied())
            .collect();
        assert_eq!(stdout, result.output());
    }

    #[tokio::test]
    async fn timeouts_and_cancellation_kill_the_unix_process_group() {
        let factory = Factory::new();
        let directory = TestDirectory::new();
        let script = "(sleep 0.45; printf survived > marker) & printf ready; wait";
        let started = SystemTime::now();
        let result = factory
            .command(script)
            .path(&directory.0)
            .timeout(Duration::from_millis(100))
            .run()
            .await;
        assert!(matches!(result, Err(Error::TimedOut { idle: false })));
        assert!(started.elapsed().unwrap() < Duration::from_secs(2));

        let result = factory
            .command(script)
            .path(&directory.0)
            .idleTimeout(Duration::from_millis(100))
            .run()
            .await;
        assert!(matches!(result, Err(Error::TimedOut { idle: true })));
        let mut process = factory.command(script).path(&directory.0).start().unwrap();
        wait_for_output(&mut process, b"ready").await;
        drop(process);
        tokio::time::sleep(Duration::from_millis(550)).await;
        assert!(!directory.0.join("marker").exists());
    }

    #[tokio::test]
    async fn stop_allows_term_traps_and_normal_exit_kills_remaining_group_members() {
        let factory = Factory::new();
        let directory = TestDirectory::new();
        let mut process = factory
            .command(
                "trap 'printf stopped; exit 0' TERM; printf ready; while :; do sleep 0.05; done",
            )
            .start()
            .unwrap();
        wait_for_output(&mut process, b"ready").await;
        let result = process.stop(Duration::from_secs(1)).await.unwrap();
        assert!(result.successful());
        assert!(result.seeInOutput("stopped"));

        let mut process = factory
            .command("trap 'exit 4' INT; printf ready; while :; do sleep 0.05; done")
            .start()
            .unwrap();
        wait_for_output(&mut process, b"ready").await;
        process.signal(Signal::Interrupt).await.unwrap();
        assert_eq!(process.wait().await.unwrap().exitCode(), 4);

        let result = factory
            .command("(trap '' TERM; sleep 0.3; printf survived > marker) & exit 0")
            .path(&directory.0)
            .run()
            .await
            .unwrap();
        assert!(result.successful());
        tokio::time::sleep(Duration::from_millis(420)).await;
        assert!(!directory.0.join("marker").exists());
    }

    #[tokio::test]
    async fn pipes_stop_on_failure_and_pools_run_named_children_together() {
        let factory = Factory::new();
        let piped = factory
            .pipe(["printf 'a b'", "tr 'a-z' 'A-Z'"])
            .await
            .unwrap();
        assert_eq!(piped.output(), b"A B");
        let failed = factory
            .pipe(["printf failure >&2; exit 4", "printf unexpected"])
            .await
            .unwrap();
        assert_eq!(failed.errorOutput(), b"failure");
        assert_eq!(failed.exitCode(), 4);

        let directory = TestDirectory::new();
        let results = factory
            .concurrently(|pool| {
                pool.r#as(
                    "first",
                    "while [ ! -f started ]; do sleep 0.02; done; printf first",
                    |command| command.path(&directory.0).timeout(Duration::from_secs(2)),
                )
                .r#as(
                    "second",
                    "touch started; printf second; exit 7",
                    |command| command.path(&directory.0),
                );
            })
            .await
            .unwrap();
        assert_eq!(
            results.keys().map(String::as_str).collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert_eq!(results["first"].as_ref().unwrap().output(), b"first");
        assert_eq!(results["second"].as_ref().unwrap().exitCode(), 7);
    }

    async fn wait_for_output(
        process: &mut openrust::process::InvokedProcess,
        expected: &[u8],
    ) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut observed = Vec::new();
            loop {
                observed.extend(process.latestOutput());
                if observed.starts_with(expected) {
                    return observed;
                }
                assert!(
                    process.running(),
                    "process exited before producing expected output"
                );
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap()
    }
}
