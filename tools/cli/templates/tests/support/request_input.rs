//! 生成应用在公开 Provider 与 Request API 上的外部消费回归。
use figment::Figment;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::support::BoxFuture;
use openrust::support::ServiceProvider;
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};
use openrust::http::InputOutcome;
use openrust::http::Json;
use openrust::http::Request as InputRequest;
use openrust::routing::Router;
use openrust::http::UploadedFile;
use openrust::support::{ArrayKey, BackedEnum, DateOptions, InputNumber, TimezoneInput};
use openrust::testing::TestApp;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
enum Approval {
    Accepted,
}

impl BackedEnum for Approval {
    type Backing = i64;

    fn try_from_backing(value: i64) -> Option<Self> {
        (value == 1).then_some(Self::Accepted)
    }
}

struct InputRoutesProvider;

impl ServiceProvider for InputRoutesProvider {
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Router| {
                routes
                    .post("/test/request-input", inspect_input)
                    .post("/test/request-file", inspect_file)
                    .route(
                        "/test/request-url",
                        openrust::routing::methods::get(inspect_url).post(inspect_url),
                    )
                    .name("test.request-url")
            })?;
            Ok(())
        })
    }
}

// 生成项目从公开模块外接线读取文件，不依赖框架私有暂存路径。
async fn inspect_file(request: InputRequest) -> openrust::Result<Json<serde_json::Value>> {
    let file = request
        .file("document")?
        .ok_or_else(|| openrust::Error::custom("file_missing", "文件缺失"))?;
    let uploaded = file
        .downcast_ref::<UploadedFile>()
        .ok_or_else(|| openrust::Error::custom("file_invalid", "文件无效"))?;
    let mime = uploaded.mime_type().await?;
    let extension = uploaded.extension().await?;
    let selected = request.only(&["document"])?;
    let selected_file = selected
        .get("document")?
        .ok_or_else(|| openrust::Error::custom("file_missing", "选取结果缺失"))?;
    Ok(Json(serde_json::json!({
        "name": uploaded.client_original_name(),
        "mime": mime,
        "extension": extension,
        "bytes": uploaded.size(),
        "has_file": request.has_file("document")?,
        "same_object": file.same_object(selected_file),
        "text": request.input("label")?,
        "raw_len": request.content_len(),
    })))
}

// 保留生成应用的 Provider 与模块装配，只在本测试追加公开路由扩展。
struct InputApplication<A>(std::marker::PhantomData<fn() -> A>);

impl<A: ApplicationDefinition> ApplicationDefinition for InputApplication<A> {
    type Config = A::Config;

    fn config() -> openrust::config::ConfigManifest {
        A::config()
    }
    fn default_providers() -> openrust::support::DefaultProviders {
        A::default_providers()
    }
    fn package_providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        A::package_providers(config)
    }
    fn additional_providers() -> Vec<Box<dyn ServiceProvider>> {
        A::additional_providers()
    }
    fn with_bootstrap_providers() -> bool {
        A::with_bootstrap_providers()
    }

    fn config_namespace() -> &'static str {
        A::config_namespace()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        providers.push(Box::new(InputRoutesProvider));
        providers
    }

    fn configure_providers(config: &Self::Config, builder: &mut openrust::foundation::ProviderBuilder) {
        A::configure_providers(config, builder);
    }
}

