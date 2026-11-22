use axum::{
    Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
};
use openrust::{
    config::Repository,
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode, client::Factory},
    support::facades::Route,
};
use openrust_socialite::{
    Error, SocialiteManager, SocialiteServiceProvider,
    two::{ProviderDefinition, ProviderEndpoints, QqProvider},
    wechat::{WeixinProvider, WeixinWebProvider},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
const NAMES: &[&str] = &["weixin", "weixinweb", "qq"];
#[derive(Default)]
struct Remote {
    calls: Mutex<Vec<Value>>,
    mode: Mutex<String>,
}
fn request(code: &str) -> Request {
    let (parts, body) = http::Request::builder()
        .uri(format!("/callback?code={code}"))
        .body(Body::empty())
        .unwrap()
        .into_parts();
    Request::from_parts(parts, body)
}
fn definition(name: &str, base: Option<&str>) -> Arc<dyn ProviderDefinition> {
    let endpoints = base.map(|base| {
        ProviderEndpoints::new(
            format!("{base}/{name}/authorize#wechat_redirect"),
            format!("{base}/{name}/token"),
            format!("{base}/{name}/user"),
        )
    });
    match (name, endpoints) {
        ("weixin", Some(e)) => Arc::new(WeixinProvider::withEndpoints(
            e,
            format!("{}/weixin/refresh", base.unwrap()),
        )),
        ("weixinweb", Some(e)) => Arc::new(WeixinWebProvider::withEndpoints(
            e,
            format!("{}/weixinweb/refresh", base.unwrap()),
        )),
        ("qq", Some(e)) => Arc::new(QqProvider::withEndpoints(
            e,
            format!("{}/qq/me", base.unwrap()),
        )),
        ("weixin", None) => Arc::new(WeixinProvider::default()),
        ("weixinweb", None) => Arc::new(WeixinWebProvider::default()),
        ("qq", None) => Arc::new(QqProvider::default()),
        _ => panic!("unknown driver"),
    }
}
fn services() -> Value {
    let credentials = json!({"client_id":"client","client_secret":"private-secret","redirect":"https://rp.test/callback"});
    json!({"wechat":{"web":credentials,"official_account":credentials},"qq":credentials})
}
fn manager(base: Option<&str>, factory: Arc<Factory>) -> SocialiteManager {
    let manager = SocialiteManager::new(
        Arc::new(Repository::new(BTreeMap::from([(
            "services".into(),
            services(),
        )]))),
        factory,
    );
    if let Some(base) = base {
        for name in NAMES {
            manager.extend(*name, definition(name, Some(base)));
        }
    }
    manager
}
async fn remote_call(
    Path((name, stage)): Path<(String, String)>,
    State(remote): State<Arc<Remote>>,
    req: axum::extract::Request,
) -> axum::response::Response {
    assert_eq!(req.method(), http::Method::GET);
    let fields = url::form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes())
        .into_owned()
        .collect::<BTreeMap<_, _>>();
    remote
        .calls
        .lock()
        .unwrap()
        .push(json!({"name":name,"stage":stage,"fields":fields}));
    let mode = remote.mode.lock().unwrap().clone();
    if mode == format!("{stage}-http") {
        return (http::StatusCode::BAD_REQUEST, "private-secret").into_response();
    }
    if mode == format!("{stage}-json") {
        return "invalid private-secret JSON".into_response();
    }
    if mode == format!("{stage}-error") {
        return axum::Json(if name == "qq" {
            json!({"ret":100,"msg":"private-secret"})
        } else {
            json!({"errcode":40029,"errmsg":"private-secret"})
        })
        .into_response();
    }
    let result = match stage.as_str() {
        "token" | "refresh" => {
            let refresh = fields["grant_type"] == "refresh_token";
            if name == "qq" {
                assert_eq!(fields["client_id"], "client");
                assert_eq!(fields["client_secret"], "private-secret");
                assert_eq!(fields["fmt"], "json");
                if !refresh {
                    assert_eq!(fields["redirect_uri"], "https://rp.test/callback");
                }
            } else {
                assert_eq!(fields["appid"], "client");
                assert!(!fields.contains_key("client_id"));
                assert!(!fields.contains_key("redirect_uri"));
                if !refresh {
                    assert_eq!(fields["secret"], "private-secret");
                } else {
                    assert!(!fields.contains_key("secret"));
                }
            }
            let code = fields
                .get("code")
                .map(String::as_str)
                .unwrap_or("refreshed");
            let mut result = json!({"access_token":format!("private-token-{code}"),"openid":format!("id-{code}"),"refresh_token":"private-refresh","expires_in":7200});
            if mode == "missing-token" {
                result.as_object_mut().unwrap().remove("access_token");
            }
            if mode == "missing-openid" {
                result.as_object_mut().unwrap().remove("openid");
            }
            result
        }
        "me" => {
            assert_eq!(fields["fmt"], "json");
            let code = fields["access_token"]
                .strip_prefix("private-token-")
                .unwrap();
            let mut result = json!({"client_id":"client","openid":format!("id-{code}")});
            if fields.get("unionid").is_some_and(|v| v == "1") {
                result["unionid"] = "union-id".into();
            }
            if mode == "missing-openid" {
                result.as_object_mut().unwrap().remove("openid");
            }
            if mode == "client-mismatch" {
                result["client_id"] = "other-client".into();
            }
            result
        }
        "user" => {
            let code = fields["access_token"]
                .strip_prefix("private-token-")
                .unwrap();
            assert_eq!(fields["openid"], format!("id-{code}"));
            if name == "qq" {
                assert_eq!(fields["oauth_consumer_key"], "client");
                json!({"ret":0,"nickname":"读者","figureurl_qq_2":"https://images.test/qq"})
            } else {
                assert_eq!(fields["lang"], "zh_CN");
                json!({"openid":if mode=="user-mismatch" {"wrong-id".into()}else{format!("id-{code}")},"unionid":"union-id","nickname":"读者","headimgurl":"https://images.test/weixin"})
            }
        }
        _ => panic!("unexpected stage"),
    };
    axum::Json(result).into_response()
}
async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (base, task)
}
async fn remote() -> (String, Arc<Remote>, tokio::task::JoinHandle<()>) {
    let remote = Arc::new(Remote::default());
    let (base, task) = serve(
        Router::new()
            .route("/{name}/{stage}", get(remote_call))
            .with_state(remote.clone()),
    )
    .await;
    (base, remote, task)
}
#[tokio::test]
async fn registered_drivers_use_upstream_endpoints_and_authorization_parameters() {
    let manager = manager(None, Arc::new(Factory::new().unwrap()));
    for name in NAMES {
        let mut req = request("one");
        let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
        p.stateless();
        let url = url::Url::parse(&p.authorizationUrl().unwrap()).unwrap();
        let fields = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        assert_eq!(fields["response_type"], "code");
        assert_eq!(fields["redirect_uri"], "https://rp.test/callback");
        assert!(!fields.contains_key("state"));
        if *name == "qq" {
            assert_eq!(url.host_str(), Some("graph.qq.com"));
            assert_eq!(fields["client_id"], "client");
            assert_eq!(fields["scope"], "get_user_info");
        } else {
            assert_eq!(url.host_str(), Some("open.weixin.qq.com"));
            assert_eq!(fields["appid"], "client");
            assert!(!fields.contains_key("client_id"));
            assert_eq!(url.fragment(), Some("wechat_redirect"));
            assert_eq!(
                fields["scope"],
                if *name == "weixin" {
                    "snsapi_userinfo"
                } else {
                    "snsapi_login"
                }
            );
            assert_eq!(
                url.path(),
                if *name == "weixin" {
                    "/connect/oauth2/authorize"
                } else {
                    "/connect/qrconnect"
                }
            );
        }
    }
}
#[tokio::test]
async fn three_drivers_exchange_profiles_refresh_and_redact_real_http() {
    let (base, remote, task) = remote().await;
    let factory = Arc::new(Factory::new().unwrap());
    factory.fake(|_| None);
    let manager = manager(Some(&base), factory.clone());
    for name in NAMES {
        let mut req = request("one");
        let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
        p.stateless().withUnionId(true);
        let user = p.user().await.unwrap();
        assert_eq!(user.getId(), "id-one");
        assert_eq!(user.getNickname(), Some("读者"));
        assert_eq!(user.getEmail(), None);
        assert_eq!(user.getName(), None);
        assert_eq!(user.attributes["unionid"], "union-id");
        assert_eq!(
            user.getAvatar(),
            Some(if *name == "qq" {
                "https://images.test/qq"
            } else {
                "https://images.test/weixin"
            })
        );
        if *name == "qq" {
            assert_eq!(
                user.getRaw(),
                &json!({"ret":0,"nickname":"读者","figureurl_qq_2":"https://images.test/qq"})
            );
        }
        assert_eq!(user.credentials.as_ref().unwrap().expiresIn, Some(7200));
        let calls = remote.calls.lock().unwrap().len();
        assert_eq!(p.user().await.unwrap().getId(), "id-one");
        assert_eq!(calls, remote.calls.lock().unwrap().len());
        assert_eq!(
            p.refreshToken("private-refresh")
                .await
                .unwrap()
                .token
                .as_str(),
            "private-token-refreshed"
        );
    }
    assert_eq!(remote.calls.lock().unwrap().len(), 10);
    for (request, response) in factory.recorded() {
        assert!(request.url.query().is_none());
        assert!(request.body.is_none());
        assert!(request.headers.is_empty());
        assert!(response.unwrap().body().unwrap().is_empty());
    }
    task.abort();
}
#[tokio::test]
async fn weixin_silent_scope_and_direct_token_require_explicit_openid() {
    let (base, remote, task) = remote().await;
    let manager = manager(Some(&base), Arc::new(Factory::new().unwrap()));
    let mut req = request("one");
    let mut p = manager
        .driver("weixin", &mut req)
        .unwrap()
        .oauth2()
        .unwrap();
    p.stateless().setScopes(vec!["snsapi_base".into()]);
    let user = p.user().await.unwrap();
    assert_eq!(user.getId(), "id-one");
    assert_eq!(user.getNickname(), None);
    assert_eq!(remote.calls.lock().unwrap().len(), 1);
    for name in ["weixin", "weixinweb"] {
        let mut req = request("one");
        let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
        assert!(matches!(
            p.userFromToken("private-token-direct").await,
            Err(Error::Configuration("openid"))
        ));
        p.setOpenId("id-direct");
        assert_eq!(
            p.userFromToken("private-token-direct")
                .await
                .unwrap()
                .getId(),
            "id-direct"
        );
    }
    task.abort();
}
#[tokio::test]
async fn provider_errors_and_inconsistent_identities_fail_without_exposing_secrets() {
    let (base, remote, task) = remote().await;
    let manager = manager(Some(&base), Arc::new(Factory::new().unwrap()));
    for name in NAMES {
        let mut modes = vec![
            "token-http",
            "token-json",
            "token-error",
            "missing-token",
            "missing-openid",
            "user-error",
            "user-http",
            "user-json",
        ];
        if *name == "qq" {
            modes.extend(["me-error", "me-json", "client-mismatch"]);
        } else {
            modes.push("user-mismatch");
        }
        for mode in modes {
            *remote.mode.lock().unwrap() = mode.into();
            let mut req = request("one");
            let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
            p.stateless();
            let err = p
                .user()
                .await
                .err()
                .unwrap_or_else(|| panic!("accepted {name}/{mode}"));
            assert!(!err.to_string().contains("private"));
        }
        *remote.mode.lock().unwrap() = "refresh-error".into();
        let mut req = request("one");
        let mut p = manager.driver(name, &mut req).unwrap().oauth2().unwrap();
        if *name == "qq" {
            *remote.mode.lock().unwrap() = "token-error".into();
        }
        assert!(p.refreshToken("private-refresh").await.is_err());
    }
    task.abort();
}
#[tokio::test]
async fn shared_driver_keeps_simultaneous_openids_and_unionid_options_isolated() {
    let (base, remote, task) = remote().await;
    let manager = manager(Some(&base), Arc::new(Factory::new().unwrap()));
    for name in NAMES {
        let (mut a, mut b) = (request("A"), request("B"));
        let mut a = manager.driver(name, &mut a).unwrap().oauth2().unwrap();
        let mut b = manager.driver(name, &mut b).unwrap().oauth2().unwrap();
        a.stateless().withUnionId(true);
        b.stateless();
        let (a, b) = tokio::join!(a.user(), b.user());
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.getId(), "id-A");
        assert_eq!(b.getId(), "id-B");
        if *name == "qq" {
            assert_eq!(a.attributes["unionid"], "union-id");
            assert_eq!(b.attributes["unionid"], "");
        }
    }
    assert_eq!(remote.calls.lock().unwrap().len(), 14);
    task.abort();
}
fn login_as<'a>(
    name: &'static str,
    request: &'a mut Request,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        Ok(
            openrust_socialite::facades::Socialite::driver(name, request)
                .unwrap()
                .redirect()
                .await
                .unwrap()
                .into(),
        )
    })
}
fn callback_as<'a>(
    name: &'static str,
    request: &'a mut Request,
) -> HttpFuture<'a, Result<Response, HttpError>> {
    Box::pin(async move {
        let result = openrust_socialite::facades::Socialite::driver(name, request)
            .unwrap()
            .user()
            .await;
        match result {
            Ok(user) => Response::json(StatusCode::OK, json!({"id":user.getId()})),
            Err(Error::InvalidState) => {
                Response::json(StatusCode::BAD_REQUEST, json!({"error":"state"}))
            }
            Err(e) => panic!("{e}"),
        }
    })
}
macro_rules! handlers {
    ($login:ident,$callback:ident,$name:literal) => {
        fn $login(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
            login_as($name, r)
        }
        fn $callback(r: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
            callback_as($name, r)
        }
    };
}
handlers!(wx_login, wx_callback, "weixin");
handlers!(web_login, web_callback, "weixinweb");
handlers!(qq_login, qq_callback, "qq");
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/weixin/login", wx_login)?;
    Route::get("/weixin/callback", wx_callback)?;
    Route::get("/weixinweb/login", web_login)?;
    Route::get("/weixinweb/callback", web_callback)?;
    Route::get("/qq/login", qq_login)?;
    Route::get("/qq/callback", qq_callback)?;
    Ok(())
}
async fn rp(base: &str) -> (String, tokio::task::JoinHandle<()>) {
    let mut app = Application::configure(std::env::temp_dir())
        .withConfig(|_, _| {
            Ok(BTreeMap::from([
                ("services".into(), services()),
                ("app".into(), json!({"key":"k".repeat(32),"env":"testing"})),
                (
                    "session".into(),
                    json!({"driver":"array","cookie":"china_session","lottery":[0,100]}),
                ),
                (
                    "cache".into(),
                    json!({"default":"array","stores":{"array":{"driver":"array"}}}),
                ),
            ]))
        })
        .withProviders(vec![Box::new(SocialiteServiceProvider)])
        .withRouting(Routing::default().web(routes))
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let manager = app.make::<SocialiteManager>().await.unwrap();
    for name in NAMES {
        manager.extend(*name, definition(name, Some(base)));
    }
    let mut kernel = Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handleTransport(request).await
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
        .find(|v| v.starts_with("china_session="))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .into()
}
#[tokio::test]
async fn browser_session_state_is_consumed_and_cross_application_callbacks_are_rejected() {
    let (base, remote, server) = remote().await;
    let (one, first) = rp(&base).await;
    let (two, second) = rp(&base).await;
    let client = Factory::new().unwrap();
    for name in NAMES {
        let start = client
            .pendingRequest()
            .withoutRedirecting()
            .get(&format!("{one}/{name}/login"))
            .await
            .unwrap();
        assert_eq!(start.status(), 302);
        let session = cookie(&start);
        let redirect =
            url::Url::parse(start.header("location").unwrap().to_str().unwrap()).unwrap();
        let state = redirect
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        assert_eq!(state.len(), 40);
        async fn callback(
            client: &Factory,
            base: &str,
            name: &str,
            session: &str,
            state: &str,
        ) -> openrust::http::client::Response {
            client
                .pendingRequest()
                .withHeaders(http::HeaderMap::from_iter([(
                    http::header::COOKIE,
                    http::HeaderValue::from_str(session).unwrap(),
                )]))
                .withoutRedirecting()
                .get(&format!(
                    "{base}/{name}/callback?code=browser&state={state}"
                ))
                .await
                .unwrap()
        }
        let count = remote.calls.lock().unwrap().len();
        assert_eq!(
            callback(&client, &two, name, &session, &state)
                .await
                .status(),
            400
        );
        assert_eq!(remote.calls.lock().unwrap().len(), count);
        let ok = callback(&client, &one, name, &session, &state).await;
        assert_eq!(ok.status(), 200);
        assert_eq!(ok.json::<Value>().unwrap()["id"], "id-browser");
        let updated = cookie(&ok);
        let count = remote.calls.lock().unwrap().len();
        assert_eq!(
            callback(&client, &one, name, &updated, &state)
                .await
                .status(),
            400
        );
        assert_eq!(remote.calls.lock().unwrap().len(), count);
    }
    first.abort();
    second.abort();
    server.abort();
}

