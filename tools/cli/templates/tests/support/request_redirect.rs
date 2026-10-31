//! 生成应用通过公开 Provider、Redirector 与 Session 错误包扩展验证跨请求业务流程。
use figment::Figment;
use openrust::config::SessionConfig;
use openrust::core::{Application, BootContext, BoxFuture, RegisterContext, ServiceProvider};
use openrust::http::axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, Request, StatusCode, header},
    response::{IntoResponse, Response},
};
use openrust::http::{
    Json, RedirectErrors, Redirector, Request as InputRequest, RequestInfo, RouteGroup, Routes,
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
const DRAFT_GUIDE: &str = "https://laravel.com/docs/11.x/redirects?source=openrust&section=draft#redirecting-to-external-domains";

struct RedirectRoutes;
impl ServiceProvider for RedirectRoutes {
    fn register<'a>(
        &'a self,
        app: &'a mut RegisterContext<'_>,
    ) -> openrust::core::BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
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
            })
            .await?;
            Ok(())
        })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            app.routes(|routes: Routes| {
                routes
                    .group(
                        RouteGroup::new()
                            .prefix(format!("{PREFIX}/actions"))
                            .namespace("Generated\\Draft"),
                        |drafts| {
                            drafts
                                // 静态概览先于参数路由；后登记的模块概览替换默认实现。
                                .get("/summary", summary_default)
                                .get_action("/summary", "DraftSummaryController", summary_page)
                                .get_action("/{id}", "DraftController@show", action_page)
                                .get_action("/{id}/preview", "PreviewDraftController", action_page)
                        },
                    )
                    .get(&format!("{PREFIX}/action"), action_redirect)
                    .get(&format!("{PREFIX}/action-preview"), action_preview)
                    .on_action(
                        &format!("{PREFIX}/action-submit"),
                        &["POST"],
                        ("Generated\\Draft\\DraftController", "store"),
                        action_submit,
                    )
                    .get(&format!("{PREFIX}/plain"), plain)
                    .get(&format!("{PREFIX}/secure"), secure)
                    .get(&format!("{PREFIX}/guide"), guide)
                    .get(&format!("{PREFIX}/refresh"), refresh_page)
                    .post(&format!("{PREFIX}/refresh"), refresh_submit)
                    .get(&format!("{PREFIX}/preview"), preview)
                    .get(&format!("{PREFIX}/confirm"), confirmation)
                    .post(&format!("{PREFIX}/confirm"), confirm)
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

async fn summary_default() -> Json<Value> {
    Json(json!({"source":"默认草稿概览"}))
}
async fn summary_page(info: RequestInfo) -> openrust::Result<Json<Value>> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("generated_route_missing", "草稿路由缺失"))?;
    Ok(Json(
        json!({"source":"生成草稿概览","action":route.action(),"id":route.parameter("id")}),
    ))
}

fn action_parameters() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("id".into(), "42".into()),
        ("section".into(), "review".into()),
    ])
}
async fn action_redirect(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info)
        .action(
            ("Generated\\Draft\\DraftController", "show"),
            &action_parameters(),
        )?
        .into_response())
}
async fn action_preview(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info)
        .action(
            "\\Generated\\Draft\\PreviewDraftController",
            &action_parameters(),
        )?
        .into_response())
}
async fn action_submit(request: InputRequest) -> openrust::Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-generated-draft-action",
        HeaderValue::from_static("review"),
    );
    let mut redirect = Redirector::from_request(&request).action_with(
        "\\Generated\\Draft\\DraftController@show",
        &action_parameters(),
        StatusCode::SEE_OTHER,
        headers,
    )?;
    redirect.only_input(&["title"])?;
    redirect.with_errors(RedirectErrors::Text("请复核动作草稿标题"), "default")?;
    Ok(redirect.into_response())
}
async fn action_page(info: RequestInfo) -> openrust::Result<Json<Value>> {
    let route = info
        .route()?
        .ok_or_else(|| openrust::Error::custom("generated_action_route_missing", "动作路由缺失"))?;
    Ok(Json(json!({
        "action":route.action(), "id":route.parameter("id"), "name":route.name(),
        "old_title":info.old("title")?,
        "title_error":info.session()?.read_errors()?.get_bag("default").as_bag().first(None, None)?,
    })))
}

