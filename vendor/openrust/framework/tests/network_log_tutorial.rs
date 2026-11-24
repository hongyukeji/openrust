use openrust::{
    config::Repository,
    log::{LogContext, LogManager},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
struct WebhookServer {
    address: std::net::SocketAddr,
    records: Arc<Mutex<Vec<Value>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl WebhookServer {
    fn new(close_first: usize, status: u16) -> Self {
        Self::delayed(close_first, status, Duration::ZERO)
    }
    fn delayed(close_first: usize, status: u16, delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let records = Arc::new(Mutex::new(Vec::new()));
        let stopped = Arc::new(AtomicBool::new(false));
        let captured = records.clone();
        let stop = stopped.clone();
        let worker = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(v) => v,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(e) => panic!("{e}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut data = Vec::new();
                let mut bytes = [0; 4096];
                loop {
                    let n = socket.read(&mut bytes).unwrap();
                    if n == 0 {
                        break;
                    }
                    data.extend_from_slice(&bytes[..n]);
                    if let Some(end) = data.windows(4).position(|x| x == b"\r\n\r\n") {
                        let headers = std::str::from_utf8(&data[..end]).unwrap();
                        assert!(headers.starts_with("POST /hook/secret "), "{headers}");
                        assert!(
                            headers
                                .to_ascii_lowercase()
                                .contains("content-type: application/json")
                        );
                        let length = headers
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|s| s.parse::<usize>().unwrap())
                            })
                            .unwrap();
                        if data.len() >= end + 4 + length {
                            let record: Value =
                                serde_json::from_slice(&data[end + 4..end + 4 + length]).unwrap();
                            let count = {
                                let mut records = captured.lock().unwrap();
                                records.push(record);
                                records.len()
                            };
                            if count > close_first {
                                std::thread::sleep(delay);
                                let body = "ok";
                                let _=socket.write_all(format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\nLocation: /redirected\r\n\r\n{body}",body.len()).as_bytes());
                            }
                            break;
                        }
                    }
                }
            }
        });
        Self {
            address,
            records,
            stopped,
            worker: Some(worker),
        }
    }
    fn url(&self) -> String {
        format!("http://{}/hook/secret", self.address)
    }
}
impl Drop for WebhookServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let result = self.worker.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn manager(root: &std::path::Path) -> LogManager {
    LogManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "app".into(),
            json!({"name":"Shop","env":"tutorial"}),
        )]))),
        root,
    )
}
#[tokio::test]
async fn slack_uses_actual_webhooks_inside_an_async_runtime_with_context_and_plain_formatters() {
    let server = WebhookServer::new(0, 200);
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    manager.register_processor("fixed", |_| {
        Ok(Arc::new(|record| {
            record.at = jiff::Timestamp::from_second(1795482123).unwrap();
            record
                .extra
                .to_mut()
                .insert("trace".into(), "trace-id".into());
            Ok(())
        }))
    });
    manager.register_formatter("plain", |_| {
        Ok(Arc::new(|record| {
            Ok(format!("PLAIN:{}\n", record.message()))
        }))
    });
    let channel=manager.build(&json!({"driver":"slack","url":server.url(),"username":"Shop Log","channel":"#ops","level":"CRITICAL","replace_placeholders":true,"processors":["fixed"],"exclude_fields":["context.password"]})).unwrap();
    let context = LogContext::from_iter([
        ("id".into(), 42.into()),
        ("password".into(), "redact-me".into()),
        ("nested".into(), json!({"name":"Ada"})),
    ]);
    channel.info("below-threshold", &context).unwrap();
    channel.critical("Order {id}", &context).unwrap();
    let plain=manager.build(&json!({"driver":"slack","url":server.url(),"attachment":false,"formatter":"plain","replace_placeholders":true})).unwrap();
    plain.notice("plain {id}", &context).unwrap();
    let short=manager.build(&json!({"driver":"slack","url":server.url(),"short":true,"emoji":"https://example.test/icon.png","processors":["fixed"]})).unwrap();
    short.warning("short", &context).unwrap();
    let records = server.records.lock().unwrap();
    assert_eq!(records.len(), 3);
    let first = &records[0];
    assert_eq!(first["username"], "Shop Log");
    assert_eq!(first["channel"], "#ops");
    assert_eq!(first["icon_emoji"], ":boom:");
    let attachment = &first["attachments"][0];
    assert_eq!(attachment["text"], "Order 42");
    assert_eq!(attachment["color"], "danger");
    assert_eq!(attachment["ts"], 1795482123i64);
    assert_eq!(attachment["fields"][0]["value"], "CRITICAL");
    assert!(!first.to_string().contains("redact-me"));
    assert!(
        attachment["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["title"] == "Nested"
                && field["value"].as_str().unwrap().starts_with("```{\n    "))
    );
    assert_eq!(records[1]["text"], "PLAIN:plain 42\n");
    assert!(records[1].get("attachments").is_none());
    assert_eq!(records[2]["icon_url"], "https://example.test/icon.png");
    assert_eq!(records[2]["attachments"][0]["title"], "WARNING");
    assert!(
        records[2]["attachments"][0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["title"] == "Context")
    );
    assert!(!root.path().join("storage/logs/app.log").exists());
}
#[test]
fn slack_retries_transport_failures_but_does_not_reclassify_http_status_codes() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let recovering = WebhookServer::new(2, 200);
    let logger = manager
        .build(&json!({"driver":"slack","url":recovering.url()}))
        .unwrap();
    logger
        .get_logger()
        .error("retryable", &LogContext::new())
        .unwrap();
    let records = recovering.records.lock().unwrap();
    assert_eq!(records.len(), 3);
    assert!(records.windows(2).all(|pair| pair[0] == pair[1]));
    drop(records);
    let rejected = WebhookServer::new(0, 500);
    manager
        .build(&json!({"driver":"slack","url":rejected.url()}))
        .unwrap()
        .get_logger()
        .error("http-status", &LogContext::new())
        .unwrap();
    assert_eq!(rejected.records.lock().unwrap().len(), 1);
    let redirected = WebhookServer::new(0, 307);
    manager
        .build(&json!({"driver":"slack","url":redirected.url()}))
        .unwrap()
        .get_logger()
        .error("no-redirect", &LogContext::new())
        .unwrap();
    assert_eq!(redirected.records.lock().unwrap().len(), 1);
    let broken = WebhookServer::new(usize::MAX, 200);
    let logger = manager
        .build(&json!({"driver":"slack","url":broken.url()}))
        .unwrap();
    let error = logger
        .get_logger()
        .error("not-delivered", &LogContext::new())
        .unwrap_err();
    assert_eq!(broken.records.lock().unwrap().len(), 5);
    assert!(!format!("{error:?}").contains("/hook/secret"));
    logger
        .error("emergency-original", &LogContext::new())
        .unwrap();
    let emergency = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert!(emergency.contains("emergency-original"));
    assert!(!emergency.contains("/hook/secret"));
}
#[test]
fn slack_transport_timeout_and_later_delivery_keep_the_same_resolved_channel_usable() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let server = WebhookServer::new(5, 200);
    let logger = manager
        .build(&json!({"driver":"slack","url":server.url()}))
        .unwrap();
    logger.error("first-failure", &LogContext::new()).unwrap();
    logger.error("later-delivery", &LogContext::new()).unwrap();
    let records = server.records.lock().unwrap();
    assert_eq!(records.len(), 6);
    assert_eq!(records[5]["attachments"][0]["text"], "later-delivery");
    drop(records);
    let fallback = std::fs::read_to_string(root.path().join("storage/logs/app.log")).unwrap();
    assert!(fallback.contains("first-failure"));
    assert!(!fallback.contains("later-delivery"));
    let slow = WebhookServer::delayed(0, 200, Duration::from_millis(100));
    let logger = manager
        .build(&json!({"driver":"slack","url":slow.url(),"timeout":0.02}))
        .unwrap();
    let start = std::time::Instant::now();
    assert!(
        logger
            .get_logger()
            .error("timeout", &LogContext::new())
            .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}
