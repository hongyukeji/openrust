use openrust::process::{Error, Factory, OutputStream, PendingProcess};
use openrust_horizon::{SupervisorOptions, SupervisorProcess, WorkerProcess};
use std::{
    io::{self, Write as _},
    sync::{Arc, Mutex},
    time::Duration,
};

type Chunks = Arc<Mutex<[Vec<u8>; 2]>>;

fn capture(chunks: &Chunks) -> impl Fn(OutputStream, &[u8]) + Send + Sync + 'static {
    let chunks = Arc::clone(chunks);
    move |stream, bytes| {
        chunks.lock().unwrap()[usize::from(stream == OutputStream::Stderr)]
            .extend_from_slice(bytes);
    }
}

fn child(which: &str) -> PendingProcess {
    Factory::new()
        .command([std::env::current_exe().unwrap().into_os_string()])
        .env([("HORIZON_PROBE_WRAPPER_CHILD", which)])
}

fn assert_output(chunks: &Chunks, names: &[&str]) {
    let chunks = chunks.lock().unwrap();
    let stdout = names
        .iter()
        .map(|name| format!("out-{name}"))
        .collect::<String>();
    let stderr = names
        .iter()
        .map(|name| format!("err-{name}"))
        .collect::<String>();
    assert_eq!(chunks[0], stdout.as_bytes());
    assert_eq!(chunks[1], stderr.as_bytes());
}

pub async fn run() {
    let first = Chunks::default();
    let second = Chunks::default();
    let mut worker = WorkerProcess::new("probe", "default", Duration::from_secs(5));
    worker
        .handle_output_using(capture(&first))
        .start(child("worker-1"))
        .unwrap();
    assert!(worker.process().unwrap().id().is_some());
    assert!(
        matches!(worker.start(child("duplicate")), Err(Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    // 更换回调仅影响下一次启动。当前子进程可能还没有写出第一段。
    worker.handle_output_using(capture(&second));
    let previous = worker.take_process().unwrap().wait().await.unwrap();
    assert!(
        previous.successful() && previous.output().is_empty() && previous.error_output().is_empty()
    );
    for name in ["worker-2", "worker-3"] {
        worker.start(child(name)).unwrap();
        assert!(
            worker
                .take_process()
                .unwrap()
                .wait()
                .await
                .unwrap()
                .successful()
        );
    }
    assert_output(&first, &["worker-1"]);
    assert_output(&second, &["worker-2", "worker-3"]);

    let first = Chunks::default();
    let second = Chunks::default();
    let mut supervisor = SupervisorProcess::new(
        SupervisorOptions {
            name: "test:supervisor".into(),
            ..Default::default()
        },
        None,
    );
    supervisor
        .handle_output_using(capture(&first))
        .start(child("supervisor-1"))
        .unwrap();
    assert!(supervisor.process().unwrap().id().is_some());
    assert!(
        matches!(supervisor.start(child("duplicate")), Err(Error::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    supervisor.handle_output_using(capture(&second));
    let previous = supervisor.take_process().unwrap().wait().await.unwrap();
    assert!(
        previous.successful() && previous.output().is_empty() && previous.error_output().is_empty()
    );
    for name in ["supervisor-2", "supervisor-3"] {
        supervisor.start(child(name)).unwrap();
        assert!(
            supervisor
                .take_process()
                .unwrap()
                .wait()
                .await
                .unwrap()
                .successful()
        );
    }
    assert_output(&first, &["supervisor-1"]);
    assert_output(&second, &["supervisor-2", "supervisor-3"]);
    println!("PASS: both wrappers retain, replace and stream the callback across native launches");
}

pub async fn run_child(name: &str) {
    tokio::time::sleep(Duration::from_millis(30)).await;
    let mut stdout = io::stdout().lock();
    stdout.write_all(format!("out-{name}").as_bytes()).unwrap();
    stdout.flush().unwrap();
    let mut stderr = io::stderr().lock();
    stderr.write_all(format!("err-{name}").as_bytes()).unwrap();
    stderr.flush().unwrap();
}
