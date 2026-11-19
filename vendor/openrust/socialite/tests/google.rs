use axum::{
    Router,
    extract::State,
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
    two::{
        AbstractProvider, GoogleProvider, ProviderConfig, ProviderDefinition, ProviderEndpoints,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/google.json")).unwrap()
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
    calls: Mutex<Vec<Value>>,
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
    remote.calls.lock().unwrap().push(json!({"fields":fields,"accept":headers.get("accept").and_then(|v|v.to_str().ok()),"authorization":headers.get("authorization").and_then(|v|v.to_str().ok())}));
    match remote.mode.lock().unwrap().as_str() {
        "token-error" => (http::StatusCode::BAD_REQUEST, "private token-error").into_response(),
        "jwt" => {
            axum::Json(json!({"access_token":fixture()["jwt"]["valid"]["token"],"expires_in":3600}))
                .into_response()
        }
        "refresh-null" => {
            axum::Json(json!({"access_token":"access","refresh_token":null})).into_response()
        }
        "refresh-rotate" => {
            axum::Json(json!({"access_token":"access","refresh_token":"rotated"})).into_response()
        }
        _ => axum::Json(
            json!({"access_token":"access","expires_in":3600,"scope":"openid profile email"}),
        )
        .into_response(),
    }
}
async fn user(
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    assert_eq!(request.headers()["accept"], "application/json");
    assert_eq!(request.headers()["authorization"], "Bearer access");
    let fields = url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    assert_eq!(fields["prettyPrint"], "false");
    remote.calls.lock().unwrap().push(json!({"profile":true}));
    match remote.mode.lock().unwrap().as_str(){"revoked"=>(http::StatusCode::UNAUTHORIZED,"private revoked").into_response(),"profile-json"=>([("content-type","application/json")],"private bad json").into_response(),_=>axum::Json(json!({"sub":"user-42","name":"Alice","email":"alice@test","email_verified":true,"picture":"https://images.test/alice"})).into_response()}
}
async fn certs(State(remote): State<Arc<Remote>>) -> axum::response::Response {
    remote.calls.lock().unwrap().push(json!({"jwks":true}));
    match remote.mode.lock().unwrap().as_str() {
        "jwks-error" => (http::StatusCode::SERVICE_UNAVAILABLE, "private certs").into_response(),
        "jwks-json" => (
            [("content-type", "application/json")],
            "private bad certs json",
        )
            .into_response(),
        _ => axum::Json(remote.keys.lock().unwrap().clone()).into_response(),
    }
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let remote = Arc::new(Remote {
        keys: Mutex::new(fixture()["keys"].clone()),
        ..Default::default()
    });
    let router = Router::new()
        .route("/token", post(token))
        .route("/user", get(user))
        .route("/certs", get(certs))
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (base, remote, task)
}
fn provider<'a>(request: &'a mut Request, base: &str, client_id: &str) -> AbstractProvider<'a> {
    AbstractProvider::new(request,ProviderConfig::from_config(&json!({"client_id":client_id,"client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap(),Arc::new(Factory::new().unwrap()),Arc::new(GoogleProvider::withEndpoints(ProviderEndpoints::new(format!("{base}/authorize"),format!("{base}/token"),format!("{base}/user")),format!("{base}/certs"))))
}
fn mapped(user: &openrust_socialite::two::User) -> Value {
    json!({"id":user.getId(),"nickname":user.getNickname(),"name":user.getName(),"email":user.getEmail(),"avatar":user.getAvatar(),"raw":user.getRaw(),"attributes":user.attributes})
}
#[tokio::test]
async fn google_default_fields_mapping_and_oauth_exchange_refresh_match_fixed_php() {
    let (base, remote, task) = remote().await;
    let mut request = request();
    let mut p = provider(&mut request, &base, "client");
    p.stateless();
    let url = url::Url::parse(&p.authorizationUrl().unwrap()).unwrap();
    let fields = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
        .collect::<serde_json::Map<_, _>>();
    let mut expected = fixture()["default_fields"].as_object().unwrap().clone();
    expected.remove("state");
    assert_eq!(fields, expected);
    let user = p.user().await.unwrap();
    assert_eq!(mapped(&user), fixture()["user"]);
    assert!(user.credentials.as_ref().unwrap().refreshToken.is_none());
    let token = p.refreshToken("keep-refresh").await.unwrap();
    assert_eq!(token.token.as_str(), fixture()["refresh"]["token"]);
    assert_eq!(token.refreshToken.unwrap().as_str(), "keep-refresh");
    assert_eq!(token.expiresIn, Some(3600));
    assert_eq!(
        json!(token.approvedScopes),
        fixture()["refresh"]["approvedScopes"]
    );
    {
        let calls = remote.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0]["fields"],
            json!({"client_id":"client","client_secret":"secret","code":"fixed-code","redirect_uri":"https://rp.test/callback","grant_type":"authorization_code"})
        );
        assert!(calls[0]["authorization"].is_null());
        assert_eq!(calls[2]["fields"], fixture()["http"][1][1]["form_params"]);
    }
    for (mode, expected) in [("refresh-null", None), ("refresh-rotate", Some("rotated"))] {
        *remote.mode.lock().unwrap() = mode.into();
        assert_eq!(
            p.refreshToken("keep")
                .await
                .unwrap()
                .refreshToken
                .as_deref()
                .map(|v| v.as_str()),
            expected
        );
    }
    task.abort();
}
#[tokio::test]
async fn google_jwt_signature_claim_matrix_matches_real_php_and_never_falls_back_to_userinfo() {
    let (base, remote, task) = remote().await;
    for (name, case) in fixture()["jwt"].as_object().unwrap() {
        remote.calls.lock().unwrap().clear();
        *remote.keys.lock().unwrap() = case["keys"].clone();
        let mut request = request();
        let mut p = provider(&mut request, &base, "client");
        let result = p.userFromToken(case["token"].as_str().unwrap()).await;
        assert_eq!(
            result.is_ok(),
            case["accepted"].as_bool().unwrap(),
            "{name}"
        );
        if let Err(error) = result {
            assert!(
                matches!(error, Error::InvalidIdentityToken),
                "{name}: {error}"
            );
        }
        let calls = remote.calls.lock().unwrap();
        assert!(!calls.iter().any(|v| v.get("profile").is_some()), "{name}");
    }
    task.abort();
}
#[tokio::test]
async fn google_jwks_rotation_and_code_exchange_jwt_path_keep_client_audience_isolated() {
    let (base, remote, task) = remote().await;
    let mut request = request();
    let mut p = provider(&mut request, &base, "client");
    p.stateless();
    *remote.mode.lock().unwrap() = "jwt".into();
    assert_eq!(p.user().await.unwrap().getId(), &json!("user-42"));
    *remote.keys.lock().unwrap() = fixture()["rotated_keys"].clone();
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
    let mut other_request = crate::request();
    let mut other = provider(&mut other_request, &base, "other-client");
    assert!(matches!(
        other.userFromToken(&rotated).await,
        Err(Error::InvalidIdentityToken)
    ));
    assert_eq!(
        remote
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|v| v.get("jwks").is_some())
            .count(),
        4
    );
    assert!(
        !remote
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|v| v.get("profile").is_some())
    );
    task.abort();
}
#[tokio::test]
async fn google_remote_and_jwks_errors_and_revocation_fail_without_private_payload() {
    let (base, remote, task) = remote().await;
    for mode in [
        "token-error",
        "revoked",
        "profile-json",
        "jwks-error",
        "jwks-json",
    ] {
        *remote.mode.lock().unwrap() = mode.into();
        let mut request = request();
        let mut p = provider(&mut request, &base, "client");
        p.stateless();
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
    task.abort();
}
#[tokio::test]
async fn google_manager_default_and_custom_definition_are_application_local() {
    let config = Arc::new(Repository::new(Default::default()));
    config
        .set(
            "services.google",
            json!({"client_id":"client","client_secret":"secret","redirect":"/callback"}),
        )
        .unwrap();
    config
        .set("app.url", json!("https://rp.test/base"))
        .unwrap();
    let manager = SocialiteManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    let mut request = request();
    let mut p = manager.driver("google", &mut request).unwrap();
    p.stateless();
    let url = p.authorizationUrl().unwrap();
    assert!(url.starts_with("https://accounts.google.com/o/oauth2/auth?"));
    assert!(url.contains("redirect_uri=https%3A%2F%2Frp.test%2Fbase%2Fcallback"));
    let other = SocialiteManager::new(config, Arc::new(Factory::new().unwrap()));
    manager.extend(
        "google",
        Arc::new(GoogleProvider::withEndpoints(
            ProviderEndpoints::new(
                "https://custom.test/auth",
                "https://custom.test/token",
                "https://custom.test/user",
            ),
            "https://custom.test/certs",
        )),
    );
    let mut p = manager.driver("google", &mut request).unwrap();
    p.stateless();
    assert!(
        p.authorizationUrl()
            .unwrap()
            .starts_with("https://custom.test/")
    );
    let mut p = other.driver("google", &mut request).unwrap();
    p.stateless();
    assert!(
        p.authorizationUrl()
            .unwrap()
            .starts_with("https://accounts.google.com/")
    );
    let d = GoogleProvider::default();
    assert_eq!(
        d.getTokenUrl(),
        "https://www.googleapis.com/oauth2/v4/token"
    );
}