async fn inspect_input(request: InputRequest) -> openrust::Result<Json<serde_json::Value>> {
    let items = request.collect("items")?;
    let profile = request.fluent("profile")?;
    let date = request
        .date_with(
            "date",
            DateOptions {
                format: Some("!Y-m-d H:i:s"),
                timezone: Some(TimezoneInput::Name("Asia/Shanghai")),
            },
        )?
        .ok_or_else(|| openrust::Error::custom("test_date_missing", "测试日期缺失"))?;
    let interval = request
        .interval("interval")?
        .ok_or_else(|| openrust::Error::custom("test_interval_missing", "测试区间缺失"))?;
    let approval = request.enum_value::<Approval>("approval")?;
    let has_false_continues = matches!(
        request.when_has("zero", |_| Ok(false))?,
        InputOutcome::Request(_)
    );
    let missing_default_is_false = matches!(
        request.when_missing_or_else("zero", |_| Ok(true), || Ok(false))?,
        InputOutcome::Default(false)
    );

    Ok(Json(serde_json::json!({
        "string": request.string("name")?.to_string(),
        "null_string_default": request.string_or("nil", "fallback")?.to_string(),
        "missing_string_default": request.string_or("missing", "fallback")?.to_string(),
        "null_integer_default": request.integer_or("nil", 7)?,
        "missing_integer_default": request.integer_or("missing", 7)?,
        "float": request.float("float")?,
        "missing_boolean_default": request.boolean_or("missing", true)?,
        "approval": approval == Some(Approval::Accepted),
        "unknown_approval_default": request.enum_or("unknown_approval", Approval::Accepted)? == Approval::Accepted,
        "item_string_key": items.get(ArrayKey::from("02")).cloned(),
        "item_integer_key": items.get(2_i64).cloned(),
        "profile_name": profile.get("person.name")?,
        "clamped": request.clamp("score", InputNumber::Integer(0), InputNumber::Integer(10))?,
        "has_false_continues": has_false_continues,
        "missing_default_is_false": missing_default_is_false,
        "date": date.format("Y-m-d H:i:s P")?,
        "interval": {
            "years": interval.years,
            "months": interval.months,
            "days": interval.days,
            "hours": interval.hours,
            "minutes": interval.minutes,
            "seconds": interval.seconds,
            "microseconds": interval.microseconds,
            "total_days": interval.total_days(),
        }
    })))
}

// 捕获生成应用消费公开 URL API 时 query bag 与原始 URI 混淆的回归。
async fn inspect_url(mut request: InputRequest) -> openrust::Result<Json<serde_json::Value>> {
    let full_url = request.full_url()?;
    let route = request.route()?.ok_or_else(|| {
        openrust::Error::custom("generated_route_missing", "生成应用未发布匹配路由")
    })?;
    request.merge(serde_json::json!({"page": 3}))?;
    let merged = request.full_url_with_query(&serde_json::json!({"view": "compact"}))?;
    let without = request.full_url_without_query(&["label"])?;
    request.replace(serde_json::json!({"page": 4}))?;
    Ok(Json(serde_json::json!({
        "route_name": request.route_name()?,
        "route_is": request.route_is(&["other.*", "test.*"])?,
        "route_uri": route.uri(), "route_methods": route.methods(),
        "route_missing": request.route_parameter("missing")?,
        "route_default": request.route_parameter_or("missing", serde_json::json!("fallback"))?,
        "root": request.root()?, "url": request.url()?, "full_url": full_url,
        "after": request.full_url()?, "scheme": request.scheme()?,
        "http_host": request.http_host()?, "port": request.port()?,
        "base_url": request.base_url()?, "client_ip": request.client_ip()?,
        "client_ips": request.client_ips()?,
        "decoded_path": request.decoded_path()?, "segments": request.segments()?,
        "segment": request.segment(2)?, "zero": request.segment(0)?,
        "is": request.is(&["test/*"])?, "full_url_is": request.full_url_is(&["*/test/request-url*"])?,
        "prefetch": request.prefetch()?, "merged": merged, "without": without,
        "replaced": request.full_url_with_query(&serde_json::json!({"view": "compact"}))?,
    })))
}