#[tokio::test]
async fn three_driver_mappings_and_auth_fields_match_fixed_php_reference() {
    let expected: Value = serde_json::from_str(include_str!("fixtures/china.json")).unwrap();
    let (base, _remote, task) = remote().await;
    let local = manager(Some(&base), Arc::new(Factory::new().unwrap()));
    let defaults = manager(None, Arc::new(Factory::new().unwrap()));
    for name in NAMES {
        let mut req = request("fixed");
        let mut p = defaults.driver(name, &mut req).unwrap().oauth2().unwrap();
        p.stateless();
        let url = url::Url::parse(&p.authorizationUrl().unwrap()).unwrap();
        let fields: serde_json::Map<String, Value> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned().into()))
            .collect();
        let mut wanted = expected[name]["fields"].clone();
        wanted.as_object_mut().unwrap().remove("state");
        assert_eq!(Value::Object(fields), wanted);
        let mut req = request("fixed");
        let mut p = local.driver(name, &mut req).unwrap().oauth2().unwrap();
        p.setOpenId("id-fixed").withUnionId(true);
        let user = p.userFromToken("private-token-fixed").await.unwrap();
        let actual = json!({"id":user.getId(),"nickname":user.getNickname(),"name":user.getName(),"email":user.getEmail(),"avatar":user.getAvatar(),"raw":user.getRaw(),"attributes":user.attributes});
        assert_eq!(actual, expected[name]["user"], "{name}");
    }
    task.abort();
}

