use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, post},
};
use openrust::{
    config::Repository,
    contracts::config::Repository as _,
    http::{Body, Request, client::Factory},
};
use openrust_socialite::{
    Error, SocialiteManager,
    two::{AbstractProvider, FacebookProvider, ProviderConfig, ProviderEndpoints},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/facebook.json")).unwrap()
}
fn request() -> Request {
    let (parts, body) = http::Request::builder()
        .uri("/callback?code=fixed-code")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, body)
}
#[derive(Default)]
struct Remote {
    keys: Mutex<Value>,
    mode: Mutex<String>,
    response: Mutex<Value>,
    calls: Mutex<Vec<Value>>,
}
async fn token(
    State(r): State<Arc<Remote>>,
    Path(version): Path<String>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let headers = request.headers().clone();
    let body = axum::body::to_bytes(request.into_body(), 65536)
        .await
        .unwrap();
    let fields = url::form_urlencoded::parse(&body)
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    r.calls.lock().unwrap().push(json!({"kind":"token","version":version,"fields":fields,"accept":headers.get("accept").and_then(|v|v.to_str().ok()),"authorization":headers.get("authorization").and_then(|v|v.to_str().ok())}));
    match r.mode.lock().unwrap().as_str() {
        "token-error" => (http::StatusCode::BAD_REQUEST, "private error").into_response(),
        "token-json" => {
            ([("content-type", "application/json")], "private bad json").into_response()
        }
        "jwt" => {
            axum::Json(json!({"access_token":fixture()["jwt"]["valid"]["token"],"expires":3600}))
                .into_response()
        }
        _ => axum::Json(r.response.lock().unwrap().clone()).into_response(),
    }
}
async fn user(
    State(r): State<Arc<Remote>>,
    Path(version): Path<String>,
    request: axum::extract::Request,
) -> axum::response::Response {
    assert_eq!(request.headers()["accept"], "application/json");
    assert!(!request.headers().contains_key("authorization"));
    let fields = url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    r.calls
        .lock()
        .unwrap()
        .push(json!({"kind":"profile","version":version,"fields":fields}));
    match r.mode.lock().unwrap().as_str() {
        "revoked" => (http::StatusCode::UNAUTHORIZED, "private revoked").into_response(),
        "profile-json" => {
            ([("content-type", "application/json")], "private bad json").into_response()
        }
        _ => axum::Json(fixture()["user"]["raw"].clone()).into_response(),
    }
}
async fn certs(State(r): State<Arc<Remote>>) -> axum::response::Response {
    r.calls.lock().unwrap().push(json!({"kind":"jwks"}));
    match r.mode.lock().unwrap().as_str() {
        "jwks-error" => (http::StatusCode::SERVICE_UNAVAILABLE, "private certs").into_response(),
        "jwks-json" => {
            ([("content-type", "application/json")], "private bad certs").into_response()
        }
        _ => axum::Json(r.keys.lock().unwrap().clone()).into_response(),
    }
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let r = Arc::new(Remote {
        keys: Mutex::new(fixture()["jwt"]["valid"]["keys"].clone()),
        response: Mutex::new(json!({"access_token":"access","expires":3600})),
        ..Default::default()
    });
    let router = Router::new()
        .route("/{version}/token", post(token))
        .route("/{version}/me", get(user))
        .route("/jwks/", get(certs))
        .with_state(r.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, r, task)
}
fn definition(base: &str) -> FacebookProvider {
    FacebookProvider::withEndpoints(
        ProviderEndpoints::new(
            format!("{base}/v23.0/dialog/oauth"),
            format!("{base}/v23.0/token"),
            format!("{base}/v23.0/me"),
        ),
        format!("{base}/jwks/"),
    )
}
fn provider<'a>(request: &'a mut Request, base: &str, secret: &str) -> AbstractProvider<'a> {
    AbstractProvider::new(request,ProviderConfig::from_config(&json!({"client_id":"client","client_secret":secret,"redirect":"https://rp.test/callback"})).unwrap(),Arc::new(Factory::new().unwrap()),Arc::new(definition(base)))
}
fn mapped(user: &openrust_socialite::two::User) -> Value {
    json!({"id":user.getId(),"nickname":user.getNickname(),"name":user.getName(),"email":user.getEmail(),"avatar":user.getAvatar(),"raw":user.getRaw(),"attributes":user.attributes})
}
#[tokio::test]
async fn facebook_graph_fields_version_popup_proof_mapping_and_expiry_match_fixed_php() {
    let (base, r, task) = remote().await;
    let mut request = request();
    let mut p = provider(&mut request, &base, "secret");
    p.stateless()
        .with(
            json!({"display":"page","auth_type":"other"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .asPopup()
        .reRequest()
        .usingGraphVersion("v99.0")
        .fields(vec!["id".into(), "name".into(), "email".into()]);
    let url = url::Url::parse(&p.authorizationUrl().unwrap()).unwrap();
    assert_eq!(url.path(), "/v99.0/dialog/oauth");
    assert_eq!(
        Value::Object(
            url.query_pairs()
                .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                .collect()
        ),
        fixture()["parameters"]
    );
    for (i, response) in [
        json!({"access_token":"access","expires":3600}),
        json!({"access_token":"access","expires":3600,"expires_in":7200}),
        json!({"access_token":"access","expires":3600,"expires_in":null}),
    ]
    .into_iter()
    .enumerate()
    {
        *r.response.lock().unwrap() = response;
        assert_eq!(
            p.getAccessTokenResponse("fixed-code").await.unwrap(),
            fixture()["expires"][i]
        );
    }
    let user = p.userFromToken("access").await.unwrap();
    assert_eq!(mapped(&user), fixture()["user"]);
    assert_eq!(p.lastToken(), Some("access"));
    let calls = r.calls.lock().unwrap();
    assert_eq!(calls.len(), 4);
    for call in &calls[..3] {
        assert_eq!(call["fields"], fixture()["http"][0][1]["form_params"]);
        assert_ne!(call["accept"], "application/json");
        assert!(call["authorization"].is_null());
        assert_eq!(call["version"], "v99.0");
    }
    assert_eq!(calls[3]["fields"], fixture()["http"][3][1]["query"]);
    assert_eq!(calls[3]["version"], "v99.0");
    task.abort();
}
#[tokio::test]
async fn facebook_limited_login_jwt_matrix_matches_real_php_with_no_graph_fallback() {
    let (base, r, task) = remote().await;
    for (name, case) in fixture()["jwt"].as_object().unwrap() {
        r.calls.lock().unwrap().clear();
        *r.keys.lock().unwrap() = case["keys"].clone();
        let mut request = request();
        let mut p = provider(&mut request, &base, "secret");
        p.with(case["parameters"].as_object().cloned().unwrap_or_default());
        if let Some(nonce) = case["expected_nonce"].as_str() {
            p.withNonce(nonce);
        }
        let token = case["token"].as_str().unwrap();
        let result = p.userFromToken(token).await;
        assert_eq!(
            result.is_ok(),
            case["accepted"].as_bool().unwrap(),
            "{name}"
        );
        if let Ok(user) = result {
            assert_eq!(user.getRaw(), &case["raw"], "{name}");
            assert_eq!(user.getId(), &json!("user-42"));
        } else {
            assert_eq!(
                result.err().unwrap().to_string(),
                Error::InvalidIdentityToken.to_string(),
                "{name}"
            );
        }
        assert_eq!(p.lastToken(), Some(token));
        assert!(
            !r.calls
                .lock()
                .unwrap()
                .iter()
                .any(|v| v["kind"] == "profile"),
            "{name}"
        );
    }
    task.abort();
}
#[tokio::test]
async fn facebook_default_fields_secret_empty_rules_and_request_options_are_isolated() {
    let (base, r, task) = remote().await;
    for secret in ["", "0", "secret"] {
        let mut request = request();
        let mut p = provider(&mut request, &base, secret);
        p.userFromToken("access").await.unwrap();
        let fields = r.calls.lock().unwrap().last().unwrap()["fields"].clone();
        assert_eq!(
            fields,
            fixture()["proof"][if secret.is_empty() { "empty" } else { secret }]
        );
    }
    let mut request = request();
    let mut changed = provider(&mut request, &base, "secret");
    changed.fields(vec![]).usingGraphVersion("v99.0").asPopup();
    changed.userFromToken("access").await.unwrap();
    let mut other_request = crate::request();
    let mut other = provider(&mut other_request, &base, "secret");
    other.stateless();
    let url = other.authorizationUrl().unwrap();
    assert!(url.contains("/v23.0/dialog/oauth?"));
    assert!(!url.contains("display="));
    other.userFromToken("access").await.unwrap();
    assert_eq!(r.calls.lock().unwrap().last().unwrap()["version"], "v23.0");
    assert_eq!(
        r.calls.lock().unwrap().last().unwrap()["fields"]["fields"],
        fixture()["proof"]["secret"]["fields"]
    );
    task.abort();
}
#[tokio::test]
async fn facebook_code_exchange_limited_login_and_jwks_rotation_enforce_nonce_and_audience() {
    let (base, r, task) = remote().await;
    *r.mode.lock().unwrap() = "jwt".into();
    let mut request = request();
    let mut p = provider(&mut request, &base, "secret");
    p.stateless().withNonce("fixed-nonce");
    let user = p.user().await.unwrap();
    assert_eq!(user.getId(), &json!("user-42"));
    assert_eq!(user.credentials.unwrap().expiresIn, Some(3600));
    *r.keys.lock().unwrap() = fixture()["jwt"]["rotation"]["keys"].clone();
    let rotated = fixture()["jwt"]["rotation"]["token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(p.userFromToken(&rotated).await.is_ok());
    assert!(matches!(
        p.userFromToken(fixture()["jwt"]["valid"]["token"].as_str().unwrap())
            .await,
        Err(Error::InvalidIdentityToken)
    ));
    assert!(
        !r.calls
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "profile")
    );
    task.abort();
}
#[tokio::test]
async fn facebook_remote_errors_revocation_and_malformed_identity_never_leak_private_body() {
    let (base, r, task) = remote().await;
    for mode in [
        "token-error",
        "token-json",
        "revoked",
        "profile-json",
        "jwks-error",
        "jwks-json",
    ] {
        *r.mode.lock().unwrap() = mode.into();
        let mut request = request();
        let mut p = provider(&mut request, &base, "secret");
        p.stateless().withNonce("fixed-nonce");
        let error = if mode.starts_with("jwks") {
            p.userFromToken(fixture()["jwt"]["valid"]["token"].as_str().unwrap())
                .await
                .err()
                .unwrap()
        } else {
            p.user().await.err().unwrap()
        };
        assert!(!error.to_string().contains("private"));
    }
    *r.mode.lock().unwrap() = "ok".into();
    r.calls.lock().unwrap().clear();
    let mut request = request();
    let mut p = provider(&mut request, &base, "secret");
    p.withNonce("fixed-nonce");
    let malformed = "eyJraWQiOiJ0ZXN0LWtleSJ9.invalid.signature";
    assert!(matches!(
        p.userFromToken(malformed).await,
        Err(Error::InvalidIdentityToken)
    ));
    assert!(
        !r.calls
            .lock()
            .unwrap()
            .iter()
            .any(|v| v["kind"] == "profile")
    );
    task.abort();
}
#[tokio::test]
async fn facebook_manager_and_sensitive_query_observations_preserve_wire_behavior() {
    let (base, r, task) = remote().await;
    let config = Arc::new(Repository::new(Default::default()));
    config.set("services.facebook",json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap();
    let http = Arc::new(Factory::new().unwrap());
    http.fake(|_| None);
    let manager = SocialiteManager::new(config.clone(), http.clone());
    let mut request = request();
    {
        let mut p = manager
            .driver("facebook", &mut request)
            .unwrap()
            .oauth2()
            .unwrap();
        p.stateless();
        assert!(
            p.authorizationUrl()
                .unwrap()
                .starts_with("https://www.facebook.com/v23.0/dialog/oauth?")
        );
    }
    manager.extend("facebook", Arc::new(definition(&base)));
    let mut p = manager
        .driver("facebook", &mut request)
        .unwrap()
        .oauth2()
        .unwrap();
    p.userFromToken("access").await.unwrap();
    assert_eq!(
        r.calls.lock().unwrap().last().unwrap()["fields"]["access_token"],
        "access"
    );
    for (request, response) in http.recorded() {
        assert!(request.url.query().is_none());
        assert!(request.body.is_none());
        assert!(response.unwrap().body().unwrap().is_empty());
    }
    let other = SocialiteManager::new(config, Arc::new(Factory::new().unwrap()));
    let mut p = other
        .driver("facebook", &mut request)
        .unwrap()
        .oauth2()
        .unwrap();
    p.stateless();
    assert!(
        p.authorizationUrl()
            .unwrap()
            .starts_with("https://www.facebook.com/")
    );
    task.abort();
}
