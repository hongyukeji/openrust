#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::*;
use openrust::support::facades::Facade;
use openrust::{
    contracts::events::{Dispatcher as _, Event, Listener},
    events::{Delivery, Dispatcher, EventContext, EventFuture, EventResult, ListenerResponse},
};
use openrust_passport::{
    GrantLifetime, HasApiTokens, PersonalAccessTokenFactory,
    events::{AccessTokenCreated, AccessTokenRevoked, RefreshTokenCreated},
    http::middleware::{CheckToken, CheckTokenForAnyScope, EnsureClientIsResourceOwner},
};
impl HasApiTokens for User {
    fn getProviderName(&self) -> &str {
        "users"
    }
}
pub(super) fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::group(openrust::routing::RouteGroup::new(), |r| {
        use openrust_passport::http::middleware::CreateFreshApiToken;
        r.get("/fresh", session)?
            .middleware(Arc::new(CreateFreshApiToken::using(Some("web"))));
        r.get("/fresh-explicit", explicit_cookie)?
            .middleware(Arc::new(CreateFreshApiToken::using(Some("web"))));
        r.post("/logout", logout)?
            .middleware(Arc::new(CreateFreshApiToken::using(Some("web"))));
        r.get("/cookie-resource", resource)?
            .without_middleware("web")
            .middleware_name("auth:api");
        r.get("/cookie-scope", ok)?
            .without_middleware("web")
            .middleware_name("auth:api")
            .middleware(CheckToken::shared(["unregistered", "other"]));
        r.get("/cookie-machine", ok)?
            .without_middleware("web")
            .middleware_name("auth:api")
            .middleware(EnsureClientIsResourceOwner::shared(["unregistered"]));

        r.get("/machine", ok)?
            .middleware(EnsureClientIsResourceOwner::shared(["read"]));
        r.get("/scope-all", ok)?
            .middleware(CheckToken::shared(["read", "write"]));
        r.get("/scope-any", ok)?
            .middleware(CheckTokenForAnyScope::shared(["write", "read"]));
        r.get("/scope-empty-all", ok)?
            .middleware(CheckToken::shared(Vec::<String>::new()));
        r.get("/scope-empty-any", ok)?
            .middleware(CheckTokenForAnyScope::shared(Vec::<String>::new()));
        r.get("/scope-user", ok)?
            .middleware_name("auth:api")
            .middleware(CheckToken::shared(["read"]));
        r.post("/pat", pat)?;
        Ok(())
    })
}
fn ok(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { Response::json(StatusCode::OK, json!({"ok":true})) })
}
fn pat(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let user = Auth::user(request)
            .await?
            .ok_or(HttpError::Unauthenticated)?;
        let name = request
            .input(Some("name"))
            .await?
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "PAT".into());
        let scopes = request
            .input(Some("scopes"))
            .await?
            .map(serde_json::from_value::<Vec<String>>)
            .transpose()
            .map_err(|_| HttpError::InvalidRequest)?
            .unwrap_or_default();
        let result =
            PersonalAccessTokenFactory::make(user.get_auth_identifier(), &name, &scopes, "users")
                .await;
        match result {
            Ok(result) => Response::json(StatusCode::OK, result),
            Err(error) => {
                use axum::response::IntoResponse as _;
                Ok(error.into_response().into())
            }
        }
    })
}
struct Machine {
    id: String,
    secret: String,
}
impl Fixture {
    async fn machine_client(&self, public: bool, scopes: Option<Vec<String>>) -> Machine {
        self.kernel
            .get_application()
            .call(
                openrust::container::Callback::new(&[], move |_| async move {
                    let repo = openrust::container::Container::get_instance()
                        .make::<ClientRepository>()?;
                    let client = repo
                        .createClientCredentialsGrantClient("Machine")
                        .await
                        .unwrap();
                    let id = AuthIdentifier::try_from(&client.getKey().await.unwrap())
                        .unwrap()
                        .to_string();
                    let model = client.model::<openrust_passport::Client>().unwrap();
                    let mut model = model.lock().await;
                    let secret = model.plainSecret.as_ref().unwrap().to_string();
                    if public {
                        model
                            .forceFillAsync(Attributes::from([("secret".into(), Value::Null)]))
                            .await
                            .unwrap();
                    }
                    if let Some(scopes) = scopes {
                        model
                            .forceFillAsync(Attributes::from([(
                                "scopes".into(),
                                Value::Json(json!(scopes)),
                            )]))
                            .await
                            .unwrap();
                    }
                    assert!(model.save().await.unwrap());
                    Ok(Machine { id, secret })
                }),
                Default::default(),
            )
            .await
            .unwrap()
    }
    async fn machine_token(&self, client: &Machine, scope: Option<&str>) -> reqwest::Response {
        let mut body = json!({"grant_type":"client_credentials","client_id":client.id,"client_secret":client.secret});
        if let Some(scope) = scope {
            body["scope"] = json!(scope);
        }
        self.wire(reqwest::Method::POST, "/oauth/token", "", Some(body))
            .await
    }
    async fn personal_client(&self, provider: Option<&str>) -> String {
        let provider = provider.map(str::to_owned);
        self.kernel
            .get_application()
            .call(
                openrust::container::Callback::new(&[], move |_| async move {
                    let repo = openrust::container::Container::get_instance()
                        .make::<ClientRepository>()?;
                    let client = repo
                        .createPersonalAccessGrantClient("Personal", provider.as_deref())
                        .await
                        .unwrap();
                    Ok(AuthIdentifier::try_from(&client.getKey().await.unwrap())
                        .unwrap()
                        .to_string())
                }),
                Default::default(),
            )
            .await
            .unwrap()
    }
}
#[tokio::test]
async fn mysql_machine_issuance_scopes_subject_and_resource_boundaries() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.machine_client(false, None).await;
    f.passport
        .clientCredentialsTokensExpireIn(Some(GrantLifetime::Seconds(90)));
    let response = f.machine_token(&client, Some("read read")).await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let token = json_body(response).await;
    assert_eq!(token["expires_in"], 90);
    assert!(token.get("refresh_token").is_none());
    let claims = jwt_claims(token["access_token"].as_str().unwrap());
    assert_eq!(claims["sub"], client.id);
    assert_eq!(claims["aud"], client.id);
    assert_eq!(claims["scopes"], json!(["read"]));
    let row =
        f.db.table("oauth_access_tokens")
            .first()
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.get("user_id"), Some(&Value::Null));
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        0
    );
    for (path, status) in [
        ("/machine", 200),
        ("/scope-any", 200),
        ("/scope-all", 403),
        ("/scope-empty-all", 200),
        ("/scope-empty-any", 403),
        ("/scope-user", 401),
        ("/resource", 401),
    ] {
        assert_eq!(
            f.resource_token(path, token["access_token"].as_str().unwrap())
                .await
                .status(),
            status,
            "{path}"
        );
    }
    let all = json_body(f.machine_token(&client, Some("*")).await).await;
    assert_eq!(
        f.resource_token("/scope-all", all["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    let user = f.issued().await;
    assert_eq!(
        f.resource_token("/machine", user["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    assert_eq!(
        f.resource_token("/scope-user", user["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_machine_validation_defaults_basic_and_client_scope_restrictions() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    f.passport.setDefaultScope(vec!["read".into()]);
    let public = f.machine_client(true, None).await;
    assert_eq!(
        json_body(f.machine_token(&public, None).await).await["error"],
        "invalid_client"
    );
    f.db.getSchemaBuilder()
        .table("oauth_clients", |t| {
            t.json("scopes").nullable();
        })
        .await
        .unwrap();
    let client = f.machine_client(false, Some(vec!["read".into()])).await;
    let token = json_body(f.machine_token(&client, None).await).await;
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["scopes"],
        json!(["read"])
    );
    let token = json_body(f.machine_token(&client, Some("read write")).await).await;
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["scopes"],
        json!(["read"])
    );
    assert_eq!(
        json_body(f.machine_token(&client, Some("unknown")).await).await["error"],
        "invalid_scope"
    );
    let wrong = f
        .client
        .post(format!("{}/oauth/token", f.base))
        .basic_auth(&client.id, Some("wrong"))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), 401);
    assert!(wrong.headers().contains_key("www-authenticate"));
    let basic = f
        .client
        .post(format!("{}/oauth/token", f.base))
        .basic_auth(&client.id, Some(&client.secret))
        .form(&[("grant_type", "client_credentials"), ("scope", "read")])
        .send()
        .await
        .unwrap();
    assert_eq!(basic.status(), 200);
    let unauthorized=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"client_credentials","client_id":f.confidential,"client_secret":f.secret}))).await;
    assert_eq!(
        json_body(unauthorized).await["error"],
        "unauthorized_client"
    );
    let forged = f
        .wire(
            reqwest::Method::POST,
            "/oauth/token",
            "",
            Some(json!({"grant_type":"personal_access","user_id":"77","provider":"users"})),
        )
        .await;
    assert_eq!(json_body(forged).await["error"], "unsupported_grant_type");
    assert_eq!(
        f.wire(reqwest::Method::GET, "/machine", "", None)
            .await
            .status(),
        401
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_personal_access_uses_provider_subject_name_lifetime_and_camel_result() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.personal_client(Some("users")).await;
    let browser = f.login("alice@test").await;
    f.passport
        .personalAccessTokensExpireIn(Some(GrantLifetime::Seconds(120)));
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"name":"My CLI","scopes":["read","write"],"_token":browser.csrf})),
        )
        .await;
    assert_eq!(response.status(), 200);
    let token = json_body(response).await;
    assert_eq!(token["expiresIn"], 120);
    assert_eq!(token["tokenType"], "Bearer");
    assert!(token.get("refreshToken").is_none());
    let claims = jwt_claims(token["accessToken"].as_str().unwrap());
    assert_eq!(claims["sub"], "77");
    assert_eq!(claims["aud"], client);
    assert_eq!(claims["jti"], token["accessTokenId"]);
    let row =
        f.db.table("oauth_access_tokens")
            .first()
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.get("name"), Some(&Value::Text("My CLI".into())));
    assert_eq!(row.get("user_id"), Some(&Value::Unsigned(77)));
    assert_eq!(
        f.resource_token("/resource", token["accessToken"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.resource_token("/resource-other", token["accessToken"].as_str().unwrap())
            .await
            .status(),
        401
    );
    assert_eq!(
        f.resource_token("/scope-all", token["accessToken"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        0
    );
    let id = token["accessTokenId"].as_str().unwrap().to_owned();
    let result = token;
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], move |_| async move {
                let result: serde_json::Value = result;
                let result = openrust_passport::PersonalAccessTokenResult::new(
                    id,
                    result["accessToken"].as_str().unwrap().into(),
                    120,
                );
                assert!(!format!("{result:?}").contains(&result.accessToken));
                assert_eq!(
                    serde_json::from_str::<J>(&result.to_json().unwrap()).unwrap(),
                    result.to_array()
                );
                let instance = result.getToken().await.unwrap().unwrap();
                assert!(Arc::ptr_eq(
                    &instance,
                    &result.getToken().await.unwrap().unwrap()
                ));
                let mut token = instance.lock().await;
                assert!(
                    openrust_passport::TokenModel::revoke(&mut *token)
                        .await
                        .unwrap()
                );
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_personal_provider_fallback_invalid_scope_and_atomic_veto() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let browser = f.login("alice@test").await;
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"_token":browser.csrf})),
        )
        .await;
    assert_eq!(response.status(), 500);
    f.personal_client(Some("other_users")).await;
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"_token":browser.csrf})),
        )
        .await;
    assert_eq!(response.status(), 500);
    let fallback = f.personal_client(None).await;
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"_token":browser.csrf,"scopes":["unknown"]})),
        )
        .await;
    assert_eq!(json_body(response).await["error"], "invalid_scope");
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"_token":browser.csrf})),
        )
        .await;
    assert_eq!(response.status(), 500);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.veto.store(false, Ordering::Relaxed);
    let response = f
        .wire(
            reqwest::Method::POST,
            "/pat",
            &browser.cookie,
            Some(json!({"_token":browser.csrf,"scopes":["*"]})),
        )
        .await;
    assert_eq!(response.status(), 200);
    let data = json_body(response).await;
    let claims = jwt_claims(data["accessToken"].as_str().unwrap());
    assert_eq!(claims["aud"], fallback);
    assert_eq!(claims["scopes"], json!(["*"]));
    f.cleanup().await;
}
struct Record {
    observed: Arc<std::sync::Mutex<Vec<J>>>,
    fail: Arc<std::sync::atomic::AtomicBool>,
}
impl Listener for Record {
    fn handle<'a>(
        &'a self,
        event: &'a dyn Event,
        _: &'a mut EventContext<'_>,
    ) -> EventFuture<'a, EventResult<Option<ListenerResponse>>> {
        Box::pin(async move {
            if self.fail.load(Ordering::Relaxed) {
                return Err(openrust::events::EventError::InvalidPayload);
            }
            let payload = if let Some(e) = event.downcast_ref::<AccessTokenCreated>() {
                json!({"access":e.tokenId,"user":e.userId,"client":e.clientId})
            } else if let Some(e) = event.downcast_ref::<RefreshTokenCreated>() {
                json!({"refresh":e.refreshTokenId,"access":e.accessTokenId})
            } else if let Some(e) = event.downcast_ref::<AccessTokenRevoked>() {
                json!({"revoked":e.tokenId})
            } else {
                panic!("unexpected event")
            };
            self.observed.lock().unwrap().push(payload);
            Ok(None)
        })
    }
}
#[tokio::test]
async fn mysql_typed_token_events_preserve_payload_order_after_commit_and_rollback() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let committed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let dispatcher = f
        .kernel
        .get_application()
        .make::<Dispatcher>()
        .await
        .unwrap();
    for event in [
        AccessTokenCreated::NAME,
        RefreshTokenCreated::NAME,
        AccessTokenRevoked::NAME,
    ] {
        dispatcher
            .listen(
                event,
                Arc::new(Record {
                    observed: observed.clone(),
                    fail: fail.clone(),
                }),
                Delivery::Sync,
            )
            .unwrap();
        dispatcher
            .listen(
                event,
                Arc::new(Record {
                    observed: committed.clone(),
                    fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                }),
                Delivery::AfterCommit,
            )
            .unwrap();
    }
    let machine = f.machine_client(false, None).await;
    let result = json_body(f.machine_token(&machine, Some("read")).await).await;
    let claims = jwt_claims(result["access_token"].as_str().unwrap());
    assert_eq!(
        observed.lock().unwrap().as_slice(),
        &[json!({"access":claims["jti"],"user":null,"client":machine.id})]
    );
    assert_eq!(committed.lock().unwrap().len(), 1);
    fail.store(true, Ordering::Relaxed);
    assert_eq!(f.machine_token(&machine, Some("read")).await.status(), 500);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    assert_eq!(committed.lock().unwrap().len(), 1);
    fail.store(false, Ordering::Relaxed);
    let user = f.issued().await;
    let before = observed.lock().unwrap().len();
    assert_eq!(before, 3);
    assert!(observed.lock().unwrap()[1].get("user").unwrap() == "77");
    assert!(observed.lock().unwrap()[2].get("refresh").is_some());
    let response = f
        .refresh(user["refresh_token"].as_str().unwrap(), None)
        .await;
    assert_eq!(response.status(), 200);
    let events = observed.lock().unwrap().clone();
    assert!(events[3].get("revoked").is_some());
    assert!(events[4].get("access").is_some());
    assert!(events[5].get("refresh").is_some());
    assert_eq!(committed.lock().unwrap().len(), 6);
    f.cleanup().await;
}

