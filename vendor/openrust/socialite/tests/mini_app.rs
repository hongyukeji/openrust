use axum::{Router, extract::State, response::IntoResponse, routing::get};
use openrust::{
    config::Repository,
    http::client::{Factory, Response},
};
use openrust_socialite::{Error, SocialiteManager, wechat::MiniApp};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

fn config() -> Value {
    json!({"app_id":"mini-app", "secret":"private-secret"})
}
fn client(http: Arc<Factory>) -> MiniApp {
    MiniApp::new(&config(), http).unwrap()
}
fn response(status: u16, body: impl Into<Vec<u8>>) -> Response {
    Response::new(
        http::StatusCode::from_u16(status).unwrap(),
        http::HeaderMap::new(),
        body,
    )
}
#[tokio::test]
async fn default_endpoint_and_fields_follow_easywechat_without_redirect_or_session() {
    let http = Arc::new(Factory::new().unwrap());
    http.fake(|r| {
        assert_eq!(r.method(), http::Method::GET);
        assert_eq!(r.url().host_str(), Some("api.weixin.qq.com"));
        assert_eq!(r.url().path(), "/sns/jscode2session");
        let query = r
            .url()
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        let reference: Value =
            serde_json::from_str(include_str!("fixtures/mini_app.json")).unwrap();
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            reference["union"]["calls"][0]["options"]["query"]
        );
        assert_eq!(reference["union"]["calls"][0]["method"], "GET");
        assert_eq!(reference["union"]["calls"][0]["url"], r.url().path());
        Some(Ok(response(
            200,
            json!({"openid":"id-one","session_key":"private-session","unionid":"union-one"})
                .to_string(),
        )))
    });
    let manager = SocialiteManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "services".into(),
            json!({"wechat":{"mini_app":config()}}),
        )]))),
        http.clone(),
    );
    let session = manager
        .miniApp()
        .unwrap()
        .codeToSession("code+&中")
        .await
        .unwrap();
    assert_eq!(session.openid, "id-one");
    assert_eq!(session.unionid.as_deref(), Some("union-one"));
    assert_eq!(session.session_key.as_str(), "private-session");
    let user = session.user();
    assert_eq!(user.getId(), "id-one");
    assert_eq!(
        user.getRaw(),
        &json!({"openid":"id-one","unionid":"union-one"})
    );
    assert_eq!(
        user.attributes,
        json!({"id":"id-one","unionid":"union-one"})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(user.credentials.is_none());
    assert!(user.getNickname().is_none());
    for (request, response) in http.recorded() {
        assert!(request.url.query().is_none());
        assert!(request.body.is_none());
        assert!(response.unwrap().body().is_empty());
    }
}
#[tokio::test]
async fn invalid_configuration_and_empty_codes_fail_before_http() {
    let http = Arc::new(Factory::new().unwrap());
    http.preventStrayRequests(true);
    for config in [
        Value::Null,
        json!({}),
        json!({"app_id":"","secret":"s"}),
        json!({"app_id":1,"secret":"s"}),
        json!({"app_id":"a","secret":"  "}),
        json!({"app_id":"a","secret":false}),
    ] {
        assert!(matches!(
            MiniApp::new(&config, http.clone()),
            Err(Error::Configuration(_))
        ));
    }
    for code in ["", " \n "] {
        assert!(matches!(
            client(http.clone()).codeToSession(code).await,
            Err(Error::MissingCode)
        ));
    }
    assert!(http.recorded().is_empty());
}
#[tokio::test]
async fn provider_errors_and_malformed_identities_are_rejected_without_secret_disclosure() {
    for body in [
        json!({"errcode":40029,"errmsg":"private-secret private-session"}),
        json!({"errcode":45011,"errmsg":"rate limit"}),
        json!({"errcode":"0","openid":"id","session_key":"key"}),
        json!({"openid":"id"}),
        json!({"session_key":"key"}),
        json!({"openid":42,"session_key":"key"}),
        json!({"openid":" ","session_key":"key"}),
        json!({"openid":"id","session_key":""}),
        json!({"openid":"id","session_key":true}),
        json!({"openid":"id","session_key":"key","unionid":5}),
        json!({"openid":"id","session_key":"key","unionid":""}),
        json!([]),
        Value::Null,
    ]
    .into_iter()
    .map(|v| v.to_string())
    .chain(["private-session invalid json".into()])
    {
        let http = Arc::new(Factory::new().unwrap());
        http.fake(move |_| Some(Ok(response(200, body.clone()))));
        let error = client(http)
            .codeToSession("private-code")
            .await
            .err()
            .unwrap();
        assert!(matches!(error, Error::InvalidResponse));
        assert!(!format!("{error:?} {error}").contains("private-"));
    }
    for status in [302, 400, 429, 500] {
        let http = Arc::new(Factory::new().unwrap());
        http.fake(move |_| Some(Ok(response(status, "private-secret"))));
        assert!(
            matches!(client(http.clone()).codeToSession("code").await, Err(Error::ProviderStatus(s)) if s==status)
        );
        assert_eq!(http.recorded().len(), 1);
    }
}
#[tokio::test]
async fn unionid_is_optional_and_session_key_is_never_a_socialite_access_token() {
    for extra in [json!({}), json!({"unionid":null}), json!({"errcode":0})] {
        let http = Arc::new(Factory::new().unwrap());
        let mut body = json!({"openid":"id", "session_key":"server-key", "access_token":"unexpected-private-token"});
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        http.fake(move |_| Some(Ok(response(200, body.to_string()))));
        let session = client(http).codeToSession("code").await.unwrap();
        assert!(session.unionid.is_none());
        assert_eq!(session.user().getRaw(), &json!({"openid":"id"}));
        assert!(session.user().credentials.is_none());
    }
}
#[derive(Default)]
struct Remote {
    seen: Mutex<Vec<String>>,
}
async fn exchange(
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let q = url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(q.len(), 4);
    assert_eq!(q["grant_type"], "authorization_code");
    assert_eq!(q["secret"], format!("secret-{}", q["appid"]));
    let identity = format!("{}:{}", q["appid"], q["js_code"]);
    let mut seen = remote.seen.lock().unwrap();
    if seen.contains(&identity) {
        return axum::Json(json!({"errcode":40163,"errmsg":"code used"})).into_response();
    }
    seen.push(identity.clone());
    axum::Json(json!({"openid":identity,"session_key":format!("key-{identity}")})).into_response()
}
#[tokio::test]
async fn real_http_exchange_keeps_concurrent_apps_separate_and_does_not_cache_or_retry_codes() {
    let remote = Arc::new(Remote::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/sns/jscode2session",
        listener.local_addr().unwrap()
    );
    let router = Router::new()
        .route("/sns/jscode2session", get(exchange))
        .with_state(remote.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let http = Arc::new(Factory::new().unwrap());
    http.fake(|_| None);
    let a = MiniApp::new(&json!({"app_id":"a","secret":"secret-a"}), http.clone())
        .unwrap()
        .withEndpoint(&endpoint);
    let b = MiniApp::new(&json!({"app_id":"b","secret":"secret-b"}), http.clone())
        .unwrap()
        .withEndpoint(&endpoint);
    let (one, two, three) = tokio::join!(
        a.codeToSession("one+中"),
        b.codeToSession("one+中"),
        a.codeToSession("two")
    );
    assert_eq!(one.unwrap().openid, "a:one+中");
    assert_eq!(two.unwrap().openid, "b:one+中");
    assert_eq!(three.unwrap().openid, "a:two");
    assert!(matches!(
        a.codeToSession("two").await,
        Err(Error::InvalidResponse)
    ));
    assert_eq!(http.recorded().len(), 4);
    task.abort();
}
#[tokio::test]
async fn credentials_are_not_forwarded_on_real_http_redirects() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/exchange",
            get(|| async {
                (
                    http::StatusCode::FOUND,
                    [("location", "/leak")],
                    "private-secret",
                )
            }),
        )
        .route(
            "/leak",
            get(|| async {
                panic!("credentials forwarded to redirect");
                #[allow(unreachable_code)]
                ""
            }),
        );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    assert!(matches!(
        client(Arc::new(Factory::new().unwrap()))
            .withEndpoint(format!("{endpoint}/exchange"))
            .codeToSession("code")
            .await,
        Err(Error::ProviderStatus(302))
    ));
    task.abort();
}

