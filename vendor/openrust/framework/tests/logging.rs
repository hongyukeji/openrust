use std::{
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use axum::{extract::State, handler::Handler as _};
use openrust::{
    config::{ConfigError, Environment, Repository, Value},
    console::ConsoleError,
    contracts::{
        config::Repository as _,
        container::Container as _,
        debug::{Exception, ExceptionHandler},
        http::Kernel as _,
    },
    foundation::{
        Application,
        configuration::{Exceptions, Routing},
        http::Kernel,
    },
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    log::{Level, LogContext, LogError, LogManager},
    routing::RoutingError,
    support::facades::{Artisan, Context, Log, Route},
};
use serde_json::json;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        // The application canonicalizes the root before joining Unix socket paths.
        let temporary = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let path = temporary.join(format!(
            "openrust-log-{}-{}",
            std::process::id(),
            fastrand::u64(..)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.0.join(path)).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn manager(root: &Root, logging: Value) -> (LogManager, Arc<Repository>) {
    let config = Arc::new(Repository::new(BTreeMap::from([
        ("app".into(), json!({"env":"testing"})),
        ("logging".into(), logging),
    ])));
    (LogManager::new(config.clone(), &root.0), config)
}

#[derive(Debug)]
struct Previous;

impl std::fmt::Display for Previous {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("previous failure")
    }
}

impl std::error::Error for Previous {}

#[derive(Debug)]
struct Failure(Previous);

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("outer failure")
    }
}

impl std::error::Error for Failure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

fn configure_exceptions(exceptions: &mut Exceptions) {
    exceptions
        .exceptionContext::<Failure>(|_| {
            LogContext::from_iter([
                ("outer".into(), json!("outer context")),
                ("class".into(), json!("ignored class")),
            ])
        })
        .exceptionContext::<Previous>(|_| {
            LogContext::from_iter([("inner".into(), json!("inner context"))])
        })
        .context(|_| LogContext::from_iter([("configured".into(), json!(true))]));
}

#[test]
fn json_formatter_handles_unconfigured_errors_and_record_options() {
    let root = Root::new();
    let (manager, _) = manager(&root, json!({}));
    let logger = manager.build(&json!({
        "driver":"single","path":"json.log","formatter":"json",
        "formatter_with":{"append_newline":false,"ignore_empty_context_and_extra":true,"max_normalize_depth":2}
    })).unwrap();
    logger
        .logException(
            Level::Error,
            "caught",
            &Failure(Previous),
            &LogContext::new(),
        )
        .unwrap();
    let output = root.read("json.log");
    assert!(!output.ends_with('\n'));
    let record: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(record["level"], 400);
    assert_eq!(record["message"], "caught");
    assert!(record.get("extra").is_none());
    assert_eq!(
        record["context"]["exception"]["class"],
        std::any::type_name::<Failure>()
    );
    assert_eq!(
        record["context"]["exception"]["previous"],
        json!(["Over 2 levels deep, aborting normalization"])
    );

    let buffered = manager
        .build(&json!({
            "driver":"single","path":"buffered.jsonl","formatter":"json","action_level":"error"
        }))
        .unwrap();
    buffered
        .logException(
            Level::Info,
            "pending",
            &Failure(Previous),
            &LogContext::new(),
        )
        .unwrap();
    assert!(!root.0.join("buffered.jsonl").exists());
    buffered.error("flush", &LogContext::new()).unwrap();
    let buffered_log = root.read("buffered.jsonl");
    let record: Value = serde_json::from_str(buffered_log.lines().next().unwrap()).unwrap();
    assert_eq!(
        record["context"]["exception"]["previous"]["message"],
        "previous failure"
    );
}

#[cfg(unix)]
fn syslog_receiver(root: &Root) -> std::os::unix::net::UnixDatagram {
    let socket = std::os::unix::net::UnixDatagram::bind(root.0.join("ingest.sock")).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket
}

#[cfg(unix)]
fn receive_syslog(socket: &std::os::unix::net::UnixDatagram) -> String {
    let mut bytes = [0; 4096];
    let size = socket.recv(&mut bytes).unwrap();
    String::from_utf8(bytes[..size].to_vec()).unwrap()
}

#[cfg(windows)]
fn windows_events(source: &str) -> Vec<Value> {
    const SCRIPT: &str = r#"
        $ErrorActionPreference = 'Stop'
        [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
        $filter = "*[System[Provider[@Name='$env:OPENRUST_EVENT_TEST_SOURCE']]]"
        $events = @(Get-WinEvent -LogName Application -FilterXPath $filter -MaxEvents 16)
        $records = @($events | ForEach-Object {
            [pscustomobject]@{
                level = $_.Level
                category = $_.Task
                tag = $_.Properties[0].Value
                message = $_.Properties[1].Value
            }
        })
        ConvertTo-Json -InputObject $records -Compress
    "#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", SCRIPT])
        .env("OPENRUST_EVENT_TEST_SOURCE", source)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[cfg(windows)]
fn event_source(root: &Path) -> &str {
    root.file_name().unwrap().to_str().unwrap()
}