fn explicit_cookie(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let csrf = request.session()?.token().unwrap();
        let cookie = openrust_passport::ApiTokenCookieFactory::make(88u64, csrf).unwrap();
        Response::empty(StatusCode::OK).with_cookie(&cookie, request)
    })
}
fn logout(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Auth::session_guard(request, Some("web"))?.logout().await?;
        Ok(Response::empty(StatusCode::NO_CONTENT))
    })
}
fn cookie_pair(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| {
            v.split(';')
                .next()
                .filter(|v| v.starts_with(&format!("{name}=")))
                .map(str::to_owned)
        })
}
fn decode_cookie_pair(pair: &str) -> String {
    url::form_urlencoded::parse(pair.as_bytes())
        .next()
        .unwrap()
        .1
        .into_owned()
}
impl Fixture {
    async fn cookie_resource(
        &self,
        path: &str,
        cookie: &str,
        csrf: Option<&str>,
    ) -> reqwest::Response {
        let mut r = self
            .client
            .get(format!("{}{path}", self.base))
            .header("Accept", "application/json")
            .header("Cookie", cookie);
        if let Some(csrf) = csrf {
            r = r.header("X-CSRF-TOKEN", csrf);
        }
        r.send().await.unwrap()
    }
    async fn fresh_cookie(&self, b: &Browser) -> String {
        let response = self
            .wire(reqwest::Method::GET, "/fresh", &b.cookie, None)
            .await;
        assert_eq!(response.status(), 200);
        let header = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .find(|v| v.starts_with("openrust_token="))
            .unwrap();
        assert!(header.to_ascii_lowercase().contains("httponly"));
        assert!(header.to_ascii_lowercase().contains("path=/"));
        cookie_pair(&response, "openrust_token").unwrap()
    }
}
#[tokio::test]
async fn mysql_cookie_fresh_user_scopes_csrf_bearer_priority_and_expiry() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    let cookie = f.fresh_cookie(&b).await;
    let response = f
        .cookie_resource("/cookie-resource", &cookie, Some(&b.csrf))
        .await;
    assert_eq!(response.status(), 200);
    let data = json_body(response).await;
    assert_eq!(data["user"], 77);
    assert_eq!(data["transient"], true);
    assert_eq!(data["read"], true);
    assert_eq!(data["write"], true);
    assert_eq!(
        f.cookie_resource("/cookie-scope", &cookie, Some(&b.csrf))
            .await
            .status(),
        200
    );
    assert_eq!(
        f.cookie_resource("/cookie-machine", &cookie, Some(&b.csrf))
            .await
            .status(),
        200
    );
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, None)
            .await
            .status(),
        401
    );
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, Some("wrong"))
            .await
            .status(),
        401
    );
    let corrupt = format!("{cookie}x");
    assert_eq!(
        f.cookie_resource("/cookie-resource", &corrupt, Some(&b.csrf))
            .await
            .status(),
        401
    );
    let response = f
        .client
        .get(format!("{}/cookie-resource", f.base))
        .header("Cookie", &cookie)
        .header("X-CSRF-TOKEN", &b.csrf)
        .bearer_auth("invalid")
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let response = f
        .client
        .get(format!("{}/cookie-resource", f.base))
        .header("Cookie", &cookie)
        .header("X-XSRF-TOKEN", {
            let mut plain =
                openrust::cookie::CookieValuePrefix::create("XSRF-TOKEN", f.encryption.getKey())
                    .to_vec();
            plain.extend_from_slice(b.csrf.as_bytes());
            f.encryption.encryptString(&plain).unwrap()
        })
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    f.kernel
        .get_application()
        .container()
        .get::<dyn openrust::contracts::config::Repository>("config")
        .unwrap()
        .set("session.lifetime", json!(-1))
        .unwrap();
    let expired = f.fresh_cookie(&b).await;
    assert_eq!(
        f.cookie_resource("/cookie-resource", &expired, Some(&b.csrf))
            .await
            .status(),
        401
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_cookie_refresh_csrf_logout_existing_cookie_and_missing_user() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let b = f.login("alice@test").await;
    assert!(
        cookie_pair(
            &f.wire(reqwest::Method::GET, "/fresh", "", None).await,
            "openrust_token"
        )
        .is_none()
    );
    let cookie = f.fresh_cookie(&b).await;
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/token/refresh",
            &b.cookie,
            Some(json!({})),
        )
        .await;
    assert_eq!(response.status(), 419);
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/token/refresh",
            &b.cookie,
            Some(json!({"_token":b.csrf})),
        )
        .await;
    assert_eq!(response.status(), 200);
    assert!(cookie_pair(&response, "openrust_token").is_some());
    assert_eq!(response.text().await.unwrap(), "Refreshed.");
    let explicit = f
        .wire(reqwest::Method::GET, "/fresh-explicit", &b.cookie, None)
        .await;
    let explicit = cookie_pair(&explicit, "openrust_token").unwrap();
    let response = f
        .cookie_resource("/cookie-resource", &explicit, Some(&b.csrf))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(json_body(response).await["user"], 88);
    let response = f
        .wire(
            reqwest::Method::POST,
            "/logout",
            &format!("{}; {cookie}", b.cookie),
            Some(json!({"_token":b.csrf})),
        )
        .await;
    assert_eq!(response.status(), 204);
    assert!(
        response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.starts_with("openrust_token=")
                && v.to_ascii_lowercase().contains("max-age=0"))
    );
    f.db.table("users")
        .r#where("subject", "=", 77)
        .delete()
        .await
        .unwrap();
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, Some(&b.csrf))
            .await
            .status(),
        401
    );
    f.cleanup().await;
}
fn alternate_key(_: &openrust::encryption::Encrypter) -> Vec<u8> {
    b"alternate Passport JWT signing key".to_vec()
}
#[tokio::test]
async fn mysql_cookie_switches_key_callback_serialization_and_application_isolation() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let b = f.login("alice@test").await;
    let cookie = f.fresh_cookie(&b).await;
    // The API routes receive the encrypted browser value; explicit opt-out expects a raw JWT.
    let encrypted = decode_cookie_pair(&cookie);
    let plain = f.encryption.decryptString(&encrypted).unwrap();
    let jwt =
        String::from_utf8(openrust::cookie::CookieValuePrefix::remove(&plain).to_vec()).unwrap();
    f.passport.withoutCookieEncryption();
    let raw = format!("openrust_token={jwt}");
    assert_eq!(
        f.cookie_resource("/cookie-resource", &raw, Some(&b.csrf))
            .await
            .status(),
        200
    );
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, Some(&b.csrf))
            .await
            .status(),
        401
    );
    f.passport.withCookieEncryption();
    assert_eq!(
        f.cookie_resource("/cookie-resource", &raw, Some(&b.csrf))
            .await
            .status(),
        401
    );
    f.passport.ignoreCsrfToken(true);
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, None)
            .await
            .status(),
        200
    );
    assert_eq!(
        other
            .cookie_resource("/cookie-resource", &cookie, None)
            .await
            .status(),
        401
    );
    f.passport.ignoreCsrfToken(false);
    f.passport.withCookieSerialization();
    let serialized = f.encryption.encrypt(&plain, true).unwrap();
    assert_eq!(
        f.cookie_resource(
            "/cookie-resource",
            &format!("openrust_token={serialized}"),
            Some(&b.csrf)
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, Some(&b.csrf))
            .await
            .status(),
        401
    );
    f.passport.withoutCookieSerialization();
    f.passport.encryptTokensUsing(Some(alternate_key));
    assert_eq!(
        f.cookie_resource("/cookie-resource", &cookie, Some(&b.csrf))
            .await
            .status(),
        401
    );
    let new = f.fresh_cookie(&b).await;
    assert_eq!(
        f.cookie_resource("/cookie-resource", &new, Some(&b.csrf))
            .await
            .status(),
        200
    );
    assert_eq!(
        other
            .cookie_resource("/cookie-resource", &new, Some(&b.csrf))
            .await
            .status(),
        401
    );
    f.passport.encryptTokensUsing(None);
    f.cleanup().await;
    other.cleanup().await;
}