async fn plain(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info).to("/api")?.into_response())
}
async fn secure(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info).secure("/api")?.into_response())
}
async fn guide(info: RequestInfo) -> openrust::Result<Response> {
    Ok(Redirector::from_info(&info)
        .away(DRAFT_GUIDE)?
        .into_response())
}
async fn refresh_submit(request: InputRequest) -> openrust::Result<Response> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-generated-draft-refresh",
        HeaderValue::from_static("review"),
    );
    let mut redirect =
        Redirector::from_request(&request).refresh_with(StatusCode::SEE_OTHER, headers)?;
    redirect.with_input()?;
    redirect.with_errors(RedirectErrors::Text("请检查草稿标题"), "default")?;
    Ok(redirect.into_response())
}
async fn refresh_page(info: RequestInfo) -> openrust::Result<Json<Value>> {
    let errors = info.session()?.read_errors()?;
    Ok(Json(json!({
        "draft":"生成草稿",
        "old_title":info.old("title")?,
        "title_error":errors.get_bag("default").as_bag().first(None, None)?,
    })))
}
const PREVIEW_CONFIRMED: &str = "generated.draft.preview_confirmed";

// 仅为非敏感草稿预览记录确认；不创建用户身份或替代 Auth/Policy。
async fn preview(info: RequestInfo) -> openrust::Result<Response> {
    if info.session()?.get(PREVIEW_CONFIRMED)? == Some(Value::Bool(true)) {
        return Ok(Json(json!({"draft":"生成草稿","preview_confirmed":true})).into_response());
    }
    Ok(Redirector::from_info(&info)
        .guest(&format!("{PREFIX}/confirm"))?
        .into_response())
}
async fn confirmation(info: RequestInfo) -> openrust::Result<Json<Value>> {
    Ok(Json(
        json!({"intended":Redirector::from_info(&info).get_intended_url()?}),
    ))
}
async fn confirm(request: InputRequest) -> openrust::Result<Response> {
    request.info().session()?.put(PREVIEW_CONFIRMED, true)?;
    Ok(Redirector::from_request(&request)
        .intended()?
        .into_response())
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
    let mut request = Request::get(path)
        .header("host", "localhost")
        .header(header::ACCEPT, "text/html");
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

fn assert_redirect(
    response: &TestResponse,
    target: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    response.assert_status(StatusCode::FOUND)?;
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .ok_or("Location 缺失")?
            .to_str()?,
        target
    );
    Ok(())
}
async fn post_confirmation<A: Application>(
    app: &TestApp<A>,
    cookie: &str,
) -> Result<TestResponse, Box<dyn std::error::Error>> {
    Ok(app
        .request_with_limits(
            Request::post(format!("{PREFIX}/confirm"))
                .header("host", "localhost")
                .header(header::ACCEPT, "text/html")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::COOKIE, cookie)
                .body(Body::from("{}"))?,
            4096,
            TIMEOUT,
        )
        .await?)
}
async fn assert_navigation<A: Application>(
    app: &TestApp<A>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 仅读取 HTTPS Location，不请求不存在的 TLS 监听器。
    assert_redirect(
        &get(app, &format!("{PREFIX}/secure"), None).await?,
        "https://localhost/api",
    )?;
    let paths = [
        format!("{PREFIX}/preview?section=summary"),
        format!("{PREFIX}/preview?section=history"),
    ];
    let first = get(app, &paths[0], None).await?;
    let second = get(app, &paths[1], None).await?;
    let confirmation_url = format!("http://localhost{PREFIX}/confirm");
    assert_redirect(&first, &confirmation_url)?;
    assert_redirect(&second, &confirmation_url)?;
    let mut cookies = [cookie_pair(&first)?, cookie_pair(&second)?];
    assert_ne!(cookies[0], cookies[1]);
    for _ in 0..2 {
        for (cookie, path) in cookies.iter_mut().zip(&paths) {
            let response = get(app, &format!("{PREFIX}/confirm"), Some(cookie)).await?;
            assert_eq!(
                data(&response)?["intended"],
                format!("http://localhost{path}")
            );
            *cookie = cookie_pair(&response)?;
        }
    }
    for (index, path) in paths.iter().enumerate() {
        let response = post_confirmation(app, &cookies[index]).await?;
        assert_redirect(&response, &format!("http://localhost{path}"))?;
        cookies[index] = cookie_pair(&response)?;
        let preview = get(app, path, Some(&cookies[index])).await?;
        assert_eq!(
            data(&preview)?,
            json!({"draft":"生成草稿","preview_confirmed":true})
        );
        cookies[index] = cookie_pair(&preview)?;
        let empty = get(app, &format!("{PREFIX}/confirm"), Some(&cookies[index])).await?;
        assert_eq!(data(&empty)?["intended"], Value::Null);
        cookies[index] = cookie_pair(&empty)?;
        let repeated = post_confirmation(app, &cookies[index]).await?;
        assert_redirect(&repeated, "http://localhost")?;
        cookies[index] = cookie_pair(&repeated)?;
        if index == 0 {
            let waiting = get(app, &format!("{PREFIX}/confirm"), Some(&cookies[1])).await?;
            assert_eq!(
                data(&waiting)?["intended"],
                format!("http://localhost{}", paths[1])
            );
            cookies[1] = cookie_pair(&waiting)?;
        }
    }
    Ok(())
}