#[cfg(windows)]
#[test]
fn windows_syslog_preserves_formatted_events_and_recovers_after_rejection() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({"channels":{
            "events":{"driver":"syslog","name":"orders","level":"info","facility":"local4","formatter":"probe"},
            "emergency":{"path":"fallback.log"}
        }}),
    );
    let source = event_source(&root.0);
    config.set("app.name", json!(source)).unwrap();
    manager.registerFormatter("probe", |_| {
        Ok(Arc::new(|record| {
            let message = record.message();
            if message == "oversize" {
                return Ok("x".repeat(31_840));
            }
            Ok(format!("formatted {message}"))
        }))
    });
    let logger = manager.channel(Some("events")).unwrap();
    logger.debug("filtered", &LogContext::new()).unwrap();
    logger.info("Unicode 数据", &LogContext::new()).unwrap();
    logger.error("oversize", &LogContext::new()).unwrap();
    logger.alert("recovered", &LogContext::new()).unwrap();
    logger.critical("escalation", &LogContext::new()).unwrap();
    assert!(root.read("fallback.log").contains("oversize"));

    let records = windows_events(source);
    assert_eq!(records.len(), 3);
    for (message, category, level) in [
        ("formatted Unicode 数据", 6, 4),
        ("formatted recovered", 1, 2),
        ("formatted escalation", 2, 3),
    ] {
        let event = records
            .iter()
            .find(|record| record["message"] == message)
            .unwrap();
        assert_eq!(event["tag"], format!("{source}[{}]", std::process::id()));
        assert_eq!(event["category"], category);
        assert_eq!(event["level"], level);
    }
}

