use super::*;
use openrust::support::facades::{App, Facade as _};
use std::sync::atomic::{AtomicI64, Ordering};
pub(super) struct Provider {
    pub(super) engine: openrust_fortify::TwoFactorAuthenticationProvider,
}
impl openrust_fortify::contracts::TwoFactorAuthenticationProvider for Provider {
    fn generate_secret_key(&self, _: usize) -> openrust_fortify::FortifyResult<String> {
        Ok("JBSWY3DPEHPK3PXP".into())
    }
    fn qr_code_url(&self, company: &str, account: &str, secret: &str) -> String {
        self.engine.qr_code_url(company, account, secret)
    }
    fn verify<'a>(
        &'a self,
        secret: &'a str,
        code: &'a str,
    ) -> openrust::http::HttpFuture<'a, openrust_fortify::FortifyResult<bool>> {
        self.engine.verify(secret, code)
    }
}
async fn server() -> (Server, Arc<AtomicI64>) {
    let server = start_security("full", false, true, "api").await;
    register_token_user(&server).await;
    let clock = Arc::new(AtomicI64::new(1_234_567_890));
    let current = clock.clone();
    App::set_facade_application(Some(server._kernel.get_application()), async {
        use openrust::contracts::{container::Container as _, foundation::Application as _};
        let app = server._kernel.get_application();
        let cache = app
            .make::<openrust::cache::CacheManager>()
            .await
            .unwrap()
            .store(None)
            .unwrap();
        app.container()
            .instance::<dyn openrust_fortify::contracts::TwoFactorAuthenticationProvider>(Arc::new(
                Provider {
                    engine: openrust_fortify::TwoFactorAuthenticationProvider::new(Some(cache))
                        .with_clock(move || Ok(current.load(Ordering::SeqCst))),
                },
            ))
            .unwrap();
    })
    .await;
    (server, clock)
}
async fn token(server: &Server) -> String {
    let response = token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(response.0, 201, "{}", response.2);
    response.2["data"]["token"].as_str().unwrap().into()
}
#[tokio::test]
async fn factor_api_enrolls_confirms_rotates_codes_and_disables_with_scoped_single_use_proofs() {
    let (server, clock) = server().await;
    let token = token(&server).await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/two-factor-authentication",
            json!({})
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, &token, "factors").await;
    let response = account_request(
        server.address,
        &token,
        "POST",
        "/api/user/two-factor-authentication",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(response.0, 200, "{}", response.2);
    assert_eq!(response.2["data"]["secretKey"], "JBSWY3DPEHPK3PXP");
    assert_eq!(response.2["data"]["confirmation_required"], true);
    assert_eq!(
        response.2["data"]["recovery_codes"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert!(response.2["data"]["svg"].as_str().unwrap().contains("<svg"));
    let old_codes = response.2["data"]["recovery_codes"].clone();
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/two-factor-authentication",
            json!({"proof":proof})
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_wire_request(
            server.address,
            &token,
            "GET",
            "/api/user/two-factor-secret-key?proof=forbidden",
            Value::Null,
            Some(&proof)
        )
        .await
        .0,
        400
    );
    let secret = account_wire_request(
        server.address,
        &token,
        "GET",
        "/api/user/two-factor-secret-key",
        Value::Null,
        Some(&proof),
    )
    .await;
    assert_eq!(secret.0, 200, "{}", secret.2);
    assert_eq!(secret.2["data"]["secretKey"], "JBSWY3DPEHPK3PXP");
    assert!(secret.1.contains("no-store"));
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/confirmed-two-factor-authentication",
            json!({"proof":proof,"code":"000000"})
        )
        .await
        .0,
        422
    );
    let proof = confirmation(&server, &token, "factors").await;
    let result = account_request(
        server.address,
        &token,
        "POST",
        "/api/user/confirmed-two-factor-authentication",
        json!({"proof":proof,"code":"742275"}),
    )
    .await;
    assert_eq!(result.0, 200, "{}", result.2);
    assert_eq!(result.2["data"]["enabled"], true);
    let login = token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(login.0, 200);
    assert!(login.2["data"]["token"].is_null());
    clock.store(1_234_567_920, Ordering::SeqCst);
    let challenge = login.2["data"]["challenge"].clone();
    let completed = token_client_request(
        server.address,
        "/api/auth/token/challenge",
        json!({"challenge":challenge,"code":"835227"}),
    )
    .await;
    assert_eq!(completed.0, 201, "{}", completed.2);
    let proof = confirmation(&server, &token, "factors").await;
    let response = account_request(
        server.address,
        &token,
        "POST",
        "/api/user/two-factor-recovery-codes",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(response.0, 200, "{}", response.2);
    assert_ne!(response.2["data"]["recovery_codes"], old_codes);
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "DELETE",
            "/api/user/two-factor-authentication",
            json!({"proof":proof})
        )
        .await
        .0,
        200
    );
    assert_eq!(
        token_client_request(server.address, "/api/auth/token", device_login())
            .await
            .0,
        201
    );
}
#[tokio::test]
async fn factor_api_is_absent_when_disabled_and_read_only_tokens_cannot_manage_secrets() {
    let disabled = start_options("full", false).await;
    register_token_user(&disabled).await;
    let bearer = token(&disabled).await;
    for (method, path) in [
        ("GET", "/api/user/two-factor-authentication"),
        ("POST", "/api/user/two-factor-authentication"),
        ("GET", "/api/user/two-factor-secret-key"),
    ] {
        assert_eq!(
            account_request(disabled.address, &bearer, method, path, json!({}))
                .await
                .0,
            404
        );
    }
    let (server, _) = server().await;
    let mut login = device_login();
    login["abilities"] = json!(["account:read"]);
    let response = token_client_request(server.address, "/api/auth/token", login).await;
    assert_eq!(response.0, 201);
    let bearer = response.2["data"]["token"].as_str().unwrap();
    assert_eq!(
        account_request(
            server.address,
            bearer,
            "GET",
            "/api/user/two-factor-authentication",
            Value::Null
        )
        .await
        .0,
        200
    );
    assert_eq!(
        account_request(
            server.address,
            bearer,
            "POST",
            "/api/user/two-factor-authentication",
            json!({})
        )
        .await
        .0,
        403
    );
    assert_eq!(
        account_request(
            server.address,
            bearer,
            "GET",
            "/api/user/two-factor-secret-key",
            Value::Null
        )
        .await
        .0,
        403
    );
}

