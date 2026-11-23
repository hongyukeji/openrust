use openrust::process::{Command, Error, Factory};

#[tokio::test]
async fn fake_configuration_and_results_are_isolated() {
    let left = Factory::new();
    let right = Factory::new();
    left.fake_command("tool *", Factory::result("left", "stderr", 3));
    right.fake_command("tool *", Factory::result("right", "", 0));
    left.prevent_stray_processes(true);
    right.prevent_stray_processes(true);

    let left_run = left
        .command(["tool", "value with spaces"])
        .env([("MODE", "test")])
        .remove_env("HOME")
        .input(b"payload")
        .run();
    let right_run = right.run(["tool", "value with spaces"]);
    let (left_result, right_result) = tokio::join!(left_run, right_run);
    let left_result = left_result.unwrap();
    assert_eq!(left_result.exit_code(), 3);
    assert_eq!(
        left_result.command(),
        &Command::from(["tool", "value with spaces"])
    );
    assert_eq!(left_result.error_output(), b"stderr");
    assert!(matches!(left_result.throw(), Err(Error::Failed(_))));
    assert_eq!(right_result.unwrap().output(), b"right");
    left.assert_ran(|process, result| {
        result.is_some_and(|result| result.exit_code() == 3)
            && process
                .environment
                .get(std::ffi::OsStr::new("MODE"))
                .is_some_and(|mode| mode.as_ref().is_some_and(|mode| mode == "test"))
            && process.environment.get(std::ffi::OsStr::new("HOME")) == Some(&None)
            && process.input == b"payload"
    });
    right.assert_ran_times(|_, _| true, 1);
    assert!(matches!(
        left.run(["other"]).await,
        Err(Error::StrayProcess)
    ));
    left.assert_didnt_run(|process, _| process.command == Command::from(["other"]));
}

#[tokio::test]
async fn uses_registered_fakes_and_keeps_the_recording_across_reconfiguration() {
    let factory = Factory::new();
    factory.prevent_stray_processes(true);
    factory.fake_sequence(
        "example",
        [
            Ok(Factory::result("first", "", 0)),
            Ok(Factory::result("second", "", 1)),
        ],
    );
    assert_eq!(factory.run("example").await.unwrap().output(), b"first");
    assert_eq!(factory.run("example").await.unwrap().exit_code(), 1);
    assert!(matches!(
        factory.run("example").await,
        Err(Error::SequenceExhausted)
    ));
    factory.fake(|_| Some(Ok(Factory::result("replaced", "", 0))));
    assert_eq!(factory.run("example").await.unwrap().output(), b"replaced");
    factory.assert_ran_times(|process, _| process.command == Command::from("example"), 4);
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
                factory.fake_command("example", Factory::result("", "", i32::from(self.0)));
                factory.prevent_stray_processes(true);
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
                Ok(result.exit_code())
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
            .with_providers(vec![Box::new(Stub(code))])
            .with_commands(register)
            .create()
            .unwrap()
            .handle_command(&["process:probe".into()])
            .await
            .unwrap()
    }

    let (left, right) = tokio::join!(invoke(4), invoke(7));
    assert_eq!((left, right), (4, 7));
}

#[tokio::test]
async fn scripted_fakes_play_ordered_chunks_and_track_signals() {
    use openrust::process::{OutputStream, Signal};
    use std::sync::{Arc, Mutex};

    let factory = Factory::new();
    factory.prevent_stray_processes(true);
    factory.fake_command(
        "script",
        Factory::describe()
            .id(314)
            .runs_for(2)
            .output("replaced")
            .error_output("diagnostic")
            .replace_output("ready\n\n")
            .output("finished")
            .exit_code(7)
            .result(),
    );
    let observed = Arc::new(Mutex::new(Vec::new()));
    let capture = observed.clone();
    let mut process = factory
        .command("script")
        .start_with_output(move |stream, bytes| {
            capture.lock().unwrap().push((stream, bytes.to_vec()));
        })
        .unwrap();
    assert_eq!(process.id(), Some(314));
    assert!(process.running());
    assert_eq!(process.latest_error_output(), b"diagnostic\n");
    process.signal(Signal::Raw(10)).await.unwrap();
    assert!(process.has_received_signal(Signal::Raw(10)));
    assert!(!process.has_received_signal(Signal::Kill));
    let mut staged = Vec::new();
    assert!(
        process
            .wait_until(|stream, bytes| {
                staged.push((stream, bytes.to_vec()));
                bytes == b"ready\n"
            })
            .await
            .unwrap()
    );
    assert_eq!(
        staged,
        [
            (OutputStream::Stderr, b"diagnostic\n".to_vec()),
            (OutputStream::Stdout, b"ready\n".to_vec())
        ]
    );
    assert!(process.running());
    assert!(!process.running());

    let mut remaining = Vec::new();
    let result = process
        .wait_with_output(|stream, bytes| {
            remaining.push((stream, bytes.to_vec()));
        })
        .await
        .unwrap();
    assert_eq!(remaining, [(OutputStream::Stdout, b"finished\n".to_vec())]);
    assert_eq!(result.exit_code(), 7);
    assert_eq!(result.output(), b"ready\nfinished\n");
    assert_eq!(result.error_output(), b"diagnostic\n");
    assert_eq!(observed.lock().unwrap().len(), 3);
    factory.assert_ran_times(|probe, _| probe.command == "script".into(), 1);
    assert_eq!(
        factory.recorded()[0].1.as_ref().unwrap().output(),
        result.output()
    );
}

