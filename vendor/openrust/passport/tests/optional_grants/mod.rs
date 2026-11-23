#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use super::*;
use openrust_passport::{Client, GrantLifetime};
struct OptionalClient {
    id: String,
    secret: Option<String>,
}
impl Fixture {
    async fn optional_client(
        &self,
        implicit: bool,
        provider: Option<&str>,
        public: bool,
    ) -> OptionalClient {
        let provider = provider.map(str::to_owned);
        self.kernel
            .get_application()
            .call(
                openrust::container::Callback::new(&[], move |_| async move {
                    let repo = openrust::container::Container::get_instance()
                        .make::<ClientRepository>()?;
                    let record = if implicit {
                        repo.createImplicitGrantClient("Implicit", &[CALLBACK.into()])
                            .await
                            .unwrap()
                    } else {
                        repo.createPasswordGrantClient("Password", provider.as_deref(), !public)
                            .await
                            .unwrap()
                    };
                    let id = AuthIdentifier::try_from(&record.getKey().await.unwrap())
                        .unwrap()
                        .to_string();
                    let model = record.model::<Client>().unwrap();
                    let secret = model
                        .lock()
                        .await
                        .plainSecret
                        .as_ref()
                        .map(|value| value.to_string());
                    Ok(OptionalClient { id, secret })
                }),
                Default::default(),
            )
            .await
            .unwrap()
    }
    async fn password_request(
        &self,
        client: &OptionalClient,
        username: &str,
        password: &str,
        scope: Option<&str>,
    ) -> reqwest::Response {
        let mut body = json!({"grant_type":"password","client_id":client.id,"username":username,"password":password});
        if let Some(secret) = &client.secret {
            body["client_secret"] = json!(secret);
        }
        if let Some(scope) = scope {
            body["scope"] = json!(scope);
        }
        self.wire(reqwest::Method::POST, "/oauth/token", "", Some(body))
            .await
    }
    async fn implicit_request(
        &self,
        browser: Option<&Browser>,
        client: &OptionalClient,
        scope: &str,
        prompt: &str,
    ) -> reqwest::Response {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("response_type", "token"),
                ("client_id", &client.id),
                ("redirect_uri", CALLBACK),
                ("scope", scope),
                ("state", "opaque & state"),
                ("prompt", prompt),
            ])
            .finish();
        self.wire(
            reqwest::Method::GET,
            &format!("/oauth/authorize?{query}"),
            browser.map(|b| b.cookie.as_str()).unwrap_or(""),
            None,
        )
        .await
    }
    async fn implicit_decide(
        &self,
        browser: &Browser,
        view: &J,
        approved: bool,
    ) -> reqwest::Response {
        self.wire(if approved { reqwest::Method::POST } else { reqwest::Method::DELETE }, "/oauth/authorize", &browser.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":browser.csrf,"response_type":"code","scope":"*","client_id":"tampered"}))).await
    }
}
fn fragment(response: &reqwest::Response) -> BTreeMap<String, String> {
    assert_eq!(response.status(), 302);
    let location = url::Url::parse(response.headers()["location"].to_str().unwrap()).unwrap();
    assert_eq!(location.query(), Some("existing=1"));
    url::form_urlencoded::parse(location.fragment().unwrap().as_bytes())
        .into_owned()
        .collect()
}
#[tokio::test]
async fn mysql_optional_grants_are_default_off_and_application_local() {
    assert!(required());
    let a = Fixture::new().await;
    let b = Fixture::new().await;
    let ap = a.optional_client(false, None, false).await;
    let ai = a.optional_client(true, None, true).await;
    let bp = b.optional_client(false, None, false).await;
    let bi = b.optional_client(true, None, true).await;
    assert_eq!(
        json_body(
            a.password_request(&ap, "alice@test", "password", None)
                .await
        )
        .await["error"],
        "unsupported_grant_type"
    );
    assert_eq!(
        json_body(a.implicit_request(None, &ai, "read", "consent").await).await["error"],
        "unsupported_grant_type"
    );
    a.passport.enablePasswordGrant();
    a.passport.enableImplicitGrant();
    assert_eq!(
        a.password_request(&ap, "alice@test", "password", None)
            .await
            .status(),
        200
    );
    assert_eq!(
        json_body(
            b.password_request(&bp, "alice@test", "password", None)
                .await
        )
        .await["error"],
        "unsupported_grant_type"
    );
    assert_eq!(
        json_body(b.implicit_request(None, &bi, "read", "consent").await).await["error"],
        "unsupported_grant_type"
    );
    a.passport
        .passwordGrantEnabled
        .store(false, Ordering::Relaxed);
    a.passport
        .implicitGrantEnabled
        .store(false, Ordering::Relaxed);
    assert_eq!(
        json_body(
            a.password_request(&ap, "alice@test", "password", None)
                .await
        )
        .await["error"],
        "unsupported_grant_type"
    );
    assert_eq!(
        json_body(a.implicit_request(None, &ai, "read", "consent").await).await["error"],
        "unsupported_grant_type"
    );
    a.cleanup().await;
    b.cleanup().await;
}
#[tokio::test]
async fn mysql_password_tokens_refresh_scopes_provider_and_real_user() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enablePasswordGrant();
    f.passport.tokensExpireIn(Some(GrantLifetime::Seconds(90)));
    f.passport
        .refreshTokensExpireIn(Some(GrantLifetime::Seconds(3600)));
    let client = f.optional_client(false, None, false).await;
    let response = f
        .password_request(&client, "alice@test", "password", Some("* read read"))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["cache-control"], "no-store");
    let token = json_body(response).await;
    assert_eq!(token["expires_in"], 90);
    assert!(token["refresh_token"].is_string());
    let claims = jwt_claims(token["access_token"].as_str().unwrap());
    assert_eq!(claims["sub"], "77");
    assert_eq!(claims["scopes"], json!(["*", "read"]));
    assert_eq!(
        f.resource_token("/resource", token["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    assert_eq!(
        f.resource_token("/resource-other", token["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    let rotated=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"refresh_token","client_id":client.id,"client_secret":client.secret,"refresh_token":token["refresh_token"]}))).await;
    assert_eq!(rotated.status(), 200);
    assert_eq!(
        jwt_claims(json_body(rotated).await["access_token"].as_str().unwrap())["scopes"],
        json!(["read"])
    );
    let other = f.optional_client(false, Some("other_users"), false).await;
    let token = json_body(
        f.password_request(&other, "bob@test", "password", Some("read"))
            .await,
    )
    .await;
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["sub"],
        "88"
    );
    assert_eq!(
        f.resource_token("/resource", token["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    assert_eq!(
        f.resource_token("/resource-other", token["access_token"].as_str().unwrap())
            .await
            .status(),
        200
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_password_rejects_invalid_credentials_parameters_and_clients() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enablePasswordGrant();
    let client = f.optional_client(false, Some("users"), false).await;
    for (name, password) in [("alice@test", "wrong"), ("missing@test", "password")] {
        let response = f
            .password_request(&client, name, password, Some("read"))
            .await;
        assert_eq!(response.status(), 400);
        let body = json_body(response).await;
        assert_eq!(body["error"], "invalid_grant");
        assert_eq!(
            body["error_description"],
            "The user credentials were incorrect."
        );
        assert!(body.get("hint").is_none());
    }
    for (username, password, field) in
        [("", "password", "username"), ("alice@test", "", "password")]
    {
        let value = json_body(f.password_request(&client, username, password, None).await).await;
        assert_eq!(value["error"], "invalid_request");
        assert_eq!(value["hint"], format!("Check the `{field}` parameter"));
    }
    assert_eq!(
        json_body(
            f.password_request(&client, "alice@test", "password", Some("unknown"))
                .await
        )
        .await["error"],
        "invalid_scope"
    );
    let bad = OptionalClient {
        id: client.id.clone(),
        secret: Some("incorrect".into()),
    };
    assert_eq!(
        f.password_request(&bad, "alice@test", "password", None)
            .await
            .status(),
        401
    );
    let code = OptionalClient {
        id: f.confidential.clone(),
        secret: Some(f.secret.clone()),
    };
    assert_eq!(
        json_body(
            f.password_request(&code, "alice@test", "password", None)
                .await
        )
        .await["error"],
        "unauthorized_client"
    );
    let absent = f.optional_client(false, Some("absent"), false).await;
    assert_eq!(
        f.password_request(&absent, "alice@test", "password", None)
            .await
            .status(),
        500
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.cleanup().await;
}
fn resolver<'a>(
    username: &'a str,
    password: &'a str,
    client: &'a openrust_passport::ClientRecord,
) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<Option<AuthIdentifier>>> {
    Box::pin(async move {
        if client.getAttributeValue("name").await.unwrap() != Some(Value::Text("Password".into())) {
            return Ok(None);
        }
        let mut query = User::query().unwrap();
        query.r#where("subject", "=", username.parse::<i64>().unwrap_or(0));
        let Some(user) = query.first().await.unwrap() else {
            return Ok(None);
        };
        if Hash::check(
            password,
            user.getAuthPassword().unwrap(),
            &Default::default(),
        )
        .await
        .unwrap()
        {
            Ok(Some(user.getAuthIdentifier()))
        } else {
            Ok(None)
        }
    })
}
#[tokio::test]
async fn mysql_password_native_model_hook_public_clients_basic_and_veto_rollback() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enablePasswordGrant();
    f.passport
        .findAndValidateForPassportUsing("users", resolver);
    let public = f.optional_client(false, Some("users"), true).await;
    assert_eq!(
        f.password_request(&public, "77", "password", Some("read"))
            .await
            .status(),
        200
    );
    assert_eq!(
        json_body(
            f.password_request(&public, "alice@test", "password", None)
                .await
        )
        .await["error"],
        "invalid_grant"
    );
    let client = f.optional_client(false, Some("users"), false).await;
    let response = f
        .client
        .post(format!("{}/oauth/token", f.base))
        .basic_auth(&client.id, client.secret.as_ref())
        .form(&[
            ("grant_type", "password"),
            ("username", "88"),
            ("password", "password"),
            ("scope", "read"),
        ])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        jwt_claims(json_body(response).await["access_token"].as_str().unwrap())["sub"],
        "88"
    );
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    let before = f.db.table("oauth_access_tokens").count("*").await.unwrap();
    let refresh_before = f.db.table("oauth_refresh_tokens").count("*").await.unwrap();
    assert_eq!(
        f.password_request(&client, "77", "password", None)
            .await
            .status(),
        500
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        before
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        refresh_before
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_implicit_consent_fragment_scope_subject_resource_and_single_consumption() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enableImplicitGrant();
    f.passport.tokensExpireIn(Some(GrantLifetime::Seconds(180)));
    let client = f.optional_client(true, None, true).await;
    let browser = f.login("alice@test").await;
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "* read read", "consent")
            .await,
    )
    .await;
    let response = f.implicit_decide(&browser, &view, true).await;
    let parts = fragment(&response);
    assert_eq!(parts["state"], "opaque & state");
    assert_eq!(parts["token_type"], "Bearer");
    assert_eq!(parts["expires_in"], "180");
    assert!(!parts.contains_key("code"));
    assert!(!parts.contains_key("refresh_token"));
    let claims = jwt_claims(&parts["access_token"]);
    assert_eq!(claims["sub"], "77");
    assert_eq!(claims["aud"], client.id);
    assert_eq!(claims["scopes"], json!(["read"]));
    assert_eq!(
        f.resource_token("/resource", &parts["access_token"])
            .await
            .status(),
        200
    );
    assert_eq!(
        f.resource_token("/machine", &parts["access_token"])
            .await
            .status(),
        401
    );
    assert_eq!(f.db.table("oauth_auth_codes").count("*").await.unwrap(), 0);
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(f.implicit_decide(&browser, &view, true).await.status(), 400);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    assert_eq!(
        f.implicit_request(Some(&browser), &client, "read", "none")
            .await
            .status(),
        302
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_implicit_errors_use_validated_fragment_and_never_untrusted_redirect() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enableImplicitGrant();
    let client = f.optional_client(true, None, true).await;
    let response = f.implicit_request(None, &client, "read", "none").await;
    assert_eq!(fragment(&response)["error"], "login_required");
    let response = f
        .implicit_request(None, &client, "unknown", "consent")
        .await;
    assert_eq!(fragment(&response)["error"], "invalid_scope");
    let browser = f.login("alice@test").await;
    assert_eq!(
        fragment(
            &f.implicit_request(Some(&browser), &client, "read", "none")
                .await
        )["error"],
        "consent_required"
    );
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    let denied = f.implicit_decide(&browser, &view, false).await;
    assert_eq!(fragment(&denied)["error"], "access_denied");
    assert_eq!(fragment(&denied)["state"], "opaque & state");
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("response_type", "token"),
            ("client_id", client.id.as_str()),
            ("redirect_uri", "https://untrusted.test/cb"),
        ])
        .finish();
    let bad = f
        .wire(
            reqwest::Method::GET,
            &format!("/oauth/authorize?{query}"),
            &browser.cookie,
            None,
        )
        .await;
    assert_eq!(bad.status(), 401);
    assert!(!bad.headers().contains_key("location"));
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/token",
            "",
            Some(json!({"grant_type":"implicit","client_id":client.id})),
        )
        .await;
    assert_eq!(json_body(response).await["error"], "unsupported_grant_type");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_implicit_csrf_nonce_disable_mutated_client_and_rollback() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enableImplicitGrant();
    let client = f.optional_client(true, None, true).await;
    let browser = f.login("alice@test").await;
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    let no_csrf = f
        .wire(
            reqwest::Method::POST,
            "/oauth/authorize",
            &browser.cookie,
            Some(json!({"auth_token":view["auth_token"]})),
        )
        .await;
    assert_eq!(no_csrf.status(), 419);
    f.passport
        .implicitGrantEnabled
        .store(false, Ordering::Relaxed);
    let disabled = f.implicit_decide(&browser, &view, true).await;
    assert_eq!(json_body(disabled).await["error"], "unsupported_grant_type");
    f.passport.enableImplicitGrant();
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    assert_eq!(f.implicit_decide(&browser, &view, true).await.status(), 500);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.veto.store(false, Ordering::Relaxed);
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    f.db.table("oauth_clients")
        .r#where("id", "=", client.id)
        .update(Attributes::from([("revoked".into(), Value::Bool(true))]))
        .await
        .unwrap();
    assert_eq!(f.implicit_decide(&browser, &view, true).await.status(), 401);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_optional_scopes_defaults_client_restrictions_and_storage_failure() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enablePasswordGrant();
    f.passport.enableImplicitGrant();
    f.passport.setDefaultScope(vec!["read write".into()]);
    f.db.getSchemaBuilder()
        .table("oauth_clients", |t| {
            t.json("scopes").nullable();
        })
        .await
        .unwrap();
    let password = f.optional_client(false, Some("users"), false).await;
    let implicit = f.optional_client(true, None, true).await;
    for id in [&password.id, &implicit.id] {
        f.db.table("oauth_clients")
            .r#where("id", "=", id.as_str())
            .update(Attributes::from([(
                "scopes".into(),
                Value::Json(json!(["write"])),
            )]))
            .await
            .unwrap();
    }
    let response = f
        .password_request(&password, "alice@test", "password", None)
        .await;
    assert_eq!(response.status(), 200);
    let token = json_body(response).await;
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["scopes"],
        json!(["write"])
    );
    let browser = f.login("alice@test").await;
    let view = json_body(
        f.implicit_request(Some(&browser), &implicit, "", "consent")
            .await,
    )
    .await;
    let response = f.implicit_decide(&browser, &view, true).await;
    assert_eq!(
        jwt_claims(&fragment(&response)["access_token"])["scopes"],
        json!(["write"])
    );
    f.db.getSchemaBuilder().dropIfExists("users").await.unwrap();
    let before = f.db.table("oauth_access_tokens").count("*").await.unwrap();
    let response = f
        .password_request(&password, "alice@test", "password", None)
        .await;
    assert_eq!(response.status(), 500);
    assert_eq!(json_body(response).await["error"], "server_error");
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        before
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_implicit_competing_consent_and_changed_redirect_cannot_issue_twice() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enableImplicitGrant();
    let client = f.optional_client(true, None, true).await;
    let browser = f.login("alice@test").await;
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    let (left, right) = tokio::join!(
        f.implicit_decide(&browser, &view, true),
        f.implicit_decide(&browser, &view, true)
    );
    let statuses = [left.status().as_u16(), right.status().as_u16()];
    assert_eq!(statuses.iter().filter(|s| **s == 302).count(), 1);
    assert_eq!(statuses.iter().filter(|s| **s == 400).count(), 1);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    let view = json_body(
        f.implicit_request(Some(&browser), &client, "read", "consent")
            .await,
    )
    .await;
    f.db.table("oauth_clients")
        .r#where("id", "=", client.id)
        .update(Attributes::from([(
            "redirect_uris".into(),
            Value::Json(json!(["https://changed.test/callback"])),
        )]))
        .await
        .unwrap();
    let response = f.implicit_decide(&browser, &view, true).await;
    assert_eq!(response.status(), 401);
    assert!(!response.headers().contains_key("location"));
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    f.cleanup().await;
}

