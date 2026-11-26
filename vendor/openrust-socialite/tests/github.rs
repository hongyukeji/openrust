use axum::{
    Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use openrust::{
    contracts::{
        container::Container as _, foundation::Application as _, http::Kernel as _,
        session::Session as _,
    },
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_socialite::{Error, SocialiteManager, SocialiteServiceProvider, two::GithubProvider};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/github.json")).unwrap()
}
#[derive(Default)]
struct Remote {
    requests: Mutex<Vec<Value>>,
    mode: Mutex<String>,
}
async fn token(
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let headers = request.headers().clone();
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    remote.requests.lock().unwrap().push(json!({"fields":fields,"accept":headers["accept"].to_str().unwrap(),"type":headers["content-type"].to_str().unwrap()}));
    match remote.mode.lock().unwrap().as_str() {
        "denied"=>(axum::http::StatusCode::BAD_REQUEST,"provider-private-error").into_response(),
        "malformed"=>([("content-type","application/json")],"not json private access token").into_response(),
        "missing"=>axum::Json(json!({"error":"denied"})).into_response(),
        _=>axum::Json(json!({"access_token":"secret-access","refresh_token":"secret-refresh","expires_in":3600,"scope":"read,user:email"})).into_response(),
    }
}
async fn profile(
    State(remote): State<Arc<Remote>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    assert_eq!(headers["authorization"], "token secret-access");
    assert_eq!(
        headers["user-agent"],
        format!("OpenRust-Socialite/{}", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(headers["accept"], "application/vnd.github.v3+json");
    remote
        .requests
        .lock()
        .unwrap()
        .push(json!({"profile":true}));
    axum::Json(
        json!({"id":42,"node_id":"node-42","login":"alice","name":"Alice","email":"unverified@test","avatar_url":"https://images.test/alice"}),
    )
}
async fn emails(State(remote): State<Arc<Remote>>) -> axum::response::Response {
    if *remote.mode.lock().unwrap() == "email-error" {
        return (axum::http::StatusCode::FORBIDDEN, "hidden").into_response();
    }
    axum::Json(json!([{"primary":false,"verified":true,"email":"secondary@test"},{"primary":true,"verified":false,"email":"bad@test"},{"primary":true,"verified":true,"email":"alice@test"}])).into_response()
}
async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, task)
}
fn request(query: &str) -> Request {
    let wire = http::Request::builder()
        .uri(format!("/callback{query}"))
        .body(openrust::http::Body::empty())
        .unwrap();
    let (parts, body) = wire.into_parts();
    Request::from_parts(parts, body)
}
async fn manager(base: &str) -> (Application, Arc<SocialiteManager>) {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([("services".into(),json!({"github":{"client_id":"app-one","client_secret":"secret-client","redirect":"/callback","scopes":["read","user:email"]}})),("app".into(),json!({"url":"https://rp.test/base"}))].into())).with_providers(vec![Box::new(SocialiteServiceProvider)]).create().unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<SocialiteManager>().await.unwrap();
    manager.extend(
        "github",
        Arc::new(GithubProvider::with_endpoints(
            format!("{base}/authorize"),
            format!("{base}/token"),
            format!("{base}/user"),
            format!("{base}/emails"),
        )),
    );
    (app, manager)
}
#[tokio::test]
async fn github_real_http_maps_verified_email_exchanges_refreshes_and_caches_only_one_provider_request()
 {
    let remote = Arc::new(Remote::default());
    let (base, server) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .route("/emails", get(emails))
            .with_state(remote.clone()),
    )
    .await;
    let (app, manager) = manager(&base).await;
    let mut req = request("?code=code-one");
    let mut provider = manager
        .driver("github", &mut req)
        .unwrap()
        .oauth2()
        .unwrap();
    provider.stateless();
    assert_eq!(provider.get_scopes(), ["user:email", "read"]);
    let url = url::Url::parse(&provider.authorization_url().unwrap()).unwrap();
    let params = url.query_pairs().collect::<BTreeMap<_, _>>();
    assert_eq!(params["redirect_uri"], "https://rp.test/base/callback");
    assert!(!params.contains_key("state"));
    let user = provider.user().await.unwrap();
    assert_eq!(user.get_id(), &json!(42));
    assert_eq!(user.get_nickname(), Some("alice"));
    assert_eq!(user.get_email(), Some("alice@test"));
    assert_eq!(user.attributes["nodeId"], "node-42");
    assert_eq!(user.get_raw()["email"], "alice@test");
    assert_eq!(
        json!({"id":user.get_id(),"nickname":user.get_nickname(),"name":user.get_name(),"email":user.get_email(),"avatar":user.get_avatar(),"raw":user.get_raw(),"attributes":user.attributes}),
        oracle()["user"]
    );
    assert_eq!(provider.user().await.unwrap().get_id(), &json!(42));
    assert_eq!(remote.requests.lock().unwrap().len(), 2);
    let tokens = user.credentials.unwrap();
    assert_eq!(tokens.token.as_str(), "secret-access");
    assert_eq!(tokens.refresh_token.unwrap().as_str(), "secret-refresh");
    assert_eq!(tokens.expires_in, Some(3600));
    assert_eq!(tokens.approved_scopes, ["read", "user:email"]);
    let refreshed = provider.refresh_token("refresh-once").await.unwrap();
    assert_eq!(refreshed.token.as_str(), "secret-access");
    {
        let records = remote.requests.lock().unwrap();
        assert_eq!(records[0]["fields"]["client_secret"], "secret-client");
        assert_eq!(records[0]["fields"]["code"], "code-one");
        assert_eq!(records[0]["accept"], "application/json");
        assert_eq!(records[2]["fields"]["grant_type"], "refresh_token");
    }
    drop(provider);
    let mut next = request("?code=code-two");
    let mut next = manager.with("github", &mut next).unwrap().oauth2().unwrap();
    next.stateless();
    next.user().await.unwrap();
    assert_eq!(remote.requests.lock().unwrap().len(), 5);
    drop(app);
    server.abort();
}
#[tokio::test]
async fn github_http_errors_bad_json_and_email_failures_preserve_protocol_boundaries() {
    let remote = Arc::new(Remote::default());
    let (base, server) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .route("/emails", get(emails))
            .with_state(remote.clone()),
    )
    .await;
    let (_app, manager) = manager(&base).await;
    for mode in ["denied", "malformed", "missing"] {
        *remote.mode.lock().unwrap() = mode.into();
        let mut req = request("?code=code");
        let mut provider = manager
            .driver("github", &mut req)
            .unwrap()
            .oauth2()
            .unwrap();
        provider.stateless();
        let error = provider.user().await.err().unwrap();
        let text = error.to_string();
        assert!(!text.contains("secret"));
        assert!(!text.contains("private"));
        assert!(match mode {
            "denied" => matches!(error, Error::ProviderStatus(400)),
            "malformed" => matches!(error, Error::Http(_)),
            _ => matches!(error, Error::InvalidResponse),
        });
    }
    *remote.mode.lock().unwrap() = "email-error".into();
    let mut req = request("");
    let mut provider = manager
        .driver("github", &mut req)
        .unwrap()
        .oauth2()
        .unwrap();
    assert!(matches!(provider.user().await, Err(Error::Session(_))));
    provider.stateless();
    assert!(matches!(provider.user().await, Err(Error::MissingCode)));
    assert_eq!(
        provider
            .user_from_token("secret-access")
            .await
            .unwrap()
            .get_email(),
        None
    );
    assert!(matches!(
        manager.driver("unknown", &mut req),
        Err(Error::UnsupportedDriver(_))
    ));
    server.abort();
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login", login)?;
    Route::get("/callback", callback)?;
    Route::get("/state", set_state)?;
    Ok(())
}
fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let mut provider = openrust_socialite::facades::Socialite::driver("github", request)
            .unwrap()
            .oauth2()
            .unwrap();
        provider.enable_pkce();
        Ok(provider.redirect().unwrap().into())
    })
}
fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let pkce = request.session()?.get("code_verifier").is_some();
        let mut provider = manager.driver("github", request).unwrap().oauth2().unwrap();
        if pkce {
            provider.enable_pkce();
        }
        match provider.user().await {
            Ok(user) => Response::json(
                StatusCode::OK,
                json!({"id":user.get_id(),"email":user.get_email()}),
            ),
            Err(Error::InvalidState) => {
                Response::json(StatusCode::BAD_REQUEST, json!({"error":"state"}))
            }
            Err(e) => panic!("{e}"),
        }
    })
}
async fn rp(base: &str) -> (String, tokio::task::JoinHandle<()>) {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
 ("services".into(),json!({"github":{"client_id":"rp","client_secret":"secret","redirect":"https://rp.test/callback"}})),
 ("app".into(),json!({"key":"k".repeat(32),"env":"testing"})),("session".into(),json!({"driver":"array","cookie":"socialite_session","lottery":[0,100]})),("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}}))].into())).with_providers(vec![Box::new(SocialiteServiceProvider)]).with_routing(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    app.make::<SocialiteManager>().await.unwrap().extend(
        "github",
        Arc::new(GithubProvider::with_endpoints(
            format!("{base}/authorize"),
            format!("{base}/token"),
            format!("{base}/user"),
            format!("{base}/emails"),
        )),
    );
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    use axum::handler::Handler as _;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap();
    });
    (base, task)
}
fn cookie(response: &openrust::http::client::Response) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter(|v| v.starts_with("socialite_session="))
        .map(|v| v.split(';').next().unwrap().to_owned())
        .next()
        .unwrap()
}
#[tokio::test]
async fn github_browser_session_state_is_consumed_replay_and_cross_application_fail_before_token_exchange()
 {
    let remote = Arc::new(Remote::default());
    let (base, server) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .route("/emails", get(emails))
            .with_state(remote.clone()),
    )
    .await;
    let (one, first) = rp(&base).await;
    let (two, second) = rp(&base).await;
    let client = Factory::new().unwrap();
    let response = client
        .pending_request()
        .without_redirecting()
        .get(&format!("{one}/login"))
        .await
        .unwrap();
    assert_eq!(response.status(), 302);
    let session = cookie(&response);
    let redirect = url::Url::parse(response.header("location").unwrap().to_str().unwrap()).unwrap();
    let state = redirect
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(state.len(), 40);
    assert!(state.bytes().all(|b| b.is_ascii_alphanumeric()));
    let challenge = redirect
        .query_pairs()
        .find(|(k, _)| k == "code_challenge")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(
        redirect
            .query_pairs()
            .find(|(k, _)| k == "code_challenge_method")
            .unwrap()
            .1,
        "S256"
    );
    async fn call(
        client: &Factory,
        base: &str,
        session: &str,
        state: &str,
    ) -> openrust::http::client::Response {
        let headers = http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(session).unwrap(),
        )]);
        client
            .pending_request()
            .with_headers(headers)
            .without_redirecting()
            .get(&format!("{base}/callback?code=valid&state={state}"))
            .await
            .unwrap()
    }
    assert_eq!(call(&client, &two, &session, &state).await.status(), 400);
    assert!(remote.requests.lock().unwrap().is_empty());
    let ok = call(&client, &one, &session, &state).await;
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.json::<Value>().unwrap()["email"], "alice@test");
    let updated = cookie(&ok);
    assert_eq!(call(&client, &one, &updated, &state).await.status(), 400);
    assert_eq!(remote.requests.lock().unwrap().len(), 2);
    let verifier = remote.requests.lock().unwrap()[0]["fields"]["code_verifier"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(verifier.len(), 96);
    use base64::Engine as _;
    assert_eq!(
        challenge,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
            aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref()
        )
    );
    first.abort();
    second.abort();
    server.abort();
}
#[tokio::test]
async fn provider_http_observations_redact_credentials_and_user_bodies_but_keep_wire_payloads() {
    use openrust::{
        events::Dispatcher,
        http::client::events::{RequestSending, ResponseReceived},
    };
    let remote = Arc::new(Remote::default());
    let (base, server) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .route("/emails", get(emails))
            .with_state(remote.clone()),
    )
    .await;
    let (app, manager) = manager(&base).await;
    let events = app.container().make::<Dispatcher>().unwrap().fake(&[]);
    let factory = app.container().make::<Factory>().unwrap();
    factory.fake(|_| None);
    let mut req = request("?code=private-code");
    let mut provider = manager
        .driver("github", &mut req)
        .unwrap()
        .oauth2()
        .unwrap();
    provider.stateless();
    provider.user().await.unwrap();
    let requests = events.dispatched(r"OpenRust\Http\Client\Events\RequestSending");
    assert_eq!(requests.len(), 3);
    for e in requests {
        let request = &e.downcast_ref::<RequestSending>().unwrap().request;
        assert!(request.body.is_none());
        assert!(request.url.query().is_none());
        assert!(!request.headers.contains_key("authorization"));
    }
    for (request, response) in factory.recorded() {
        assert!(request.body.is_none());
        assert!(!request.headers.contains_key("authorization"));
        assert!(response.unwrap().body().unwrap().is_empty());
    }
    let responses = events.dispatched(r"OpenRust\Http\Client\Events\ResponseReceived");
    assert_eq!(responses.len(), 3);
    for e in responses {
        let response = &e.downcast_ref::<ResponseReceived>().unwrap().response;
        assert!(response.body().unwrap().is_empty());
        assert!(response.headers().is_empty());
        assert_eq!(response.status(), 200);
    }
    assert_eq!(
        remote.requests.lock().unwrap()[0]["fields"]["code"],
        "private-code"
    );
    server.abort();
}

