use openrust::process::{Factory, PendingProcess};
use openrust_horizon::{SupervisorOptions, SupervisorProcess, WorkerProcess};
use std::{
    io::{self, Write as _},
    sync::{Arc, Mutex},
    time::Duration,
};

fn child(action: &str) -> PendingProcess {
    let child = Factory::new()
        .command([std::env::current_exe().unwrap().into_os_string()])
        .env([("HORIZON_PROBE_WRAPPER_CONTROL_CHILD", action)]);
    #[cfg(windows)]
    let child = child.with_shutdown_pipe();
    child
}

async fn until(mut check: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

pub async fn run_supervisor_monitor(repository: &openrust_horizon::Repository, master: &str) {
    use serde_json::json;

    let empty = &[][..];
    for (suffix, action, terminal, stale) in [
        ("ok", "ok", None, false),
        ("terminal", "terminal", None, false),
        ("duplicate", "duplicate", Some(empty), false),
        ("reprovision", "reprovision", None, false),
        ("stale", "reprovision", None, true),
        ("blocked", "reprovision", Some(&[1][..]), false),
        ("terminal-override", "terminal", Some(empty), false),
    ] {
        let queue = if suffix == "terminal-override" {
            "wrapper-monitor-custom"
        } else {
            "wrapper-monitor"
        };
        let name = format!("{master}:monitor-{suffix}");
        let mut wrapper = SupervisorProcess::new(
            SupervisorOptions {
                name: name.clone(),
                queue: vec![queue.into()],
                max_processes: 1,
                timeout: 5,
                ..Default::default()
            },
            None,
        );
        if let Some(codes) = terminal {
            wrapper.dont_restart_on = codes.to_vec();
        }
        wrapper
            .monitor(child(action), repository, master)
            .await
            .unwrap();
        let pid = wrapper.process().unwrap().spawned_id().unwrap();
        assert!(wrapper.cooling_down());
        until(|| !wrapper.process().unwrap().running()).await;

        if action == "reprovision" || suffix == "terminal-override" {
            repository
                .heartbeat_supervisor(
                    &name,
                    &json!({
                        "pid": if stale { pid + 1 } else { pid }, "master": master,
                        "host": "monitor-probe", "status": "running",
                    }),
                )
                .await
                .unwrap();
        }
        wrapper
            .monitor(child("fail"), repository, master)
            .await
            .unwrap();
        assert!(wrapper.process().is_none());
        assert_eq!(wrapper.is_dead(), action == "duplicate");
        if !wrapper.is_dead() {
            until(|| !wrapper.cooling_down()).await;
            wrapper
                .monitor(child("fail"), repository, master)
                .await
                .unwrap();
        }
        assert!(wrapper.is_dead());
        // 监控旧包装器不会重复入队，也不会清除继任进程的心跳。
        wrapper
            .monitor(child("fail"), repository, master)
            .await
            .unwrap();
        if stale || suffix == "blocked" {
            assert_eq!(
                repository.supervisor(&name).await.unwrap().unwrap()["pid"],
                pid + u32::from(stale)
            );
            repository.forget_supervisor(&name).await.unwrap();
        }
    }
    println!(
        "PASS: independent Supervisor monitor applies default and custom exit codes and preserves a replacement heartbeat"
    );
}

pub async fn run() {
    let mut invalid = WorkerProcess::new("probe", "default", Duration::from_secs(2));
    let missing = Factory::new().command(Vec::<std::ffi::OsString>::new());
    assert!(invalid.start(missing).is_err());
    assert!(invalid.cooling_down());
    assert!(!invalid.monitor(child("ok")).await.unwrap());

    let mut worker = WorkerProcess::new("probe", "default", Duration::from_secs(2));
    assert!(worker.monitor(child("fail")).await.unwrap());
    assert!(worker.cooling_down());
    until(|| !worker.process().unwrap().running()).await;
    assert!(!worker.monitor(child("ok")).await.unwrap());
    assert!(worker.process().is_none());
    until(|| !worker.cooling_down()).await;
    assert!(worker.monitor(child("ok")).await.unwrap());
    assert!(worker.cooling_down());
    until(|| !worker.process().unwrap().running()).await;
    assert!(worker.monitor(child("controls")).await.unwrap());

    // Attach the callback for the next launch, then stop the currently owned process.
    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    worker.handle_output_using(move |_, bytes| received.lock().unwrap().extend_from_slice(bytes));
    worker.stop().await.unwrap();
    worker.start(child("controls")).unwrap();
    until(|| {
        messages
            .lock()
            .unwrap()
            .windows(5)
            .any(|value| value == b"ready")
    })
    .await;
    assert!(!worker.monitor(child("ok")).await.unwrap());
    #[cfg(unix)]
    {
        worker.pause().await.unwrap();
        until(|| {
            messages
                .lock()
                .unwrap()
                .windows(6)
                .any(|value| value == b"paused")
        })
        .await;
        worker.continue_working().await.unwrap();
        until(|| {
            messages
                .lock()
                .unwrap()
                .windows(9)
                .any(|value| value == b"continued")
        })
        .await;
    }
    #[cfg(windows)]
    {
        assert!(matches!(
            worker.pause().await,
            Err(openrust_horizon::Error::Unavailable)
        ));
        assert!(matches!(
            worker.continue_working().await,
            Err(openrust_horizon::Error::Unavailable)
        ));
    }
    worker.terminate().await.unwrap();
    assert_eq!(
        worker
            .take_process()
            .unwrap()
            .wait()
            .await
            .unwrap()
            .exit_code(),
        17
    );
    assert!(messages.lock().unwrap().ends_with(b"terminated\n"));
    worker.stop().await.unwrap();
    worker.pause().await.unwrap();
    worker.continue_working().await.unwrap();

    let messages = Arc::new(Mutex::new(Vec::new()));
    let received = messages.clone();
    let mut supervisor = SupervisorProcess::new(SupervisorOptions::default(), None);
    supervisor
        .handle_output_using(move |_, bytes| received.lock().unwrap().extend_from_slice(bytes));
    supervisor.start(child("controls")).unwrap();
    until(|| {
        messages
            .lock()
            .unwrap()
            .windows(5)
            .any(|value| value == b"ready")
    })
    .await;
    #[cfg(unix)]
    {
        supervisor.pause().await.unwrap();
        until(|| {
            messages
                .lock()
                .unwrap()
                .windows(6)
                .any(|value| value == b"paused")
        })
        .await;
        supervisor.continue_working().await.unwrap();
        until(|| {
            messages
                .lock()
                .unwrap()
                .windows(9)
                .any(|value| value == b"continued")
        })
        .await;
    }
    supervisor.stop().await.unwrap();
    assert!(supervisor.process().is_none());
    assert!(messages.lock().unwrap().ends_with(b"terminated\n"));
    println!("PASS: native wrapper controls, graceful drain, and Worker cooldown and restart");
}

fn line(message: &str) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{message}").unwrap();
    stdout.flush().unwrap();
}

pub async fn run_child(action: &str) -> u8 {
    match action {
        "duplicate" => return 13,
        "terminal" => return 2,
        "reprovision" => return 1,
        "ok" => return 0,
        "fail" => return 11,
        "controls" => {}
        _ => unreachable!(),
    }
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut pause = signal(SignalKind::user_defined2()).unwrap();
        let mut resume =
            signal(SignalKind::from_raw(rustix::process::Signal::CONT.as_raw())).unwrap();
        let mut stop = signal(SignalKind::terminate()).unwrap();
        line("ready");
        loop {
            tokio::select! {
                _ = pause.recv() => line("paused"),
                _ = resume.recv() => line("continued"),
                _ = stop.recv() => break,
            }
        }
    }
    #[cfg(windows)]
    {
        let parent = openrust::process::ParentShutdown::inherited().unwrap();
        line("ready");
        parent.closed().await.unwrap();
    }
    line("terminated");
    17
}