fn browser_view(
    request: &mut Request,
    params: AuthorizationViewParameters,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let csrf = request.session()?.token().unwrap();
        Ok(Response::from(format!(
            "<!doctype html><html><body><h1>Authorize optional client</h1><form method='post' action='/oauth/authorize'><input type='hidden' name='_token' value='{csrf}'><input type='hidden' name='auth_token' value='{}'><button>Approve</button></form></body></html>",
            params.auth_token
        )))
    })
}
#[tokio::test]
#[ignore = "Run explicitly with the installed Playwright and Chromium acceptance runtime"]
async fn mysql_optional_real_chromium_implicit_consent_and_password_client() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enableImplicitGrant();
    f.passport.enablePasswordGrant();
    f.passport.authorizationView(browser_view);
    let client = f.optional_client(true, None, true).await;
    let callback = format!("{}/session?existing=1", f.base);
    f.db.table("oauth_clients")
        .r#where("id", "=", client.id.as_str())
        .update(Attributes::from([(
            "redirect_uris".into(),
            Value::Json(json!([callback])),
        )]))
        .await
        .unwrap();
    let password = f.optional_client(false, Some("users"), false).await;
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/optional_grants/browser.mjs");
    let base = f.base.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg(script)
            .env("OPENRUST_BROWSER_BASE", base)
            .env("OPENRUST_BROWSER_IMPLICIT_ID", client.id)
            .env("OPENRUST_BROWSER_PASSWORD_ID", password.id)
            .env("OPENRUST_BROWSER_PASSWORD_SECRET", password.secret.unwrap())
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

