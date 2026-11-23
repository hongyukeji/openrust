use super::*;
const GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
pub(super) fn code_view(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        Response::json(
            StatusCode::OK,
            json!({"csrf":request.session()?.token(),"status":request.session()?.get("status"),"old":request.session()?.getOldInput(None),"errors":request.session()?.get("errors")}),
        )
    })
}
pub(super) struct DeviceClient {
    pub(super) id: String,
    pub(super) secret: Option<String>,
}
impl Fixture {
    pub(super) async fn device_client(&self, confidential: bool) -> DeviceClient {
        self.kernel
            .get_application()
            .call(
                openrust::container::Callback::new(&[], move |_| async move {
                    let repo = openrust::container::Container::get_instance()
                        .make::<ClientRepository>()
                        .unwrap();
                    let client = repo
                        .createDeviceAuthorizationGrantClient("Device", confidential, None)
                        .await
                        .unwrap();
                    let id = match client.getKey().await.unwrap() {
                        Value::Text(v) => v,
                        _ => panic!("UUID expected"),
                    };
                    let secret = client
                        .model::<openrust_passport::Client>()
                        .unwrap()
                        .lock()
                        .await
                        .plainSecret
                        .as_ref()
                        .map(|s| s.to_string());
                    Ok(DeviceClient { id, secret })
                }),
                Default::default(),
            )
            .await
            .unwrap()
    }
    pub(super) async fn device_code(&self, client: &DeviceClient, scope: &str) -> J {
        let response = self
            .wire(
                reqwest::Method::POST,
                "/oauth/device/code",
                "",
                Some(json!({"client_id":client.id,"scope":scope})),
            )
            .await;
        let status = response.status();
        assert_eq!(response.headers()["cache-control"], "no-store");
        let data = json_body(response).await;
        assert_eq!(status, 200, "{data}");
        data
    }
    pub(super) async fn device_poll(&self, client: &DeviceClient, code: &str) -> reqwest::Response {
        let mut data = json!({"grant_type":GRANT,"client_id":client.id,"device_code":code});
        if let Some(secret) = &client.secret {
            data["client_secret"] = json!(secret);
        }
        self.wire(reqwest::Method::POST, "/oauth/token", "", Some(data))
            .await
    }
    pub(super) async fn device_view(&self, b: &Browser, code: &str) -> J {
        let response = self
            .wire(
                reqwest::Method::GET,
                &format!("/oauth/device/authorize?user_code={code}"),
                &b.cookie,
                None,
            )
            .await;
        let status = response.status();
        let data = json_body(response).await;
        assert_eq!(status, 200, "{data}");
        data
    }
    pub(super) async fn device_decide(
        &self,
        b: &Browser,
        view: &J,
        approve: bool,
    ) -> reqwest::Response {
        self.wire(
            if approve {
                reqwest::Method::POST
            } else {
                reqwest::Method::DELETE
            },
            "/oauth/device/authorize",
            &b.cookie,
            Some(json!({"auth_token":view["auth_token"],"_token":b.csrf})),
        )
        .await
    }
}
#[tokio::test]
async fn mysql_device_creation_pending_polling_and_client_validation() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(true).await;
    let data = f.device_code(&client, "read write").await;
    assert_eq!(data["expires_in"], 600);
    assert_eq!(data["interval"], 5);
    assert_eq!(data["verification_uri"], format!("{}/oauth/device", f.base));
    assert_eq!(
        data["verification_uri_complete"],
        format!(
            "{}/oauth/device?user_code={}",
            f.base,
            data["user_code"].as_str().unwrap()
        )
    );
    assert_eq!(data["device_code"].as_str().unwrap().len(), 80);
    let user_code = data["user_code"].as_str().unwrap();
    assert_eq!(user_code.len(), 8);
    assert!(
        user_code
            .bytes()
            .all(|b| b"BCDFGHJKLMNPQRSTVWXZ".contains(&b))
    );
    let form = f
        .client
        .post(format!("{}/oauth/device/code", f.base))
        .basic_auth(&client.id, Some("not-checked-at-device-issuance"))
        .form(&[("scope", "read")])
        .send()
        .await
        .unwrap();
    assert_eq!(form.status(), 200);
    let code = data["device_code"].as_str().unwrap();
    let pending = f.device_poll(&client, code).await;
    assert_eq!(pending.status(), 400);
    assert_eq!(json_body(pending).await["error"], "authorization_pending");
    let slow = f.device_poll(&client, code).await;
    assert_eq!(slow.status(), 400);
    assert_eq!(json_body(slow).await["error"], "slow_down");
    let row =
        f.db.table("oauth_device_codes")
            .r#where("id", "=", code)
            .first()
            .await
            .unwrap()
            .unwrap();
    assert_ne!(row.get("last_polled_at"), Some(&Value::Null));
    f.db.table("oauth_device_codes")
        .r#where("id", "=", code)
        .update(Attributes::from([(
            "last_polled_at".into(),
            "2000-01-01 00:00:00".into(),
        )]))
        .await
        .unwrap();
    assert_eq!(
        json_body(f.device_poll(&client, code).await).await["error"],
        "authorization_pending"
    );
    for (body, expected) in [
        (json!({}), "invalid_request"),
        (json!({"client_id":f.public}), "unauthorized_client"),
        (json!({"client_id":"missing"}), "invalid_client"),
        (
            json!({"client_id":client.id,"scope":"unknown"}),
            "invalid_scope",
        ),
    ] {
        let response = f
            .wire(reqwest::Method::POST, "/oauth/device/code", "", Some(body))
            .await;
        assert_eq!(response.headers()["pragma"], "no-cache");
        assert_eq!(json_body(response).await["error"], expected);
    }
    let bad = DeviceClient {
        id: client.id.clone(),
        secret: Some("wrong".into()),
    };
    assert_eq!(f.device_poll(&bad, code).await.status(), 401);
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_public_and_confidential_complete_real_browser_resource_and_refresh() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    for confidential in [false, true] {
        let client = f.device_client(confidential).await;
        let data = f.device_code(&client, "read").await;
        let user_code = data["user_code"].as_str().unwrap();
        let entry = f
            .wire(reqwest::Method::GET, "/oauth/device", "", None)
            .await;
        assert_eq!(entry.status(), 200);
        let entry = json_body(entry).await;
        assert!(entry["csrf"].is_string());
        let b = f.login("alice@test").await;
        let redirect = f
            .wire(
                reqwest::Method::GET,
                &format!("/oauth/device?user_code={user_code}"),
                &b.cookie,
                None,
            )
            .await;
        assert_eq!(redirect.status(), 302);
        assert_eq!(
            redirect.headers()["location"],
            format!("{}/oauth/device/authorize?user_code={user_code}", f.base)
        );
        let formatted = format!("{}-{}", &user_code[..4], &user_code[4..]);
        let view = f.device_view(&b, &formatted).await;
        assert_eq!(view["user"], 77);
        assert_eq!(view["scopes"][0]["id"], "read");
        let no_csrf = f
            .wire(
                reqwest::Method::POST,
                "/oauth/device/authorize",
                &b.cookie,
                Some(json!({"auth_token":view["auth_token"]})),
            )
            .await;
        assert_eq!(no_csrf.status(), 419);
        let approved = f.device_decide(&b, &view, true).await;
        assert_eq!(approved.status(), 302);
        let status = json_body(
            f.wire(reqwest::Method::GET, "/oauth/device", &b.cookie, None)
                .await,
        )
        .await;
        assert_eq!(status["status"], "authorization-approved");
        let code = data["device_code"].as_str().unwrap();
        let response = f.device_poll(&client, code).await;
        let status = response.status();
        let token = json_body(response).await;
        assert_eq!(status, 200, "{token}");
        assert_eq!(
            jwt_claims(token["access_token"].as_str().unwrap())["scopes"],
            json!(["read"])
        );
        assert_eq!(
            f.resource_token("/resource", token["access_token"].as_str().unwrap())
                .await
                .status(),
            200
        );
        assert_eq!(f.device_poll(&client, code).await.status(), 400);
        let mut refresh = json!({"grant_type":"refresh_token","client_id":client.id,"refresh_token":token["refresh_token"]});
        if let Some(secret) = client.secret {
            refresh["client_secret"] = json!(secret);
        }
        assert_eq!(
            f.wire(reqwest::Method::POST, "/oauth/token", "", Some(refresh))
                .await
                .status(),
            200
        );
        assert_eq!(f.device_decide(&b, &view, true).await.status(), 400);
    }
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_denial_expiry_missing_code_and_client_binding() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    let other = f.device_client(false).await;
    let data = f.device_code(&client, "read").await;
    let b = f.login("alice@test").await;
    let missing = f
        .wire(
            reqwest::Method::GET,
            "/oauth/device/authorize",
            &b.cookie,
            None,
        )
        .await;
    assert_eq!(missing.status(), 302);
    let wrong = f
        .wire(
            reqwest::Method::GET,
            "/oauth/device/authorize?user_code=INVALID",
            &b.cookie,
            None,
        )
        .await;
    assert_eq!(wrong.status(), 302);
    let error = json_body(
        f.wire(reqwest::Method::GET, "/oauth/device", &b.cookie, None)
            .await,
    )
    .await;
    assert_eq!(error["old"]["user_code"], "INVALID");
    assert_eq!(
        error["errors"]["default"]["user_code"],
        json!(["Incorrect code."])
    );
    let code = data["device_code"].as_str().unwrap();
    let wrong = f.device_poll(&other, code).await;
    assert_eq!(wrong.status(), 400);
    assert_eq!(
        json_body(wrong).await["hint"],
        "Device code was not issued to this client"
    );
    let view = f.device_view(&b, data["user_code"].as_str().unwrap()).await;
    assert_eq!(f.device_decide(&b, &view, false).await.status(), 302);
    let denied = f.device_poll(&client, code).await;
    assert_eq!(denied.status(), 401);
    assert_eq!(json_body(denied).await["error"], "access_denied");
    let data = f.device_code(&client, "read").await;
    let code = data["device_code"].as_str().unwrap();
    f.db.table("oauth_device_codes")
        .r#where("id", "=", code)
        .update(Attributes::from([(
            "expires_at".into(),
            "2000-01-01 00:00:00".into(),
        )]))
        .await
        .unwrap();
    let expired = f.device_poll(&client, code).await;
    assert_eq!(expired.status(), 400);
    assert_eq!(json_body(expired).await["error"], "expired_token");
    assert_eq!(f.device_poll(&client, "missing").await.status(), 400);
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_atomic_competition_and_token_save_veto_preserve_code() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    let data = f.device_code(&client, "read").await;
    let b = f.login("alice@test").await;
    let view = f.device_view(&b, data["user_code"].as_str().unwrap()).await;
    assert_eq!(f.device_decide(&b, &view, true).await.status(), 302);
    f.passport.useTokenModel::<SelectedToken>();
    f.veto.store(true, Ordering::Relaxed);
    let code = data["device_code"].as_str().unwrap();
    assert_eq!(f.device_poll(&client, code).await.status(), 500);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        f.db.table("oauth_device_codes")
            .r#where("id", "=", code)
            .r#where("revoked", "=", false)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.veto.store(false, Ordering::Relaxed);
    let (a, b) = tokio::join!(f.device_poll(&client, code), f.device_poll(&client, code));
    let statuses = [a.status().as_u16(), b.status().as_u16()];
    assert!(
        statuses == [200, 400] || statuses == [400, 200],
        "{statuses:?}"
    );
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        1
    );
    assert_eq!(
        f.db.table("oauth_refresh_tokens").count("*").await.unwrap(),
        1
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_session_nonce_cross_session_and_runtime_revocation() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    let data = f.device_code(&client, "read").await;
    let a = f.login("alice@test").await;
    let b = f.login("bob@test").await;
    let view = f.device_view(&a, data["user_code"].as_str().unwrap()).await;
    assert_eq!(f.device_decide(&b, &view, true).await.status(), 400);
    let wrong = f
        .wire(
            reqwest::Method::POST,
            "/oauth/device/authorize",
            &a.cookie,
            Some(json!({"auth_token":"wrong","_token":a.csrf})),
        )
        .await;
    assert_eq!(wrong.status(), 400);
    assert_eq!(f.device_decide(&a, &view, true).await.status(), 400);
    let view = f.device_view(&a, data["user_code"].as_str().unwrap()).await;
    f.db.table("oauth_device_codes")
        .r#where("id", "=", data["device_code"].as_str().unwrap())
        .update(Attributes::from([("revoked".into(), true.into())]))
        .await
        .unwrap();
    assert_eq!(f.device_decide(&a, &view, true).await.status(), 400);
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_default_routes_can_be_disabled_and_use_named_prefix() {
    if !required() {
        return;
    }
    let disabled = Fixture::with_device(false, None).await;
    for path in ["/oauth/device", "/oauth/device/authorize"] {
        assert_eq!(
            disabled
                .wire(reqwest::Method::GET, path, "", None)
                .await
                .status(),
            404
        );
    }
    assert_eq!(
        disabled
            .wire(
                reqwest::Method::POST,
                "/oauth/device/code",
                "",
                Some(json!({}))
            )
            .await
            .status(),
        404
    );
    let client = disabled.device_client(false).await;
    let response = disabled.device_poll(&client, "missing").await;
    assert_eq!(json_body(response).await["error"], "unsupported_grant_type");
    let f = Fixture::with_device(true, Some("nested/oauth")).await;
    let client = f.device_client(false).await;
    let response = f
        .wire(
            reqwest::Method::POST,
            "/nested/oauth/device/code",
            "",
            Some(json!({"client_id":client.id})),
        )
        .await;
    assert_eq!(response.status(), 200);
    let data = json_body(response).await;
    assert_eq!(
        data["verification_uri"],
        format!("{}/nested/oauth/device", f.base)
    );
    assert_eq!(
        f.wire(reqwest::Method::GET, "/oauth/device", "", None)
            .await
            .status(),
        404
    );
    disabled.cleanup().await;
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_star_scopes_are_removed_by_fixed_scope_repository() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    let data = f.device_code(&client, "*").await;
    let b = f.login("alice@test").await;
    let view = f.device_view(&b, data["user_code"].as_str().unwrap()).await;
    assert_eq!(view["scopes"], json!([]));
    assert_eq!(f.device_decide(&b, &view, true).await.status(), 302);
    let token = json_body(
        f.device_poll(&client, data["device_code"].as_str().unwrap())
            .await,
    )
    .await;
    assert_eq!(
        jwt_claims(token["access_token"].as_str().unwrap())["scopes"],
        json!([])
    );
    let resource = json_body(
        f.resource_token("/resource", token["access_token"].as_str().unwrap())
            .await,
    )
    .await;
    assert_eq!(resource["write"], false);
    let refresh=f.wire(reqwest::Method::POST,"/oauth/token","",Some(json!({"grant_type":"refresh_token","client_id":client.id,"refresh_token":token["refresh_token"]}))).await;
    assert_eq!(refresh.status(), 200);
    let refresh = json_body(refresh).await;
    assert_eq!(
        jwt_claims(refresh["access_token"].as_str().unwrap())["scopes"],
        json!([])
    );
    f.cleanup().await;
}

pub(super) struct Options {
    pub enabled: bool,
    pub register: bool,
    pub prefix: Option<String>,
}
impl openrust::support::ServiceProvider for Options {
    fn boot<'a>(
        &'a mut self,
        app: &'a Application,
    ) -> HttpFuture<'a, openrust::support::ProviderResult> {
        Box::pin(async move {
            if !self.register {
                app.make::<Passport>().await?.ignoreRoutes();
            }
            app.make::<Passport>()
                .await?
                .deviceCodeGrantEnabled
                .store(self.enabled, Ordering::Relaxed);
            if let Some(prefix) = &self.prefix {
                app.container()
                    .get::<dyn openrust::contracts::config::Repository>("config")?
                    .set("passport.path", json!(prefix))?;
            }
            Ok(())
        })
    }
}
struct ForceCode {
    code: String,
    attempts: Arc<AtomicUsize>,
    always: bool,
    cancel: bool,
}
impl openrust::database::eloquent::Observer<openrust_passport::DeviceCode> for ForceCode {
    const EVENTS: &'static [openrust::database::eloquent::concerns::ModelEvent] =
        &[openrust::database::eloquent::concerns::ModelEvent::Creating];
    fn creating<'a>(
        &'a self,
        model: &'a mut openrust_passport::DeviceCode,
        _: &'a Connection,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<
            openrust::database::eloquent::concerns::ModelEventResponse,
        >,
    > {
        Box::pin(async move {
            use openrust::database::eloquent::concerns::ModelEventResponse;
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            if self.always || attempt == 0 {
                model.forceFill(Attributes::from([(
                    "user_code".into(),
                    self.code.clone().into(),
                )]))?;
            }
            Ok(if self.cancel {
                ModelEventResponse::Cancel
            } else {
                ModelEventResponse::Continue
            })
        })
    }
}
#[tokio::test]
async fn mysql_device_unique_code_retries_are_bounded_and_model_cancellation_fails() {
    if !required() {
        return;
    }
    for (always, cancel, expected_attempts, expected_status) in [
        (false, false, 2, 200),
        (true, false, 10, 500),
        (false, true, 1, 500),
    ] {
        let f = Fixture::new().await;
        let client = f.device_client(false).await;
        let existing = f.device_code(&client, "read").await;
        let attempts = Arc::new(AtomicUsize::new(0));
        openrust_passport::DeviceCode::observe(
            f.kernel.get_application(),
            ForceCode {
                code: existing["user_code"].as_str().unwrap().into(),
                attempts: attempts.clone(),
                always,
                cancel,
            },
        )
        .unwrap();
        let response = f
            .wire(
                reqwest::Method::POST,
                "/oauth/device/code",
                "",
                Some(json!({"client_id":client.id,"scope":"read"})),
            )
            .await;
        assert_eq!(response.status(), expected_status);
        assert_eq!(attempts.load(Ordering::Relaxed), expected_attempts);
        assert_eq!(
            f.db.table("oauth_device_codes").count("*").await.unwrap(),
            if expected_status == 200 { 2 } else { 1 }
        );
        f.cleanup().await;
    }
}

