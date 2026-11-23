use axum::{
    Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use openrust::{
    config::Repository,
    contracts::{config::Repository as _, container::Container as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_socialite::{
    Error, SocialiteManager, SocialiteServiceProvider,
    two::{
        AbstractProvider, ProviderConfig, ProviderDefinition, ProviderEndpoints, TwitterProvider,
        XProvider,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/x.json")).unwrap()
}
fn raw() -> Value {
    oracle()["x"]["user"]["raw"].clone()
}
fn req() -> Request {
    let (parts, body) = http::Request::new(Body::empty()).into_parts();
    Request::from_parts(parts, body)
}
fn definition(name: &str, base: Option<&str>) -> Arc<dyn ProviderDefinition> {
    let endpoints = base.map(|b| {
        ProviderEndpoints::new(
            format!("{b}/authorize"),
            format!("{b}/token"),
            format!("{b}/user"),
        )
    });
    if name == "x" {
        Arc::new(endpoints.map(XProvider::withEndpoints).unwrap_or_default())
    } else {
        Arc::new(
            endpoints
                .map(TwitterProvider::withEndpoints)
                .unwrap_or_default(),
        )
    }
}
#[tokio::test]
async fn twitter_and_x_mapping_and_endpoints_match_fixed_php() {
    for name in ["twitter", "x"] {
        let d = definition(name, None);
        let u = d.mapUserToObject(raw()).unwrap();
        assert_eq!(
            json!({"id":u.get_id(),"nickname":u.getNickname(),"name":u.get_name(),"email":u.getEmail(),"avatar":u.getAvatar(),"raw":u.getRaw(),"attributes":u.attributes}),
            oracle()[name]["user"]
        );
        assert_eq!(
            d.getAuthUrl(),
            if name == "x" {
                "https://x.com/i/oauth2/authorize"
            } else {
                "https://twitter.com/i/oauth2/authorize"
            }
        );
        assert_eq!(d.getTokenUrl(), oracle()[name]["http"][0][0]);
        let u=d.mapUserToObject(json!({"id":"u","username":"u","name":"U","profile_image_url":"https://image.test"})).unwrap();
        assert!(u.getEmail().is_none());
    }
}
#[derive(Default)]
struct Remote {
    calls: Mutex<Vec<Value>>,
    mode: Mutex<String>,
}
async fn token(
    State(r): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let headers = request.headers().clone();
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    r.calls.lock().unwrap().push(json!({"fields":fields,"authorization":headers.get("authorization").and_then(|v|v.to_str().ok()),"accept":headers.get("accept").and_then(|v|v.to_str().ok())}));
    match r.mode.lock().unwrap().as_str() {
  "denied"=>(StatusCode::BAD_REQUEST,"private denied").into_response(),
  "malformed"=>([("content-type","application/json")],"private not json").into_response(),
  "missing"=>axum::Json(json!({"error":"private missing token"})).into_response(),
  _=>axum::Json(json!({"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":"users.read users.email tweet.read"})).into_response()
 }
}
async fn profile(
    State(r): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    assert_eq!(request.headers()["authorization"], "Bearer access");
    let fields = url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fields["user.fields"], "profile_image_url,confirmed_email");
    r.calls.lock().unwrap().push(json!({"profile":true}));
    match r.mode.lock().unwrap().as_str() {
        "revoked" => (StatusCode::UNAUTHORIZED, "private revoked").into_response(),
        "profile-malformed" => axum::Json(json!({"data":[]})).into_response(),
        _ => axum::Json(json!({"data":raw()})).into_response(),
    }
}
async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, task)
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let r = Arc::new(Remote::default());
    let (base, task) = serve(
        Router::new()
            .route("/token", post(token))
            .route("/user", get(profile))
            .with_state(r.clone()),
    )
    .await;
    (base, r, task)
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login", login)?;
    Route::get("/callback", callback)?;
    Ok(())
}
fn login(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request
            .query(Some("driver"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap();
        let stateless = request.query(Some("stateless"))?.is_some();
        let parameters = request.query(Some("parameters"))?.is_some();
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let mut p = manager.driver(&name, request).unwrap().oauth2().unwrap();
        if stateless {
            p.stateless();
        }
        if parameters {
            p.with(
                json!({"prompt":"select account + ~ *","state":"override"})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        }
        Ok(p.redirect().unwrap().into())
    })
}
fn callback(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let name = request
            .query(Some("driver"))?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap();
        let stateless = request.query(Some("stateless"))?.is_some();
        let refresh = request.query(Some("refresh"))?.is_some();
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let mut p = manager.driver(&name, request).unwrap().oauth2().unwrap();
        if stateless {
            p.stateless();
        }
        match p.user().await {
            Ok(user) => {
                if refresh {
                    let token = p.refreshToken("refresh-input").await.unwrap();
                    assert_eq!(
                        token.approvedScopes,
                        ["users.read", "users.email", "tweet.read"]
                    );
                }
                Response::json(
                    StatusCode::OK,
                    json!({"id":user.get_id(),"email":user.getEmail()}),
                )
            }
            Err(e) => Response::json(StatusCode::BAD_REQUEST, json!({"error":e.to_string()})),
        }
    })
}
async fn rp(remote: &str) -> (String, tokio::task::JoinHandle<()>) {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
 ("services".into(),json!({"x":{"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"},"twitter":{"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback","oauth":2}})),
 ("app".into(),json!({"key":"k".repeat(32),"env":"testing"})),("session".into(),json!({"driver":"array","cookie":"x_session","lottery":[0,100]})),("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}}))].into())).with_providers(vec![Box::new(SocialiteServiceProvider)]).with_routing(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<SocialiteManager>().await.unwrap();
    for name in ["twitter", "x"] {
        manager.extend(name, definition(name, Some(remote)));
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(k): State<Arc<Kernel>>,
        r: axum::extract::Request,
    ) -> axum::response::Response {
        k.handle_transport(r).await
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
        .unwrap()
    });
    (base, task)
}
fn cookie(r: &openrust::http::client::Response) -> String {
    r.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("x_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
async fn call(http: &Factory, url: &str, cookie: Option<&str>) -> openrust::http::client::Response {
    let mut r = http.pending_request().without_redirecting();
    if let Some(c) = cookie {
        r = r.with_headers(http::HeaderMap::from_iter([(
            http::header::COOKIE,
            http::HeaderValue::from_str(c).unwrap(),
        )]));
    }
    r.get(url).await.unwrap()
}
#[tokio::test]
async fn twitter_and_x_default_pkce_browser_sessions_exchange_refresh_and_reject_cross_app_replay()
{
    let (base, r, remote_task) = remote().await;
    let (one, first) = rp(&base).await;
    let (two, second) = rp(&base).await;
    let client = Factory::new().unwrap();
    for name in ["twitter", "x"] {
        r.calls.lock().unwrap().clear();
        let start = call(&client, &format!("{one}/login?driver={name}"), None).await;
        assert_eq!(start.status(), 302);
        let session = cookie(&start);
        let location = start.header("location").unwrap().to_str().unwrap();
        assert!(location.contains("users.read%20users.email%20tweet.read"));
        assert!(!location.contains('+'));
        let url = url::Url::parse(location).unwrap();
        let fields = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        assert_eq!(fields["state"].len(), 40);
        assert_eq!(fields["code_challenge_method"], "S256");
        let query = format!(
            "driver={name}&code=fixed-code&state={}&refresh=1",
            fields["state"]
        );
        assert_eq!(
            call(&client, &format!("{two}/callback?{query}"), Some(&session))
                .await
                .status(),
            400
        );
        assert!(r.calls.lock().unwrap().is_empty());
        let ok = call(&client, &format!("{one}/callback?{query}"), Some(&session)).await;
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.json::<Value>().unwrap()["email"], "alice@test");
        let updated = cookie(&ok);
        assert_eq!(
            call(&client, &format!("{one}/callback?{query}"), Some(&updated))
                .await
                .status(),
            400
        );
        let calls = r.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        let verifier = calls[0]["fields"]["code_verifier"].as_str().unwrap();
        assert_eq!(verifier.len(), 96);
        use base64::Engine as _;
        assert_eq!(
            fields["code_challenge"],
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
                aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, verifier.as_bytes()).as_ref()
            )
        );
        assert_eq!(calls[0]["authorization"], "Basic Y2xpZW50OnNlY3JldA==");
        assert_eq!(calls[0]["accept"], "application/json");
        assert_eq!(calls[0]["fields"]["client_secret"], "secret");
        assert_eq!(
            calls[2]["fields"],
            oracle()[name]["http"][1][1]["form_params"]
        );
        assert_eq!(calls[2]["authorization"], "Basic Y2xpZW50OnNlY3JldA==");
    }
    first.abort();
    second.abort();
    remote_task.abort();
}
#[tokio::test]
async fn stateless_keeps_default_pkce_session_and_literal_state_and_rfc3986_parameters() {
    let (base, r, remote_task) = remote().await;
    let (one, task) = rp(&base).await;
    let client = Factory::new().unwrap();
    for name in ["twitter", "x"] {
        for parameters in [false, true] {
            let start = call(
                &client,
                &format!(
                    "{one}/login?driver={name}&stateless=1{}",
                    if parameters { "&parameters=1" } else { "" }
                ),
                None,
            )
            .await;
            let session = cookie(&start);
            let location = start.header("location").unwrap().to_str().unwrap();
            let url = url::Url::parse(location).unwrap();
            let mut fields = url
                .query_pairs()
                .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                .collect::<serde_json::Map<_, _>>();
            fields.remove("code_challenge");
            let mut expected = oracle()[name][if parameters {
                "parameters"
            } else {
                "stateless"
            }]
            .as_object()
            .unwrap()
            .clone();
            expected.remove("code_challenge");
            assert_eq!(fields, expected);
            if parameters {
                assert!(location.contains("prompt=select%20account%20%2B%20~%20%2A"));
            }
            let ok = call(
                &client,
                &format!("{one}/callback?driver={name}&stateless=1&code=fixed-code&state=any"),
                Some(&session),
            )
            .await;
            assert_eq!(ok.status(), 200);
        }
        let mut request = req();
        let mut p=AbstractProvider::new(&mut request,ProviderConfig::from_config(&json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap(),Arc::new(Factory::new().unwrap()),definition(name,Some(&base)));
        p.stateless();
        assert!(matches!(p.authorizationUrl(), Err(Error::Session(_))));
        assert!(matches!(
            p.getAccessTokenResponse("code").await,
            Err(Error::Session(_))
        ));
        assert_eq!(
            p.userFromToken("access").await.unwrap().get_id(),
            &json!("user-42")
        );
    }
    assert_eq!(
        r.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.get("fields").is_some())
            .count(),
        4
    );
    task.abort();
    remote_task.abort();
}
#[tokio::test]
async fn manager_alias_fallback_requires_integer_oauth_two_and_isolates_registrations() {
    let config = Arc::new(Repository::new(Default::default()));
    let value = json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"});
    config
        .set("services.twitter-oauth-2", value.clone())
        .unwrap();
    config.set("services.x-oauth-2", value.clone()).unwrap();
    let manager = SocialiteManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    let mut request = req();
    assert!(manager.driver("twitter-oauth-2", &mut request).is_ok());
    assert!(manager.driver("x", &mut request).is_ok());
    for oauth in [json!(null), json!("2"), json!(1), json!(2)] {
        let mut v = value.clone();
        v["oauth"] = oauth.clone();
        config.set("services.twitter", v).unwrap();
        assert_eq!(
            manager
                .driver("twitter", &mut request)
                .unwrap()
                .oauth2()
                .is_ok(),
            oauth == json!(2)
        );
    }
    config
        .set("services.twitter.client_id", json!("primary"))
        .unwrap();
    config
        .set("services.twitter.scopes", json!(["primary-scope"]))
        .unwrap();
    let mut p = manager
        .driver("twitter-oauth-2", &mut request)
        .unwrap()
        .oauth2()
        .unwrap();
    assert!(p.getScopes().contains(&"primary-scope".to_owned()));
    assert!(p.authorizationUrl().is_err());
    drop(p);
    manager.extend("custom", definition("x", None));
    config.set("services.custom", value).unwrap();
    let other = SocialiteManager::new(config, Arc::new(Factory::new().unwrap()));
    assert!(manager.driver("custom", &mut request).is_ok());
    assert!(matches!(
        other.driver("custom", &mut request),
        Err(Error::UnsupportedDriver(_))
    ));
}
#[tokio::test]
async fn remote_failures_and_revocation_reject_without_exposing_provider_body() {
    let (base, r, remote_task) = remote().await;
    let (one, task) = rp(&base).await;
    let client = Factory::new().unwrap();
    for name in ["twitter", "x"] {
        for mode in [
            "denied",
            "malformed",
            "missing",
            "revoked",
            "profile-malformed",
        ] {
            *r.mode.lock().unwrap() = mode.into();
            let start = call(&client, &format!("{one}/login?driver={name}"), None).await;
            let session = cookie(&start);
            let url = url::Url::parse(start.header("location").unwrap().to_str().unwrap()).unwrap();
            let state = url
                .query_pairs()
                .find(|(k, _)| k == "state")
                .unwrap()
                .1
                .into_owned();
            let failed = call(
                &client,
                &format!("{one}/callback?driver={name}&code=fixed-code&state={state}"),
                Some(&session),
            )
            .await;
            assert_eq!(failed.status(), 400);
            assert!(!String::from_utf8_lossy(&failed.body().unwrap()).contains("private"));
        }
    }
    task.abort();
    remote_task.abort();
}