async fn browser(server: &Server) -> Client {
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    assert_eq!(
        browser
            .send(
                "POST",
                "/login",
                r#"{"email":"device@example.test","password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        200
    );
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    browser
}
async fn confirm_browser(browser: &mut Client) {
    let (status, body) = browser
        .send(
            "POST",
            "/user/confirm-password",
            r#"{"password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 201, "{body}");
}
#[tokio::test]
async fn factor_web_page_uses_native_forms_and_never_shows_secrets_without_fresh_confirmation() {
    let (server, _) = server().await;
    let mut browser = browser(&server).await;
    let (status, body) = browser
        .send("GET", "/user/two-factor-authentication", "", false)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("two-factor-authentication"));
    let (status, headers, body) =
        form_request(&mut browser, "/user/two-factor-authentication", "").await;
    assert_eq!(status, 302, "{body}");
    assert!(headers.contains("confirm-password"));
    confirm_browser(&mut browser).await;
    let (status, _, body) = form_request(&mut browser, "/user/two-factor-authentication", "").await;
    assert_eq!(status, 302, "{body}");
    let (status, body) = browser
        .send("GET", "/user/two-factor-authentication", "", false)
        .await;
    assert_eq!(status, 302, "{body}");
    assert!(!body.contains("JBSWY3DPEHPK3PXP"));
    confirm_browser(&mut browser).await;
    let (status, headers, body) = browser
        .exchange_origin("GET", "/user/two-factor-authentication", "", false, false)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("JBSWY3DPEHPK3PXP"));
    assert!(body.contains("<svg"));
    assert!(headers.contains("no-store"));
    assert!(headers.contains("no-referrer"));
    assert_eq!(
        form_request(
            &mut browser,
            "/user/confirmed-two-factor-authentication",
            "code=742275"
        )
        .await
        .0,
        302
    );
    let (status, body) = browser
        .send("GET", "/user/two-factor-authentication", "", false)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("JBSWY3DPEHPK3PXP"));
    assert!(!body.contains("<svg"));
    assert_eq!(
        form_request(&mut browser, "/user/two-factor-recovery-codes", "")
            .await
            .0,
        302
    );
    confirm_browser(&mut browser).await;
    assert_eq!(
        form_request(&mut browser, "/user/two-factor-recovery-codes", "")
            .await
            .0,
        302
    );
    let (status, body) = browser
        .send(
            "GET",
            "/user/two-factor-authentication?recovery=1",
            "",
            false,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("<li><code>"));
    assert!(!body.contains("JBSWY3DPEHPK3PXP"));
    assert_eq!(
        form_request(
            &mut browser,
            "/user/two-factor-authentication",
            "_method=DELETE"
        )
        .await
        .0,
        302
    );
    let (status, body) = browser
        .send("GET", "/user/two-factor-authentication", "", false)
        .await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("<li><code>"));
}