#[cfg(unix)]
#[test]
fn remote_syslog_port_zero_uses_a_real_local_datagram_socket() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("u.sock");
    let receiver = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let manager = manager(root.path());
    manager.build(&json!({"driver":"monolog","handler":"syslog_udp","handler_with":{"host":"u.sock","port":0,"ident":"local"}})).unwrap().notice("local socket",&LogContext::new()).unwrap();
    let mut packet = [0; 2048];
    let count = receiver.recv(&mut packet).unwrap();
    let text = std::str::from_utf8(&packet[..count]).unwrap();
    assert!(text.starts_with("<13>1 "));
    assert!(text.contains("local socket"));
    assert!(text.contains(" local "));
}

#[test]
fn papertrail_and_monolog_udp_handlers_send_real_rfc_frames_and_keep_formatter_configuration() {
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
    receiver
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    manager.register_formatter("two-lines", |_| {
        Ok(Arc::new(|_| Ok("first\nsecond\n".into())))
    });
    for rfc in [0, 1, 2] {
        let config = json!({"driver":"monolog","handler":"syslog_udp","level":"NOTICE","handler_with":{"host":"127.0.0.1","port":receiver.local_addr().unwrap().port().to_string(),"ident":"orders","facility":"local4","rfc":rfc},"formatter":"two-lines"});
        let logger = manager.build(&config).unwrap();
        logger.debug("ignored", &LogContext::new()).unwrap();
        logger.notice("written", &LogContext::new()).unwrap();
        for line in ["first", "second"] {
            let mut bytes = vec![0; 65536];
            let size = receiver.recv(&mut bytes).unwrap();
            let text = String::from_utf8(bytes[..size].to_vec()).unwrap();
            assert!(
                text.starts_with(if rfc == 0 { "<165>" } else { "<165>1 " }),
                "{text}"
            );
            assert!(text.ends_with(line), "{text}");
            assert!(text.contains("orders"), "{text}");
        }
    }
    let logger=manager.build(&json!({"driver":"papertrail","host":"127.0.0.1","port":receiver.local_addr().unwrap().port(),"replace_placeholders":true})).unwrap();
    logger
        .info(
            "remote {id}",
            &LogContext::from_iter([("id".into(), 42.into())]),
        )
        .unwrap();
    let mut bytes = vec![0; 65536];
    let count = receiver.recv(&mut bytes).unwrap();
    let text = String::from_utf8(bytes[..count].to_vec()).unwrap();
    assert!(text.starts_with("<14>1 "), "{text}");
    assert!(text.contains("Shop"));
    assert!(text.contains("remote 42"));
}

