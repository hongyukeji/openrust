include!("support/accounts.rs");
fn reset_settings() -> Json {
    let mut config = settings();
    config["features"]
        .as_array_mut()
        .unwrap()
        .push(json!("reset-passwords"));
    config
}
async fn prepare(server: &Server) -> Client {
    let mut account = Client::new(server);
    account.send("POST", "/register", REGISTER, 201).await;
    account.send("POST", "/logout", "{}", 204).await;
    server.events.lock().unwrap().clear();
    RESET_NOTIFICATIONS.lock().unwrap().clear();
    Client::new(server)
}
fn reset_body(token: &str) -> String {
    json!({"email":"A@EXAMPLE.TEST","token":token,"password":"changed:secret","password_confirmation":"changed:secret"}).to_string()
}
async fn token_count(server: &Server, table: &str) -> u64 {
    let row = server
        .database
        .selectOne(&format!("SELECT COUNT(*) AS count FROM {table}"), &[])
        .await
        .unwrap()
        .unwrap();
    match row.get("count").unwrap() {
        Value::Integer(v) => *v as u64,
        Value::Unsigned(v) => *v,
        _ => panic!("invalid count"),
    }
}
#[tokio::test]
async fn password_reset_uses_named_broker_rotates_remember_token_and_keeps_guest_session() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(reset_settings(), true, |_| {}).await;
    let mut guest = prepare(&server).await;
    let invalid = guest
        .send("POST", "/forgot-password", r#"{"email":"bad"}"#, 422)
        .await;
    assert!(invalid.body["errors"]["email"].is_array());
    assert!(RESET_NOTIFICATIONS.lock().unwrap().is_empty());
    guest
        .send(
            "POST",
            "/forgot-password",
            r#"{"email":["a@example.test"]}"#,
            422,
        )
        .await;
    guest
        .send(
            "POST",
            "/reset-password",
            r#"{"email":["a@example.test"],"token":"token","password":"password"}"#,
            500,
        )
        .await;
    guest
        .send(
            "POST",
            "/reset-password",
            r#"{"email":"a@example.test","token":["token"],"password":"password"}"#,
            500,
        )
        .await;
    guest
        .send(
            "POST",
            "/reset-password",
            r#"{"email":"a@example.test","token":123,"password":"password"}"#,
            422,
        )
        .await;

    let sent = guest
        .send(
            "POST",
            "/forgot-password",
            r#"{"email":"A@EXAMPLE.TEST","name":"ignored"}"#,
            200,
        )
        .await;
    assert_eq!(
        sent.body,
        json!({"message":"We have emailed your password reset link."})
    );
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    assert_eq!(RESET_NOTIFICATIONS.lock().unwrap()[0].0, "a@example.test");
    assert_eq!(token_count(&server, "fortify_named_resets").await, 1);
    assert_eq!(token_count(&server, "fortify_default_resets").await, 0);
    let throttle = guest.send("POST", "/forgot-password", LOGIN, 422).await;
    assert!(throttle.body["errors"]["email"].is_array());
    guest
        .send("POST", "/reset-password", &reset_body("wrong"), 422)
        .await;
    assert_eq!(token_count(&server, "fortify_named_resets").await, 1);
    server.events.lock().unwrap().clear();
    let result = guest
        .send("POST", "/reset-password", &reset_body(&token), 200)
        .await;
    assert_eq!(
        result.body,
        json!({"message":"Your password has been reset."})
    );
    assert_eq!(*server.events.lock().unwrap(), ["PasswordReset"]);
    assert_eq!(token_count(&server, "fortify_named_resets").await, 0);
    assert!(guest.send("GET", "/state", "", 200).await.body["user"].is_null());
    let row = server
        .database
        .selectOne(
            "SELECT password,remember_token FROM fortify_account_users WHERE identity='001'",
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    let Value::Text(hash) = row.get("password").unwrap() else {
        panic!()
    };
    assert!(
        server
            .hasher
            .check(b"changed:secret", hash, &HashOptions::new())
            .await
            .unwrap()
    );
    assert!(matches!(row.get("remember_token"),Some(Value::Text(v)) if v.len()==60));
    guest
        .send("POST", "/reset-password", &reset_body(&token), 422)
        .await;
    guest.send("POST", "/login", LOGIN, 422).await;
    guest
        .send(
            "POST",
            "/login",
            r#"{"email":"a@example.test","password":"changed:secret"}"#,
            200,
        )
        .await;
    guest.send("POST", "/forgot-password", LOGIN, 302).await;
}
#[tokio::test]
async fn password_reset_action_validation_and_save_failures_preserve_retryable_tokens() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let server = start(reset_settings(), false, |_| {}).await;
    let mut guest = prepare(&server).await;
    guest.send("POST", "/forgot-password", LOGIN, 200).await;
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    let mut values: Json = serde_json::from_str(&reset_body(&token)).unwrap();
    values["password_confirmation"] = json!("wrong");
    let response = guest
        .send("POST", "/reset-password", &values.to_string(), 422)
        .await;
    assert!(response.body["errors"]["password"].is_array());
    assert_eq!(token_count(&server, "fortify_named_resets").await, 1);
    assert!(
        server
            .events
            .lock()
            .unwrap()
            .iter()
            .all(|name| name != "PasswordReset")
    );
    server.database.unprepared("CREATE TRIGGER fortify_reset_fail BEFORE UPDATE ON fortify_account_users FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='save failed'").await.unwrap();
    guest
        .send("POST", "/reset-password", &reset_body(&token), 500)
        .await;
    assert_eq!(token_count(&server, "fortify_named_resets").await, 1);
    server
        .database
        .unprepared("DROP TRIGGER fortify_reset_fail")
        .await
        .unwrap();
    guest
        .send("POST", "/reset-password", &reset_body(&token), 200)
        .await;
    assert_eq!(token_count(&server, "fortify_named_resets").await, 0);
}
#[tokio::test]
async fn password_reset_views_web_responses_prefixes_and_csrf_follow_routes() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let mut config = reset_settings();
    config["prefix"] = json!("auth");
    config["paths"] = json!({"password":{"request":"/forgot","email":"/send-reset","reset":"/new/{token}","update":"/complete"}});
    config["redirects"] = json!({"password-reset":"/done"});
    let server = start(config, true, |_| {}).await;
    for (is_reset, view) in [(false, "forgot"), (true, "reset")] {
        let response = openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(
            move |request| {
                Box::pin(async move {
                    Response::json(
                        StatusCode::OK,
                        json!({"view":view,"token":request.route("token")}),
                    )
                })
            },
        ));
        if is_reset {
            server.fortify.resetPasswordView(response);
        } else {
            server.fortify.requestPasswordResetLinkView(response);
        }
    }
    let mut user = Client::new(&server);
    user.send("POST", "/auth/register", REGISTER, 201).await;
    user.send("POST", "/auth/logout", "{}", 204).await;
    let mut guest = Client::new(&server);
    assert_eq!(
        guest.send("GET", "/auth/forgot", "", 200).await.body["view"],
        "forgot"
    );
    assert_eq!(
        guest.send("GET", "/auth/new/token123", "", 200).await.body["token"],
        "token123"
    );
    guest.send("GET", "/forgot-password", "", 404).await;
    guest.fetch = false;
    guest.send("POST", "/auth/send-reset", LOGIN, 419).await;
    guest.fetch = true;
    guest.html = true;
    guest
        .send(
            "POST",
            "/auth/send-reset",
            r#"{"email":"missing@example.test"}"#,
            302,
        )
        .await;
    guest.html = false;
    assert!(
        guest.send("GET", "/state", "", 200).await.body["errors"]["default"]["email"].is_array()
    );
    RESET_NOTIFICATIONS.lock().unwrap().clear();
    guest.html = true;
    guest.send("POST", "/auth/send-reset", LOGIN, 302).await;
    guest.html = false;
    assert_eq!(
        guest.send("GET", "/state", "", 200).await.body["status"],
        "We have emailed your password reset link."
    );
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    guest.html = true;
    let done = guest
        .send("POST", "/auth/complete", &reset_body(&token), 302)
        .await;
    assert!(
        done.headers
            .contains(&format!("location: http://{}/done", server.address)),
        "{}",
        done.headers
    );
}