#[tokio::test]
async fn api_only_factor_management_follows_multilevel_prefix_without_an_account_page() {
    let server = start_security("api", false, true, "api/admin").await;
    let registration=token_client_request(server.address,"/api/admin/auth/register",json!({"name":"API-only user","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"})).await;
    assert_eq!(registration.0, 201, "{}", registration.2);
    let response =
        token_client_request(server.address, "/api/admin/auth/token", device_login()).await;
    assert_eq!(response.0, 201, "{}", response.2);
    let bearer = response.2["data"]["token"].as_str().unwrap();
    let response = account_request(
        server.address,
        bearer,
        "POST",
        "/api/admin/auth/confirm",
        json!({"purpose":"factors","password":"long-password-123"}),
    )
    .await;
    assert_eq!(response.0, 201, "{}", response.2);
    let proof = response.2["data"]["proof"].as_str().unwrap();
    let response = account_wire_request(
        server.address,
        bearer,
        "POST",
        "/api/admin/user/two-factor-authentication",
        json!({}),
        Some(proof),
    )
    .await;
    assert_eq!(response.0, 200, "{}", response.2);
    assert_eq!(
        response.2["data"]["recovery_codes"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(
        account_request(
            server.address,
            bearer,
            "GET",
            "/api/user/two-factor-authentication",
            Value::Null
        )
        .await
        .0,
        404
    );
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(
        browser
            .send("GET", "/user/two-factor-authentication", "", true)
            .await
            .0,
        405
    );
}

#[tokio::test]
async fn cookie_second_factor_rejects_primary_proof_after_password_rotation() {
    let (server, clock) = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/two-factor-authentication",
            json!({"proof":proof})
        )
        .await
        .0,
        200
    );
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/confirmed-two-factor-authentication",
            json!({"proof":proof,"code":"742275"})
        )
        .await
        .0,
        200
    );
    clock.store(1_234_567_920, Ordering::SeqCst);
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let login = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(login.0, 200, "{}", login.1);
    assert_eq!(
        serde_json::from_str::<Value>(&login.1).unwrap()["data"]["two_factor"],
        true
    );
    // 在另一客户端变更凭据；旧Cookie的主因素不能被有效TOTP复活。
    App::set_facade_application(Some(server._kernel.get_application()), async {
        use openrust::database::eloquent::Model;
        let mut query = app::models::User::query().unwrap();
        query.r#where("email", "=", "device@example.test");
        let mut user = query.first().await.unwrap().unwrap();
        openrust_fortify::starter::credentials::change_password(
            &mut user,
            "another-long-password-123",
        )
        .await
        .unwrap();
    })
    .await;
    let attempt = browser
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"code":"835227"}"#,
            true,
        )
        .await;
    assert_eq!(attempt.0, 401, "{}", attempt.1);
    assert_eq!(browser.send("GET", "/home", "", true).await.0, 401);
}

#[tokio::test]
async fn cookie_second_factor_completes_native_login_without_creating_a_personal_token() {
    let (server, clock) = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/two-factor-authentication",
            json!({"proof":proof})
        )
        .await
        .0,
        200
    );
    let proof = confirmation(&server, &token, "factors").await;
    assert_eq!(
        account_request(
            server.address,
            &token,
            "POST",
            "/api/user/confirmed-two-factor-authentication",
            json!({"proof":proof,"code":"742275"})
        )
        .await
        .0,
        200
    );
    clock.store(1_234_567_920, Ordering::SeqCst);
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let stage = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(stage.0, 200, "{}", stage.1);
    assert_eq!(browser.send("GET", "/home", "", true).await.0, 401);
    let invalid = browser
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"code":"000000"}"#,
            true,
        )
        .await;
    assert_eq!(invalid.0, 422, "{}", invalid.1);
    let completed = browser
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"code":"835227"}"#,
            true,
        )
        .await;
    assert_eq!(completed.0, 200, "{}", completed.1);
    assert!(!completed.1.contains("plain_text_token"));
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let replay = browser
        .send(
            "POST",
            "/two-factor-challenge",
            r#"{"code":"835227"}"#,
            true,
        )
        .await;
    // 原生guest中间件将已登录用户重定向，不再进入挑战控制器。
    assert_eq!(replay.0, 302, "{}", replay.1);
    let tokens = account_request(
        server.address,
        &token,
        "GET",
        "/api/user/tokens",
        Value::Null,
    )
    .await;
    assert_eq!(tokens.0, 200, "{}", tokens.2);
    assert_eq!(tokens.2["data"].as_array().unwrap().len(), 1);
}