#[cfg(unix)]
mod unix {
    use super::*;
    use openrust::process::{OutputStream, Signal};
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
        time::{Duration, SystemTime},
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
        assert_eq!(result.error_output(), b"warning");
        assert_eq!(result.exit_code(), 7);
        assert!(result.see_in_output("from stdin"));
        assert!(result.see_in_error_output("warning"));
        assert!(result.throw_if(false).is_ok());

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
        assert!(quiet.output().is_empty() && quiet.error_output().is_empty());
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
            .start_with_output(move |stream, bytes| {
                captured.lock().unwrap().push((stream, bytes.to_vec()))
            })
            .unwrap();
        assert!(process.id().is_some());
        let first = wait_for_output(&mut process, b"first").await;
        assert!(first.starts_with(b"first"));
        let result = process.wait().await.unwrap();
        assert_eq!(result.output(), b"firstsecond");
        assert_eq!(result.error_output(), b"problem");
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
        assert!(matches!(result, Err(Error::TimedOut { idle: false, .. })));
        assert!(started.elapsed().unwrap() < Duration::from_secs(2));

        let result = factory
            .command(script)
            .path(&directory.0)
            .idle_timeout(Duration::from_millis(100))
            .run()
            .await;
        assert!(matches!(result, Err(Error::TimedOut { idle: true, .. })));
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
        assert!(result.see_in_output("stopped"));

