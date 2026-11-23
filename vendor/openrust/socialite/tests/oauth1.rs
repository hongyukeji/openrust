use axum::{
    Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use openrust::{
    contracts::{container::Container as _, http::Kernel as _},
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_socialite::{
    Error, SocialiteManager, SocialiteServiceProvider,
    one::{TwitterProvider, map_user, signature::sign},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
fn oracle() -> Value {
    serde_json::from_str(include_str!("fixtures/oauth1.json")).unwrap()
}
fn snapshot(u: &openrust_socialite::two::User) -> Value {
    json!({"id":u.get_id(),"nickname":u.get_nickname(),"name":u.get_name(),"email":u.get_email(),"avatar":u.get_avatar(),"raw":u.get_raw(),"attributes":u.attributes})
}
#[test]
fn signatures_and_two_mapping_paths_match_pinned_league_and_socialite() {
    for case in oracle()["signatures"].as_array().unwrap() {
        let parameters = serde_json::from_value(case["parameters"].clone()).unwrap();
        assert_eq!(
            sign(
                case["method"].as_str().unwrap(),
                case["url"].as_str().unwrap(),
                &parameters,
                "consumer + %25",
                case["secret"].as_str().unwrap()
            )
            .unwrap(),
            case["signature"],
            "{case}"
        );
    }
    for (callback, key) in [(true, "callback"), (false, "direct")] {
        assert_eq!(
            snapshot(&map_user(oracle()["raw"].clone(), callback).unwrap()),
            oracle()[key]
        );
    }
}
#[derive(Default)]
struct Remote {
    calls: Mutex<Vec<Value>>,
    consumed: Mutex<bool>,
    mode: Mutex<String>,
    nonces: Mutex<Vec<String>>,
}
fn header(request: &axum::extract::Request) -> BTreeMap<String, String> {
    request.headers()["authorization"]
        .to_str()
        .unwrap()
        .strip_prefix("OAuth ")
        .unwrap()
        .split(", ")
        .map(|part| {
            let (k, v) = part.split_once('=').unwrap();
            (
                k.to_owned(),
                url::form_urlencoded::parse(format!("v={}", v.trim_matches('"')).as_bytes())
                    .next()
                    .unwrap()
                    .1
                    .into_owned(),
            )
        })
        .collect()
}
async fn endpoint(
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let mut protocol = header(&request);
    let signature = protocol.remove("oauth_signature").unwrap();
    assert_eq!(protocol["oauth_consumer_key"], "client");
    assert_eq!(protocol["oauth_signature_method"], "HMAC-SHA1");
    assert_eq!(protocol["oauth_version"], "1.0");
    assert_eq!(protocol["oauth_nonce"].len(), 32);
    {
        let mut nonces = remote.nonces.lock().unwrap();
        assert!(!nonces.contains(&protocol["oauth_nonce"]));
        nonces.push(protocol["oauth_nonce"].clone());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(now.abs_diff(protocol["oauth_timestamp"].parse::<u64>().unwrap()) < 10);
    let endpoint = format!(
        "http://{}{}",
        request.headers()["host"].to_str().unwrap(),
        request.uri()
    );
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    let secret = match path.as_str() {
        "/temporary" => "",
        "/token" => "temp-secret",
        _ => {
            if protocol["oauth_token"] == "access" {
                "access-secret"
            } else {
                "other-secret"
            }
        }
    };
    let mut parameters = protocol.clone();
    parameters.extend(fields.clone());
    let mode = remote.mode.lock().unwrap().clone();
    if path == "/token" && (!fields.contains_key("oauth_verifier") || mode == "typed-false") {
        parameters.insert("oauth_verifier".into(), "".into());
    }

    let expected = sign(&method, &endpoint, &parameters, "consumer + %25", secret).unwrap();
    let valid = signature == expected
        || (path == "/user"
            && protocol.get("oauth_token").map(String::as_str) == Some("access")
            && signature
                == sign(
                    &method,
                    &endpoint,
                    &parameters,
                    "consumer + %25",
                    "rotated-secret",
                )
                .unwrap());
    assert!(valid, "{method} {path} signature mismatch");
    remote
        .calls
        .lock()
        .unwrap()
        .push(json!({"path":path,"query":query,"fields":fields,"protocol":protocol}));
    let mode = remote.mode.lock().unwrap().clone();
    match path.as_str() {
        "/temporary" => {
            assert!(body.is_empty());
            assert_eq!(protocol["oauth_callback"], "https://rp.test/callback");
            assert!(!protocol.contains_key("oauth_token"));
            *remote.consumed.lock().unwrap() = false;
            let text = match mode.as_str() {
                "unconfirmed" => {
                    "oauth_token=temp&oauth_token_secret=temp-secret&oauth_callback_confirmed=false"
                }
                "malformed" => "private malformed",
                "missing-secret" => "oauth_token=temp&oauth_callback_confirmed=true",
                _ => {
                    "oauth_token=temp&oauth_token_secret=temp-secret&oauth_callback_confirmed=true"
                }
            };
            if mode == "denied" {
                return (StatusCode::FORBIDDEN, "private denied").into_response();
            }
            text.into_response()
        }
        "/token" => {
            assert_eq!(protocol["oauth_token"], "temp");
            assert!(!protocol.contains_key("oauth_verifier"));
            if mode == "typed-null" {
                assert!(fields.is_empty());
            } else {
                assert!(fields.len() <= 1);
            }
            if mode == "typed-false" || mode == "typed-numeric" {
                assert_eq!(fields["oauth_verifier"], "0");
            }
            if mode == "typed-true" {
                assert_eq!(fields["oauth_verifier"], "1");
            }
            if mode == "exchange-error" {
                return "error=private-denied".into_response();
            }
            let mut consumed = remote.consumed.lock().unwrap();
            if *consumed {
                return (StatusCode::UNAUTHORIZED, "private replay").into_response();
            }
            *consumed = true;
            "oauth_token=access&oauth_token_secret=access-secret".into_response()
        }
        _ => {
            assert!(body.is_empty());
            assert_eq!(query, "include_email=true");
            if mode == "revoked" {
                return (StatusCode::UNAUTHORIZED, "private revoked").into_response();
            }
            if mode == "bad-profile" {
                return axum::Json(json!([])).into_response();
            }
            axum::Json(oracle()["raw"].clone()).into_response()
        }
    }
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let r = Arc::new(Remote::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route("/temporary", post(endpoint))
        .route("/token", post(endpoint))
        .route("/user", get(endpoint))
        .with_state(r.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, r, task)
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/login", login)?;
    Route::get("/callback", callback)?;
    Route::get("/direct", direct)?;
    Ok(())
}
fn failure(e: Error) -> Result<Response, HttpError> {
    Response::json(StatusCode::BAD_REQUEST, json!({"error":e.to_string()}))
}
fn login(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let mut p = manager.driver("twitter", r).unwrap().oauth1().unwrap();
        p.scope("write + 中文");
        match p.redirect().await {
            Ok(response) => Ok(response.into()),
            Err(e) => failure(e),
        }
    })
}
fn callback(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if let Some(typed) = r
            .query(Some("typed"))?
            .and_then(|v| v.as_str().map(str::to_owned))
        {
            let value = match typed.as_str() {
                "null" => Value::Null,
                "false" => json!(false),
                "true" => json!(true),
                _ => json!(0),
            };
            r.merge(json!({"oauth_verifier":value})).await?;
        }

        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let result = manager.driver("twitter", r).unwrap().user().await;
        match result {
            Ok(user) => {
                let token = user.credentials.as_ref().unwrap();
                assert_eq!(token.token.as_str(), "access");
                assert_eq!(
                    token.token_secret.as_ref().unwrap().as_str(),
                    "access-secret"
                );
                assert!(token.refresh_token.is_none());
                use openrust::contracts::session::Session as _;
                assert!(r.session()?.get("oauth.temp").is_some());
                Response::json(StatusCode::OK, snapshot(&user))
            }
            Err(e) => failure(e),
        }
    })
}
fn direct(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let manager = openrust::container::Container::get_instance()
            .make::<SocialiteManager>()
            .unwrap();
        let mut p = manager.driver("twitter", r).unwrap().oauth1().unwrap();
        let first = p
            .user_from_token_and_secret("access", "access-secret")
            .await;
        match first {
            Ok(user) => {
                assert_eq!(
                    snapshot(
                        &p.user_from_token_and_secret("access", "access-secret")
                            .await
                            .unwrap()
                    ),
                    snapshot(&user)
                );
                p.user_from_token_and_secret("access", "rotated-secret")
                    .await
                    .unwrap();
                p.user_from_token_and_secret("other", "other-secret")
                    .await
                    .unwrap();
                Response::json(StatusCode::OK, snapshot(&user))
            }
            Err(e) => failure(e),
        }
    })
}
async fn rp(remote: &str) -> (String, Arc<Factory>, tokio::task::JoinHandle<()>) {
    let mut app=Application::configure(std::env::temp_dir()).with_config(|_,_|Ok([
        ("services".into(),json!({"twitter":{"client_id":"client","client_secret":"consumer + %25","redirect":"https://rp.test/callback","scope":"read"}})),
        ("app".into(),json!({"key":"k".repeat(32),"env":"testing"})),("session".into(),json!({"driver":"array","cookie":"one_session","lottery":[0,100]})),("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"}}}))
    ].into())).with_providers(vec![Box::new(SocialiteServiceProvider)]).with_routing(Routing::default().web(routes)).create().unwrap();
    app.boot().await.unwrap();
    let http = app.make::<Factory>().await.unwrap();
    http.fake(|_| None);
    app.make::<SocialiteManager>().await.unwrap().extend_oauth1(
        "twitter",
        TwitterProvider::with_endpoints(
            format!("{remote}/temporary"),
            format!("{remote}/authorize"),
            format!("{remote}/token"),
            format!("{remote}/user?include_email=true"),
        ),
    );
    let mut k = Kernel::new(app);
    k.bootstrap().await.unwrap();
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
            transport.with_state(Arc::new(k)).into_make_service(),
        )
        .await
        .unwrap()
    });
    (base, http, task)
}
fn cookie(r: &openrust::http::client::Response) -> String {
    r.headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.starts_with("one_session="))
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
async fn real_sessions_sign_exchange_map_cache_and_reject_cross_app_and_replay() {
    let (remote, r, remote_task) = remote().await;
    let (one, observations, first) = rp(&remote).await;
    let (two, _, second) = rp(&remote).await;
    let http = Factory::new().unwrap();
    let start = call(&http, &format!("{one}/login"), None).await;
    assert_eq!(start.status(), StatusCode::FOUND);
    assert_eq!(
        start.headers()["location"],
        format!("{remote}/authorize?oauth_token=temp")
    );
    let session = cookie(&start);
    assert_eq!(
        r.calls.lock().unwrap()[0]["query"],
        "x_auth_access_type=write+%2B+%E4%B8%AD%E6%96%87"
    );
    let wrong = call(
        &http,
        &format!("{two}/callback?oauth_token=temp&oauth_verifier=v"),
        Some(&session),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert_eq!(r.calls.lock().unwrap().len(), 1);
    for query in [
        "oauth_token=temp",
        "oauth_token=bad&oauth_verifier=v",
        "oauth_token=0&oauth_verifier=v",
    ] {
        assert_eq!(
            call(&http, &format!("{one}/callback?{query}"), Some(&session))
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(r.calls.lock().unwrap().len(), 1);
    }
    let ok = call(
        &http,
        &format!("{one}/callback?oauth_token=temp&oauth_verifier="),
        Some(&session),
    )
    .await;
    assert_eq!(ok.status(), StatusCode::OK);
    assert_eq!(ok.json::<Value>().unwrap(), oracle()["callback"]);
    let updated = cookie(&ok);
    let replay = call(
        &http,
        &format!("{one}/callback?oauth_token=temp&oauth_verifier=v"),
        Some(&updated),
    )
    .await;
    assert_eq!(replay.status(), StatusCode::BAD_REQUEST);
    assert!(!String::from_utf8_lossy(&replay.body().unwrap()).contains("private"));
    assert_eq!(r.calls.lock().unwrap().len(), 4);
    let direct = call(&http, &format!("{one}/direct"), None).await;
    assert_eq!(direct.status(), StatusCode::OK);
    assert_eq!(direct.json::<Value>().unwrap(), oracle()["direct"]);
    assert_eq!(r.calls.lock().unwrap().len(), 7);
    for (request, response) in observations.recorded() {
        assert!(request.body.is_none());
        assert!(response.unwrap().body().unwrap().is_empty());
        assert!(!request.headers.contains_key("authorization"));
    }
    first.abort();
    second.abort();
    remote_task.abort();
}
#[tokio::test]
async fn credential_and_profile_failures_remain_explicit_and_redacted() {
    let (remote, r, task) = remote().await;
    let (base, _, app) = rp(&remote).await;
    let http = Factory::new().unwrap();
    for mode in ["unconfirmed", "malformed", "missing-secret", "denied"] {
        *r.mode.lock().unwrap() = mode.into();
        let response = call(&http, &format!("{base}/login"), None).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!String::from_utf8_lossy(&response.body().unwrap()).contains("private"));
    }
    for mode in ["exchange-error", "revoked", "bad-profile"] {
        *r.mode.lock().unwrap() = String::new();
        let start = call(&http, &format!("{base}/login"), None).await;
        let session = cookie(&start);
        *r.mode.lock().unwrap() = mode.into();
        let response = call(
            &http,
            &format!("{base}/callback?oauth_token=temp&oauth_verifier=v"),
            Some(&session),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(!String::from_utf8_lossy(&response.body().unwrap()).contains("private"));
    }
    app.abort();
    task.abort();
}
#[tokio::test]
async fn factory_facade_protocol_selection_is_explicit_and_application_local() {
    use openrust::contracts::config::Repository as _;
    let config = Arc::new(openrust::config::Repository::new(Default::default()));
    let value = json!({"client_id":"client","client_secret":"secret","redirect":"/callback"});
    config.set("services.twitter", value.clone()).unwrap();
    config
        .set("app.url", json!("https://rp.test/base"))
        .unwrap();
    let manager = SocialiteManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    let mut r = {
        let (parts, body) = http::Request::new(openrust::http::Body::empty()).into_parts();
        Request::from_parts(parts, body)
    };
    let factory: &dyn openrust_socialite::contracts::Factory = &manager;
    assert!(factory.driver("twitter", &mut r).unwrap().oauth1().is_ok());
    assert!(matches!(
        manager.driver("twitter", &mut r).unwrap().oauth2(),
        Err(Error::Protocol("OAuth2"))
    ));
    for oauth in [json!(null), json!(1), json!("2"), json!(2), json!(2.0)] {
        config.set("services.twitter.oauth", oauth.clone()).unwrap();
        assert_eq!(
            manager.driver("twitter", &mut r).unwrap().oauth2().is_ok(),
            oauth.as_i64() == Some(2)
        );
    }
    config.set("services.custom", value.clone()).unwrap();
    manager.extend_oauth1("custom", TwitterProvider::default());
    assert!(manager.driver("custom", &mut r).unwrap().oauth1().is_ok());
    let other = SocialiteManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    assert!(matches!(
        other.driver("custom", &mut r),
        Err(Error::UnsupportedDriver(_))
    ));
    config.set("services.twitter.oauth", json!(1)).unwrap();
    config.set("services.twitter.scope", json!([])).unwrap();
    assert!(matches!(
        manager.driver("twitter", &mut r),
        Err(Error::Configuration("scope"))
    ));
    for endpoint in [
        "file:///tmp/x",
        "https://user:secret@example.test/x",
        "invalid",
    ] {
        assert!(sign("GET", endpoint, &BTreeMap::new(), "secret", "").is_err());
    }
}

#[tokio::test]
async fn present_null_boolean_and_numeric_verifiers_preserve_php_signing_and_form_values() {
    let (remote, r, task) = remote().await;
    let (base, _, app) = rp(&remote).await;
    let http = Factory::new().unwrap();
    for kind in ["null", "false", "true", "numeric"] {
        *r.mode.lock().unwrap() = String::new();
        let start = call(&http, &format!("{base}/login"), None).await;
        let session = cookie(&start);
        *r.mode.lock().unwrap() = format!("typed-{kind}");
        let response = call(
            &http,
            &format!("{base}/callback?oauth_token=temp&typed={kind}"),
            Some(&session),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK, "{kind}");
        assert_eq!(
            oracle()["callbacks"][format!(
                "{}-verifier",
                if kind == "numeric" { "numeric" } else { kind }
            )]["error"],
            Value::Null
        );
    }
    app.abort();
    task.abort();
}