// This function is the three-language documentation example, compiled and exercised below.
async fn mini_login(
    manager: &SocialiteManager,
    code: &str,
) -> openrust_socialite::Result<openrust_socialite::two::User> {
    let session = manager.miniApp()?.codeToSession(code).await?;
    Ok(session.user())
}
fn login_routes() -> Result<(), openrust::routing::RoutingError> {
    openrust::support::facades::Route::r#match(
        &[http::Method::POST],
        "/mini-login",
        login_handler,
    )?;
    Ok(())
}
fn login_handler(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let input = request.input(Some("code")).await?;
        let code = input
            .as_ref()
            .and_then(Value::as_str)
            .ok_or(openrust::http::HttpError::InvalidRequest)?;
        let session = openrust_socialite::facades::Socialite::miniApp()
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?
            .codeToSession(code)
            .await
            .map_err(|_| openrust::http::HttpError::InvalidRequest)?;
        let user = session.user();
        openrust::http::Response::json(http::StatusCode::OK, json!({"id":user.getId()}))
    })
}
#[tokio::test]
async fn registered_manager_and_facade_work_in_json_api_without_web_session() {
    use openrust::{
        contracts::http::Kernel as _,
        foundation::{Application, configuration::Routing, http::Kernel},
    };
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([(
                "services".into(),
                json!({"wechat":{"mini_app":config()}}),
            )]))
        })
        .withProviders(vec![Box::new(openrust_socialite::SocialiteServiceProvider)])
        .withRouting(Routing::default().api(login_routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let http = app.make::<Factory>().await.unwrap();
    http.fake(|r| {
        let code = r
            .url()
            .query_pairs()
            .find(|(k, _)| k == "js_code")
            .unwrap()
            .1
            .into_owned();
        Some(Ok(response(
            200,
            json!({"openid":format!("id-{code}"),"session_key":"private-server-key"}).to_string(),
        )))
    });
    let manager = app.make::<SocialiteManager>().await.unwrap();
    assert_eq!(
        mini_login(&manager, "example").await.unwrap().getId(),
        "id-example"
    );
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let request = http::Request::builder()
        .method("POST")
        .uri("/api/mini-login")
        .header("content-type", "application/json")
        .body(openrust::http::Body::from(r#"{"code":"request"}"#))
        .unwrap();
    let result = Arc::new(kernel).handleTransport(request).await;
    assert_eq!(result.status(), http::StatusCode::OK);
    assert!(!result.headers().contains_key("set-cookie"));
    let body = axum::body::to_bytes(result.into_body(), 65536)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"id":"id-request"})
    );
    assert!(
        !String::from_utf8(body.to_vec())
            .unwrap()
            .contains("private-")
    );
}