#[tokio::test]
async fn mysql_device_competing_browser_decisions_bind_one_real_user() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    let data = f.device_code(&client, "read").await;
    let a = f.login("alice@test").await;
    let b = f.login("bob@test").await;
    let first = f.device_view(&a, data["user_code"].as_str().unwrap()).await;
    let second = f.device_view(&b, data["user_code"].as_str().unwrap()).await;
    let (a, b) = tokio::join!(
        f.device_decide(&a, &first, true),
        f.device_decide(&b, &second, true)
    );
    let statuses = [a.status().as_u16(), b.status().as_u16()];
    assert!(
        statuses == [302, 400] || statuses == [400, 302],
        "{statuses:?}"
    );
    let response = f
        .device_poll(&client, data["device_code"].as_str().unwrap())
        .await;
    assert_eq!(response.status(), 200);
    let data = json_body(response).await;
    assert_eq!(
        jwt_claims(data["access_token"].as_str().unwrap())["sub"],
        if statuses[0] == 302 { "77" } else { "88" }
    );
    f.cleanup().await;
}
#[tokio::test]
async fn mysql_device_database_errors_and_other_application_never_issue_credentials() {
    if !required() {
        return;
    }
    let f = Fixture::new().await;
    let other = Fixture::new().await;
    let client = f.device_client(false).await;
    let data = f.device_code(&client, "read").await;
    let row =
        f.db.table("oauth_clients")
            .r#where("id", "=", client.id.clone())
            .first()
            .await
            .unwrap()
            .unwrap();
    other
        .db
        .table("oauth_clients")
        .insert(&[row.columns().iter().cloned().collect()])
        .await
        .unwrap();
    let response = other
        .device_poll(&client, data["device_code"].as_str().unwrap())
        .await;
    assert_eq!(response.status(), 400);
    assert_eq!(json_body(response).await["error"], "invalid_grant");
    f.db.getSchemaBuilder()
        .rename("oauth_device_codes", "hidden_device_codes")
        .await
        .unwrap();
    let response = f
        .wire(
            reqwest::Method::POST,
            "/oauth/device/code",
            "",
            Some(json!({"client_id":client.id})),
        )
        .await;
    assert_eq!(response.status(), 500);
    assert_eq!(
        f.device_poll(&client, data["device_code"].as_str().unwrap())
            .await
            .status(),
        500
    );
    f.db.getSchemaBuilder()
        .rename("hidden_device_codes", "oauth_device_codes")
        .await
        .unwrap();
    assert_eq!(
        f.db.table("oauth_access_tokens").count("*").await.unwrap(),
        0
    );
    assert_eq!(
        other
            .db
            .table("oauth_access_tokens")
            .count("*")
            .await
            .unwrap(),
        0
    );
    f.cleanup().await;
    other.cleanup().await;
}
fn html_code_view(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let html = format!(
            "<form method=get action=/oauth/device/authorize><label>Device code<input name=user_code></label><button>Authorize</button></form><p id=status>{}</p>",
            request
                .session()?
                .get("status")
                .and_then(J::as_str)
                .unwrap_or("")
        );
        Ok(
            Response::new(StatusCode::OK, openrust::http::Body::from(html)).header(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
            ),
        )
    })
}
fn html_authorization_view(
    request: &mut Request,
    params: openrust_passport::http::AuthorizationViewParameters,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let html = format!(
            "<h1>Device consent</h1><p>read</p><form method=post action=/oauth/device/authorize><input type=hidden name=_token value=\"{}\"><input type=hidden name=auth_token value=\"{}\"><button>Approve</button></form>",
            request.session()?.token().unwrap(),
            params.auth_token
        );
        Ok(
            Response::new(StatusCode::OK, openrust::http::Body::from(html)).header(
                axum::http::header::CONTENT_TYPE,
                axum::http::HeaderValue::from_static("text/html; charset=utf-8"),
            ),
        )
    })
}
#[tokio::test]
#[ignore = "Run explicitly with the installed Playwright and Chromium acceptance runtime"]
async fn mysql_device_real_chromium_browser_approval() {
    assert!(required());
    let f = Fixture::new().await;
    let client = f.device_client(false).await;
    f.passport.deviceUserCodeView(html_code_view);
    f.passport.deviceAuthorizationView(html_authorization_view);
    let base = f.base.clone();
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/device_authorization/browser.mjs");
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("node")
            .arg(script)
            .arg(base)
            .arg(client.id)
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
    assert_eq!(
        f.db.table("oauth_device_codes")
            .r#where("revoked", "=", true)
            .count("*")
            .await
            .unwrap(),
        1
    );
    f.cleanup().await;
}

#[tokio::test]
async fn mysql_device_ignored_routes_do_not_enable_the_grant() {
    if !required() {
        return;
    }
    let f = Fixture::with_device_routes(true, None, false).await;
    let initial = f.wire(reqwest::Method::GET, "/session", "", None).await;
    let cookie = session_cookie(&initial);
    let csrf = json_body(initial).await["csrf"].clone();
    let response = f
        .wire(
            reqwest::Method::POST,
            "/custom-token",
            &cookie,
            Some(json!({"grant_type":GRANT,"client_id":"untrusted","device_code":"missing","_token":csrf})),
        )
        .await;
    assert_eq!(response.status(), 400);
    assert_eq!(json_body(response).await["error"], "unsupported_grant_type");
    assert_eq!(
        f.wire(reqwest::Method::GET, "/oauth/device", "", None)
            .await
            .status(),
        404
    );
    f.cleanup().await;
}