        let mut process = factory
            .command("trap 'exit 4' INT; printf ready; while :; do sleep 0.05; done")
            .start()
            .unwrap();
        wait_for_output(&mut process, b"ready").await;
        process.signal(Signal::Interrupt).await.unwrap();
        assert_eq!(process.wait().await.unwrap().exit_code(), 4);

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
        assert_eq!(failed.error_output(), b"failure");
        assert_eq!(failed.exit_code(), 4);

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
        assert_eq!(results["second"].as_ref().unwrap().exit_code(), 7);
    }

    async fn wait_for_output(
        process: &mut openrust::process::InvokedProcess,
        expected: &[u8],
    ) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(3), async {
            let mut observed = Vec::new();
            loop {
                observed.extend(process.latest_output());
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

    #[tokio::test]
    async fn wait_until_observes_live_chunks_without_consuming_the_process() {
        let directory = TestDirectory::new();
        let mut process = Factory::new().command(
            "printf ready >&2; while [ ! -f proceed ]; do sleep 0.02; done; printf done; exit 4",
        ).path(&directory.0).tty(false).timeout(Duration::from_secs(4)).start().unwrap();
        assert!(
            process
                .wait_until(|stream, bytes| stream == OutputStream::Stderr && bytes == b"ready")
                .await
                .unwrap()
        );
        assert!(process.running());
        assert_eq!(process.latest_error_output(), b"ready");

        assert!(
            tokio::time::timeout(Duration::from_millis(15), process.wait_until(|_, _| false))
                .await
                .is_err()
        );
        assert!(process.running());
        std::fs::write(directory.0.join("proceed"), b"").unwrap();
        let mut remaining = Vec::new();
        let result = process
            .wait_with_output(|stream, bytes| {
                if stream == OutputStream::Stdout {
                    remaining.extend_from_slice(bytes);
                }
            })
            .await
            .unwrap();
        assert_eq!(result.exit_code(), 4);
        assert!(result.see_in_output("done"));
        assert_eq!(remaining, b"done");
    }

    #[tokio::test]
    async fn repeated_wait_until_preserves_remaining_output_and_exit_status() {
        let directory = TestDirectory::new();
        let mut process = Factory::new().command(
            "printf ready; while [ ! -f proceed ]; do sleep 0.02; done; printf done; printf diagnostic >&2; exit 4",
        ).path(&directory.0).timeout(Duration::from_secs(4)).start().unwrap();
        assert!(
            process
                .wait_until(|stream, bytes| stream == OutputStream::Stdout && bytes == b"ready")
                .await
                .unwrap()
        );
        assert!(process.running());
        std::fs::write(directory.0.join("proceed"), b"").unwrap();
        assert!(
            !process
                .wait_until(|_, bytes| bytes == b"not printed")
                .await
                .unwrap()
        );
        assert!(!process.running());
        let result = process.wait().await.unwrap();
        assert_eq!(result.exit_code(), 4);
        assert_eq!(result.output(), b"readydone");
        assert_eq!(result.error_output(), b"diagnostic");
    }

    #[tokio::test]
    async fn wait_until_timeout_retains_output_and_reaps_the_process_group() {
        let directory = TestDirectory::new();
        let mut process = Factory::new()
            .command(
                "(sleep 0.4; printf survived > marker) & printf ready; printf warning >&2; wait",
            )
            .path(&directory.0)
            .idle_timeout(Duration::from_millis(100))
            .start()
            .unwrap();
        let error = process.wait_until(|_, _| false).await.unwrap_err();
        assert!(matches!(error, Error::TimedOut { idle: true, .. }));
        assert_eq!(error.result().unwrap().output(), b"ready");
        assert_eq!(error.result().unwrap().error_output(), b"warning");
        assert!(matches!(
            process.ensure_not_timed_out().await,
            Err(Error::TimedOut { idle: true, .. })
        ));
        assert!(matches!(
            process.wait().await,
            Err(Error::TimedOut { idle: true, .. })
        ));
        tokio::time::sleep(Duration::from_millis(450)).await;
        assert!(!directory.0.join("marker").exists());
    }
    #[tokio::test]
    async fn sends_a_numeric_unix_signal_and_rejects_invalid_numbers() {
        let mut child = Factory::new()
            .command(
                "trap 'printf caught; exit 6' USR1; printf ready; while :; do sleep 0.02; done",
            )
            .timeout(Duration::from_secs(3))
            .start()
            .unwrap();
        assert!(
            child
                .wait_until(|_, bytes| bytes == b"ready")
                .await
                .unwrap()
        );
        assert!(
            matches!(child.signal(Signal::Raw(0)).await, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput)
        );
        assert!(
            matches!(child.signal(Signal::Raw(i32::MAX)).await, Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput)
        );
        child.signal(Signal::Raw(libc::SIGUSR1)).await.unwrap();
        let result = child.wait().await.unwrap();
        assert_eq!(result.exit_code(), 6);
        assert_eq!(result.output(), b"readycaught");
    }
    #[test]
    fn detached_process_survives_its_runtime_and_finishes() {
        let directory = TestDirectory::new();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let process = Factory::new()
                .command("sleep 0.2; printf completed > marker")
                .path(&directory.0)
                .forever()
                .quietly()
                .start()
                .unwrap();
            process.detach().await.unwrap();
        });
        drop(runtime);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !directory.0.join("marker").exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read(directory.0.join("marker")).unwrap(),
            b"completed"
        );
    }
}

#[tokio::test]
async fn ordered_and_negative_assertions_inspect_real_record_order_and_results() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let factory = Factory::new();
    factory.assert_nothing_ran().assert_ran_in_order(&[]);
    factory.fake_command("*", Factory::result("output", "", 0));
    factory.run(["tool", "first"]).await.unwrap();
    factory.run(["tool", "second"]).await.unwrap();
    let first = |process: &openrust::process::RecordedProcess,
                 result: Option<&openrust::process::ProcessResult>| {
        process.command == Command::from(["tool", "first"])
            && result.is_some_and(|result| result.output() == b"output")
    };
    let second = |process: &openrust::process::RecordedProcess,
                  _: Option<&openrust::process::ProcessResult>| {
        process.command == Command::from(["tool", "second"])
    };
    factory
        .assert_ran_in_order(&[&first, &second])
        .assert_not_ran(|process, _| process.command == Command::from("missing"));
    assert!(
        catch_unwind(AssertUnwindSafe(
            || factory.assert_ran_in_order(&[&second, &first])
        ))
        .is_err()
    );
    assert!(catch_unwind(AssertUnwindSafe(|| factory.assert_ran_in_order(&[&first]))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| factory.assert_not_ran(first))).is_err());
    assert!(catch_unwind(AssertUnwindSafe(|| factory.assert_nothing_ran())).is_err());
    Factory::new().assert_nothing_ran();
}
