use super::*;
use openrust::support::facades::Facade as _;
async fn login(client: &mut Client) {
    assert_eq!(client.send("GET", "/login", "", false).await.0, 200);
    let (status, body) = client
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(client.send("GET", "/home", "", false).await.0, 200);
}
async fn confirm(client: &mut Client) {
    let (status, body) = client
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
async fn token_web_native_forms_confirm_issue_once_without_secret_flash_and_revoke() {
    let server = start_security("full", false, false, "api").await;
    register_token_user(&server).await;
    let mut client = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    login(&mut client).await;
    let (status, body) = client.send("GET", "/user/api-tokens", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("API 令牌"));
    let (status, headers, body) = form_request(
        &mut client,
        "/user/api-tokens",
        "device_name=web&abilities%5B%5D=account%3Aread",
    )
    .await;
    assert_eq!(status, 302, "{body}");
    assert!(headers.contains("confirm-password"));
    confirm(&mut client).await;
    let (status, headers, body) = form_request(
        &mut client,
        "/user/api-tokens",
        "device_name=web&abilities%5B%5D=account%3Aread",
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert!(
        headers
            .to_ascii_lowercase()
            .contains("referrer-policy: no-referrer")
    );
    assert!(headers.contains("no-store"));
    let token = body
        .split("id=\"new-token\">")
        .nth(1)
        .unwrap()
        .split("</code>")
        .next()
        .unwrap();
    assert!(token.contains('|'));
    let (status, _, body) = account_wire_request(
        server.address,
        token,
        "GET",
        "/api/token-me",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = client.send("GET", "/user/api-tokens", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains(token));
    assert!(!body.contains("id=\"new-token\""));
    let path = body
        .split("action=\"")
        .filter(|s| s.starts_with("http") || s.starts_with("/user/api-tokens/"))
        .find_map(|v| {
            let x = v.split('"').next()?;
            x.rfind("/user/api-tokens/").map(|i| x[i..].to_owned())
        })
        .unwrap();
    let (status, _, body) = form_request(&mut client, &path, "_method=DELETE").await;
    assert_eq!(status, 302, "{body}");
    assert_eq!(
        account_wire_request(
            server.address,
            token,
            "GET",
            "/api/token-me",
            Value::Null,
            None
        )
        .await
        .0,
        401
    );
    assert_eq!(client.send("GET", "/home", "", false).await.0, 200);
}
#[tokio::test]
async fn token_web_empty_or_unapproved_abilities_do_not_issue_and_api_only_has_no_page() {
    let server = start_security("full", false, false, "api").await;
    register_token_user(&server).await;
    let mut client = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    login(&mut client).await;
    confirm(&mut client).await;
    for data in [
        r#"{"device_name":"web"}"#,
        r#"{"device_name":"web","abilities":["*"]}"#,
    ] {
        let (status, body) = client.send("POST", "/user/api-tokens", data, true).await;
        assert_eq!(status, 422, "{body}");
        let error: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(error["success"], false);
        assert_eq!(error["code"], "validation_failed");
        assert!(error["errors"]["abilities"].is_array(), "{body}");
    }
    let (status, _, body) = client
        .exchange_origin("GET", "/api/user/tokens", "", true, true)
        .await;
    assert_eq!(status, 200, "{body}");
    let value: Value = serde_json::from_str(&body).unwrap();
    assert!(value["data"].as_array().unwrap().is_empty());
    let api = start_security("api", false, false, "api").await;
    let mut other = Client {
        address: api.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(
        other.send("GET", "/user/api-tokens", "", false).await.0,
        404
    );
}

#[tokio::test]
async fn token_web_rejects_csrf_and_other_owners_without_revoking_their_token() {
    let server = start_security("full", false, false, "api").await;
    register_token_user(&server).await;
    let mut owner = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    login(&mut owner).await;
    confirm(&mut owner).await;
    let (status, body) = owner
        .send(
            "POST",
            "/user/api-tokens",
            r#"{"device_name":"first","abilities":["account:read"]}"#,
            true,
        )
        .await;
    assert_eq!(status, 201, "{body}");
    let issued: Value = serde_json::from_str(&body).unwrap();
    let token = issued["data"]["token"].as_str().unwrap();
    let id = token.split('|').next().unwrap();
    let mut other = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    assert_eq!(other.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=other.send("POST","/register",r#"{"name":"另一个用户","email":"other-token@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(other.send("GET", "/home", "", false).await.0, 200);
    confirm(&mut other).await;
    let path = format!("/user/api-tokens/{id}");
    let (status, _, body) = form_request(&mut other, &path, "_method=DELETE").await;
    assert_eq!(status, 404, "{body}");
    let (status, body) = other.send("DELETE", &path, "{}", true).await;
    assert_eq!(status, 404, "{body}");
    let error: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(error["success"], false);
    assert_eq!(error["code"], "not_found");
    let saved = owner.csrf.clone();
    owner.csrf = "invalid-csrf".into();
    let (status, _, body) = form_request(&mut owner, &path, "_method=DELETE").await;
    assert_eq!(status, 419, "{body}");
    owner.csrf = saved;
    assert_eq!(
        account_wire_request(
            server.address,
            token,
            "GET",
            "/api/token-me",
            Value::Null,
            None
        )
        .await
        .0,
        200
    );
    let (status, headers, body) = owner.exchange("DELETE", &path, "{}", true).await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_empty());
    assert!(headers.contains("cache-control: no-store"), "{headers}");
}

#[tokio::test]
async fn token_web_empty_server_whitelist_preserves_existing_token_revocation() {
    let server = start_security("full", false, false, "api").await;
    register_token_user(&server).await;
    let mut client = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    login(&mut client).await;
    confirm(&mut client).await;
    let (status, body) = client
        .send(
            "POST",
            "/user/api-tokens",
            r#"{"device_name":"before-policy","abilities":["account:read"]}"#,
            true,
        )
        .await;
    assert_eq!(status, 201);
    let issued: Value = serde_json::from_str(&body).unwrap();
    let token = issued["data"]["token"].as_str().unwrap();
    let id = token.split('|').next().unwrap();
    openrust::support::facades::App::set_facade_application(
        Some(server._kernel.get_application()),
        async {
            openrust::support::facades::Config::set("user.token_abilities", json!([])).unwrap();
        },
    )
    .await;
    let (status, body) = client.send("GET", "/user/api-tokens", "", false).await;
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("name=\"device_name\""));
    assert!(body.contains("before-policy"));
    assert_eq!(
        form_request(
            &mut client,
            &format!("/user/api-tokens/{id}"),
            "_method=DELETE"
        )
        .await
        .0,
        302
    );
}