#[tokio::test]
async fn mysql_native_user_token_trait_and_testing_substitutions_stay_in_one_request() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    f.personal_client(Some("users")).await;
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], |_| async move {
                let passport = openrust::container::Container::get_instance().make::<Passport>()?;
                let mut query = User::query().unwrap();
                query.r#where("subject", "=", 77);
                let user = query.first().await.unwrap().unwrap();
                let issued = user.create_token("Native", &["read".into()]).await.unwrap();
                assert_eq!(
                    issued
                        .getToken()
                        .await
                        .unwrap()
                        .unwrap()
                        .lock()
                        .await
                        .getAttribute("name"),
                    Some(&Value::Text("Native".into()))
                );
                let (parts, body) =
                    axum::http::Request::new(openrust::http::Body::empty()).into_parts();
                let mut request = Request::from_parts(parts, body);
                passport
                    .actingAs(&mut request, Box::new(user), &["read".into()], "api", None)
                    .await
                    .unwrap();
                assert_eq!(
                    Auth::user(&mut request)
                        .await
                        .unwrap()
                        .unwrap()
                        .get_auth_identifier()
                        .to_string(),
                    "77"
                );
                let token = openrust_passport::guards::TokenGuard::currentToken(&request).unwrap();
                assert!(token.can("read").unwrap());
                assert!(token.cant("write").unwrap());
                assert!(!token.transient());
                let repo =
                    openrust::container::Container::get_instance().make::<ClientRepository>()?;
                let client = repo
                    .createClientCredentialsGrantClient("Test")
                    .await
                    .unwrap();
                assert!(
                    passport
                        .actingAsClient(&mut request, &client, &["read".into()], "api")
                        .await
                        .is_err()
                );
                let (parts, body) =
                    axum::http::Request::new(openrust::http::Body::empty()).into_parts();
                let mut second = Request::from_parts(parts, body);
                passport
                    .actingAsClient(&mut second, &client, &["write".into()], "api")
                    .await
                    .unwrap();
                let server = openrust_passport::ResourceServer::get_facade_root().unwrap();
                let token = server
                    .validateAuthenticatedRequest(&mut second)
                    .await
                    .unwrap();
                assert!(token.can("write").unwrap());
                assert!(token.cant("read").unwrap());
                assert!(token.oauth_user_id.is_none());
                let (parts, body) =
                    axum::http::Request::new(openrust::http::Body::empty()).into_parts();
                let mut fresh = Request::from_parts(parts, body);
                assert!(
                    server
                        .validateAuthenticatedRequest(&mut fresh)
                        .await
                        .is_err()
                );
                assert!(Auth::user(&mut fresh).await.unwrap().is_none());
                let transient = openrust_passport::TransientToken;
                assert!(transient.can("anything"));
                assert!(!transient.cant("anything"));
                assert!(transient.transient());
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    f.cleanup().await;
}

