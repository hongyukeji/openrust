//! 模块阅读摘要预览的真实 TCP locale/复数/固定偏移格式化验收。

use super::config_sources::enabled_example_sources;
use figment::{Figment, providers::Serialized};
use openrust::http::axum;
use openrust::testing::TestApp;
use serde_json::Value;
use std::{net::SocketAddr, time::Duration};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BUDGET: Duration = Duration::from_secs(8);
const AT: &str = "2026-10-28T23:30:00Z";
struct ListenerTask(tokio::task::JoinHandle<std::io::Result<()>>);
impl Drop for ListenerTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
struct Fixture {
    app: TestApp<crate::bootstrap::Server>,
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    server: ListenerTask,
    client: reqwest::Client,
    _storage: tempfile::TempDir,
}
impl Fixture {
    async fn create() -> TestResult<Self> {
        Self::create_with_locale("en").await
    }
    async fn create_with_locale(default_locale: &str) -> TestResult<Self> {
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let storage = tempfile::tempdir()?;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source=Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","debug":false},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"localization-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "i18n":{"default_locale":default_locale,"fallback_locale":"en"}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(source))?)
            .build()
            .await?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = openrust::foundation::http::build_router(app.app()?)?;
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                router.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(BUDGET)
            .build()?;
        Ok(Self {
            app,
            address,
            stop: Some(stop),
            server: ListenerTask(server),
            client,
            _storage: storage,
        })
    }
    async fn response(
        &self,
        locale: Option<&str>,
        parameters: &[(&str, &str)],
    ) -> TestResult<reqwest::Response> {
        let mut request = self
            .client
            .get(format!(
                "http://{}/api/example/reading-summary",
                self.address
            ))
            .header("accept", "application/json")
            .query(parameters);
        if let Some(locale) = locale {
            request = request.header("accept-language", locale);
        }
        Ok(request.send().await?)
    }
    async fn summary(&self, locale: Option<&str>, count: &str, zone: &str) -> TestResult<Value> {
        let response = self
            .response(locale, &[("count", count), ("at", AT), ("zone", zone)])
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        let json: Value = response.json().await?;
        json.get("data")
            .cloned()
            .ok_or_else(|| "response data missing".into())
    }
    async fn close(mut self) -> TestResult {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        tokio::time::timeout(BUDGET, &mut self.server.0).await???;
        self.app
            .shutdown(tokio::time::Instant::now() + BUDGET)
            .await?;
        Ok(())
    }
}
#[tokio::test]
async fn reading_summary_formats_plural_locales_fixed_offsets_and_fallback_over_tcp() -> TestResult
{
    let f = Fixture::create().await?;
    let result = async {
        for (count, expected) in [
            ("0", "0 articles read"),
            ("1", "1 article read"),
            ("2", "2 articles read"),
        ] {
            let body = f.summary(Some("en"), count, "utc").await?;
            assert_eq!(body["reading"], expected);
            assert_eq!(body["locale"], "en");
            assert_eq!(body["preview"], true);
            assert_eq!(body["date"], "October 28, 2026");
            assert_eq!(body["as_of"], "As of October 28, 2026");
            assert_eq!(body["local_iso"], "2026-10-28T23:30:00+00:00");
        }
        for count in ["1", "2"] {
            let body = f.summary(Some("zh-CN"), count, "utc+08:00").await?;
            assert_eq!(body["reading"], format!("已阅读 {count} 篇文章"));
            assert_eq!(body["locale"], "zh-CN");
            assert_eq!(body["date"], "2026年10月29日");
            assert_eq!(body["as_of"], "统计截至 2026年10月29日");
            assert_eq!(body["local_iso"], "2026-10-29T07:30:00+08:00");
            assert_eq!(body["time_zone"], "utc+08:00");
            assert_ne!(
                body["datetime"]
                    .as_str()
                    .ok_or("formatted datetime missing")?,
                ""
            );
        }
        let utc = f.summary(Some("en"), "1", "utc").await?;
        let east = f.summary(Some("en"), "1", "utc+08:00").await?;
        assert_ne!(utc["datetime"], east["datetime"]);
        for locale in [None, Some("xx-YY"), Some("zh-CN;q=0,en;q=0.8")] {
            let body = f.summary(locale, "2", "utc").await?;
            assert_eq!(body["locale"], "en");
            assert_eq!(body["reading"], "2 articles read");
        }
        let preferred = f.summary(Some("en;q=0.2,zh-CN;q=0.9"), "2", "utc").await?;
        assert_eq!(preferred["locale"], "zh-CN");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let cleanup = f.close().await;
    result?;
    cleanup
}
#[tokio::test]
async fn reading_summary_rejects_unbounded_or_ambiguous_input_over_tcp() -> TestResult {
    let f = Fixture::create().await?;
    let result = async {
        for parameters in [
            vec![("count", "1000001"), ("at", AT), ("zone", "utc")],
            vec![("count", "-1"), ("at", AT), ("zone", "utc")],
            vec![("count", "1.5"), ("at", AT), ("zone", "utc")],
            vec![
                ("count", "1"),
                ("at", "not-a-date-sentinel"),
                ("zone", "utc"),
            ],
            vec![
                ("count", "1"),
                ("at", "1999-01-01T00:00:00Z"),
                ("zone", "utc"),
            ],
            vec![("count", "1"), ("at", AT), ("zone", "Asia/Shanghai")],
            vec![("count", "1"), ("at", AT)],
            vec![
                ("count", "1"),
                ("at", AT),
                ("zone", "utc"),
                ("token", "invalid-input-sentinel"),
            ],
        ] {
            let response = f.response(Some("en"), &parameters).await?;
            assert_eq!(response.status().as_u16(), 400);
            let text = response.text().await?;
            assert!(!text.contains("not-a-date-sentinel"));
            assert!(!text.contains("invalid-input-sentinel"));
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let cleanup = f.close().await;
    result?;
    cleanup
}

/// PHP query 参数后值覆盖前值，业务校验使用最终解析值。
#[tokio::test]
async fn reading_summary_uses_last_duplicate_query_value_over_tcp() -> TestResult {
    let f = Fixture::create().await?;
    let result = async {
        let response = f
            .response(
                Some("en"),
                &[("count", "1"), ("at", AT), ("zone", "utc"), ("count", "2")],
            )
            .await?;
        assert_eq!(response.status().as_u16(), 200);
        let body: Value = response.json().await?;
        assert_eq!(body["success"], true);
        assert_eq!(body["data"]["count"], 2);
        assert_eq!(body["data"]["reading"], "2 articles read");
        assert_eq!(body["data"]["locale"], "en");
        assert_eq!(body["data"]["preview"], true);
        assert_eq!(body["data"]["time_zone"], "utc");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let cleanup = f.close().await;
    result?;
    cleanup
}

async fn translated_error(
    response: reqwest::Response,
    status: u16,
    code: &str,
    message: &str,
) -> TestResult<Value> {
    assert_eq!(response.status().as_u16(), status);
    let text = response.text().await?;
    assert!(!text.contains("translation-private-sentinel"));
    let body: Value = serde_json::from_str(&text)?;
    assert_eq!(body["success"], false);
    assert_eq!(body["code"], code);
    assert_eq!(body["message"], message);
    assert!(body["request_id"].as_str().is_some_and(|id| !id.is_empty()));
    Ok(body)
}

fn validation_messages(locale: &str) -> (&'static str, &'static str, &'static str) {
    if locale == "zh-CN" {
        (
            "阅读计数不能超过 1000000。",
            "阅读时区请选择 utc 或 utc+08:00。",
            "阅读计数不能超过 1000000。（另有 1 项错误）",
        )
    } else {
        (
            "Reading count must not exceed 1000000.",
            "Choose utc or utc+08:00 for the reading time zone.",
            "Reading count must not exceed 1000000. (and 1 more error)",
        )
    }
}

#[tokio::test]
async fn reading_validation_uses_configured_default_and_explicit_locale_for_summary_and_fields()
-> TestResult {
    for default in ["en", "zh-CN"] {
        let fixture = Fixture::create_with_locale(default).await?;
        let outcome = async {
            for requested in [None, Some("en"), Some("zh-CN"), Some("fr")] {
                let selected = requested.filter(|value| *value != "fr").unwrap_or(default);
                let (count, zone, summary) = validation_messages(selected);
                let response = fixture
                    .response(
                        requested,
                        &[("count", "1000001"), ("at", AT), ("zone", "unsupported")],
                    )
                    .await?;
                let body = translated_error(response, 400, "validation_failed", summary).await?;
                assert_eq!(body["errors"]["count"][0], count);
                assert_eq!(body["errors"]["zone"][0], zone);
                assert_eq!(
                    body["errors"].as_object().map(serde_json::Map::len),
                    Some(2)
                );
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let cleanup = fixture.close().await;
        outcome?;
        cleanup?;
    }
    Ok(())
}

#[tokio::test]
async fn normal_config_read_failure_keeps_safe_localized_5xx_and_stable_code() -> TestResult {
    for default in ["en", "zh-CN"] {
        let fixture = Fixture::create_with_locale(default).await?;
        let outcome = async {
            // 可信运行期原始配置写入可制造类型错误，下一次真实业务读取必须安全失败。
            fixture.app.app()?.config_repository().set(
                "example.greeting",
                serde_json::json!({"private_source":"translation-private-sentinel"}),
            )?;
            for requested in [None, Some("en"), Some("zh-CN")] {
                let selected = requested.unwrap_or(default);
                let expected = if selected == "zh-CN" {
                    "服务器内部错误"
                } else {
                    "internal server error"
                };
                let mut request = fixture
                    .client
                    .get(format!(
                        "http://{}/api/example/config-greeting",
                        fixture.address
                    ))
                    .header("accept", "application/json");
                if let Some(locale) = requested {
                    request = request.header("accept-language", locale);
                }
                let body =
                    translated_error(request.send().await?, 500, "configuration_error", expected)
                        .await?;
                assert!(body.get("errors").is_none_or(Value::is_null));
                assert!(!body.to_string().contains("example.greeting"));
                assert!(!body.to_string().contains("private_source"));
            }
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let cleanup = fixture.close().await;
        outcome?;
        cleanup?;
    }
    Ok(())
}
