use openrust::foundation::cloud::{Events, SocketFactory};
use serde_json::json;
use std::{
    io,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};
use tokio::io::AsyncWrite;
struct Writer {
    bytes: Arc<Mutex<Vec<u8>>>,
    writes: Arc<AtomicUsize>,
    limit: usize,
    fail: bool,
}
impl AsyncWrite for Writer {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            return Poll::Ready(Err(io::Error::other("connection lost")));
        }
        let size = buf.len().min(self.limit);
        self.bytes.lock().unwrap().extend_from_slice(&buf[..size]);
        Poll::Ready(Ok(size))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn json_lines_preserve_unicode_fraction_and_partial_writes_and_reuse_connections() {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let writes = Arc::new(AtomicUsize::new(0));
    let connections = Arc::new(AtomicUsize::new(0));
    let (b, w, c) = (bytes.clone(), writes.clone(), connections.clone());
    let factory: SocketFactory = Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        let writer = Writer {
            bytes: b.clone(),
            writes: w.clone(),
            limit: 3,
            fail: false,
        };
        Box::pin(async move { Ok(Box::new(writer) as Box<dyn AsyncWrite + Unpin + Send>) })
    });
    let events = Events::new("controlled").with_socket_factory(factory);
    assert!(events.emit_many(&[]).await);
    assert!(events.emit(&json!({})).await);
    assert_eq!(connections.load(Ordering::SeqCst), 0);
    assert!(
        events
            .emit_many(&[json!({"value":"中国/路径"}), json!({"value":0.0})])
            .await
    );
    assert!(events.emit(&json!({"value":true})).await);
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    assert!(writes.load(Ordering::SeqCst) > 3);
    assert_eq!(
        String::from_utf8(bytes.lock().unwrap().clone()).unwrap(),
        serde_json::from_str::<serde_json::Value>(include_str!(
            "fixtures/cloud/events-contract.json"
        ))
        .unwrap()["wire"]
            .as_str()
            .unwrap()
    );
    assert!(!events.emit(&json!("bad")).await);
    events.disconnect().await;
    assert!(events.emit(&json!({"again":1})).await);
    assert_eq!(connections.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn failed_writes_disconnect_and_zero_length_writes_stop_after_five_attempts() {
    let counter = Arc::new(AtomicUsize::new(0));
    let capture = counter.clone();
    let factory: SocketFactory = Arc::new(move |_| {
        let first = capture.fetch_add(1, Ordering::SeqCst) == 0;
        Box::pin(async move {
            Ok(Box::new(Writer {
                bytes: Arc::default(),
                writes: Arc::default(),
                limit: 1024,
                fail: first,
            }) as Box<dyn AsyncWrite + Unpin + Send>)
        })
    });
    let events = Events::new("controlled").with_socket_factory(factory);
    assert!(!events.emit(&json!({"first":1})).await);
    assert!(events.emit(&json!({"second":2})).await);
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    let writes = Arc::new(AtomicUsize::new(0));
    let observed = writes.clone();
    let factory: SocketFactory = Arc::new(move |_| {
        let writes = observed.clone();
        Box::pin(async move {
            Ok(Box::new(Writer {
                bytes: Arc::default(),
                writes,
                limit: 0,
                fail: false,
            }) as Box<dyn AsyncWrite + Unpin + Send>)
        })
    });
    assert!(
        !Events::new("controlled")
            .with_socket_factory(factory)
            .emit(&json!({"a":1}))
            .await
    );
    assert_eq!(writes.load(Ordering::SeqCst), 5);
}
#[tokio::test]
async fn tcp_socket_receives_actual_json_lines_and_connections_are_service_local() {
    use tokio::io::AsyncBufReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let received = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = tokio::io::BufReader::new(stream);
        let mut data = String::new();
        reader.read_line(&mut data).await.unwrap();
        data
    });
    let events = Events::new(format!("tcp://{address}"));
    assert!(events.emit(&json!({"event":"实际"})).await);
    assert_eq!(received.await.unwrap(), "{\"event\":\"实际\"}\n");
    assert!(!Events::new("unsupported://bad").emit(&json!({"a":1})).await);
}