fn config_file(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"name":"Shop","env":"production","key":"a".repeat(32)}),
        ),
        (
            "session".into(),
            json!({"driver":"array","lottery":[0,100]}),
        ),
        (
            "logging".into(),
            serde_json::from_slice(&std::fs::read(root.join("logging.json")).unwrap()).unwrap(),
        ),
    ]))
}
fn http_write(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::support::facades::Log;
        let id = request.route("id").unwrap().to_owned();
        if id != "clean" {
            Log::share_context(LogContext::from_iter([(
                "request_id".into(),
                id.clone().into(),
            )]))?;
        }
        let context = LogContext::from_iter([("id".into(), id.clone().into())]);
        Log::debug("debug {id}", &context)?;
        tokio::task::yield_now().await;
        Log::critical("critical {id}", &context)?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, json!({"written":id}))
    })
}
fn http_routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::get("/write/{id}", http_write)
}
fn cli_routes() -> Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::command("logs:remote", |_| {
        openrust::support::facades::Log::critical(
            "artisan remote",
            &LogContext::from_iter([("request_id".into(), "console".into())]),
        )
        .unwrap();
    })?
    .purpose("验收远程日志")
}
fn get(address: std::net::SocketAddr, path: &str) -> String {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    socket.write_all(format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut text = String::new();
    socket.read_to_string(&mut text).unwrap();
    text
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_http_and_artisan_write_the_default_stack_to_live_webhook_udp_and_file_without_scope_leaks()
 {
    use axum::handler::Handler as _;
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    const CHILD: &str = "TEST_REMOTE_LOG_ROOT";
    if let Some(root) = std::env::var_os(CHILD) {
        let app = Application::configure(root.into())
            .with_config(config_file)
            .with_routing(Routing::default().commands(cli_routes))
            .create()
            .unwrap();
        assert_eq!(
            app.handle_command(&["logs:remote".into()]).await.unwrap(),
            0
        );
        return;
    }
    let webhook = WebhookServer::new(0, 200);
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    udp.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("logging.json"),json!({"default":"stack","channels":{"stack":{"driver":"stack","channels":["slack","papertrail","file"]},"slack":{"driver":"slack","url":webhook.url(),"level":"critical","replace_placeholders":true},"papertrail":{"driver":"papertrail","host":"127.0.0.1","port":udp.local_addr().unwrap().port(),"replace_placeholders":true},"file":{"driver":"single","path":"http.log","replace_placeholders":true},"emergency":{"path":"emergency.log"}}}).to_string()).unwrap();
    let mut kernel = Kernel::new(
        Application::configure(root.path().into())
            .with_config(config_file)
            .with_routing(Routing::default().web(http_routes))
            .create()
            .unwrap(),
    );
    kernel.bootstrap().await.unwrap();
    let kernel = Arc::new(kernel);
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, transport.with_state(kernel).into_make_service())
            .await
            .unwrap();
    });
    let tasks = (0..16)
        .map(|id| {
            tokio::task::spawn_blocking(move || {
                let text = get(address, &format!("/write/{id}"));
                assert!(text.starts_with("HTTP/1.1 200"), "{text}");
            })
        })
        .collect::<Vec<_>>();
    for task in tasks {
        task.await.unwrap();
    }
    assert!(
        tokio::task::spawn_blocking(move || get(address, "/write/clean"))
            .await
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    let owned = root.path().to_owned();
    let output=tokio::task::spawn_blocking(move||std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","real_http_and_artisan_write_the_default_stack_to_live_webhook_udp_and_file_without_scope_leaks","--nocapture"]).env(CHILD,owned).output().unwrap()).await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    {
        let records = webhook.records.lock().unwrap();
        assert_eq!(records.len(), 18);
        for id in 0..16 {
            let id_text = id.to_string();
            let message = format!("critical {id}");
            let packet = records
                .iter()
                .find(|packet| packet["attachments"][0]["text"] == message)
                .unwrap();
            assert!(
                packet["attachments"][0]["fields"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|field| field["title"] == "Request_id"
                        && field["value"].as_str() == Some(id_text.as_str())),
                "{packet}"
            );
        }
        let clean = records
            .iter()
            .find(|packet| packet["attachments"][0]["text"] == "critical clean")
            .unwrap();
        assert!(!clean.to_string().contains("Request_id"));
        assert!(
            records
                .iter()
                .any(|packet| packet["attachments"][0]["text"] == "artisan remote")
        );
    }
    let mut buffer = vec![0; 65536];
    for _ in 0..35 {
        let n = udp.recv(&mut buffer).unwrap();
        let text = String::from_utf8_lossy(&buffer[..n]);
        assert!(
            text.starts_with("<15>1 ") || text.starts_with("<10>1 "),
            "{text}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join("http.log"))
            .unwrap()
            .lines()
            .count(),
        35
    );
    assert!(!root.path().join("emergency.log").exists());
    server.abort();
    let _ = server.await;
}

#[test]
fn actual_slack_payloads_match_fixed_public_reference_vectors() {
    use openrust::log::Level;
    let reference: Value =
        serde_json::from_str(include_str!("reference/network-log.json")).unwrap();
    let server = WebhookServer::new(0, 200);
    let root = tempfile::tempdir().unwrap();
    let manager = manager(root.path());
    manager.register_formatter("plain", |_| {
        Ok(Arc::new(|record| {
            Ok(format!("PLAIN:{}\n", record.message()))
        }))
    });
    for case in reference["cases"].as_array().unwrap() {
        let extra = case["extra"].as_object().unwrap().clone();
        manager.register_processor("reference", move |_| {
            let extra = extra.clone();
            Ok(Arc::new(move |record| {
                record.at = jiff::Timestamp::from_second(1795482123).unwrap();
                *record.extra.to_mut() = extra.clone();
                Ok(())
            }))
        });
        let mut config = case["config"].clone();
        config["driver"] = "slack".into();
        config["url"] = server.url().into();
        config["processors"] = json!(["reference"]);
        let logger = manager.build(&config).unwrap();
        let level = match case["level"].as_str().unwrap() {
            "Critical" => Level::Critical,
            "Warning" => Level::Warning,
            "Notice" => Level::Notice,
            "Debug" => Level::Debug,
            other => panic!("{other}"),
        };
        logger
            .log(
                level,
                case["message"].as_str().unwrap(),
                case["context"].as_object().unwrap(),
            )
            .unwrap();
    }
    let records = server.records.lock().unwrap();
    assert_eq!(records.len(), reference["cases"].as_array().unwrap().len());
    for (packet, case) in records.iter().zip(reference["cases"].as_array().unwrap()) {
        assert_eq!(packet, &case["expected"], "{}", case["name"]);
    }
}
