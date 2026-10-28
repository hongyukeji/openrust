//! 模块阅读摘要预览的真实 TCP locale/复数/固定偏移格式化验收。

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
        let mut defaults = openrust::config::FrameworkConfig::default();
        defaults.api.http_status = openrust::config::HttpStatusMode::Semantic;
        let storage = tempfile::tempdir()?;
        let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
        let source=Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test"},
            "cache":{"default_store":"default","stores":{"default":{"driver":"memory"}}},
            "storage":{"default_disk":"local","disks":{"local":{"driver":"fs","root":storage.path()}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"localization-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}},
            "i18n":{"default_locale":"en","fallback_locale":"en"}
        })));
        let app = TestApp::<crate::bootstrap::Server>::builder(source)
            .build()
            .await?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = openrust::http::build_router(app.app()?)?;
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
            vec![("count", "1"), ("at", AT), ("zone", "utc"), ("count", "2")],
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