struct PendingWriter {
    partial: bool,
    signal: Arc<tokio::sync::Notify>,
    dropped: Arc<AtomicUsize>,
}
impl Drop for PendingWriter {
    fn drop(&mut self) {
        self.dropped.fetch_add(1, Ordering::SeqCst);
    }
}
impl AsyncWrite for PendingWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.partial {
            self.partial = true;
            return Poll::Ready(Ok(buf.len().min(2)));
        }
        self.signal.notify_one();
        Poll::Pending
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn cancelling_a_partial_write_closes_the_socket_and_next_emit_starts_a_clean_line() {
    let connected = Arc::new(AtomicUsize::new(0));
    let signal = Arc::new(tokio::sync::Notify::new());
    let dropped = Arc::new(AtomicUsize::new(0));
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let (c, s, d, b) = (
        connected.clone(),
        signal.clone(),
        dropped.clone(),
        bytes.clone(),
    );
    let factory: SocketFactory = Arc::new(move |_| {
        let socket: Box<dyn AsyncWrite + Unpin + Send> = if c.fetch_add(1, Ordering::SeqCst) == 0 {
            Box::new(PendingWriter {
                partial: false,
                signal: s.clone(),
                dropped: d.clone(),
            })
        } else {
            Box::new(Writer {
                bytes: b.clone(),
                writes: Arc::default(),
                limit: 1024,
                fail: false,
            })
        };
        Box::pin(async move { Ok(socket) })
    });
    let events = Arc::new(Events::new("controlled").with_socket_factory(factory));
    let copy = events.clone();
    let task = tokio::spawn(async move { copy.emit(&json!({"cancelled":1})).await });
    signal.notified().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(events.emit(&json!({"next":2})).await);
    assert_eq!(connected.load(Ordering::SeqCst), 2);
    assert_eq!(*bytes.lock().unwrap(), b"{\"next\":2}\n");
}

#[tokio::test]
async fn queueing_uses_the_formal_connection_and_normalizes_fifo_names_without_active_job_state() {
    use openrust::{
        contracts::container::Container as _,
        support::facades::{App, Cloud, Facade},
        testing::TestApp,
    };
    let mut app=TestApp::create(|builder|builder.with_config(|_,_|Ok([
        ("app".into(),json!({"env":"testing"})),
        ("queue".into(),json!({"default":"cloud","connections":{"cloud":{"driver":"cloud","queues":["priority.fifo"],"connection":{"prefix":"https://sqs.test/account","suffix":"-env","queue":"default","key":"test","secret":"test","region":"us-east-1"}}}})),
    ].into()))).unwrap();
    std::fs::write(app.root().join(".env"), "LARAVEL_CLOUD=1\n").unwrap();
    app.boot().await.unwrap();
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let capture = bytes.clone();
    let factory: SocketFactory = Arc::new(move |_| {
        let writer = Writer {
            bytes: capture.clone(),
            writes: Arc::default(),
            limit: 1024,
            fail: false,
        };
        Box::pin(async move { Ok(Box::new(writer) as Box<dyn AsyncWrite + Unpin + Send>) })
    });
    openrust::contracts::foundation::Application::container(app.app())
        .instance(Arc::new(
            Events::new("controlled").with_socket_factory(factory),
        ))
        .unwrap();
    App::set_facade_application(Some(app.app()), async {
        let queue = Cloud::queue().unwrap();
        assert!(queue.processing_job_details().unwrap().is_none());
        queue.finish_processing_job(None, None).await.unwrap();
        assert!(bytes.lock().unwrap().is_empty());
        queue
            .finish_queueing_job(Some("priority.fifo"))
            .await
            .unwrap();
    })
    .await;
    let event: serde_json::Value = serde_json::from_slice(&bytes.lock().unwrap()).unwrap();
    assert_eq!(event["type"], "queued");
    assert_eq!(event["queue"], "priority.fifo");
    assert_eq!(event["_cloud_event"], "queue");
    assert_eq!(event["timestamp"].as_str().unwrap().len(), 26);
    assert!(event.get("duration_ms").is_none());
}
#[tokio::test]
async fn stalled_connect_has_the_fixed_two_second_budget_and_returns_false() {
    let factory: SocketFactory = Arc::new(|_| Box::pin(std::future::pending()));
    let start = tokio::time::Instant::now();
    assert!(
        !Events::new("controlled")
            .with_socket_factory(factory)
            .emit(&json!({"a":1}))
            .await
    );
    assert!(start.elapsed() >= std::time::Duration::from_secs(2));
}
