//! 生成应用在公开 Provider 与 Request API 上的外部消费回归。
use figment::Figment;
use openrust::core::{Application, BootContext, BoxFuture, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode, header::CONTENT_TYPE},
};
use openrust::http::{InputOutcome, Json, Request as InputRequest, Routes};
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
            app.routes(|routes: Routes| routes.post("/test/request-input", inspect_input))?;
            Ok(())
        })
    }
}

// 保留生成应用的 Provider 与模块装配，只在本测试追加公开路由扩展。
struct InputApplication<A>(std::marker::PhantomData<fn() -> A>);

impl<A: Application> Application for InputApplication<A> {
    type Config = A::Config;

    fn config_namespace() -> &'static str {
        A::config_namespace()
    }

    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        providers.push(Box::new(InputRoutesProvider));
        providers
    }

    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        A::modules(config)
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

/// 通过生成应用自身的 Provider 清单发送进程内请求，不代表 TCP 验收。
pub async fn assert_request_input<A: Application>(
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