#[cfg(unix)]
#[test]
fn syslog_formats_real_datagrams_and_recovers_when_the_receiver_returns() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({"channels":{
            "native":{"driver":"syslog","socket":"ingest.sock","facility":"LOCAL4","name":"orders","formatter":"default","level":"notice"},
            "normal":{"driver":"syslog","socket":"ingest.sock","facility":8,"name":"orders","replace_placeholders":true},
            "custom":{"driver":"syslog","socket":"ingest.sock","formatter":"label","formatter_with":{"prefix":"SYSLOG"}},
            "invalid":{"driver":"syslog","socket":"ingest.sock","facility":9},
            "emergency":{"path":"fallback.log"}
        }}),
    );
    config.set("app.name", json!("AuditCollector")).unwrap();
    register_customization(&manager);
    let native = manager.channel(Some("native")).unwrap();
    native
        .warning("receiver unavailable", &LogContext::new())
        .unwrap();
    assert!(root.read("fallback.log").contains("receiver unavailable"));

    let socket = syslog_receiver(&root);
    native.info("below threshold", &LogContext::new()).unwrap();
    native
        .notice("fallback recovered", &LogContext::new())
        .unwrap();
    let packet = receive_syslog(&socket);
    assert!(packet.starts_with("<165>"));
    assert!(packet.ends_with(&format!(
        " audit-collector[{}]: orders.NOTICE: fallback recovered",
        std::process::id()
    )));
    let normal = manager.channel(Some("normal")).unwrap();
    normal
        .error(
            "order {id}",
            &LogContext::from_iter([("id".into(), json!(42))]),
        )
        .unwrap();
    let packet = receive_syslog(&socket);
    assert!(packet.starts_with("<11>"));
    assert!(packet.contains("]: ["));
    assert!(packet.ends_with(r#"orders.ERROR: order 42 {"id":42}"#));
    manager
        .channel(Some("custom"))
        .unwrap()
        .critical("custom", &LogContext::new())
        .unwrap();
    assert!(receive_syslog(&socket).ends_with("]: SYSLOG:custom:"));
    manager
        .channel(Some("invalid"))
        .unwrap()
        .error("invalid facility", &LogContext::new())
        .unwrap();
    assert!(root.read("fallback.log").contains("invalid facility"));
    socket.set_nonblocking(true).unwrap();
    assert_eq!(
        socket.recv(&mut [0; 1]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

fn register_customization(manager: &LogManager) {
    manager.registerFormatter("label", |arguments| {
        let prefix = arguments["prefix"]
            .as_str()
            .ok_or(LogError::InvalidConfiguration)?
            .to_owned();
        Ok(Arc::new(move |record| {
            let trace = record
                .extra
                .get("trace")
                .and_then(Value::as_str)
                .unwrap_or("");
            Ok(format!("{prefix}:{}:{trace}\n", record.message()))
        }))
    });
    manager.registerProcessor("trace", |arguments| {
        let label = arguments["label"].as_str().unwrap_or("default").to_owned();
        Ok(Arc::new(move |record| {
            let mut trace = record
                .extra
                .get("trace")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            trace.push_str(&label);
            record.extra.to_mut().insert("trace".into(), json!(trace));
            Ok(())
        }))
    });
    manager.registerTap("prepend", |logger, arguments| {
        let marker = arguments.join("/");
        logger.pushProcessor(Arc::new(move |record| {
            record.extra.to_mut().insert("trace".into(), json!(marker));
            Ok(())
        }));
        Ok(())
    });
    manager.registerTap("format", |logger, arguments| {
        let label = arguments[0].to_owned();
        logger.setFormatter(Arc::new(move |record| {
            Ok(format!("{label}:{}\n", record.message()))
        }));
        Ok(())
    });
}

#[test]
fn customized_stacks_preserve_each_formatter_and_process_in_order() {
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({"channels":{
            "first": {"driver":"single","path":"first.log","formatter":"label","formatter_with":{"prefix":"ONE"},"processors":[{"processor":"trace","with":{"label":"-1"}}]},
            "second": {"driver":"single","path":"second.log","formatter":"label","formatter_with":{"prefix":"TWO"},"processors":[{"processor":"trace","with":{"label":"-2"}}]},
            "stack": {"driver":"stack","channels":["first","second"],"tap":["prepend:tap,arg"]},
        "filtered": {"driver":"single","path":"filtered.log","level":"error","processors":["unexpected"]},
        "invalid": {"driver":"single","path":"invalid.log","formatter":"unknown"},
        "tapped": {"driver":"single","path":"tapped.log","formatter":"label","formatter_with":{"prefix":"before"},"tap":["format:first","format:final"]},
            "emergency":{"path":"emergency.log"}
        }}),
    );
    register_customization(&manager);
    manager.registerProcessor("unexpected", |_| {
        Ok(Arc::new(|_| {
            panic!("filtered records must skip processors")
        }))
    });
    let contexts = LogContext::new();
    manager
        .channel(Some("stack"))
        .unwrap()
        .info("message", &contexts)
        .unwrap();
    assert_eq!(root.read("first.log"), "ONE:message:tap/arg-1-2\n");
    assert_eq!(root.read("second.log"), "TWO:message:tap/arg-1-2\n");
    manager
        .channel(Some("filtered"))
        .unwrap()
        .debug("skip", &contexts)
        .unwrap();
    assert!(!root.0.join("filtered.log").exists());
    manager
        .channel(Some("invalid"))
        .unwrap()
        .warning("saved", &contexts)
        .unwrap();
    assert!(!root.0.join("invalid.log").exists());
    assert!(
        root.read("emergency.log")
            .contains("openrust.WARNING: saved")
    );
    let named_default = manager
        .build(&json!({"driver":"single","path":"default.log","formatter":"default"}))
        .unwrap();
    named_default.info("normal", &contexts).unwrap();
    assert!(root.read("default.log").contains(".INFO: normal"));
    manager
        .channel(Some("tapped"))
        .unwrap()
        .info("finished", &contexts)
        .unwrap();
    assert_eq!(root.read("tapped.log"), "final:finished\n");
}

#[test]
fn configured_handler_merges_arguments_and_controls_stack_propagation() {
    use std::sync::Mutex;

    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({"channels":{
            "capture": {
                "driver":"monolog","handler":"capture","level":"warning","formatter":"label",
                "formatter_with":{"prefix":"CAPTURE"},
                "with":{"level":"info","label":"before","bubble":true},
                "handler_with":{"level":"notice","label":"after","bubble":false}
            },
            "leaf":{"driver":"single","path":"leaf.log"},
            "retry":{"driver":"monolog","handler":"fail-once"},
            "emergency":{"path":"emergency.log"}
        }}),
    );
    register_customization(&manager);
    let observed = Arc::new(Mutex::new(Vec::new()));
    manager.registerHandler("capture", {
        let observed = observed.clone();
        move |options| {
            assert_eq!(
                options,
                &json!({"level":"notice","label":"after","bubble":false})
            );
            let observed = observed.clone();
            Ok(Arc::new(move |record, formatted| {
                observed
                    .lock()
                    .unwrap()
                    .push((record.level, formatted.to_owned()));
                Ok(())
            }))
        }
    });
    let stack = manager.stack(&["capture", "leaf"], None).unwrap();
    stack.info("continues", &LogContext::new()).unwrap();
    stack.notice("captured", &LogContext::new()).unwrap();
    assert_eq!(
        *observed.lock().unwrap(),
        [(openrust::log::Level::Notice, "CAPTURE:captured:\n".into())]
    );
    let leaf = root.read("leaf.log");
    assert!(leaf.contains("continues"));
    assert!(!leaf.contains("captured"));

    let attempts = std::sync::atomic::AtomicUsize::new(0);
    manager.registerHandler("fail-once", move |_| {
        if attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
            return Err(LogError::InvalidConfiguration);
        }
        Ok(Arc::new(|_, _| Ok(())))
    });
    let fallback = manager.channel(Some("retry")).unwrap();
    fallback
        .error("saved during factory failure", &LogContext::new())
        .unwrap();
    let recovered = manager.channel(Some("retry")).unwrap();
    assert!(!Arc::ptr_eq(&fallback, &recovered));
    assert!(Arc::ptr_eq(
        &recovered,
        &manager.channel(Some("retry")).unwrap()
    ));
    assert!(
        root.read("emergency.log")
            .contains("saved during factory failure")
    );
}

#[test]
fn buffered_handler_flushes_with_original_context_and_can_rearm() {
    let root = Root::new();
    let (manager, _) = manager(&root, json!({}));
    for mode in [None, Some(false)] {
        let mut config = json!({
            "driver":"monolog","handler":"stream",
            "with":{"stream":"activation.log"},
            "handler_with":{"stream":format!("activation-{mode:?}.log"),"level":"info"},
            "action_level":"critical",
            "replace_placeholders":true
        });
        if let Some(mode) = mode {
            config["stop_buffering"] = json!(mode);
        }
        let channel = manager.build(&config).unwrap();
        let path = format!("activation-{mode:?}.log");
        channel
            .debug("under handler level", &LogContext::new())
            .unwrap();
        channel
            .info(
                "first {id}",
                &LogContext::from_iter([("id".into(), json!(1))]),
            )
            .unwrap();
        assert!(!root.0.join(&path).exists());
        channel.critical("activate", &LogContext::new()).unwrap();
        let first = root.read(&path);
        assert_eq!(first.lines().count(), 2);
        assert!(first.contains("INFO: first 1"));
        assert!(first.contains("CRITICAL: activate"));
        channel.info("next", &LogContext::new()).unwrap();
        if mode == Some(false) {
            assert_eq!(first, root.read(&path));
            channel
                .critical("activate again", &LogContext::new())
                .unwrap();
            assert_eq!(root.read(&path).lines().count(), 4);
        } else {
            assert!(root.read(&path).contains("INFO: next"));
        }
        assert!(!root.0.join("activation.log").exists());
    }
}

#[test]
fn resolves_cached_stacks_with_severity_bubbling_and_fresh_driver_settings() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"stack",
            "channels": {
                "stack":{"driver":"stack", "name":"worker", "channels":"first, second"},
                "first":{"driver":"single", "path":"first.log", "level":"error", "bubble":false, "locking":true, "permission":0o600, "replace_placeholders":true},
                "second":{"driver":"single", "path":"second.log"},
                "quiet":{"driver":"null"},
            }
        }),
    );
    let stack = manager.channel(None).unwrap();
    assert_eq!(stack.name(), "worker");
    assert!(Arc::ptr_eq(&stack, &manager.driver(Some("stack")).unwrap()));
    let context = LogContext::from_iter([("value".into(), json!("first\nsecond\u{2028}"))]);
    stack.info("received {value}", &context).unwrap();
    assert!(!root.0.join("first.log").exists());
    assert!(
        root.read("second.log")
            .contains("worker.INFO: received first\\nsecond\\u{2028}")
    );
    assert_eq!(root.read("second.log").lines().count(), 1);
    stack
        .error("error stays in first", &LogContext::new())
        .unwrap();
    assert!(
        root.read("first.log")
            .contains("worker.ERROR: error stays in first")
    );
    assert!(!root.read("second.log").contains("error stays"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            fs::metadata(root.0.join("first.log"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    manager
        .stack(&["quiet", "second"], Some("muted"))
        .unwrap()
        .info("discard", &LogContext::new())
        .unwrap();
    assert!(!root.read("second.log").contains("discard"));
    manager.setDefaultDriver("second").unwrap();
    assert_eq!(manager.getDefaultDriver().as_deref(), Some("second"));
    let old = manager.channel(None).unwrap();
    config
        .set("logging.channels.second.path", json!("new.log"))
        .unwrap();
    manager.forgetChannel(None);
    manager.warning("new target", &LogContext::new()).unwrap();
    old.warning("old target", &LogContext::new()).unwrap();
    assert!(root.read("new.log").contains("new target"));
    assert!(root.read("second.log").contains("old target"));
    config
        .set("logging.channels.null", json!({"driver":"null"}))
        .unwrap();
    config.set("logging.default", Value::Null).unwrap();
    assert_eq!(manager.getDefaultDriver(), None);
    manager
        .info("test null default", &LogContext::new())
        .unwrap();
    assert!(!root.0.join("storage/logs/app.log").exists());
}

#[test]
fn emergency_recovers_resolution_and_file_errors_and_ignored_failures_continue() {
    let root = Root::new();
    fs::write(root.0.join("blocked"), "not a directory").unwrap();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"missing",
            "channels":{
                "emergency":{"path":"emergency.log"},
                "blocked":{"driver":"single","path":"blocked/file.log"},
                "later":{"driver":"single","path":"later.log"},
                "strict":{"driver":"stack","channels":["blocked","later"]},
            "ignored":{"driver":"stack","channels":["strict"],"ignore_exceptions":true},
                "cycle":{"driver":"stack","channels":["cycle"]},
            "invalid":{"driver":"stderr","formatter":"unsupported"}
            }
        }),
    );
    manager
        .info("configuration fallback", &LogContext::new())
        .unwrap();
    let blocked = manager.channel(Some("blocked")).unwrap();
    blocked.info("write fallback", &LogContext::new()).unwrap();
    manager
        .channel(Some("ignored"))
        .unwrap()
        .info("continued", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("cycle"))
        .unwrap()
        .warning("cycle fallback", &LogContext::new())
        .unwrap();
    manager
        .channel(Some("invalid"))
        .unwrap()
        .warning("formatter fallback", &LogContext::new())
        .unwrap();
    let emergency = root.read("emergency.log");
    for marker in [
        "Unable to create configured logger",
        "configuration fallback",
        "Unable to write configured logger",
        "write fallback",
        "cyclic or excessively deep log stack",
        "cycle fallback",
        "formatter fallback",
    ] {
        assert!(emergency.contains(marker), "{marker}");
    }
    assert!(!emergency.contains(root.0.to_str().unwrap()));
    assert!(!emergency.contains("continued"));
    assert!(root.read("later.log").contains("continued"));
    fs::remove_file(root.0.join("blocked")).unwrap();
    blocked
        .info("retry original sink", &LogContext::new())
        .unwrap();
    assert!(
        root.read("blocked/file.log")
            .contains("retry original sink")
    );
    config
        .set(
            "logging.channels.missing",
            json!({"driver":"single","path":"recovered.log"}),
        )
        .unwrap();
    manager
        .info("retry missing channel", &LogContext::new())
        .unwrap();
    assert!(root.read("recovered.log").contains("retry missing channel"));
}