struct RejectReset;
impl contracts::ResetsUserPasswords for RejectReset {
    fn reset<'a>(
        &'a self,
        _: &'a mut dyn CanResetPassword,
        _: &'a RequestValue,
    ) -> HttpFuture<'a, Result<(), HttpError>> {
        Box::pin(async {
            let mut errors = openrust::support::MessageBag::default();
            errors.add("password", "Application rejected password.");
            Err(HttpError::Validation(Box::new(ValidationException::new(
                errors,
            ))))
        })
    }
}
#[tokio::test]
async fn fixed_php_password_reset_responses_and_completion_match_mysql_http() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let cases: Vec<Json> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/password_reset.json"
    ))
    .unwrap();
    for case in cases {
        let mut config = reset_settings();
        let email = case["email_field"].as_str().unwrap();
        config["email"] = json!(email);
        if let Some(redirect) = case["redirect"].as_str() {
            config["redirects"] = json!({"password-reset":redirect});
        }
        let server = start(config, true, |_| {}).await;
        let mut guest = prepare(&server).await;
        if email != "email" {
            server
                .database
                .statement(
                    "ALTER TABLE fortify_account_users ADD COLUMN contact VARCHAR(255)",
                    &[],
                )
                .await
                .unwrap();
            server
                .database
                .statement("UPDATE fortify_account_users SET contact=email", &[])
                .await
                .unwrap();
        }
        if case["status"] == "passwords.user" {
            server
                .database
                .statement("DELETE FROM fortify_account_users", &[])
                .await
                .unwrap();
        }
        let operation = case["operation"].as_str().unwrap();
        let mut body = case["request"].clone();
        // Real broker-generated tokens replace the deterministic PHP boundary token.
        if operation == "reset" && case["status"] == "passwords.reset" {
            let mut send = Json::Object(Default::default());
            send[email] = json!("a@example.test");
            guest
                .send("POST", "/forgot-password", &send.to_string(), 200)
                .await;
            body["token"] = json!(RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone());
        }
        if operation == "send" && case["status"] == "passwords.throttled" {
            guest
                .send("POST", "/forgot-password", &body.to_string(), 200)
                .await;
        }
        if case["action_error"] == true {
            server
                .fortify
                .resetUserPasswordsUsing(Arc::new(RejectReset));
        }
        server.events.lock().unwrap().clear();
        guest.html = !case["json"].as_bool().unwrap();
        // Application validation exceptions use the native Web redirect renderer.
        let status = if guest.html && case["action_error"] == true {
            302
        } else {
            case["http"].as_u64().unwrap() as u16
        };
        let response = guest
            .send(
                "POST",
                if operation == "send" {
                    "/forgot-password"
                } else {
                    "/reset-password"
                },
                &body.to_string(),
                status,
            )
            .await;
        if case["json"] == true {
            if status == 422 {
                assert_eq!(response.body["errors"], case["errors"], "{case}");
            } else {
                assert_eq!(response.body, case["body"], "{case}");
            }
        } else {
            if let Some(location) = case["location"].as_str() {
                let expected = if location.starts_with("http") {
                    location.replace("http://localhost", &format!("http://{}", server.address))
                } else {
                    format!("http://{}{location}", server.address)
                };
                if operation == "reset" && case["status"] == "passwords.reset" {
                    assert!(
                        response.headers.contains(&format!("location: {expected}")),
                        "{}",
                        response.headers
                    );
                }
            }
            guest.html = false;
            let state = guest.send("GET", "/state", "", 200).await.body;
            if let Some(message) = case["flash"].get("status") {
                assert_eq!(state["status"], *message, "{case}");
            }
            if case["errors"].as_object().is_some_and(|v| !v.is_empty()) {
                for (key, expected) in case["errors"].as_object().unwrap() {
                    let expected = if expected.is_array() {
                        expected.clone()
                    } else {
                        json!([expected])
                    };
                    assert_eq!(state["errors"]["default"][key], expected, "{case}");
                }
            }
        }
        let expected = operation == "reset"
            && case["status"] == "passwords.reset"
            && case["action_error"] == false;
        assert_eq!(
            server
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|v| v.as_str() == "PasswordReset")
                .count(),
            usize::from(expected),
            "{case}"
        );
    }
}

