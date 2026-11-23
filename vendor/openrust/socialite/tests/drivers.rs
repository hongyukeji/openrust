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
    two::{
        AbstractProvider, BitbucketProvider, GitlabProvider, LinkedInOpenIdProvider,
        LinkedInProvider, ProviderConfig, ProviderDefinition, ProviderEndpoints,
        SlackOpenIdProvider, SlackProvider,
    },
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
const NAMES: &[&str] = &[
    "gitlab",
    "bitbucket",
    "linkedin",
    "linkedin-openid",
    "slack",
    "slack-openid",
];
fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/drivers.json")).unwrap()
}
fn inputs() -> Value {
    serde_json::from_str(include_str!("fixtures/driver-inputs.json")).unwrap()
}
fn request() -> Request {
    let (parts, body) = http::Request::builder()
        .uri("/callback?code=fixed-code")
        .body(Body::empty())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, body)
}
fn definition(name: &str, base: Option<&str>) -> Arc<dyn ProviderDefinition> {
    let endpoints = base.map(|base| {
        ProviderEndpoints::new(
            format!("{base}/{name}/authorize"),
            format!("{base}/{name}/token"),
            format!("{base}/{name}/user"),
        )
        .email(format!("{base}/{name}/emails"))
    });
    match name {
        "gitlab" => Arc::new(
            endpoints
                .map(GitlabProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        "bitbucket" => Arc::new(
            endpoints
                .map(BitbucketProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        "linkedin" => Arc::new(
            endpoints
                .map(LinkedInProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        "linkedin-openid" => Arc::new(
            endpoints
                .map(LinkedInOpenIdProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        "slack" => Arc::new(
            endpoints
                .map(SlackProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        "slack-openid" => Arc::new(
            endpoints
                .map(SlackOpenIdProvider::withEndpoints)
                .unwrap_or_default(),
        ),
        _ => panic!("unknown driver"),
    }
}
fn profile(user: &openrust_socialite::two::User) -> Value {
    json!({"id":user.get_id(),"nickname":user.getNickname(),"name":user.get_name(),"email":user.getEmail(),"avatar":user.getAvatar(),"raw":user.getRaw(),"attributes":user.attributes})
}
#[tokio::test]
async fn six_driver_scopes_parameters_and_user_mappings_match_fixed_php_sources() {
    for name in NAMES {
        let definition = definition(name, None);
        let mapped = definition.mapUserToObject(inputs()[name].clone()).unwrap();
        assert_eq!(profile(&mapped), fixture()[name]["user"], "{name}");
        let mut request = request();
        let config=ProviderConfig::from_config(&json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap();
        let mut provider = AbstractProvider::new(
            &mut request,
            config,
            Arc::new(Factory::new().unwrap()),
            definition,
        );
        provider.stateless();
        for key in ["stateless", "parameters", "bot"] {
            if key == "parameters" {
                provider.scopes(["extra".into(), "extra".into()]);
                provider.with(
                    serde_json::from_value(
                        json!({"scope":"override","user_scope":"custom","prompt":"select account"}),
                    )
                    .unwrap(),
                );
            }
            if key == "bot" {
                if *name != "slack" {
                    continue;
                }
                provider.asBotUser();
            }
            let url = url::Url::parse(&provider.authorizationUrl().unwrap()).unwrap();
            let actual: serde_json::Map<String, Value> = url
                .query_pairs()
                .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                .collect();
            assert_eq!(Value::Object(actual), fixture()[name][key], "{name}/{key}");
        }
    }
}
#[derive(Default)]
struct Remote {
    mode: Mutex<String>,
    calls: Mutex<Vec<Value>>,
}
async fn exchange(
    Path(name): Path<String>,
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
    assert_eq!(headers["accept"], "application/json");
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/x-www-form-urlencoded")
    );
    remote.calls.lock().unwrap().push(json!({"name":name,"fields":fields,"authorization":headers.get("authorization").and_then(|v|v.to_str().ok())}));
    if *remote.mode.lock().unwrap() == "token-error" {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "private provider failure",
        )
            .into_response();
    }
    let token = json!({"access_token":"secret-token","refresh_token":"refresh","expires_in":300,"scope":"one two"});
    axum::Json(if name=="slack" {json!({"access_token":"bot-token","refresh_token":"bot-refresh","expires_in":600,"scope":"one,two","authed_user":{"access_token":"user-token","refresh_token":"user-refresh","expires_in":100,"scope":"identity.basic,identity.email"}})}else{token}).into_response()
}
async fn user(
    Path(name): Path<String>,
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let query = url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    let headers = request.headers();
    let authorization = headers.get("authorization").and_then(|v| v.to_str().ok());
    if name == "gitlab" {
        assert_eq!(query.get("access_token").unwrap(), "secret-token");
        assert!(authorization.is_none());
    } else if name == "slack" {
        assert!(matches!(
            authorization,
            Some("Bearer user-token" | "Bearer bot-token")
        ));
    } else {
        assert_eq!(authorization, Some("Bearer secret-token"));
    }
    if name.starts_with("linkedin") {
        assert_eq!(headers["x-restli-protocol-version"], "2.0.0");
        if name == "linkedin" {
            assert_eq!(
                query["projection"],
                "(id,firstName,lastName,profilePicture(displayImage~:playableStreams),vanityName)"
            );
        } else {
            assert_eq!(
                query["projection"],
                "(sub,email,email_verified,name,given_name,family_name,picture)"
            );
        }
    }
    remote
        .calls
        .lock()
        .unwrap()
        .push(json!({"profile":name,"query":query,"authorization":authorization}));
    if *remote.mode.lock().unwrap() == "revoked" {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            "revoked private token",
        )
            .into_response();
    }
    if *remote.mode.lock().unwrap() == "profile-error" {
        return ([("content-type", "application/json")], "not valid JSON").into_response();
    }
    let mut user = inputs()[&name].clone();
    if name == "linkedin" {
        user.as_object_mut().unwrap().remove("emailAddress");
    }
    if name == "bitbucket" {
        user["email"] = "unconfirmed@test".into();
    }
    axum::Json(user).into_response()
}
async fn emails(
    Path(name): Path<String>,
    State(remote): State<Arc<Remote>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    assert_eq!(request.headers()["authorization"], "Bearer secret-token");
    if *remote.mode.lock().unwrap() == "email-error" {
        return (axum::http::StatusCode::FORBIDDEN, "private").into_response();
    }
    if name == "linkedin" {
        assert_eq!(request.headers()["x-restli-protocol-version"], "2.0.0");
        let pairs = url::form_urlencoded::parse(request.uri().query().unwrap().as_bytes())
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(pairs["q"], "members");
        assert_eq!(pairs["projection"], "(elements*(handle~))");
        axum::Json(json!({"elements":[{"handle~":{"emailAddress":"alice@test"}}]})).into_response()
    } else {
        assert_eq!(name, "bitbucket");
        axum::Json(json!({"values":[{"type":"other","is_primary":true,"is_confirmed":true,"email":"bad-type@test"},{"type":"email","is_primary":true,"is_confirmed":false,"email":"unconfirmed@test"},{"type":"email","is_primary":true,"is_confirmed":true,"email":"alice@test"}]})).into_response()
    }
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let remote = Arc::new(Remote::default());
    let router = Router::new()
        .route("/{name}/token", post(exchange))
        .route("/{name}/user", get(user))
        .route("/{name}/emails", get(emails))
        .with_state(remote.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, remote, task)
}
fn provider<'a>(request: &'a mut Request, name: &str, base: &str) -> AbstractProvider<'a> {
    let config=ProviderConfig::from_config(&json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback"})).unwrap();
    AbstractProvider::new(
        request,
        config,
        Arc::new(Factory::new().unwrap()),
        definition(name, Some(base)),
    )
}
#[tokio::test]
async fn six_drivers_exchange_profile_and_refresh_through_real_http() {
    let (base, remote, task) = remote().await;
    for name in NAMES {
        let mut req = request();
        let mut provider = provider(&mut req, name, &base);
        provider.stateless();
        let user = provider.user().await.unwrap();
        assert_eq!(profile(&user), fixture()[name]["user"], "{name}");
        let token = user.credentials.unwrap();
        assert_eq!(
            token.token.as_str(),
            if *name == "slack" {
                "user-token"
            } else {
                "secret-token"
            }
        );
        assert_eq!(
            token.expiresIn,
            Some(if *name == "slack" { 100 } else { 300 })
        );
        let refreshed = provider.refreshToken("refresh-input").await.unwrap();
        assert_eq!(
            refreshed.token.as_str(),
            if *name == "slack" {
                "bot-token"
            } else {
                "secret-token"
            }
        );
        let calls = remote.calls.lock().unwrap();
        let calls = calls
            .iter()
            .filter(|v| v.get("fields").is_some() && v["name"] == *name)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["fields"]["client_id"], "client");
        assert_eq!(calls[0]["fields"]["client_secret"], "secret");
        assert_eq!(calls[0]["fields"]["code"], "fixed-code");
        assert!(calls[0]["authorization"].is_null());
        assert_eq!(calls[1]["fields"]["refresh_token"], "refresh-input");
    }
    task.abort();
}
#[tokio::test]
async fn bitbucket_direct_token_uses_basic_and_slack_bot_uses_top_level_credentials() {
    let (base, remote, task) = remote().await;
    let mut req = request();
    let mut p = provider(&mut req, "bitbucket", &base);
    p.stateless();
    assert_eq!(
        p.getAccessToken("direct-code").await.unwrap().as_str(),
        "secret-token"
    );
    assert_eq!(
        remote.calls.lock().unwrap()[0]["authorization"],
        "Basic Y2xpZW50OnNlY3JldA=="
    );
    let mut req = request();
    let mut p = provider(&mut req, "slack", &base);
    p.stateless().asBotUser();
    assert_eq!(
        p.user().await.unwrap().credentials.unwrap().token.as_str(),
        "bot-token"
    );
    task.abort();
}
#[tokio::test]
async fn driver_remote_failures_propagate_except_optional_bitbucket_email_lookup() {
    let (base, remote, task) = remote().await;
    for name in NAMES {
        for mode in ["token-error", "profile-error"] {
            *remote.mode.lock().unwrap() = mode.into();
            let mut req = request();
            let mut p = provider(&mut req, name, &base);
            p.stateless();
            let e = p.user().await.err().unwrap();
            assert!(!e.to_string().contains("private"));
            assert!(if mode == "token-error" {
                matches!(e, Error::ProviderStatus(400))
            } else {
                matches!(e, Error::Http(_))
            });
        }
    }
    *remote.mode.lock().unwrap() = "email-error".into();
    let mut req = request();
    let mut p = provider(&mut req, "bitbucket", &base);
    p.stateless();
    assert_eq!(p.user().await.unwrap().getEmail(), None);
    let mut req = request();
    let mut p = provider(&mut req, "linkedin", &base);
    p.stateless();
    assert!(matches!(p.user().await, Err(Error::ProviderStatus(403))));
    task.abort();
}
#[tokio::test]
async fn manager_selects_all_six_default_drivers_and_self_hosted_gitlab_without_cross_application_registration()
 {
    let services=NAMES.iter().map(|name|((*name).into(),json!({"client_id":"client","client_secret":"secret","redirect":"https://rp.test/callback","host":"https://gitlab.test/prefix///"}))).collect::<serde_json::Map<String,Value>>();
    let config = Arc::new(Repository::new(
        [("services".into(), Value::Object(services))].into(),
    ));
    let manager = SocialiteManager::new(config.clone(), Arc::new(Factory::new().unwrap()));
    for name in NAMES {
        let mut req = request();
        let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
        p.stateless();
        let uri = p.authorizationUrl().unwrap();
        if *name == "gitlab" {
            let actual = url::Url::parse(&uri).unwrap();
            let expected = url::Url::parse(fixture()[name]["host"].as_str().unwrap()).unwrap();
            assert_eq!(actual.origin(), expected.origin());
            assert_eq!(actual.path(), expected.path());
            assert_eq!(
                actual.query_pairs().find(|(k, _)| k == "scope").unwrap().1,
                "read_user"
            );
        } else {
            let uri = url::Url::parse(&uri).unwrap();
            assert!(matches!(
                uri.host_str(),
                Some("www.linkedin.com" | "slack.com" | "bitbucket.org")
            ));
        }
    }
    config
        .set("services.gitlab.host", json!({"invalid":true}))
        .unwrap();
    let mut req = request();
    assert!(matches!(
        manager.driver("gitlab", &mut req),
        Err(Error::Configuration("host"))
    ));
}

#[tokio::test]
async fn revoked_remote_tokens_fail_profile_lookup_for_all_six_drivers() {
    let (base, remote, task) = remote().await;
    *remote.mode.lock().unwrap() = "revoked".into();
    for name in NAMES {
        let mut req = request();
        let mut p = provider(&mut req, name, &base);
        let credential = if *name == "slack" {
            "user-token"
        } else {
            "secret-token"
        };
        let error = p.userFromToken(credential).await.err().unwrap();
        assert!(matches!(error, Error::ProviderStatus(401)));
        assert!(!error.to_string().contains("private"));
    }
    task.abort();
}