fn config(root: &Path, _: &Environment) -> Result<BTreeMap<String, Value>, ConfigError> {
    let mut syslog = json!({"driver":"syslog","formatter":"default"});
    if cfg!(unix) {
        syslog["socket"] = json!("ingest.sock");
    }
    Ok(BTreeMap::from([
        (
            "app".into(),
            json!({"env":"test","host":"127.0.0.1","port":0,"name":root.file_name().and_then(|name| name.to_str())}),
        ),
        (
            "logging".into(),
            json!({
                "default":"stack",
                "channels":{
                    "stack":{"driver":"stack","channels":["single","json"]},
                    "single":{"driver":"single","path":"storage/logs/app.log","replace_placeholders":true},
                    "stderr":{"driver":"stderr"},
                    "syslog":syslog,
                    "json":{"driver":"single","path":"json.log","formatter":"json"},
                    "limited":{"driver":"single","path":"limited.log","formatter":"json","formatter_with":{"max_normalize_depth":3}},
                    "buffered":{"driver":"monolog","handler":"stream","with":{"stream":"buffered-http.log"},"action_level":"error","stop_buffering":false}
                }
            }),
        ),
    ]))
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/exception/{id}", log_exception)?;
    Route::get("/log/{id}", record)
}

fn log_exception(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let failure = Failure(Previous);
        Context::add("request", json!("json-probe")).unwrap();
        Context::addHidden("hidden", json!("private-json-marker")).unwrap();
        Log::logException(Level::Error, "direct failure", &failure, &LogContext::new()).unwrap();
        Log::channel(Some("limited"))
            .unwrap()
            .logException(
                Level::Error,
                "limited failure",
                &failure,
                &LogContext::new(),
            )
            .unwrap();
        let container = openrust::container::Container::getInstance();
        container
            .make::<dyn ExceptionHandler>()
            .unwrap()
            .report(&Exception::Error(&failure))
            .await
            .unwrap();
        Ok(Response::new(StatusCode::OK, Body::from("logged")))
    })
}

