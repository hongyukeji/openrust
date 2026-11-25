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