struct NativeProvider {
    inner: Arc<dyn openrust::contracts::auth::UserProvider>,
    lookups: Arc<AtomicUsize>,
    validations: Arc<AtomicUsize>,
}
impl openrust::contracts::auth::UserProvider for NativeProvider {
    fn retrieveById<'a>(
        &'a self,
        id: &'a AuthIdentifier,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<Option<Box<dyn Authenticatable>>>>
    {
        self.inner.retrieveById(id)
    }
    fn retrieveByToken<'a>(
        &'a self,
        id: &'a AuthIdentifier,
        token: &'a str,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<Option<Box<dyn Authenticatable>>>>
    {
        self.inner.retrieveByToken(id, token)
    }
    fn updateRememberToken<'a>(
        &'a self,
        user: &'a mut dyn Authenticatable,
        token: &'a str,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        self.inner.updateRememberToken(user, token)
    }
    fn retrieveByCredentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<Option<Box<dyn Authenticatable>>>>
    {
        self.lookups.fetch_add(1, Ordering::Relaxed);
        self.inner.retrieveByCredentials(credentials)
    }
    fn validateCredentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<bool>> {
        self.validations.fetch_add(1, Ordering::Relaxed);
        self.inner.validateCredentials(user, credentials)
    }
    fn rehashPasswordIfRequired<'a>(
        &'a self,
        _user: &'a mut dyn Authenticatable,
        _credentials: &'a Credentials,
        _force: bool,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        panic!("Password grant must not rehash or start a session")
    }
}
#[tokio::test]
async fn mysql_password_compiled_user_provider_uses_real_lookup_validation_and_identity() {
    assert!(required());
    let f = Fixture::new().await;
    f.passport.enablePasswordGrant();
    let lookups = Arc::new(AtomicUsize::new(0));
    let validations = Arc::new(AtomicUsize::new(0));
    let a = lookups.clone();
    let b = validations.clone();
    f.kernel
        .get_application()
        .call(
            openrust::container::Callback::new(&[], move |_| async move {
                let container = openrust::container::Container::get_instance();
                let manager = container.make::<AuthManager>()?;
                manager.provider("native-password", move |container, _| {
                    Ok(Arc::new(NativeProvider {
                        inner: container
                            .make::<AuthManager>()
                            .unwrap()
                            .createUserProvider("users")?,
                        lookups: a.clone(),
                        validations: b.clone(),
                    }))
                });
                container
                    .get::<dyn openrust::contracts::config::Repository>("config")?
                    .set("auth.providers.native", json!({"driver":"native-password"}))
                    .unwrap();
                container
                    .get::<dyn openrust::contracts::config::Repository>("config")?
                    .set(
                        "auth.guards.native",
                        json!({"driver":"passport","provider":"native"}),
                    )
                    .unwrap();
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
    let client = f.optional_client(false, Some("native"), false).await;
    assert_eq!(
        f.password_request(&client, "alice@test", "wrong", None)
            .await
            .status(),
        400
    );
    let response = f
        .password_request(&client, "alice@test", "password", Some("read"))
        .await;
    assert_eq!(response.status(), 200);
    assert_eq!(lookups.load(Ordering::Relaxed), 2);
    assert_eq!(validations.load(Ordering::Relaxed), 2);
    let token = json_body(response).await;
    assert_eq!(
        f.resource_token("/resource", token["access_token"].as_str().unwrap())
            .await
            .status(),
        401
    );
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["sub"],
        "77"
    );
    f.cleanup().await;
}
