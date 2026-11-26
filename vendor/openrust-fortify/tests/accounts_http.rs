include!("support/accounts.rs");

#[tokio::test]
async fn registration_profile_and_password_updates_use_native_eloquent_sessions_and_named_broker() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    NOTIFICATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    let server = start(settings(), true, |_| {}).await;
    let mut client = Client::new(&server);
    let before = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(
        client.send("GET", "/register", "", 200).await.body,
        json!({"register_view":true})
    );
    client.send("POST", "/register", "{}", 422).await;
    assert!(server.events.lock().unwrap().is_empty());
    let response = client.send("POST", "/register", REGISTER, 201).await;
    assert_eq!(response.body, "");
    assert_eq!(*server.events.lock().unwrap(), ["Registered", "Login"]);
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 1);
    let after = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(after["user"], "001");
    assert_eq!(after["email"], "a@example.test");
    assert_ne!(after["id"], before["id"]);
    assert_ne!(after["csrf"], before["csrf"]);
    assert!(client.cookies.contains_key("remember_web"));
    client.send("POST", "/register", REGISTER, 302).await;
    let mut remembered = Client::new(&server);
    remembered.cookies.insert(
        "remember_web".into(),
        client.cookies["remember_web"].clone(),
    );
    assert_eq!(
        remembered.send("GET", "/state", "", 200).await.body["user"],
        "001"
    );
    client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"Tester","email":"invalid"}"#,
            422,
        )
        .await;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["email"],
        "a@example.test"
    );
    client.html = true;
    client
        .send("PUT", "/user/profile-information", "{}", 302)
        .await;
    client.html = false;
    let errors = client.send("GET", "/state", "", 200).await.body["errors"].clone();
    assert!(errors["updateProfileInformation"].is_object());
    client.html = true;
    client
        .send(
            "PUT",
            "/user/profile-information",
            r#"{"name":"Changed","email":"CHANGED@EXAMPLE.TEST"}"#,
            302,
        )
        .await;
    client.html = false;
    let changed = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(changed["email"], "changed@example.test");
    assert_eq!(changed["status"], "profile-information-updated");
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 2);
    client.send("POST", "/seed-tokens", "{}", 204).await;
    client.send("PUT","/user/password",r#"{"current_password":"wrong","password":"updated:secret","password_confirmation":"updated:secret"}"#,422).await;
    let count = server
        .database
        .select_one("SELECT COUNT(*) AS count FROM fortify_named_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(1) | Value::Unsigned(1))
    ));
    server.events.lock().unwrap().clear();
    client.send("PUT","/user/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,200).await;
    assert_eq!(
        *server.events.lock().unwrap(),
        ["OpenRust\\Fortify\\Events\\PasswordUpdatedViaController"]
    );
    let count = server
        .database
        .select_one("SELECT COUNT(*) AS count FROM fortify_named_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(0) | Value::Unsigned(0))
    ));
    let count = server
        .database
        .select_one("SELECT COUNT(*) AS count FROM fortify_default_resets", &[])
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        count.get("count"),
        Some(Value::Integer(1) | Value::Unsigned(1))
    ));
    client.send("POST", "/logout", "{}", 204).await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"changed@example.test","password":"good:secret"}"#,
            422,
        )
        .await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"changed@example.test","password":"updated:secret"}"#,
            200,
        )
        .await;
    server.close().await;
}

#[tokio::test]
async fn container_account_actions_web_intended_and_broker_failure_preserve_effect_order() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(settings(), false, |_| {}).await;
    server.fortify.register_view(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"replacement_view":true})) })
        })),
    );
    let mut client = Client::new(&server);
    assert_eq!(
        client.send("GET", "/register", "", 200).await.body,
        json!({"replacement_view":true})
    );
    client.send("GET", "/intended", "", 200).await;
    client.html = true;
    let response = client.send("POST", "/register", REGISTER, 302).await;
    assert!(response.headers.contains("/account/settings"));
    client.html = false;
    client.send("POST", "/seed-tokens", "{}", 204).await;
    server
        .database
        .statement("DROP TABLE fortify_named_resets", &[])
        .await
        .unwrap();
    server.events.lock().unwrap().clear();
    client.send("PUT","/user/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,500).await;
    assert!(server.events.lock().unwrap().is_empty());
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 422).await;
    client
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"updated:secret"}"#,
            200,
        )
        .await;
    server.close().await;
}

