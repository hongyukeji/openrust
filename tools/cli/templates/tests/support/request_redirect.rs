//! 生成应用通过公开 Provider、Redirector 与 Session 错误包扩展验证跨请求业务流程。
use figment::Figment;
use openrust::config::SessionConfig;
use openrust::core::{Application, BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use openrust::http::{
    Json, RedirectErrors, Redirector, Request as InputRequest, RequestInfo, Routes,
};
use openrust::session::{ErrorBagCodecs, MemorySessionStore, SessionServiceProvider};
use openrust::support::{
    ArrayKey, Collection, MessageBag, MessageBagContract, MessageKeys, MessageLookup,
    MessageProvider, MessageValue,
};
use openrust::testing::{TestApp, TestResponse};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

const PREFIX: &str = "/__framework_contract/redirect";
const TIMEOUT: Duration = Duration::from_secs(5);

struct RedirectRoutes;
impl ServiceProvider for RedirectRoutes {
    fn register(&self, app: &mut RegisterContext<'_>) -> openrust::Result<()> {
        app.extend::<ErrorBagCodecs, _>(|original, _resolver| {
            Box::pin(async move {
                let mut codecs = (*original).clone();
                codecs.register::<GeneratedErrors>(
                    "generated.redirect-errors",
                    1,
                    encode_errors,
                    decode_errors,
                )?;
                Ok(Arc::new(codecs))
            })
        });
        Ok(())
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| {
                routes
                    .get(&format!("{PREFIX}/plain"), plain)
                    .post(&format!("{PREFIX}/submit"), submit)
                    .get(&format!("{PREFIX}/review"), review)
                    .name("generated.draft.review")
                    .get(&format!("{PREFIX}/view"), view)
            })?;
            Ok(())
        })
    }
}

struct RedirectApplication<A>(std::marker::PhantomData<fn() -> A>);
impl<A: Application> Application for RedirectApplication<A> {
    type Config = A::Config;
    fn config_namespace() -> &'static str {
        A::config_namespace()
    }
    fn providers(config: &Self::Config) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers = A::providers(config);
        if !providers.iter().any(|provider| {
            provider.id() == openrust::core::ProviderId::of::<SessionServiceProvider>()
        }) {
            providers.push(Box::new(
                SessionServiceProvider::with_store(MemorySessionStore::default())
                    .with_cookie_config(SessionConfig {
                        driver: Some("array".into()),
                        cookie: Some("generated_redirect_session".into()),
                        secure: Some(false),
                        ..SessionConfig::default()
                    }),
            ));
        }
        providers.push(Box::new(RedirectRoutes));
        providers
    }
    fn modules(config: &Self::Config) -> openrust::core::ModuleManifest {
        A::modules(config)
    }
}

async fn plain(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info).to("/api")?.into_response())
}
async fn submit(request: InputRequest) -> openrust::Result<Response> {
    let mut redirect =
        Redirector::from_request(&request).route("generated.draft.review", &BTreeMap::new())?;
    redirect.except_input(&["token"])?;
    redirect.with_errors(RedirectErrors::Text("摘要缺失"), "other")?;
    let errors = GeneratedErrors::new("请检查标题")?;
    redirect.with_errors(RedirectErrors::Provider(&errors), "default")?;
    Ok(redirect.into_response())
}
async fn review(info: RequestInfo) -> openrust::Result<Json<Value>> {
    let errors = info.session()?.read_errors()?;
    Ok(Json(json!({
        "old": info.old_all()?,
        "default_present": errors.has_bag("default"),
        "default_title": errors.get_bag("default").as_bag().first(Some("title"), None)?,
        "other_present": errors.has_bag("other"),
        "other_message": errors.get_bag("other").as_bag().first(None, None)?,
    })))
}

async fn view(
    info: RequestInfo,
    openrust::prelude::Inject(views): openrust::prelude::Inject<openrust::view::Views>,
) -> openrust::Result<openrust::http::axum::response::Html<String>> {
    views.render_for_request("index.html", &info, &json!({"app_name":"Generated"}))
}

struct GeneratedErrors {
    messages: MessageBag,
    guidance: String,
}
impl GeneratedErrors {
    fn new(guidance: impl Into<String>) -> openrust::Result<Self> {
        let mut messages = MessageBag::new();
        messages.add("title", "标题待检查")?;
        Ok(Self {
            messages,
            guidance: guidance.into(),
        })
    }
}
impl MessageProvider for GeneratedErrors {
    fn message_bag(&self) -> &dyn MessageBagContract {
        self
    }
}
impl MessageBagContract for GeneratedErrors {
    fn keys(&self) -> Vec<ArrayKey> {
        self.messages.keys()
    }
    fn messages(&self) -> &Collection<Collection<MessageValue>> {
        self.messages.messages()
    }
    fn to_array(&self) -> &Collection<Collection<MessageValue>> {
        self.messages.to_array()
    }
    fn format(&self) -> &str {
        self.messages.format()
    }
    fn set_format(&mut self, format: String) {
        self.messages.set_format(format);
    }
    fn add(&mut self, key: ArrayKey, message: String) -> openrust::Result<()> {
        self.messages.add(key, message)
    }
    fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> openrust::Result<()> {
        self.messages.merge(messages)
    }
    fn has(&self, keys: MessageKeys) -> openrust::Result<bool> {
        self.messages.has(keys)
    }
    fn first(&self, key: Option<&str>, format: Option<&str>) -> openrust::Result<Option<String>> {
        if key == Some("title") && format.is_none() {
            return Ok(Some(format!("生成项目：{}", self.guidance)));
        }
        self.messages.first(key, format)
    }
    fn get(&self, key: &str, format: Option<&str>) -> openrust::Result<MessageLookup> {
        self.messages.get(key, format)
    }
    fn all(&self, format: Option<&str>) -> openrust::Result<Collection<MessageValue>> {
        self.messages.all(format)
    }
    fn forget(&mut self, key: &ArrayKey) {
        self.messages.forget(key.clone());
    }
    fn count(&self) -> usize {
        self.messages.count()
    }
    fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
    fn is_not_empty(&self) -> bool {
        self.messages.is_not_empty()
    }
}
fn encode_errors(bag: &GeneratedErrors) -> openrust::Result<Value> {
    Ok(json!({"guidance": bag.guidance}))
}
fn decode_errors(value: Value) -> openrust::Result<GeneratedErrors> {
    let guidance = value
        .get("guidance")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            openrust::Error::custom("generated_redirect_error_invalid", "错误包格式无效")
        })?;
    GeneratedErrors::new(guidance)
}