struct CustomResetResponse;
impl contracts::PasswordResetResponse for CustomResetResponse {}
impl contracts::PasswordResponse for CustomResetResponse {
    fn to_response<'a>(
        &'a self,
        request: &'a mut Request,
        status: openrust::auth::passwords::PasswordResetStatus,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            Response::json(
                StatusCode::ACCEPTED,
                json!({"status":status.as_str(),"guest":Auth::user(request).await?.is_none()}),
            )
        })
    }
}
struct RejectResetEvent(Arc<std::sync::atomic::AtomicBool>);
impl Listener for RejectResetEvent {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async move {
            if self.0.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(openrust::events::EventError::Listener {
                    source: Box::new(std::io::Error::other("reset listener rejected")),
                });
            }
            Ok(None)
        })
    }
}
#[tokio::test]
async fn password_reset_concurrent_token_claim_and_event_failure_keep_native_broker_semantics() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let reject = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let server = start(reset_settings(), true, |app| {
        app.container()
            .make::<openrust::events::Dispatcher>()
            .unwrap()
            .listen(
                "PasswordReset",
                Arc::new(RejectResetEvent(reject.clone())),
                openrust::events::Delivery::Sync,
            )
            .unwrap();
    })
    .await;
    let mut guest = prepare(&server).await;
    guest.send("POST", "/forgot-password", LOGIN, 200).await;
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    guest
        .send("POST", "/reset-password", &reset_body(&token), 500)
        .await;
    assert_eq!(token_count(&server, "fortify_named_resets").await, 1);
    let row = server
        .database
        .selectOne(
            "SELECT password,remember_token FROM fortify_account_users",
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    let Value::Text(hash) = row.get("password").unwrap() else {
        panic!()
    };
    assert!(
        server
            .hasher
            .check(b"changed:secret", hash, &HashOptions::new())
            .await
            .unwrap()
    );
    assert!(matches!(row.get("remember_token"),Some(Value::Text(v)) if v.len()==60));
    reject.store(false, std::sync::atomic::Ordering::Relaxed);
    server.events.lock().unwrap().clear();
    let mut left = Client::new(&server);
    let mut right = Client::new(&server);
    let body = reset_body(&token);
    let (a, b) = tokio::join!(
        left.send("POST", "/reset-password", &body, 0),
        right.send("POST", "/reset-password", &body, 0)
    );
    let successes = [
        a.headers.starts_with("HTTP/1.1 200 "),
        b.headers.starts_with("HTTP/1.1 200 "),
    ];
    assert_eq!(successes.into_iter().filter(|success| *success).count(), 1);
    assert!(a.headers.starts_with("HTTP/1.1 422 ") || b.headers.starts_with("HTTP/1.1 422 "));
    assert_eq!(token_count(&server, "fortify_named_resets").await, 0);
    assert_eq!(*server.events.lock().unwrap(), ["PasswordReset"]);
}
#[tokio::test]
async fn password_reset_feature_views_and_response_replacement_are_application_scoped() {
    if !mysql() {
        return;
    }
    let _serial = SERIAL.lock().await;
    let mut config = reset_settings();
    config["views"] = json!(false);
    let server = start(config, true, |app| {
        app.container()
            .instance::<dyn contracts::PasswordResetResponse>(Arc::new(CustomResetResponse))
            .unwrap();
    })
    .await;
    let mut guest = prepare(&server).await;
    guest.send("GET", "/forgot-password", "", 405).await;
    guest.send("GET", "/reset-password/token", "", 404).await;
    guest.send("POST", "/forgot-password", LOGIN, 200).await;
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    assert_eq!(
        guest
            .send("POST", "/reset-password", &reset_body(&token), 202)
            .await
            .body,
        json!({"status":"passwords.reset","guest":true})
    );
    drop(server);
    let mut config = reset_settings();
    config["views"] = json!(false);
    let server = start(config, true, |_| {}).await;
    let mut guest = prepare(&server).await;
    guest.send("POST", "/forgot-password", LOGIN, 200).await;
    let token = RESET_NOTIFICATIONS.lock().unwrap()[0].1.clone();
    guest.html = true;
    let response = guest
        .send("POST", "/reset-password", &reset_body(&token), 302)
        .await;
    assert!(
        response
            .headers
            .contains(&format!("location: http://{}/home", server.address)),
        "{}",
        response.headers
    );
    drop(server);
    let server = start(settings(), true, |_| {}).await;
    let mut guest = Client::new(&server);
    guest.send("POST", "/forgot-password", LOGIN, 404).await;
    guest.send("POST", "/reset-password", "{}", 404).await;
}
