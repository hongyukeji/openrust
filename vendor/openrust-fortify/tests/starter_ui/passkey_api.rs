use super::passkey_authenticator::Authenticator;
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use openrust::support::facades::Facade as _;
const PREFIX: &str = "/api/admin";
async fn server() -> Server {
    server_with_factors(false).await
}
async fn server_with_factors(factors: bool) -> Server {
    start_security_catalog_mail("api",false,factors,"api/admin",false,Some(json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test","name":"验收"},"__user_passkeys":true}))).await
}
async fn token(server: &Server, abilities: Value) -> String {
    let mut login = device_login();
    login["abilities"] = abilities;
    let result = token_client_request(server.address, &format!("{PREFIX}/auth/token"), login).await;
    assert_eq!(result.0, 201, "{}", result.2);
    result.2["data"]["token"].as_str().unwrap().into()
}
async fn proof(server: &Server, token: &str, purpose: &str) -> String {
    let result = account_request(
        server.address,
        token,
        "POST",
        &format!("{PREFIX}/auth/confirm"),
        json!({"purpose":purpose,"password":"long-password-123"}),
    )
    .await;
    assert_eq!(result.0, 201, "{}", result.2);
    result.2["data"]["proof"].as_str().unwrap().into()
}
async fn request(
    server: &Server,
    token: &str,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, String, Value) {
    account_request(
        server.address,
        token,
        method,
        &format!("{PREFIX}{path}"),
        body,
    )
    .await
}
async fn register(server: &Server) {
    let result=token_client_request(server.address,&format!("{PREFIX}/auth/register"),json!({"name":"无会话通行密钥","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"})).await;
    assert_eq!(result.0, 201, "{}", result.2);
}
async fn enroll(server: &Server, token: &str) -> (Authenticator, String, Value) {
    let proof = proof(server, token, "factors").await;
    let response = request(
        server,
        token,
        "POST",
        "/user/passkeys/options",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(response.0, 200, "{}", response.2);
    let options = &response.2["data"]["options"];
    let handle = String::from_utf8(
        URL_SAFE_NO_PAD
            .decode(options["user"]["id"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    let signer =
        Authenticator::with_origin(81, &format!("http://localhost:{}", server.address.port()));
    let result=request(server,token,"POST","/user/passkeys",json!({"name":"客户端密钥","challenge":response.2["data"]["challenge"],"credential":signer.register(options)})).await;
    assert_eq!(result.0, 201, "{}", result.2);
    (signer, handle, result.2["data"]["id"].clone())
}
async fn confirm(
    server: &Server,
    token: &str,
    signer: &Authenticator,
    handle: &str,
    purpose: &str,
    counter: u32,
) -> String {
    let result = request(
        server,
        token,
        "POST",
        "/auth/passkeys/confirm/options",
        json!({"purpose":purpose}),
    )
    .await;
    assert_eq!(result.0, 200, "{}", result.2);
    let result=request(server,token,"POST","/auth/passkeys/confirm",json!({"challenge":result.2["data"]["challenge"],"credential":signer.assertion(&result.2["data"]["options"],handle,counter)})).await;
    assert_eq!(result.0, 201, "{}", result.2);
    result.2["data"]["proof"].as_str().unwrap().into()
}
#[tokio::test]
async fn token_passkeys_enroll_login_confirm_and_preserve_last_primary_without_cookie_sessions() {
    let server = server().await;
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            openrust::support::facades::Config::set(
                "fortify.limiters.passkeys",
                json!("session-only"),
            )
            .unwrap();
            openrust::support::facades::RateLimiter::for_async("session-only", |request| {
                Box::pin(async move {
                    let _ = request.session()?;
                    Err(openrust::http::HttpError::InvalidRequest)
                })
            })
            .unwrap();
        },
    )
    .await;
    register(&server).await;
    let token = token(
        &server,
        json!(["account:read", "account:update", "tokens:manage"]),
    )
    .await;
    let (signer, handle, id) = enroll(&server, &token).await;
    let response = request(&server, &token, "GET", "/user/passkeys", Value::Null).await;
    assert_eq!(response.0, 200, "{}", response.2);
    assert_eq!(response.2["data"].as_array().unwrap().len(), 1);
    assert!(response.2["data"][0].get("credential").is_none());
    assert_eq!(response.2["data"][0]["id"], id);
    // 模拟已建立通行密钥的无本地密码身份；递增版本，先撤销旧令牌。
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            use openrust::{contracts::auth::Authenticatable as _, database::eloquent::Model};
            let mut query = app::models::User::query().unwrap();
            query.r#where("email", "=", "device@example.test");
            let mut user = query.first().await.unwrap().unwrap();
            let column = user.get_auth_password_name().to_owned();
            user.set_attribute_value(&column, openrust::database::Value::Null)
                .unwrap();
            assert!(user.save().await.unwrap());
            openrust_fortify::starter::credentials::revoke_all(&mut user)
                .await
                .unwrap();
        },
    )
    .await;
    assert_eq!(
        request(&server, &token, "GET", "/user/passkeys", Value::Null)
            .await
            .0,
        401
    );
    let begin=token_client_request(server.address,&format!("{PREFIX}/auth/passkeys/options"),json!({"device_name":"真实签名客户端","abilities":["account:read","account:update","tokens:manage"]})).await;
    assert_eq!(begin.0, 200, "{}", begin.2);
    let unknown = Authenticator::new(99);
    let rejected=token_client_request(server.address,&format!("{PREFIX}/auth/passkeys"),json!({"challenge":begin.2["data"]["challenge"],"credential":unknown.assertion(&begin.2["data"]["options"],&handle,1)})).await;
    assert_eq!(rejected.0, 422, "{}", rejected.2);
    assert_eq!(
        rejected.2["errors"]["credential"][0],
        "无法识别通行密钥，它可能已从账户中移除。"
    );
    let begin=token_client_request(server.address,&format!("{PREFIX}/auth/passkeys/options"),json!({"device_name":"真实签名客户端","abilities":["account:read","account:update","tokens:manage"]})).await;
    assert_eq!(begin.0, 200, "{}", begin.2);
    let body = json!({"challenge":begin.2["data"]["challenge"],"credential":signer.assertion(&begin.2["data"]["options"],&handle,1),"abilities":["*"]});
    let completed = token_client_request(
        server.address,
        &format!("{PREFIX}/auth/passkeys"),
        body.clone(),
    )
    .await;
    assert_eq!(completed.0, 201, "{}", completed.2);
    let second = completed.2["data"]["token"].as_str().unwrap();
    let listed = request(&server, second, "GET", "/user/tokens", Value::Null).await;
    assert_eq!(listed.0, 200, "{}", listed.2);
    let current = listed.2["data"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["current"] == true)
        .unwrap();
    assert_eq!(
        current["abilities"],
        json!(["account:read", "account:update", "tokens:manage"])
    );
    assert_eq!(
        token_client_request(server.address, &format!("{PREFIX}/auth/passkeys"), body)
            .await
            .0,
        401
    );
    let token_proof = confirm(&server, second, &signer, &handle, "tokens", 2).await;
    let factor_proof = confirm(&server, second, &signer, &handle, "factors", 3).await;
    let deleted = request(
        &server,
        second,
        "DELETE",
        &format!("/user/passkeys/{id}"),
        json!({"proof":factor_proof}),
    )
    .await;
    assert_eq!(deleted.0, 422, "{}", deleted.2);
    let preserved = request(
        &server,
        second,
        "POST",
        "/user/tokens",
        json!({"proof":token_proof,"device_name":"撤销被拒后原凭据仍有效","abilities":["account:read"]}),
    )
    .await;
    assert_eq!(preserved.0, 201, "{}", token_failure(&preserved.2));
}

#[tokio::test]
async fn passkey_primary_requires_second_factor_and_revoked_key_cannot_finish_pending_login() {
    use openrust::{
        contracts::{container::Container as _, foundation::Application as _},
        support::facades::{App, Facade as _},
    };
    use std::sync::atomic::{AtomicI64, Ordering};
    let server = server_with_factors(true).await;
    register(&server).await;
    let token = token(
        &server,
        json!(["account:read", "account:update", "tokens:manage"]),
    )
    .await;
    let (signer, handle, id) = enroll(&server, &token).await;
    let clock = Arc::new(AtomicI64::new(1_234_567_890));
    let current = clock.clone();
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let cache = app
            .make::<openrust::cache::CacheManager>()
            .await
            .unwrap()
            .store(None)
            .unwrap();
        app.container()
            .instance::<dyn openrust_fortify::contracts::TwoFactorAuthenticationProvider>(Arc::new(
                super::two_factor_management::Provider {
                    engine: openrust_fortify::TwoFactorAuthenticationProvider::new(Some(cache))
                        .with_clock(move || Ok(current.load(Ordering::SeqCst))),
                },
            ))
            .unwrap();
    })
    .await;
    let p = proof(&server, &token, "factors").await;
    let factor = request(
        &server,
        &token,
        "POST",
        "/user/two-factor-authentication",
        json!({"proof":p}),
    )
    .await;
    assert_eq!(factor.0, 200, "{}", factor.2);
    let codes = factor.2["data"]["recovery_codes"]
        .as_array()
        .unwrap()
        .clone();
    let p = proof(&server, &token, "factors").await;
    assert_eq!(
        request(
            &server,
            &token,
            "POST",
            "/user/confirmed-two-factor-authentication",
            json!({"proof":p,"code":"742275"})
        )
        .await
        .0,
        200
    );
    clock.store(1_234_567_920, Ordering::SeqCst);
    async fn begin(server: &Server, signer: &Authenticator, handle: &str, count: u32) -> Value {
        let options = token_client_request(
            server.address,
            &format!("{PREFIX}/auth/passkeys/options"),
            device_login(),
        )
        .await;
        assert_eq!(options.0, 200, "{}", options.2);
        let result=token_client_request(server.address,&format!("{PREFIX}/auth/passkeys"),json!({"challenge":options.2["data"]["challenge"],"credential":signer.assertion(&options.2["data"]["options"],handle,count)})).await;
        assert_eq!(result.0, 200, "{}", result.2);
        assert_eq!(result.2["code"], "auth_challenge_required");
        assert!(result.2["data"]["token"].is_null());
        result.2["data"]["challenge"].clone()
    }
    let stage = begin(&server, &signer, &handle, 1).await;
    let accepted = token_client_request(
        server.address,
        &format!("{PREFIX}/auth/token/challenge"),
        json!({"challenge":stage,"recovery_code":codes[0]}),
    )
    .await;
    assert_eq!(accepted.0, 201, "{}", accepted.2);
    assert!(accepted.2["data"]["token"].is_string());
    let stage = begin(&server, &signer, &handle, 2).await;
    let p = proof(&server, &token, "factors").await;
    assert_eq!(
        request(
            &server,
            &token,
            "DELETE",
            &format!("/user/passkeys/{id}"),
            json!({"proof":p})
        )
        .await
        .0,
        204
    );
    let rejected = token_client_request(
        server.address,
        &format!("{PREFIX}/auth/token/challenge"),
        json!({"challenge":stage,"recovery_code":codes[1]}),
    )
    .await;
    assert_eq!(rejected.0, 401, "{}", rejected.2);
    let list = request(&server, &token, "GET", "/user/tokens", Value::Null).await;
    assert_eq!(list.0, 200, "{}", list.2);
    assert_eq!(list.2["data"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn passkey_stages_enforce_scopes_clients_purposes_expiry_and_disabled_feature() {
    use openrust::{database::DatabaseManager, support::facades::App};
    let server = server().await;
    register(&server).await;
    let full = token(
        &server,
        json!(["account:read", "account:update", "tokens:manage"]),
    )
    .await;
    let read = token(&server, json!(["account:read"])).await;
    let p = proof(&server, &read, "factors").await;
    assert_eq!(
        request(
            &server,
            &read,
            "POST",
            "/user/passkeys/options",
            json!({"proof":p})
        )
        .await
        .0,
        403
    );
    let (signer, handle, _) = enroll(&server, &full).await;
    let options = request(
        &server,
        &full,
        "POST",
        "/auth/passkeys/confirm/options",
        json!({"purpose":"factors"}),
    )
    .await;
    assert_eq!(options.0, 200, "{}", options.2);
    let body = json!({"challenge":options.2["data"]["challenge"],"credential":signer.assertion(&options.2["data"]["options"],&handle,1)});
    assert_eq!(
        request(
            &server,
            &read,
            "POST",
            "/auth/passkeys/confirm",
            body.clone()
        )
        .await
        .0,
        401
    );
    assert_eq!(
        request(&server, &full, "POST", "/auth/passkeys/confirm", body)
            .await
            .0,
        401
    );
    let options = request(
        &server,
        &full,
        "POST",
        "/auth/passkeys/confirm/options",
        json!({"purpose":"factors"}),
    )
    .await;
    let checked=request(&server,&full,"POST","/auth/passkeys/confirm",json!({"challenge":options.2["data"]["challenge"],"purpose":"tokens","credential":signer.assertion(&options.2["data"]["options"],&handle,2)})).await;
    assert_eq!(checked.0, 201, "{}", checked.2);
    let wrong_purpose=request(&server,&full,"POST","/user/tokens",json!({"proof":checked.2["data"]["proof"],"device_name":"用途不可篡改","abilities":["account:read"]})).await;
    assert_eq!(wrong_purpose.0, 401, "{}", wrong_purpose.2);
    let expiring = request(
        &server,
        &full,
        "POST",
        "/auth/passkeys/confirm/options",
        json!({"purpose":"factors"}),
    )
    .await;
    assert_eq!(expiring.0, 200, "{}", expiring.2);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let database = App::make::<DatabaseManager>().await.unwrap();
        let connection = database.connection(None).unwrap();
        connection
            .table("user_auth_challenges")
            .r#where("purpose", "=", "passkey:confirm")
            .where_null("consumed_at")
            .update([("expires_at".into(), 0i64.into())].into())
            .await
            .unwrap();
    })
    .await;
    let expired=request(&server,&full,"POST","/auth/passkeys/confirm",json!({"challenge":expiring.2["data"]["challenge"],"credential":signer.assertion(&expiring.2["data"]["options"],&handle,3)})).await;
    assert_eq!(expired.0, 401, "{}", expired.2);
    let disabled = start_security("api", false, false, "api/admin").await;
    let result = token_client_request(
        disabled.address,
        &format!("{PREFIX}/auth/passkeys/options"),
        json!({"device_name":"关闭功能"}),
    )
    .await;
    assert_eq!(result.0, 404, "{}", result.2);
}