fn cookie_pair(response: &TestResponse) -> Result<String, Box<dyn std::error::Error>> {
    let value = response
        .headers()
        .get(header::SET_COOKIE)
        .ok_or("缺少 Session Cookie")?
        .to_str()?;
    Ok(value
        .split(';')
        .next()
        .ok_or("Session Cookie 格式无效")?
        .to_owned())
}
async fn get<A: Application>(
    app: &TestApp<A>,
    path: &str,
    cookie: Option<&str>,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    let mut request = Request::get(path).header("host", "localhost");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    Ok(app
        .request_with_limits(request.body(Body::empty())?, 4096, TIMEOUT)
        .await?)
}
fn data(response: &TestResponse) -> Result<Value, Box<dyn std::error::Error>> {
    response.assert_status(StatusCode::OK)?;
    let body: Value = response.json()?;
    Ok(body["data"].clone())
}

/// 经生成应用 Provider、命名路由、Request 和 Session Cookie 验证重定向与一次性错误包。
pub async fn assert_request_redirect<A: Application>(
    source: Figment,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = source.merge(figment::providers::Serialized::defaults(
        json!({"app":{"url":"http://localhost"}}),
    ));
    let mut app = TestApp::<RedirectApplication<A>>::builder(source)
        .build()
        .await?;
    let result: Result<(), Box<dyn std::error::Error>> = async {
        let plain = get(&app, &format!("{PREFIX}/plain"), None).await?;
        plain.assert_status(StatusCode::FOUND)?;
        assert_eq!(
            plain
                .headers()
                .get(header::LOCATION)
                .ok_or("Location 缺失")?
                .to_str()?,
            "http://localhost/api"
        );
        assert!(std::str::from_utf8(plain.body())?.contains("<!DOCTYPE html>"));
        let first = get(&app, &format!("{PREFIX}/review"), None).await?;
        let cookie = cookie_pair(&first)?;
        let body = json!({"title":"  Ada  ","token":"private"});
        let response = app
            .request_with_limits(
                Request::post(format!("{PREFIX}/submit"))
                    .header("host", "localhost")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&body)?))?,
                4096,
                TIMEOUT,
            )
            .await?;
        response.assert_status(StatusCode::FOUND)?;
        assert_eq!(
            response
                .headers()
                .get(header::LOCATION)
                .ok_or("Location 缺失")?
                .to_str()?,
            format!("http://localhost{PREFIX}/review")
        );
        let cookie = cookie_pair(&response)?;
        let current = get(&app, &format!("{PREFIX}/review"), Some(&cookie)).await?;
        assert_eq!(
            data(&current)?,
            json!({
                "old":{"title":"Ada"},
                "default_present":true,
                "default_title":"生成项目：请检查标题",
                "other_present":true,
                "other_message":"摘要缺失"
            })
        );
        let cookie = cookie_pair(&current)?;
        let aged = get(&app, &format!("{PREFIX}/review"), Some(&cookie)).await?;
        assert_eq!(
            data(&aged)?,
            json!({
                "old":[],"default_present":false,"default_title":"",
                "other_present":false,"other_message":""
            })
        );
        let cookie = cookie_pair(&aged)?;
        let response = app
            .request_with_limits(
                Request::post(format!("{PREFIX}/submit"))
                    .header("host", "localhost")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::to_vec(&body)?))?,
                4096,
                TIMEOUT,
            )
            .await?;
        response.assert_status(StatusCode::FOUND)?;
        let cookie = cookie_pair(&response)?;
        let rendered = get(&app, &format!("{PREFIX}/view"), Some(&cookie)).await?;
        rendered.assert_status(StatusCode::OK)?;
        let html = std::str::from_utf8(rendered.body())?;
        assert!(html.contains("<title>Generated</title>"));
        assert!(html.contains("<h1>Generated</h1>"));
        assert!(html.contains("生成项目：请检查标题"));
        assert!(html.contains("摘要缺失"));
        let cookie = cookie_pair(&rendered)?;
        let aged_view = get(&app, &format!("{PREFIX}/view"), Some(&cookie)).await?;
        aged_view.assert_status(StatusCode::OK)?;
        let aged_html = std::str::from_utf8(aged_view.body())?;
        assert!(!aged_html.contains("生成项目：请检查标题"));
        assert!(!aged_html.contains("摘要缺失"));
        Ok(())
    }
    .await;
    let shutdown = app
        .shutdown(openrust::tokio::time::Instant::now() + TIMEOUT)
        .await;
    result?;
    shutdown?;
    Ok(())
}