fn record(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        assert!(Context::isEmpty().unwrap());
        let id = request.route("id").unwrap();
        Context::add("id", json!(format!("request-{id}"))).unwrap();
        Context::add("scope_only", json!("public")).unwrap();
        Context::addHidden("token", json!("private-http-token")).unwrap();
        tokio::task::yield_now().await;
        assert_eq!(
            Context::get("id").unwrap(),
            Some(json!(format!("request-{id}")))
        );
        let context = LogContext::from_iter([("id".into(), json!(id))]);
        Log::info("request {id} {scope_only}", &context).map_err(|source| HttpError::Internal {
            source: source.into(),
        })?;
        let container = openrust::container::Container::getInstance();
        let manager = container.make::<LogManager>().unwrap();
        #[cfg(any(unix, windows))]
        manager
            .channel(Some("syslog"))
            .and_then(|logger| logger.warning("syslog HTTP", &context))
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        (|| {
            let buffered = manager.channel(Some("buffered"))?;
            buffered.info("before error", &context)?;
            buffered.error("HTTP activation", &context)
        })()
        .map_err(|source: LogError| HttpError::Internal {
            source: source.into(),
        })?;
        register_customization(&manager);
        Log::build(&json!({
            "driver":"single","path":"custom-http.log","formatter":"label",
            "formatter_with":{"prefix":"HTTP"},"processors":["trace"],
            "replace_placeholders":true
        }))
        .and_then(|logger| logger.info("request {id}", &context))
        .map_err(|source| HttpError::Internal {
            source: source.into(),
        })?;
        let logger = container
            .make::<dyn openrust::contracts::log::Logger>()
            .unwrap();
        let shared: Arc<dyn openrust::contracts::log::Logger> = manager;
        assert!(Arc::ptr_eq(&logger, &shared));
        Log::build(&json!({"driver":"daily", "path":"http.log"}))
            .and_then(|logger| logger.info("daily HTTP", &context))
            .map_err(|source| HttpError::Internal {
                source: source.into(),
            })?;
        Ok(Response::new(StatusCode::OK, Body::from("logged")))
    })
}

async fn transport(
    State(kernel): State<Arc<Kernel>>,
    incoming: axum::extract::Request,
) -> axum::response::Response {
    let (parts, body) = incoming.into_parts();
    let mut request = Request::from_parts(parts, body);
    let (parts, body) = kernel.handle(&mut request).await.into_parts();
    axum::response::Response::from_parts(parts, body)
}

async fn server(root: &Root) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let application = Application::configure(root.0.clone())
        .withConfig(config)
        .withExceptions(configure_exceptions)
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap();
    let mut kernel = Kernel::new(application);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let service = transport.with_state(Arc::new(kernel)).into_make_service();
    (
        address,
        tokio::spawn(async move { axum::serve(listener, service).await.unwrap() }),
    )
}

async fn send(address: std::net::SocketAddr, id: u32) {
    send_path(address, format!("/log/{id}")).await;
}