async fn assert_refresh_and_away<A: Application>(
    app: &TestApp<A>,
) -> Result<(), Box<dyn std::error::Error>> {
    let guide = get(
        app,
        &format!("{PREFIX}/guide?url=https://untrusted.example.test"),
        None,
    )
    .await?;
    assert_redirect(&guide, DRAFT_GUIDE)?;
    let response = app
        .request_with_limits(
            Request::post(format!("{PREFIX}/refresh?discard=once"))
                .header("host", "localhost")
                .header(header::ACCEPT, "text/html")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"title":"  Recheck  "}"#))?,
            4096,
            TIMEOUT,
        )
        .await?;
    response.assert_status(StatusCode::SEE_OTHER)?;
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .ok_or("Location 缺失")?
            .to_str()?,
        format!("http://localhost{PREFIX}/refresh")
    );
    assert_eq!(
        response
            .headers()
            .get("x-generated-draft-refresh")
            .ok_or("刷新头缺失")?
            .to_str()?,
        "review"
    );
    let cookie = cookie_pair(&response)?;
    let page = get(app, &format!("{PREFIX}/refresh"), Some(&cookie)).await?;
    assert_eq!(
        data(&page)?,
        json!({
            "draft":"生成草稿","old_title":"Recheck","title_error":"请检查草稿标题"
        })
    );
    let cookie = cookie_pair(&page)?;
    let aged = get(app, &format!("{PREFIX}/refresh"), Some(&cookie)).await?;
    assert_eq!(
        data(&aged)?,
        json!({
            "draft":"生成草稿","old_title":null,"title_error":""
        })
    );
    Ok(())
}

async fn assert_actions<A: Application>(
    app: &TestApp<A>,
) -> Result<(), Box<dyn std::error::Error>> {
    let summary = get(app, &format!("{PREFIX}/actions/summary"), None).await?;
    assert_eq!(
        data(&summary)?,
        json!({"source":"生成草稿概览","action":"Generated\\Draft\\DraftSummaryController","id":null})
    );
    for (entry, suffix, action) in [
        ("action", "42", "Generated\\Draft\\DraftController@show"),
        (
            "action-preview",
            "42/preview",
            "Generated\\Draft\\PreviewDraftController",
        ),
    ] {
        let response = get(app, &format!("{PREFIX}/{entry}"), None).await?;
        assert_redirect(
            &response,
            &format!("http://localhost{PREFIX}/actions/{suffix}?section=review"),
        )?;
        let cookie = cookie_pair(&response)?;
        let page = get(app, &format!("{PREFIX}/actions/{suffix}"), Some(&cookie)).await?;
        assert_eq!(
            data(&page)?,
            json!({"action":action,"id":"42","name":null,"old_title":null,"title_error":""})
        );
    }
    let response = app
        .request_with_limits(
            Request::post(format!("{PREFIX}/action-submit"))
                .header("host", "localhost")
                .header(header::ACCEPT, "text/html")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"title":"  Action draft  ","token":"private"}"#,
                ))?,
            4096,
            TIMEOUT,
        )
        .await?;
    response.assert_status(StatusCode::SEE_OTHER)?;
    assert_eq!(
        response.headers()[header::LOCATION],
        format!("http://localhost{PREFIX}/actions/42?section=review")
    );
    assert_eq!(response.headers()["x-generated-draft-action"], "review");
    let cookie = cookie_pair(&response)?;
    let page = get(app, &format!("{PREFIX}/actions/42"), Some(&cookie)).await?;
    assert_eq!(
        data(&page)?,
        json!({"action":"Generated\\Draft\\DraftController@show","id":"42","name":null,"old_title":"Action draft","title_error":"请复核动作草稿标题"})
    );
    let cookie = cookie_pair(&page)?;
    let aged = data(&get(app, &format!("{PREFIX}/actions/42"), Some(&cookie)).await?)?;
    assert_eq!(aged["old_title"], Value::Null);
    assert_eq!(aged["title_error"], "");
    Ok(())
}

/// 经生成应用 Provider、匿名动作/命名路由与 Session Cookie 验证回跳、闪存及错误包。
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
        assert_navigation(&app).await?;
        assert_refresh_and_away(&app).await?;
        assert_actions(&app).await?;
        let first = get(&app, &format!("{PREFIX}/review"), None).await?;
        let cookie = cookie_pair(&first)?;
        let body = json!({"title":"  Ada  ","token":"private"});
        let response = app
            .request_with_limits(
                Request::post(format!("{PREFIX}/submit"))
                    .header("host", "localhost")
                    .header(header::COOKIE, cookie)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::ACCEPT, "application/json")
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
                    .header(header::ACCEPT, "application/json")
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
