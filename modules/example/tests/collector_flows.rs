//! 真实 Collector 出口验收；traces 为 OTLP，metrics 由 Collector 抓取 Prometheus。
//! 全局 subscriber/recorder 在独立子进程安装，服务与证据 sink 均由集成人持有。
#![cfg(all(
    feature = "otel",
    feature = "metrics",
    feature = "auth",
    any(feature = "db-postgres", feature = "db-mysql")
))]
#![allow(clippy::print_stdout)] // 子进程与测试宿主仅输出脱敏阶段和布尔报告。

use figment::{Figment, providers::Serialized};
use openrust::Db;
use openrust::auth::JwtTokens;
use openrust::db::sea_orm::{ConnectionTrait, Statement};
use openrust::db::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum;
use openrust::testing::{TestApp, TestDatabase};
use std::{fmt::Write, net::SocketAddr, path::PathBuf, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const CHILD: &str = "modules::example::collector_flows::collector_child_runtime";
const BUDGET: Duration = Duration::from_secs(10);
const PRIVATE: &str = "COLLECTOR_PRIVATE_INPUT_SENTINEL";
struct Migrator;
#[openrust::db::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = openrust::db::FrameworkMigrator::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}
fn setting(name: &str) -> TestResult<String> {
    std::env::var(name)
        .map_err(|_| format!("required Collector fixture setting is unset: {name}").into())
}
fn run_id() -> TestResult<String> {
    let id = setting("OPENRUST_TEST_COLLECTOR_RUN_ID")?;
    if id.is_empty()
        || id.len() > 48
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err("Collector run id must contain only bounded safe ASCII characters".into());
    }
    Ok(id)
}
fn trace_id() -> TestResult<String> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes)?;
    let mut id = String::with_capacity(32);
    for byte in bytes {
        write!(&mut id, "{byte:02x}")?;
    }
    Ok(id)
}
fn endpoint(name: &str) -> TestResult<String> {
    let value = setting(name)?;
    let url = url::Url::parse(&value).map_err(|_| "invalid Collector loopback endpoint")?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err("Collector endpoint must be a credential-free loopback HTTP base URL".into());
    }
    Ok(value)
}
async fn evidence() -> TestResult<String> {
    let path = PathBuf::from(setting("OPENRUST_TEST_COLLECTOR_EVIDENCE_PATH")?);
    let metadata = tokio::fs::metadata(&path).await?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("Collector evidence must be a bounded regular text sink".into());
    }
    Ok(tokio::fs::read_to_string(path).await?)
}
fn resource_blocks(text: &str) -> Vec<&str> {
    let mut starts: Vec<_> = text
        .match_indices("ResourceSpans #")
        .map(|(at, _)| at)
        .chain(text.match_indices("ResourceMetrics #").map(|(at, _)| at))
        .collect();
    starts.sort_unstable();
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| &text[*start..starts.get(index + 1).copied().unwrap_or(text.len())])
        .collect()
}
fn field(block: &str, name: &str, value: &str) -> bool {
    block.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, text)| {
            key.trim().trim_start_matches("-> ") == name && text.trim() == value
        })
    })
}
fn service(block: &str, id: &str) -> bool {
    block.contains(&format!("service.name: Str(example-collector-{id})"))
}
fn valid_trace_id(span: &str) -> bool {
    span.lines().any(|line| {
        line.split_once(':').is_some_and(|(key, value)| {
            let id = value.trim();
            key.trim() == "Trace ID"
                && id.len() == 32
                && id.bytes().all(|byte| byte.is_ascii_hexdigit())
                && id.bytes().any(|byte| byte != b'0')
        })
    })
}
fn has_span(text: &str, id: &str, operation: &str) -> bool {
    resource_blocks(text)
        .into_iter()
        .filter(|block| {
            let resource = block.split("ScopeSpans #").next().unwrap_or(block);
            block.starts_with("ResourceSpans #")
                && service(resource, id)
                && resource.contains(&format!("openrust.acceptance.run_id: Str({id})"))
        })
        .any(|block| {
            block.split("Span #").any(|span| {
                field(span, "Name", "example.article")
                    && valid_trace_id(span)
                    && span.contains(&format!("operation: Str({operation})"))
                    && span.contains("outcome: Str(success)")
            })
        })
}
fn has_metrics(text: &str, id: &str) -> bool {
    resource_blocks(text)
        .into_iter()
        .filter(|block| {
            block.starts_with("ResourceMetrics #")
                && service(block.split("ScopeMetrics #").next().unwrap_or(block), id)
        })
        .any(|block| {
            block.split("Metric #").any(|metric| {
                (field(metric, "Name", "example_article_operations_total")
                    || field(metric, "Name", "example_article_operations"))
                    && metric.split("NumberDataPoints #").skip(1).any(|point| {
                        point.contains("operation: Str(create)")
                            && point.contains("outcome: Str(success)")
                    })
            })
        })
}
async fn await_evidence(id: &str, operation: &str, metrics: bool, baseline: usize) -> TestResult {
    let waited = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let text = evidence().await?;
            if text.contains(PRIVATE) {
                return Err("private article input escaped into Collector evidence".into());
            }
            // ADR-044 仅把入站 traceparent 作为 ExecutionMetadata 传递；这里用唯一
            // service/run_id 与请求前的文件位置归属 OTel span，不冒称 W3C parent 已接入。
            let new = text
                .get(baseline..)
                .ok_or("Collector evidence file was truncated during acceptance")?;
            if has_span(new, id, operation) && (!metrics || has_metrics(new, id)) {
                return Ok::<(), Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await;
    if waited.is_err() {
        let full = evidence().await?;
        let new = full.get(baseline..).unwrap_or("");
        println!(
            "collector-evidence-diagnostic {}",
            serde_json::json!({
                "span_new": has_span(new, id, operation),
                "metrics_new": has_metrics(new, id),
                "span_full": has_span(&full, id, operation),
                "metrics_full": has_metrics(&full, id),
                "baseline_bytes": baseline,
                "current_bytes": full.len(),
                "new_resource_blocks": resource_blocks(new).len(),
            })
        );
    }
    waited?
}
struct ListenerTask(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for ListenerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    database: TestDatabase,
    address: SocketAddr,
    token: String,
    stop: Option<oneshot::Sender<()>>,
    server: ListenerTask,
    _storage: tempfile::TempDir,
}
impl Fixture {
    async fn create() -> TestResult<Self> {
        let admin = setting("OPENRUST_TEST_DATABASE_URL")?;
        let database = TestDatabase::create::<Migrator>(&admin).await?;
        let storage = tempfile::tempdir()?;
        let address: SocketAddr = setting("OPENRUST_TEST_COLLECTOR_APP_ADDR")?.parse()?;
        if !address.ip().is_loopback() && !address.ip().is_unspecified() {
            return Err("Collector fixture App must use a local bind address".into());
        }
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        defaults.server.metrics_endpoint_enabled = true;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let config=Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"},"database":{"url":database.url()},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"collector-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(config)
            .build()
            .await?;
        let db = app.app()?.state().resolve::<Db>().await?;
        let connection = db.primary_connection()?;
        let users = db.quoted_table_name("users")?;
        connection.execute_raw(Statement::from_string(connection.get_database_backend(),format!("INSERT INTO {users} (id,email,password_hash,email_verified_at,created_at,updated_at) VALUES (1,'collector-fixture@example.test','unused',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)"))).await?;
        let token = app
            .app()?
            .state()
            .resolve::<JwtTokens>()
            .await?
            .issue_access_token(1)?;
        // 复用生产全局桥接；不能用局部测试 subscriber 或另建 SDK 代替出口。
        openrust::core::lifecycle::init_tracing(&app.app()?.config().log);
        let router = openrust::http::build_router(app.app()?)?;
        let (stop, stopped) = oneshot::channel();
        let server = ListenerTask(tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        }));
        Ok(Self {
            app,
            database,
            address,
            token,
            stop: Some(stop),
            server,
            _storage: storage,
        })
    }
    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        trace_id: &str,
    ) -> TestResult<reqwest::Response> {
        let address = if self.address.ip().is_unspecified() {
            SocketAddr::from(([127, 0, 0, 1], self.address.port()))
        } else {
            self.address
        };
        let mut request = reqwest::Client::builder()
            .no_proxy()
            .timeout(BUDGET)
            .build()?
            .request(method.clone(), format!("http://{address}{path}"))
            .bearer_auth(&self.token)
            .header("traceparent", format!("00-{trace_id}-0102030405060708-01"));
        if method == reqwest::Method::POST {
            request = request.json(&serde_json::json!({"title":PRIVATE,"body":PRIVATE}));
        }
        match request.send().await {
            Ok(response) => Ok(response),
            Err(error) => {
                println!(
                    "collector_transport_diagnostic {}",
                    serde_json::json!({
                        "connect":error.is_connect(),
                        "timeout":error.is_timeout(),
                        "request":error.is_request(),
                        "body":error.is_body(),
                        "server_task_finished":self.server.0.is_finished(),
                    })
                );
                Err(error.into())
            }
        }
    }
    async fn stop(&mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
            let joined = tokio::time::timeout(BUDGET, &mut self.server.0).await;
            let shutdown = self
                .app
                .shutdown(tokio::time::Instant::now() + BUDGET)
                .await;
            joined???;
            shutdown?;
        } else {
            self.app
                .shutdown(tokio::time::Instant::now() + BUDGET)
                .await?;
        }
        Ok(())
    }
}
fn stage(phase: &'static str) {
    println!("collector_stage {}", serde_json::json!({"phase":phase}));
}
async fn business_child(unavailable: bool) -> TestResult {
    stage("boot");
    let mut fixture = Fixture::create().await?;
    let outcome = async {
        stage("create");
        let initial_evidence = if unavailable {
            0
        } else {
            evidence().await?.len()
        };
        let first = trace_id()?;
        let response = fixture
            .request(reqwest::Method::POST, "/api/example/articles", &first)
            .await?;
        if response.status().as_u16() != 201 {
            println!(
                "collector_http_status {}",
                serde_json::json!({"operation":"create","status":response.status().as_u16()})
            );
            return Err("Collector fixture article creation did not commit".into());
        }
        let body: serde_json::Value = response.json().await?;
        let article_id = body["data"]["id"]
            .as_i64()
            .ok_or("created article id absent")?;
        stage("initial_flush");
        tokio::time::timeout(BUDGET, openrust::core::lifecycle::flush_telemetry()).await?;
        if !unavailable {
            stage("collector_delivery");
            await_evidence(&run_id()?, "create", true, initial_evidence).await?;
            if evidence().await?.contains(&fixture.token) {
                return Err("credential escaped into Collector evidence".into());
            }
        }
        stage("delete");
        let before_delete = if unavailable {
            0
        } else {
            evidence().await?.len()
        };
        let last = trace_id()?;
        let response = fixture
            .request(
                reqwest::Method::DELETE,
                &format!("/api/example/articles/{article_id}"),
                &last,
            )
            .await?;
        if response.status().as_u16() != 200 {
            return Err("Collector fixture final article deletion did not commit".into());
        }
        if !unavailable
            && has_span(
                evidence()
                    .await?
                    .get(before_delete..)
                    .ok_or("Collector evidence file was truncated during acceptance")?,
                &run_id()?,
                "delete",
            )
        {
            return Err("final trace arrived before the explicit shutdown flush boundary".into());
        }
        // TestApp.shutdown 不隐式承诺 flush，按公开生命周期显式刷新并查真实接收证据。
        stage("shutdown");
        fixture.stop().await?;
        stage("final_flush");
        tokio::time::timeout(BUDGET, openrust::core::lifecycle::flush_telemetry()).await?;
        if !unavailable {
            stage("final_delivery");
            await_evidence(&run_id()?, "delete", false, before_delete).await?;
            if evidence().await?.contains(&fixture.token) {
                return Err("credential escaped into Collector evidence".into());
            }
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    // 无论出口是否成功，回收本隔离应用与数据库。
    let stopped = fixture.stop().await;
    let database = fixture.database.close().await;
    outcome?;
    stopped?;
    database?;
    println!(
        "collector_acceptance {}",
        serde_json::json!({"http_business_committed":true,"traces_delivered":!unavailable,"collector_scraped_prometheus":!unavailable,"shutdown_flush_delivered":!unavailable})
    );
    Ok(())
}
async fn output(reader: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(262_145).read_to_end(&mut bytes).await?;
    Ok(bytes)
}
#[allow(clippy::too_many_lines)] // 子进程统一隔离环境、真实出口验收和日志脱敏回收。
async fn subprocess(unavailable: bool) -> TestResult {
    let id = run_id()?;
    let endpoint = if unavailable {
        endpoint("OPENRUST_TEST_COLLECTOR_UNAVAILABLE_ENDPOINT")?
    } else {
        endpoint("OPENRUST_TEST_COLLECTOR_OTLP_ENDPOINT")?
    };
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            CHILD,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(
            "OPENRUST_COLLECTOR_CHILD",
            if unavailable {
                "unavailable"
            } else {
                "delivery"
            },
        )
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", endpoint)
        .env("OTEL_SERVICE_NAME", format!("example-collector-{id}"))
        .env(
            "OTEL_RESOURCE_ATTRIBUTES",
            format!("openrust.acceptance.run_id={id}"),
        )
        .env("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")
        .env("OTEL_EXPORTER_OTLP_TIMEOUT", "2000")
        .env("OTEL_BSP_SCHEDULE_DELAY", "3600000")
        .env("OTEL_BSP_EXPORT_TIMEOUT", "2000")
        .env("OTEL_BSP_MAX_EXPORT_BATCH_SIZE", "512")
        .env("OTEL_BSP_MAX_QUEUE_SIZE", "2048")
        .env("OTEL_BSP_MAX_CONCURRENT_EXPORTS", "1")
        .env("OTEL_TRACES_SAMPLER", "always_on")
        .env("RUST_LOG", "info")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for name in [
        "OTEL_EXPORTER_OTLP_TRACES_ENDPOINT",
        "OTEL_EXPORTER_OTLP_HEADERS",
        "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
        "OTEL_EXPORTER_OTLP_TRACES_TIMEOUT",
        "OTEL_EXPORTER_OTLP_TRACES_PROTOCOL",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(name);
    }
    let mut child = command.spawn()?;
    let stdout = tokio::spawn(output(
        child.stdout.take().ok_or("Collector child stdout absent")?,
    ));
    let stderr = tokio::spawn(output(
        child.stderr.take().ok_or("Collector child stderr absent")?,
    ));
    let status = tokio::time::timeout(Duration::from_secs(100), child.wait()).await;
    if status.is_err() {
        let _ = tokio::time::timeout(BUDGET, child.kill()).await;
    }
    let stdout = tokio::time::timeout(BUDGET, stdout).await???;
    let stderr = tokio::time::timeout(BUDGET, stderr).await???;
    let status = status??;
    if stdout.len() > 262_144 || stderr.len() > 262_144 {
        return Err("Collector child output exceeded safe bound".into());
    }
    let report = stdout
        .split(|byte| *byte == b'\n')
        .find_map(|line| {
            line.strip_prefix(b"collector_acceptance ")
                .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
        })
        .ok_or_else(|| {
            let phase = stdout
                .split(|byte| *byte == b'\n')
                .filter_map(|line| {
                    line.strip_prefix(b"collector_stage ")
                        .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
                })
                .filter_map(|value| value["phase"].as_str().map(str::to_owned))
                .rfind(|phase| {
                    [
                        "boot",
                        "create",
                        "initial_flush",
                        "collector_delivery",
                        "delete",
                        "shutdown",
                        "final_flush",
                        "final_delivery",
                    ]
                    .contains(&phase.as_str())
                })
                .unwrap_or_else(|| "unknown".into());
            let diagnostic = stdout
                .split(|byte| *byte == b'\n')
                .find_map(|line| {
                    line.strip_prefix(b"collector-evidence-diagnostic ")
                        .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
                })
                .map(|value| {
                    format!(
                        " span_new={} metrics_new={} span_full={} metrics_full={} baseline={} current={} blocks={}",
                        value["span_new"],
                        value["metrics_new"],
                        value["span_full"],
                        value["metrics_full"],
                        value["baseline_bytes"],
                        value["current_bytes"],
                        value["new_resource_blocks"],
                    )
                })
                .unwrap_or_default();
            let http = stdout
                .split(|byte| *byte == b'\n')
                .find_map(|line| {
                    line.strip_prefix(b"collector_http_status ")
                        .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
                })
                .map(|value| {
                    format!(
                        " http_operation={} http_status={}",
                        value["operation"], value["status"]
                    )
                })
                .unwrap_or_default();
            let transport = stdout
                .split(|byte| *byte == b'\n')
                .find_map(|line| {
                    line.strip_prefix(b"collector_transport_diagnostic ")
                        .and_then(|json| serde_json::from_slice::<serde_json::Value>(json).ok())
                })
                .map(|value| {
                    format!(
                        " transport_connect={} transport_timeout={} transport_request={} transport_body={} server_finished={}",
                        value["connect"],
                        value["timeout"],
                        value["request"],
                        value["body"],
                        value["server_task_finished"],
                    )
                })
                .unwrap_or_default();
            format!("Collector child failed at safe phase {phase}; raw logs withheld;{diagnostic}{http}{transport}")
        })?;
    if !status.success() || report["http_business_committed"].as_bool() != Some(true) {
        return Err("Collector child business HTTP/lifecycle failed; raw logs withheld".into());
    }
    for field in [
        "traces_delivered",
        "collector_scraped_prometheus",
        "shutdown_flush_delivered",
    ] {
        if report[field].as_bool() != Some(!unavailable) {
            return Err(
                "Collector delivery evidence report did not match the actual test mode".into(),
            );
        }
    }
    if unavailable {
        let warning = b"OpenTelemetry span flush failed";
        if !stdout
            .windows(warning.len())
            .any(|window| window == warning)
            && !stderr
                .windows(warning.len())
                .any(|window| window == warning)
        {
            return Err(
                "unavailable Collector did not produce the public flush failure warning".into(),
            );
        }
    }
    eprintln!(
        "collector-runtime {}",
        serde_json::json!({"http_business_committed":true,"traces_delivered":!unavailable,"collector_scraped_prometheus":!unavailable,"shutdown_flush_delivered":!unavailable,"unavailable_warning_observed":unavailable})
    );
    Ok(())
}
#[tokio::test]
#[ignore = "需真实 Collector OTLP/debug sink、Prometheus scrape 与 PostgreSQL/MySQL，必须独立子进程"]
async fn collector_runtime_exports_article_spans_and_prometheus_metrics() -> TestResult {
    subprocess(false).await
}
#[tokio::test]
#[ignore = "需专属不可达 Collector endpoint 与 PostgreSQL/MySQL；warn 不算交付成功"]
async fn collector_unavailable_warns_without_claiming_delivery() -> TestResult {
    subprocess(true).await
}
#[tokio::test]
#[ignore = "仅由 Collector 验收父测试以精确过滤器和子进程环境启动"]
async fn collector_child_runtime() -> TestResult {
    match setting("OPENRUST_COLLECTOR_CHILD")?.as_str() {
        "delivery" => business_child(false).await,
        "unavailable" => business_child(true).await,
        _ => Err("invalid Collector subprocess mode".into()),
    }
}

#[test]
fn collector_text_parser_requires_fixture_scoped_real_delivery_fields() {
    // 仅验证格式分类器；这些合成文本不构成 Collector 服务验收。
    let trace = "ResourceSpans #0\nservice.name: Str(example-collector-parser)\nopenrust.acceptance.run_id: Str(parser)\nScopeSpans #0\nSpan #0\nTrace ID: 0102030405060708090a0b0c0d0e0f10\nName: example.article\noperation: Str(create)\noutcome: Str(success)\n";
    let metric = "ResourceMetrics #0\nservice.name: Str(example-collector-parser)\nScopeMetrics #0\nMetric #0\n-> Name: example_article_operations_total\nNumberDataPoints #0\noperation: Str(create)\noutcome: Str(success)\n";
    assert!(has_span(trace, "parser", "create"));
    assert!(has_metrics(metric, "parser"));
    for altered in [
        trace.replace(
            "openrust.acceptance.run_id: Str(parser)",
            "openrust.acceptance.run_id: Str(other)",
        ),
        trace.replace(
            "0102030405060708090a0b0c0d0e0f10",
            "00000000000000000000000000000000",
        ),
        trace.replace("0102030405060708090a0b0c0d0e0f10", "invalid"),
        trace.replace("Str(success)", "Str(failed)"),
        trace.replace("Str(create)", "Str(delete)"),
        trace.replace("example-collector-parser", "example-collector-other"),
    ] {
        assert!(!has_span(&altered, "parser", "create"));
    }
    assert!(!has_metrics(trace, "parser"));
    assert!(!has_span(metric, "parser", "create"));
    assert!(!has_metrics(
        &metric.replace("example_article_operations_total", "http_requests_total"),
        "parser"
    ));
    let split_points = metric.replace(
        "outcome: Str(success)",
        "outcome: Str(failed)\nNumberDataPoints #1\noperation: Str(delete)\noutcome: Str(success)",
    );
    assert!(!has_metrics(&split_points, "parser"));
    let old = format!("{trace}ResourceMetrics #0\nservice.name: Str(other)\n");
    assert!(!has_span(&old[trace.len()..], "parser", "create"));
    // service 不能从邻近另一资源块借用。
    let unrelated = format!(
        "ResourceSpans #0\nservice.name: Str(example-collector-parser)\n{}",
        trace.replace("example-collector-parser", "example-collector-other")
    );
    assert!(!has_span(&unrelated, "parser", "create"));
}