async fn send_path(address: std::net::SocketAddr, path: String) {
    tokio::task::spawn_blocking(move || {
        let timeout = Duration::from_secs(5);
        let mut stream = std::net::TcpStream::connect_timeout(&address, timeout).unwrap();
        stream.set_read_timeout(Some(timeout)).unwrap();
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("logged"));
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn json_exception_context_is_enriched_once_on_real_http() {
    let root = Root::new();
    let (address, task) = server(&root).await;
    send_path(address, "/exception/1".into()).await;
    let log = root.read("json.log");
    assert!(!log.contains("private-json-marker"));
    let records: Vec<Value> = log
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 2);
    let direct = &records[0];
    let reported = &records[1];
    assert_eq!(direct["message"], "direct failure");
    assert!(direct["context"].get("outer").is_none());
    assert_eq!(direct["context"]["exception"]["outer"], "outer context");
    assert_eq!(
        direct["context"]["exception"]["class"],
        std::any::type_name::<Failure>()
    );
    assert_eq!(direct["context"]["exception"]["configured"], true);
    assert_eq!(reported["message"], "outer failure");
    assert_eq!(reported["context"]["outer"], "outer context");
    assert_eq!(reported["context"]["configured"], true);
    assert!(reported["context"]["exception"].get("outer").is_none());
    assert!(reported["context"]["exception"].get("configured").is_none());
    for record in &records {
        assert_eq!(record["extra"]["request"], "json-probe");
        let previous = &record["context"]["exception"]["previous"];
        assert_eq!(previous["class"], std::any::type_name::<Previous>());
        assert_eq!(previous["inner"], "inner context");
        assert!(previous.get("outer").is_none());
    }
    let limited: Value = serde_json::from_str(&root.read("limited.log")).unwrap();
    assert_eq!(limited["context"]["exception"]["outer"], "outer context");
    let previous = &limited["context"]["exception"]["previous"];
    assert_eq!(previous["class"], std::any::type_name::<Previous>());
    assert!(previous.get("inner").is_none());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn real_http_logs_concurrent_requests_to_their_own_application() {
    let first = Root::new();
    let second = Root::new();
    #[cfg(unix)]
    let (first_socket, second_socket) = (syslog_receiver(&first), syslog_receiver(&second));
    let (a, first_task) = server(&first).await;
    let (b, second_task) = server(&second).await;
    tokio::join!(send(a, 101), send(a, 102), send(b, 201));
    send(a, 103).await;
    let first_log = first.read("storage/logs/app.log");
    let second_log = second.read("storage/logs/app.log");
    #[cfg(windows)]
    {
        let records = windows_events(event_source(&first.0));
        assert_eq!(records.len(), 3);
        for id in [101, 102, 103] {
            assert!(records.iter().any(|record| {
                record["message"]
                    .as_str()
                    .unwrap()
                    .contains(&format!("request-{id}"))
            }));
        }
        let records = windows_events(event_source(&second.0));
        assert_eq!(records.len(), 1);
        assert!(
            records[0]["message"]
                .as_str()
                .unwrap()
                .contains("request-201")
        );
    }
    #[cfg(unix)]
    {
        let records = (0..3)
            .map(|_| receive_syslog(&first_socket))
            .collect::<Vec<_>>();
        for id in [101, 102, 103] {
            assert!(
                records.iter().any(|record| record.starts_with("<12>")
                    && record.contains(&format!("request-{id}")))
            );
        }
        assert!(receive_syslog(&second_socket).contains("request-201"));
    }
    assert_eq!(first_log.lines().count(), 3);
    assert_eq!(second_log.lines().count(), 1);
    for (log, ids) in [
        (&first_log, &[101, 102, 103][..]),
        (&second_log, &[201][..]),
    ] {
        assert!(!log.contains("private-http-token"));
        for id in ids {
            assert!(log.contains(&format!(
                r#"test.INFO: request {id} {{scope_only}} {{"id":"{id}"}} {{"id":"request-{id}","scope_only":"public"}}"#
            )));
        }
    }
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    let daily = format!("http-{}.log", date.format("%Y-%m-%d"));
    assert_eq!(first.read(&daily).lines().count(), 3);
    assert_eq!(second.read(&daily).lines().count(), 1);
    assert_eq!(first.read("custom-http.log").lines().count(), 3);
    assert_eq!(second.read("custom-http.log").lines().count(), 1);
    assert!(!first.read("custom-http.log").contains("private-http-token"));
    let first_buffered = first.read("buffered-http.log");
    let second_buffered = second.read("buffered-http.log");
    assert_eq!(first_buffered.lines().count(), 6);
    assert_eq!(second_buffered.lines().count(), 2);
    assert!(!first_buffered.contains("request-201"));
    assert!(!second_buffered.contains("request-101"));
    for task in [first_task, second_task] {
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

fn console_routes() -> Result<(), ConsoleError> {
    Artisan::command("log:probe", |_| {
        assert!(Context::isEmpty().unwrap());
        Context::add("command", json!("log:probe")).unwrap();
        Context::addHidden("token", json!("private-artisan-token")).unwrap();
        Context::scopeSync(
            || {
                assert_eq!(Context::get("command").unwrap(), Some(json!("nested")));
            },
            LogContext::from_iter([("command".into(), json!("nested"))]),
            LogContext::new(),
        )
        .unwrap();
        Log::warning("artisan wrote a file", &LogContext::new()).unwrap();
        Log::logException(
            Level::Error,
            "artisan failure",
            &Failure(Previous),
            &LogContext::new(),
        )
        .unwrap();
        #[cfg(any(unix, windows))]
        Log::channel(Some("syslog"))
            .unwrap()
            .error("syslog Artisan", &LogContext::new())
            .unwrap();
        let manager = openrust::container::Container::getInstance()
            .make::<LogManager>()
            .unwrap();
        register_customization(&manager);
        let buffered = Log::build(&json!({
            "driver":"monolog","handler":"stream","with":{"stream":"php://stderr"},
            "action_level":"warning"
        }))
        .unwrap();
        buffered
            .info("before command activation", &LogContext::new())
            .unwrap();
        buffered
            .warning("command activation", &LogContext::new())
            .unwrap();
        Log::build(&json!({
            "driver":"monthly","path":"custom-console.log","formatter":"label",
            "formatter_with":{"prefix":"ARTISAN"},"tap":["prepend:artisan,hook"]
        }))
        .unwrap()
        .notice("formatted command", &LogContext::new())
        .unwrap();
        manager.registerFactory("console", |manager, _| {
            Ok(manager.build(&json!({"driver":"monthly","path":"console.log"}))?)
        });
        Log::build(&json!({"driver":"custom","via":"console"}))
            .unwrap()
            .notice("monthly Artisan", &LogContext::new())
            .unwrap();
        Log::channel(Some("stderr"))
            .unwrap()
            .notice("artisan stderr probe", &LogContext::new())
            .unwrap();
        openrust::container::Container::getInstance()
            .get::<dyn openrust::contracts::config::Repository>("config")
            .unwrap()
            .set(
                "logging.channels.emergency.path",
                json!("storage/logs/app.log/emergency"),
            )
            .unwrap();
        Log::channel(Some("absent"))
            .unwrap()
            .warning("emergency stderr probe", &LogContext::new())
            .unwrap();
    })?
    .purpose("Write a log test record")
}

#[tokio::test]
async fn artisan_logs_to_file_and_stderr() {
    const CHILD_ROOT: &str = "OPENRUST_TEST_LOG_CHILD_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let application = Application::configure(PathBuf::from(root))
            .withConfig(config)
            .withExceptions(configure_exceptions)
            .withRouting(Routing::default().commands(console_routes))
            .create()
            .unwrap();
        assert_eq!(
            application
                .handleCommand(&["log:probe".into()])
                .await
                .unwrap(),
            0
        );
        return;
    }
    let root = Root::new();
    #[cfg(unix)]
    let syslog = syslog_receiver(&root);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "artisan_logs_to_file_and_stderr", "--nocapture"])
        .env(CHILD_ROOT, &root.0)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let records = root.read("json.log");
    let artifact = records
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["message"] == "artisan failure")
        .unwrap();
    assert_eq!(artifact["context"]["exception"]["outer"], "outer context");
    #[cfg(unix)]
    assert!(receive_syslog(&syslog).contains("syslog Artisan"));
    #[cfg(windows)]
    assert!(
        windows_events(event_source(&root.0))[0]["message"]
            .as_str()
            .unwrap()
            .contains("syslog Artisan")
    );
    assert!(
        output.status.success(),
        "{}\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(stderr.contains("test.NOTICE: artisan stderr probe"));
    assert!(stderr.contains("test.INFO: before command activation"));
    assert!(stderr.contains("test.WARNING: command activation"));
    assert!(stderr.contains(r#"artisan stderr probe {} {"command":"log:probe"}"#));
    assert!(!stderr.contains("private-artisan-token"));
    assert!(
        !root
            .read("storage/logs/app.log")
            .contains("private-artisan-token")
    );
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    assert_eq!(
        root.read(&format!("custom-console-{}.log", date.format("%Y-%m"))),
        "ARTISAN:formatted command:artisan/hook\n"
    );
    assert!(
        root.read(&format!("console-{}.log", date.format("%Y-%m")))
            .contains(r#"monthly Artisan {} {"command":"log:probe"}"#)
    );
    assert!(stderr.contains("openrust.EMERGENCY: Unable to create configured logger"));
    assert!(stderr.contains("openrust.WARNING: emergency stderr probe"));
    assert!(
        root.read("storage/logs/app.log")
            .contains("test.WARNING: artisan wrote a file")
    );
}

#[test]
fn simultaneous_threads_share_a_channel_and_append_complete_records() {
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"single",
            "channels":{"single":{"driver":"single","path":"threads.log","locking":true}}
        }),
    );
    let barrier = std::sync::Barrier::new(6);
    let channels = std::thread::scope(|scope| {
        let threads: Vec<_> = (0..6)
            .map(|index| {
                let manager = &manager;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let channel = manager.channel(None).unwrap();
                    channel
                        .info(&format!("thread {index}"), &LogContext::new())
                        .unwrap();
                    channel
                })
            })
            .collect();
        threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(
        channels
            .iter()
            .all(|channel| Arc::ptr_eq(&channels[0], channel))
    );
    let records = root.read("threads.log");
    assert_eq!(records.lines().count(), 6);
    for index in 0..6 {
        assert!(
            records
                .lines()
                .any(|line| line.ends_with(&format!("testing.INFO: thread {index}")))
        );
    }
}

#[test]
fn daily_and_monthly_channels_prune_their_own_periods() {
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"daily",
            "channels": {
                "daily":{"driver":"daily","path":"shared.log","level":"warning","days":1},
                "monthly":{"driver":"monthly","path":"shared.log","max_files":1}
            }
        }),
    );
    for name in [
        "shared-2000-01-01.log",
        "shared-2000-01.log",
        "shared-note.log",
    ] {
        fs::write(root.0.join(name), "old").unwrap();
    }
    let context = LogContext::new();
    manager.info("filtered", &context).unwrap();
    assert!(root.0.join("shared-2000-01-01.log").exists());
    manager.warning("today", &context).unwrap();
    assert!(!root.0.join("shared-2000-01-01.log").exists());
    assert!(root.0.join("shared-2000-01.log").exists());
    manager
        .channel(Some("monthly"))
        .unwrap()
        .info("this month", &context)
        .unwrap();
    assert!(!root.0.join("shared-2000-01.log").exists());
    assert!(root.0.join("shared-note.log").exists());
    let date = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now());
    assert!(
        root.read(&format!("shared-{}.log", date.format("%Y-%m-%d")))
            .contains("today")
    );
    assert!(
        root.read(&format!("shared-{}.log", date.format("%Y-%m")))
            .contains("this month")
    );
}