#[tokio::test]
async fn grouped_wechat_configuration_selects_the_correct_application_without_flat_fallback() {
    let http = Arc::new(Factory::new().unwrap());
    http.fake(|r| {
        let query = r
            .url()
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>();
        assert_eq!(query["appid"], "mini-client");
        assert_eq!(query["secret"], "mini-secret");
        Some(Ok(openrust::http::client::Response::new(
            http::StatusCode::OK,
            http::HeaderMap::new(),
            json!({"openid":"mini-id","session_key":"mini-session"}).to_string(),
        )))
    });
    let web = json!({"client_id":"web-client","client_secret":"web-secret","redirect":"/web/callback","scopes":["web-extra"]});
    let official = json!({"client_id":"official-client","client_secret":"official-secret","redirect":"/official/callback","scopes":["official-extra"]});
    let old = json!({"client_id":"wrong-client","client_secret":"wrong-secret","redirect":"https://wrong.test"});
    let mini = json!({"app_id":"mini-client","secret":"mini-secret"});
    let configured = json!({"wechat":{"web":web,"official_account":official,"mini_app":mini},"weixin":old,"weixinweb":old,"weixin_mini_app":{"app_id":"wrong-mini","secret":"wrong-mini-secret"},"qq":old});
    let build = |services| {
        SocialiteManager::new(
            Arc::new(Repository::new(BTreeMap::from([
                ("services".into(), services),
                ("app".into(), json!({"url":"https://rp.test"})),
            ]))),
            http.clone(),
        )
    };
    let manager = build(configured);
    for (driver, id, callback, scope) in [
        ("weixinweb", "web-client", "web", "web-extra"),
        ("weixin", "official-client", "official", "official-extra"),
    ] {
        let mut req = request("code");
        let mut provider = manager.driver(driver, &mut req).unwrap().oauth2().unwrap();
        provider.stateless();
        let url = url::Url::parse(&provider.authorizationUrl().unwrap()).unwrap();
        let fields = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        assert_eq!(fields["appid"], id);
        assert_eq!(
            fields["redirect_uri"],
            format!("https://rp.test/{callback}/callback")
        );
        assert!(fields["scope"].contains(scope));
    }
    assert_eq!(
        manager
            .miniApp()
            .unwrap()
            .codeToSession("mini-code")
            .await
            .unwrap()
            .openid,
        "mini-id"
    );
    // Flat credentials cannot silently select a different WeChat application.
    let manager = build(json!({"weixin":old,"weixinweb":old,"weixin_mini_app":mini}));
    for driver in ["weixin", "weixinweb"] {
        let mut req = request("code");
        assert!(matches!(
            manager.driver(driver, &mut req),
            Err(Error::Configuration(_))
        ));
    }
    assert!(matches!(manager.miniApp(), Err(Error::Configuration(_))));
}