/// 通过生成应用自身的 Provider 清单发送进程内请求，不代表 TCP 验收。
pub async fn assert_request_input<A: ApplicationDefinition>(
    source: Figment,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = TestApp::<InputApplication<A>>::builder(source)
        .build()
        .await?;
    let result: Result<(), Box<dyn std::error::Error>> = async {
        let body = serde_json::json!({
            "name": "  Ada  ",
            "nil": null,
            "float": "2.5suffix",
            "approval": "1",
            "unknown_approval": 8,
            "items": {"02": "string", "2": "integer"},
            "profile": {"person": {"name": "Ada"}},
            "score": 14,
            "zero": 0,
            "date": "2024-03-10 12:30:40",
            "interval": "P1Y2M3DT4H5M6S"
        });
        let request = Request::builder()
            .method(Method::POST)
            .uri("/test/request-input")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body)?))?;
        app.request(request)
            .await?
            .assert_status(StatusCode::OK)?
            .assert_json(&serde_json::json!({
                "success": true,
                "code": "ok",
                "message": "ok",
                "data": {
                "string": "Ada",
                "null_string_default": "",
                "missing_string_default": "fallback",
                "null_integer_default": 0,
                "missing_integer_default": 7,
                "float": 2.5,
                "missing_boolean_default": true,
                "approval": true,
                "unknown_approval_default": true,
                "item_string_key": "string",
                "item_integer_key": "integer",
                "profile_name": "Ada",
                "clamped": 10,
                "has_false_continues": true,
                "missing_default_is_false": true,
                "date": "2024-03-10 12:30:40 +08:00",
                "interval": {
                    "years": 1,
                    "months": 2,
                    "days": 3,
                    "hours": 4,
                    "minutes": 5,
                    "seconds": 6,
                    "microseconds": 0,
                    "total_days": null
                }
                }
            }))?;
        let multipart = concat!(
            "--generated\r\nContent-Disposition: form-data; name=\"label\"\r\n\r\npublic\r\n",
            "--generated\r\nContent-Disposition: form-data; name=\"document\"; filename=\"client.txt\"\r\nContent-Type: text/plain\r\n\r\nhello\r\n",
            "--generated--\r\n"
        );
        app.request(Request::builder()
            .method(Method::POST).uri("/test/request-file")
            .header(CONTENT_TYPE, "multipart/form-data; boundary=generated")
            .body(Body::from(multipart))?)
            .await?
            .assert_status(StatusCode::OK)?
            .assert_json(&serde_json::json!({
                "success": true,
                "code": "ok",
                "message": "ok",
                "data": {
                    "name": "client.txt", "mime": "text/plain", "extension": "txt",
                    "bytes": 5, "has_file": true,
                    "same_object": true, "text": "public", "raw_len": multipart.len()
                }
            }))?;
        for (method, json_source) in [
            (Method::GET, false),
            (Method::GET, true),
            (Method::POST, true),
        ] {
            let mutates_query = method == Method::GET && !json_source;
            let mut builder = Request::builder()
                .method(method)
                .uri("/test/request-url?page=1&label=public")
                .header("host", "example.test:8080")
                .header("purpose", "prefetch");
            if json_source {
                builder = builder.header(CONTENT_TYPE, "application/json");
            }
            let response = app
                .request(builder.body(if json_source {
                    Body::from(r#"{"secret":"body-only"}"#)
                } else {
                    Body::empty()
                })?)
                .await?;
            response.assert_status(StatusCode::OK)?;
            let json: serde_json::Value = serde_json::from_slice(response.body())?;
            let data = &json["data"];
            let base = "http://example.test:8080/test/request-url";
            assert_eq!(data["route_name"], "test.request-url");
            assert_eq!(data["route_is"], true);
            assert_eq!(data["route_uri"], "test/request-url");
            assert_eq!(
                data["route_methods"],
                serde_json::json!(["GET", "HEAD", "POST"])
            );
            assert_eq!(data["route_missing"], serde_json::Value::Null);
            assert_eq!(data["route_default"], "fallback");
            assert_eq!(data["root"], "http://example.test:8080");
            assert_eq!(data["url"], base);
            assert_eq!(data["full_url"], format!("{base}?label=public&page=1"));
            assert_eq!(data["after"], data["full_url"]);
            assert_eq!(data["scheme"], "http");
            assert_eq!(data["http_host"], "example.test:8080");
            assert_eq!(data["port"], 8080);
            assert_eq!(data["base_url"], "");
            assert_eq!(data["client_ip"], serde_json::Value::Null);
            assert_eq!(data["client_ips"], serde_json::json!([]));
            assert_eq!(data["decoded_path"], "test/request-url");
            assert_eq!(data["segments"], serde_json::json!(["test", "request-url"]));
            assert_eq!(data["segment"], "request-url");
            assert_eq!(data["zero"], serde_json::Value::Null);
            assert_eq!(data["is"], true);
            assert_eq!(data["full_url_is"], true);
            assert_eq!(data["prefetch"], true);
            let page = if mutates_query { 3 } else { 1 };
            assert_eq!(
                data["merged"],
                format!("{base}?page={page}&label=public&view=compact")
            );
            assert_eq!(data["without"], format!("{base}?page={page}"));
            let replaced = if mutates_query {
                format!("{base}?page=4&view=compact")
            } else {
                format!("{base}?page=1&label=public&view=compact")
            };
            assert_eq!(data["replaced"], replaced);
        }
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    result?;
    shutdown?;
    Ok(())
}