#[test]
fn registered_factories_are_lazy_and_mix_with_on_demand_channels() {
    use openrust::log::{LogError, StackChannel};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let root = Root::new();
    let (manager, _) = manager(
        &root,
        json!({
            "default":"audit",
            "channels": {
                "audit":{"driver":"custom","via":"audit-factory","target":"audit.log"},
                "named":{"driver":"application","target":"named.log","level":"warning"},
                "emergency":{"path":"emergency.log"}
            }
        }),
    );
    let count = Arc::new(AtomicUsize::new(0));
    let hits = count.clone();
    manager.registerFactory("audit-factory", move |manager, config| {
        if hits.fetch_add(1, Ordering::Relaxed) == 0 {
            return Err(LogError::UnknownChannel("private-factory-error".into()));
        }
        Ok(manager.build(&json!({"driver":"single","path": config["target"]}))?)
    });
    manager.extend("application", |manager, config| {
        Ok(manager.build(&json!({"driver":"single","path":config["target"]}))?)
    });
    assert_eq!(count.load(Ordering::Relaxed), 0);
    manager
        .info("first factory error", &LogContext::new())
        .unwrap();
    let emergency = root.read("emergency.log");
    assert!(emergency.contains("custom log channel factory failed"));
    assert!(!emergency.contains("private-factory-error"));
    let audit = manager.channel(None).unwrap();
    assert!(Arc::ptr_eq(&audit, &manager.channel(None).unwrap()));
    let temp_config = json!({"driver":"single","path":"temporary.log"});
    let temporary = manager.build(&temp_config).unwrap();
    assert!(!Arc::ptr_eq(
        &temporary,
        &manager.build(&temp_config).unwrap()
    ));
    let combined = manager
        .stackChannels(
            &[
                StackChannel::Named("audit"),
                StackChannel::Logger(&temporary),
                StackChannel::Named("named"),
            ],
            Some("combined"),
        )
        .unwrap();
    combined.info("mixed record", &LogContext::new()).unwrap();
    assert!(root.read("audit.log").contains("mixed record"));
    assert!(
        root.read("temporary.log")
            .contains("combined.INFO: mixed record")
    );
    assert!(!root.0.join("named.log").exists());
    combined
        .warning("high severity", &LogContext::new())
        .unwrap();
    assert!(root.read("named.log").contains("high severity"));
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn forgetting_a_channel_during_factory_execution_prevents_stale_publication() {
    let root = Root::new();
    let (manager, config) = manager(
        &root,
        json!({
            "default":"selected",
            "channels":{"selected":{"driver":"extension","target":"old.log"}}
        }),
    );
    let started = Arc::new(std::sync::Barrier::new(2));
    let release = Arc::new(std::sync::Barrier::new(2));
    let factory_started = started.clone();
    let factory_release = release.clone();
    manager.extend("extension", move |manager, config| {
        if config["target"] == "old.log" {
            factory_started.wait();
            factory_release.wait();
        }
        Ok(manager.build(&json!({"driver":"single","path":config["target"]}))?)
    });
    std::thread::scope(|scope| {
        let pending = scope.spawn(|| manager.channel(None).unwrap());
        started.wait();
        config
            .set("logging.channels.selected.target", json!("new.log"))
            .unwrap();
        manager.forgetChannel(None);
        let current = manager.channel(None).unwrap();
        release.wait();
        let old = pending.join().unwrap();
        assert!(!Arc::ptr_eq(&current, &old));
        assert!(Arc::ptr_eq(&current, &manager.channel(None).unwrap()));
        current.info("current", &LogContext::new()).unwrap();
        old.info("in flight", &LogContext::new()).unwrap();
    });
    assert!(root.read("new.log").contains("current"));
    assert!(root.read("old.log").contains("in flight"));
}