#[tokio::test]
#[ignore = "Run explicitly with the installed Playwright and Chromium acceptance runtime"]
async fn mysql_default_grants_real_chromium_first_party_cookie_and_personal() {
    assert!(required());
    let f = Fixture::new().await;
    let machine = f.machine_client(false, None).await;
    f.personal_client(Some("users")).await;
    let base = f.base.clone();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/default_grants/browser.mjs");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg(script)
            .env("OPENRUST_BROWSER_BASE", base)
            .env("OPENRUST_BROWSER_MACHINE_ID", machine.id)
            .env("OPENRUST_BROWSER_MACHINE_SECRET", machine.secret)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_public_scope_queries_and_default_scope_string_conversion() {
    assert!(required());
    let f = Fixture::new().await;
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], |_| async move {
                let passport = openrust::container::Container::get_instance().make::<Passport>()?;
                passport.tokensCan(std::collections::BTreeMap::from([
                    ("read".into(), "Read".into()),
                    ("write".into(), "Write".into()),
                    ("0".into(), "Zero".into()),
                ]));
                assert_eq!(passport.scopeIds(), ["0", "read", "write"]);
                assert!(passport.hasScope("*"));
                assert!(!passport.hasScope("unknown"));
                let ids: Vec<String> = ["write", "unknown", "read", "write", "*"]
                    .map(str::to_owned)
                    .to_vec();
                assert_eq!(passport.validScopes(&ids), ["write", "read"]);
                assert_eq!(
                    passport
                        .scopesFor(&ids)
                        .iter()
                        .map(|scope| scope.id.as_str())
                        .collect::<Vec<_>>(),
                    ["write", "read", "write"]
                );
                assert_eq!(
                    passport.defaultScopes(Some(vec![" read  write ".into()])),
                    ["", "read", "", "write", ""]
                );
                assert_eq!(passport.defaultScopes(None), ["", "read", "", "write", ""]);
                assert!(passport.defaultScopes(Some(vec!["0".into()])).is_empty());
                passport.setDefaultScope(vec!["read  write".into()]);
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let machine = f.machine_client(false, None).await;
    let response = f.machine_token(&machine, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let value: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        jwt_claims(value["access_token"].as_str().unwrap())["scopes"],
        json!(["read", "write"])
    );
    f.cleanup().await;
}