struct CustomProfileResponse;
impl contracts::ProfileInformationUpdatedResponse for CustomProfileResponse {}
impl contracts::Response for CustomProfileResponse {
    fn to_response<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.unwrap();
            let email = user.get_auth_attribute("email")?.unwrap();
            let Value::Text(email) = email else {
                panic!("native model email")
            };
            Response::json(StatusCode::ACCEPTED, json!({"current_email":email}))
        })
    }
}
#[tokio::test]
async fn configured_account_paths_csrf_and_response_replacements_use_the_updated_guard_user() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let mut options = settings();
    options["views"] = false.into();
    options["prefix"] = "account".into();
    options["paths"] = json!({"register":"/join","user-profile-information":{"update":"/profile"},"user-password":{"update":"/password"}});
    let server = start(options, true, |app| {
        app.container()
            .instance::<dyn contracts::ProfileInformationUpdatedResponse>(Arc::new(
                CustomProfileResponse,
            ))
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    client.fetch = false;
    client.send("POST", "/account/join", REGISTER, 419).await;
    client.fetch = true;
    client.send("GET", "/account/join", "", 405).await;
    client.send("POST", "/register", REGISTER, 404).await;
    client.send("POST", "/account/join", REGISTER, 201).await;
    let updated = client
        .send(
            "PUT",
            "/account/profile",
            r#"{"name":"Changed","email":"CHANGED@EXAMPLE.TEST"}"#,
            202,
        )
        .await;
    assert_eq!(
        updated.body,
        json!({"current_email":"changed@example.test"})
    );
    client.send("PUT","/account/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"mismatch"}"#,422).await;
    client.html = true;
    client.send("PUT","/account/password",r#"{"current_password":"good:secret","password":"updated:secret","password_confirmation":"updated:secret"}"#,302).await;
    client.html = false;
    assert_eq!(
        client.send("GET", "/state", "", 200).await.body["status"],
        "password-updated"
    );
    server.close().await;
}

#[tokio::test]
async fn fixed_php_account_controllers_match_native_mysql_effect_and_event_order() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(settings(), false, |_| {}).await;
    let mut client = Client::new(&server);
    let cases: Vec<Json> = serde_json::from_str(include_str!(
        "../../../.agents/project/audits/api-f01/fixtures/accounts.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 4);
    let mut session_id = client.send("GET", "/state", "", 200).await.body["id"].clone();
    for case in cases {
        let action = case["action"].as_str().unwrap();
        if action == "password" {
            client.send("POST", "/seed-tokens", "{}", 204).await;
        }
        if case["fail"] == true {
            server.database.unprepared("CREATE TRIGGER fortify_reject_token_delete BEFORE DELETE ON fortify_named_resets FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='fixture rejects token deletion'").await.unwrap();
        }
        server.events.lock().unwrap().clear();
        let (method, path) = match action {
            "register" => ("POST", "/register"),
            "profile" => ("PUT", "/user/profile-information"),
            "password" => ("PUT", "/user/password"),
            _ => panic!("invalid oracle action"),
        };
        client
            .send(
                method,
                path,
                &case["input"].to_string(),
                u16::try_from(case["status"].as_u64().unwrap()).unwrap(),
            )
            .await;
        let state = client.send("GET", "/state", "", 200).await.body;
        assert_eq!(state["email"], case["email"]);
        if action == "register" {
            assert_ne!(state["id"], session_id);
        } else {
            assert_eq!(state["id"], session_id);
        }
        session_id = state["id"].clone();
        let events: Vec<_> = server
            .events
            .lock()
            .unwrap()
            .iter()
            .map(|v| v.rsplit('\\').next().unwrap().to_owned())
            .collect();
        assert_eq!(json!(events), case["events"]);
        assert_eq!(
            json!(
                server
                    .database
                    .table("fortify_named_resets")
                    .count("*")
                    .await
                    .unwrap()
            ),
            case["tokens"]
        );
        let row = server
            .database
            .table("fortify_account_users")
            .first()
            .await
            .unwrap()
            .unwrap();
        let Some(Value::Text(hash)) = row.get("password") else {
            panic!("password missing")
        };
        assert!(
            server
                .hasher
                .check(
                    case["password"].as_str().unwrap().as_bytes(),
                    hash,
                    &HashOptions::new()
                )
                .await
                .unwrap()
        );
        if case["fail"] == true {
            server
                .database
                .unprepared("DROP TRIGGER fortify_reject_token_delete")
                .await
                .unwrap();
        }
    }
    server.close().await;
}

#[tokio::test]
async fn account_http_apps_do_not_share_session_tables() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let first = start(settings(), true, |_| {}).await;
    let mut client = Client::new(&first);
    let before = client.send("GET", "/state", "", 200).await.body;
    let second = start(settings(), true, |_| {}).await;
    second
        .database
        .statement("DROP TABLE fortify_account_sessions", &[])
        .await
        .unwrap();

    let after = client.send("GET", "/state", "", 200).await.body;
    assert_eq!(after["id"], before["id"]);
    assert_eq!(after["csrf"], before["csrf"]);
    second.close().await;
    first.close().await;
}