fn set_state(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if let Some(stored) = request.query(Some("stored"))? {
            request.session_mut()?.put("state", stored)?;
        }
        Response::json(StatusCode::OK, json!({"ready":true}))
    })
}
#[tokio::test]
async fn github_fixed_php_scope_and_parameter_fields_match() {
    use openrust_socialite::two::{AbstractProvider, ProviderConfig};
    let mut req = request("");
    let config=ProviderConfig::from_config(&json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap();
    let mut p = AbstractProvider::new(
        &mut req,
        config,
        Arc::new(Factory::new().unwrap()),
        Arc::new(GithubProvider::default()),
    );
    p.stateless();
    fn fields(p: &mut AbstractProvider<'_>) -> Value {
        url::Url::parse(&p.authorization_url().unwrap())
            .unwrap()
            .query_pairs()
            .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
            .collect::<serde_json::Map<String, Value>>()
            .into()
    }
    for key in ["default", "merged", "set", "stateless", "parameters"] {
        match key {
            "merged" => {
                p.scopes(["read".into(), "user:email".into(), "read".into()]);
            }
            "set" => {
                p.set_scopes(vec!["profile".into(), "profile".into(), "email".into()]);
            }
            "parameters" => {
                p.with(serde_json::from_value(json!({"prompt":"select account","login_hint":"中文 & +","scope":"override"})).unwrap());
            }
            _ => {}
        }
        let mut expected = oracle()[key].clone();
        expected.as_object_mut().unwrap().remove("state");
        assert_eq!(fields(&mut p), expected, "{key}");
    }
}
#[tokio::test]
async fn github_fixed_php_state_matrix_and_replays_match_over_browser_session() {
    let remote = Arc::new(Remote::default());
    let (base, server) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .route("/emails", get(emails))
            .with_state(remote.clone()),
    )
    .await;
    let (rp, task) = rp(&base).await;
    let client = Factory::new().unwrap();
    for (i, (stored, incoming)) in [
        (None, None),
        (Some("abc"), Some("state=abc")),
        (Some("abc"), Some("state=wrong")),
        (Some("abc"), None),
        (Some("abc"), Some("state[]=abc")),
        (Some(""), Some("state=")),
        (Some("0"), Some("state=0")),
    ]
    .into_iter()
    .enumerate()
    {
        let url = format!(
            "{rp}/state{}",
            stored.map(|v| format!("?stored={v}")).unwrap_or_default()
        );
        let start = client.get(&url).await.unwrap();
        let session = cookie(&start);
        let headers = http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(&session).unwrap(),
        )]);
        let url = format!(
            "{rp}/callback?code=valid{}",
            incoming.map(|v| format!("&{v}")).unwrap_or_default()
        );
        let response = client
            .pending_request()
            .with_headers(headers)
            .get(&url)
            .await
            .unwrap();
        assert_eq!(
            response.status() == 400,
            oracle()["state"][i]["first"].as_bool().unwrap(),
            "case {i}"
        );
        let updated = cookie(&response);
        let headers = http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(&updated).unwrap(),
        )]);
        let replay = client
            .pending_request()
            .with_headers(headers)
            .get(&url)
            .await
            .unwrap();
        assert_eq!(
            replay.status() == 400,
            oracle()["state"][i]["replay"].as_bool().unwrap()
        );
    }
    task.abort();
    server.abort();
}
#[tokio::test]
async fn manager_configuration_and_custom_provider_registrations_are_application_local() {
    let (one, first) = manager("http://127.0.0.1:1").await;
    let (_two, second) = manager("http://127.0.0.1:2").await;
    let config = one
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap();
    config
        .set(
            "services.custom",
            json!({"client_id":"custom","client_secret":"secret","redirect":"/custom"}),
        )
        .unwrap();
    first.extend("custom", Arc::new(GithubProvider::default()));
    let mut req = request("");
    let mut p = first.driver("custom", &mut req).unwrap().oauth2().unwrap();
    p.stateless();
    let uri = url::Url::parse(&p.authorization_url().unwrap()).unwrap();
    assert!(uri.as_str().starts_with("https://github.com/"));
    assert_eq!(
        uri.query_pairs().find(|(k, _)| k == "client_id").unwrap().1,
        "custom"
    );
    drop(p);
    assert!(matches!(
        second.driver("custom", &mut req),
        Err(Error::UnsupportedDriver(_))
    ));
    for key in ["client_id", "client_secret", "redirect", "scopes"] {
        config
            .set(&format!("services.github.{key}"), json!({"invalid":true}))
            .unwrap();
        assert!(matches!(
            first.driver("github", &mut req),
            Err(Error::Configuration(_))
        ));
        config
            .set(
                &format!("services.github.{key}"),
                if key == "scopes" {
                    json!([])
                } else {
                    json!("valid")
                },
            )
            .unwrap();
    }
    config
        .set("services.github.redirect", json!("/callback"))
        .unwrap();
    for url in [
        "relative",
        "ftp://rp.test",
        "javascript:alert(1)",
        "https://",
    ] {
        config.set("app.url", json!(url)).unwrap();
        assert!(matches!(
            first.driver("github", &mut req),
            Err(Error::Configuration("app.url"))
        ));
    }
    config
        .set(
            "services.github.redirect",
            json!("https://registered.test/callback"),
        )
        .unwrap();
    assert!(first.driver("github", &mut req).is_ok());
}
