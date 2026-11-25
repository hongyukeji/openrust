use super::passkey_authenticator::Authenticator;
use super::*;

async fn server() -> Server {
    start_security_catalog_mail("full", false, false, "api", false, Some(json!({"default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"admin@example.test","name":"验收"},"__user_passkeys":true}))).await
}
async fn browser(server: &Server) -> Client {
    register_token_user(server).await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let response = browser
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(response.0, 200, "{}", response.1);
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    browser
}
async fn options(browser: &mut Client, path: &str) -> Value {
    let response = browser.send("GET", path, "", true).await;
    assert_eq!(response.0, 200, "{}", response.1);
    serde_json::from_str::<Value>(&response.1).unwrap()["options"].clone()
}
async fn post(browser: &mut Client, path: &str, body: Value) -> (u16, String) {
    browser.send("POST", path, &body.to_string(), true).await
}
#[tokio::test]
async fn native_passkeys_enroll_confirm_revoke_and_invalidate_recent_proof() {
    let server = server().await;
    let mut browser = browser(&server).await;
    assert_eq!(
        browser
            .send("GET", "/user/passkeys/options", "", true)
            .await
            .0,
        423
    );
    assert_eq!(
        post(
            &mut browser,
            "/user/confirm-password",
            json!({"password":"long-password-123"})
        )
        .await
        .0,
        201
    );
    let opts = options(&mut browser, "/user/passkeys/options").await;
    let handle = opts["user"]["id"].as_str().unwrap().to_owned();
    use base64::Engine as _;
    let handle = String::from_utf8(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(handle)
            .unwrap(),
    )
    .unwrap();
    let authenticator =
        Authenticator::with_origin(41, &format!("http://localhost:{}", server.address.port()));
    let wrong_origin = Authenticator::new(40);
    let rejected = post(
        &mut browser,
        "/user/passkeys",
        json!({"name":"错误来源","credential":wrong_origin.register(&opts)}),
    )
    .await;
    assert_eq!(rejected.0, 422, "{}", rejected.1);
    let opts = options(&mut browser, "/user/passkeys/options").await;
    let registration = authenticator.register(&opts);
    let registered = post(
        &mut browser,
        "/user/passkeys",
        json!({"name":"本机测试","credential":registration}),
    )
    .await;
    assert_eq!(registered.0, 200, "{}", registered.1);
    let id = serde_json::from_str::<Value>(&registered.1).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let page = browser.send("GET", "/user/passkeys", "", false).await;
    assert_eq!(page.0, 200, "{}", page.1);
    assert!(page.1.contains("本机测试"));
    assert!(!page.1.contains("credential_id"));
    assert_eq!(browser.send("POST", "/logout", "{}", true).await.0, 204);
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let login_options = options(&mut browser, "/passkeys/login/options").await;
    let login = post(
        &mut browser,
        "/passkeys/login",
        json!({"credential":authenticator.assertion(&login_options,&handle,1)}),
    )
    .await;
    assert_eq!(login.0, 200, "{}", login.1);
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let opts = options(&mut browser, "/passkeys/confirm/options").await;
    let credential = authenticator.assertion(&opts, &handle, 2);
    let response = post(
        &mut browser,
        "/passkeys/confirm",
        json!({"credential":credential.clone()}),
    )
    .await;
    assert_eq!(response.0, 200, "{}", response.1);
    assert_eq!(
        post(
            &mut browser,
            "/passkeys/confirm",
            json!({"credential":credential})
        )
        .await
        .0,
        422
    );
    assert_eq!(
        browser
            .send("GET", "/user/passkeys/options", "", true)
            .await
            .0,
        200
    );
    let revoked = browser
        .send("DELETE", &format!("/user/passkeys/{id}"), "{}", true)
        .await;
    assert_eq!(revoked.0, 200, "{}", revoked.1);
    assert_eq!(
        browser
            .send("GET", "/user/passkeys/options", "", true)
            .await
            .0,
        423
    );
    let asset = browser
        .send("GET", "/vendor/user-starter/passkeys.js", "", false)
        .await;
    assert_eq!(asset.0, 200);
    assert!(asset.1.contains("Passkeys"));
    // 被撤销的凭据不能通过匿名通行密钥登录。
    assert_eq!(browser.send("POST", "/logout", "{}", true).await.0, 204);
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let opts = options(&mut browser, "/passkeys/login/options").await;
    let rejected = post(
        &mut browser,
        "/passkeys/login",
        json!({"credential":authenticator.assertion(&opts,&handle,3)}),
    )
    .await;
    assert_eq!(rejected.0, 422, "{}", rejected.1);
}

#[tokio::test]
async fn passwordless_user_confirms_with_passkey_and_safely_establishes_a_password() {
    use base64::Engine as _;
    use openrust::{
        contracts::auth::Authenticatable as _,
        database::eloquent::Model,
        support::facades::{App, Facade as _},
    };
    let server = server().await;
    let mut browser = browser(&server).await;
    assert_eq!(
        post(
            &mut browser,
            "/user/confirm-password",
            json!({"password":"long-password-123"})
        )
        .await
        .0,
        201
    );
    let opts = options(&mut browser, "/user/passkeys/options").await;
    let handle = String::from_utf8(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(opts["user"]["id"].as_str().unwrap())
            .unwrap(),
    )
    .unwrap();
    let authenticator =
        Authenticator::with_origin(42, &format!("http://localhost:{}", server.address.port()));
    assert_eq!(
        post(
            &mut browser,
            "/user/passkeys",
            json!({"name":"无密码账户密钥","credential":authenticator.register(&opts)})
        )
        .await
        .0,
        200
    );
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let mut query = app::models::User::query().unwrap();
        query.r#where("email", "=", "device@example.test");
        let mut user = query.first().await.unwrap().unwrap();
        let column = user.get_auth_password_name().to_owned();
        user.set_attribute_value(&column, openrust::database::Value::Null)
            .unwrap();
        assert!(user.save().await.unwrap());
    })
    .await;
    let expired = browser.send("GET", "/home", "", true).await;
    assert_eq!(expired.0, 401, "{}", expired.1);
    assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
    let opts = options(&mut browser, "/passkeys/login/options").await;
    let login = post(
        &mut browser,
        "/passkeys/login",
        json!({"credential":authenticator.assertion(&opts,&handle,1)}),
    )
    .await;
    assert_eq!(login.0, 200, "{}", login.1);
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let page = browser
        .send("GET", "/user/confirm-password", "", false)
        .await;
    assert_eq!(page.0, 200, "{}", page.1);
    assert!(!page.1.contains("id=\"password\""));
    assert!(page.1.contains("id=\"passkey-auth\""));
    assert_eq!(browser.send("PUT","/user/password",r#"{"password":"new-local-password-123","password_confirmation":"new-local-password-123"}"#,true).await.0,423);
    let opts = options(&mut browser, "/passkeys/confirm/options").await;
    assert_eq!(
        post(
            &mut browser,
            "/passkeys/confirm",
            json!({"credential":authenticator.assertion(&opts,&handle,2)})
        )
        .await
        .0,
        200
    );
    let key = App::set_facade_application(Some(server._kernel.get_application()), async {
        let user = app::models::User::query()
            .unwrap()
            .first()
            .await
            .unwrap()
            .unwrap();
        let service = server
            ._kernel
            .get_application()
            .make::<openrust_fortify::passkeys::Passkeys>()
            .await
            .unwrap();
        openrust::auth::AuthIdentifier::try_from(
            &service.for_user(&user).await.unwrap()[0]
                .get_key()
                .await
                .unwrap(),
        )
        .unwrap()
        .to_string()
    })
    .await;
    let denied = browser
        .send("DELETE", &format!("/user/passkeys/{key}"), "{}", true)
        .await;
    assert_eq!(denied.0, 422, "{}", denied.1);
    let update=browser.send("PUT","/user/password",r#"{"password":"new-local-password-123","password_confirmation":"new-local-password-123"}"#,true).await;
    assert_eq!(update.0, 200, "{}", update.1);
    let home = browser.send("GET", "/home", "", false).await;
    assert_eq!(home.0, 200, "{}", home.1);
    assert!(home.1.contains("name=\"current_password\""));
}
