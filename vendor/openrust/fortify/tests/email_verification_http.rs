include!("support/accounts.rs");

fn email_settings() -> Json {
    let mut config = settings();
    config["features"]
        .as_array_mut()
        .unwrap()
        .push(json!("email-verification"));
    config["limiters"] = json!({"verification":"100,1"});
    config
}
async fn signup(client: &mut Client) {
    client.send("POST", "/register", REGISTER, 201).await;
}
fn path(wire: Wire) -> String {
    let url = wire.body["url"].as_str().unwrap();
    url[url.find("://").unwrap() + 3..]
        .find('/')
        .map(|i| url[url.find("://").unwrap() + 3 + i..].to_owned())
        .unwrap()
}
#[tokio::test]
async fn signed_email_verification_uses_native_mysql_sessions_and_rejects_wrong_identity_hash_and_expiry()
 {
    let _serial = SERIAL.lock().await;
    if !mysql() {
        return;
    }
    NOTIFICATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    let server = start(email_settings(), true, |_| {}).await;
    server.fortify.verify_email_view(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"verification_view":true})) })
        })),
    );
    let mut guest = Client::new(&server);
    guest.send("GET", "/email/verify", "", 401).await;
    let mut client = Client::new(&server);
    signup(&mut client).await;
    assert_eq!(
        client.send("GET", "/email/verify", "", 200).await.body["verification_view"],
        true
    );
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 1);
    client
        .send("POST", "/email/verification-notification", "{}", 202)
        .await;
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 2);
    for query in ["?id=1", "?email=other@example.test", "?expired=1"] {
        let url = path(
            client
                .send("GET", &format!("/verification-link{query}"), "", 200)
                .await,
        );
        client.send("GET", &url, "", 403).await;
    }
    let url = path(client.send("GET", "/verification-link", "", 200).await);
    client
        .send("GET", &url.replace("signature=", "signature=0"), "", 403)
        .await;
    assert!(
        !server
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v == "Verified")
    );
    client.send("GET", &url, "", 204).await;
    let verified = server
        .database
        .select_one(
            "SELECT email_verified_at FROM fortify_account_users WHERE identity='001'",
            &[],
        )
        .await
        .unwrap()
        .unwrap();
    assert_ne!(verified.get("email_verified_at"), Some(&Value::Null));
    client.send("GET", &url, "", 204).await;
    assert_eq!(
        server
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|v| *v == "Verified")
            .count(),
        1
    );
    client
        .send("POST", "/email/verification-notification", "{}", 204)
        .await;
    assert_eq!(NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed), 2);
    client.html = true;
    assert!(
        client
            .send("GET", "/email/verify", "", 302)
            .await
            .headers
            .lines()
            .any(|line| line.starts_with("location: ") && line.ends_with("/home"))
    );
    assert!(
        client
            .send("GET", &url, "", 302)
            .await
            .headers
            .lines()
            .any(|line| line.starts_with("location: ") && line.ends_with("/home?verified=1"))
    );
    client.send("GET", "/intended", "", 200).await;
    assert!(
        client
            .send("GET", &url, "", 302)
            .await
            .headers
            .lines()
            .any(|line| line.starts_with("location: ") && line.ends_with("/account/settings"))
    );
    client.html = false;
    client.send("POST", "/logout", "{}", 204).await;
    client.send("POST", "/login", LOGIN, 200).await;
    client
        .send("POST", "/email/verification-notification", "{}", 204)
        .await;
    server.close().await;
}

#[tokio::test]
async fn email_verification_preserves_save_failure_csrf_throttling_paths_and_feature_gates() {
    let _serial = SERIAL.lock().await;
    if !mysql() {
        return;
    }
    let mut config = email_settings();
    config["prefix"] = json!("auth");
    config["views"] = json!(false);
    config["paths"] = json!({"verification":{"send":"/resend","verify":"/verify/{id}/{hash}"}});
    let server = start(config, true, |_| {}).await;
    let mut client = Client::new(&server);
    client.send("POST", "/auth/register", REGISTER, 201).await;
    client.send("GET", "/auth/email/verify", "", 404).await;
    client.fetch = false;
    client.send("POST", "/auth/resend", "{}", 419).await;
    client.fetch = true;
    client.send("POST", "/auth/resend", "{}", 202).await;
    let url = path(client.send("GET", "/verification-link", "", 200).await);
    assert!(url.starts_with("/auth/verify/001/"));
    server.database.unprepared("CREATE TRIGGER reject_email BEFORE UPDATE ON fortify_account_users FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='reject verification'").await.unwrap();
    client.send("GET", &url, "", 500).await;
    assert!(
        !server
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|v| v == "Verified")
    );
    server
        .database
        .unprepared("DROP TRIGGER reject_email")
        .await
        .unwrap();
    client.send("GET", &url, "", 204).await;
    server.close().await;
    let server = start(
        json!({"features":["registration","email-verification"],"lowercase_usernames":true,"limiters":{"verification":"1,1"}}),
        true,
        |_| {},
    )
    .await;
    let mut client = Client::new(&server);
    signup(&mut client).await;
    client
        .send("POST", "/email/verification-notification", "{}", 202)
        .await;
    client
        .send("POST", "/email/verification-notification", "{}", 429)
        .await;
    server.close().await;
    let server = start(settings(), true, |_| {}).await;
    let mut client = Client::new(&server);
    signup(&mut client).await;
    client.send("GET", "/email/verify", "", 404).await;
    client
        .send("POST", "/email/verification-notification", "{}", 404)
        .await;
    server.close().await;
}

#[tokio::test]
async fn fixed_php_email_verification_controllers_and_responses_match_native_http_effects() {
    let _serial = SERIAL.lock().await;
    if !mysql() {
        return;
    }
    let fixture: Json = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/api-f01/fixtures/email_verification.json"
    ))
    .unwrap();
    let server = start(email_settings(), true, |_| {}).await;
    server.fortify.verify_email_view(
        openrust_fortify::http::responses::SimpleViewResponse::callback(Arc::new(|_| {
            Box::pin(async { Response::json(StatusCode::OK, json!({"verification_view":true})) })
        })),
    );
    let mut client = Client::new(&server);
    signup(&mut client).await;
    for case in fixture["authorization"].as_array().unwrap() {
        if case["id"] == "" {
            continue;
        } // An empty primary key does not match the native required route segment.
        let url = path(
            client
                .send(
                    "GET",
                    &format!(
                        "/verification-link?id={}&hash={}",
                        case["id"].as_str().unwrap(),
                        case["hash"].as_str().unwrap()
                    ),
                    "",
                    200,
                )
                .await,
        );
        let status = if case["authorized"] == true { 204 } else { 403 };
        client.send("GET", &url, "", status).await;
    }
    for case in fixture["cases"].as_array().unwrap() {
        let initial = if case["verified"] == true {
            Value::Text("2026-11-20 00:00:00".into())
        } else {
            Value::Null
        };
        server
            .database
            .statement(
                "UPDATE fortify_account_users SET email_verified_at=?",
                &[initial],
            )
            .await
            .unwrap();
        MARK_EMAIL_RETURNS_FALSE.store(case["save"] == false, std::sync::atomic::Ordering::Relaxed);
        let failure = case["save"] == "throw";
        if failure {
            server.database.unprepared("CREATE TRIGGER reject_email BEFORE UPDATE ON fortify_account_users FOR EACH ROW SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT='reject verification'").await.unwrap();
        }
        client.html = case["json"] == false;
        if case["intended"].is_string() {
            client.send("GET", "/intended", "", 200).await;
        }
        server.events.lock().unwrap().clear();
        let count = NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed);
        let (method, url) = match case["action"].as_str().unwrap() {
            "prompt" => ("GET", "/email/verify".into()),
            "send" => ("POST", "/email/verification-notification".into()),
            "verify" => (
                "GET",
                path(client.send("GET", "/verification-link", "", 200).await),
            ),
            _ => unreachable!(),
        };
        let response = client
            .send(
                method,
                &url,
                if method == "POST" { "{}" } else { "" },
                case["status"].as_u64().unwrap() as u16,
            )
            .await;
        if let Some(location) = case["location"].as_str() {
            assert!(
                response
                    .headers
                    .lines()
                    .any(|line| line.starts_with("location: ") && line.ends_with(location)),
                "{case}: {}",
                response.headers
            );
        }
        assert_eq!(
            NOTIFICATIONS.load(std::sync::atomic::Ordering::Relaxed) - count,
            case["notifications"].as_u64().unwrap() as usize,
            "{case}"
        );
        let row = server
            .database
            .select_one("SELECT email_verified_at FROM fortify_account_users", &[])
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.get("email_verified_at") != Some(&Value::Null),
            case["after_verified"].as_bool().unwrap(),
            "{case}"
        );
        assert_eq!(
            json!(*server.events.lock().unwrap()),
            case["events"],
            "{case}"
        );
        if case["flash"].is_object() {
            client.html = false;
            assert_eq!(
                client.send("GET", "/state", "", 200).await.body["status"],
                case["flash"]["status"]
            );
        }
        if failure {
            server
                .database
                .unprepared("DROP TRIGGER reject_email")
                .await
                .unwrap();
        }
    }
    MARK_EMAIL_RETURNS_FALSE.store(false, std::sync::atomic::Ordering::Relaxed);
    server.close().await;
}

struct UpdatedVerificationResponse;
impl contracts::VerifyEmailResponse for UpdatedVerificationResponse {}
impl contracts::Response for UpdatedVerificationResponse {
    fn to_response<'a>(
        &'a self,
        request: &'a mut Request,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let user = Auth::user(request).await?.unwrap();
            Response::json(
                StatusCode::OK,
                json!({"verified":user.as_email_verification().unwrap().has_verified_email()}),
            )
        })
    }
}
struct RejectVerified;
impl Listener for RejectVerified {
    fn handle<'a>(
        &'a self,
        _: &'a dyn Event,
        _: &'a mut openrust::events::EventContext<'_>,
    ) -> openrust::events::EventFuture<
        'a,
        openrust::events::EventResult<Option<openrust::events::ListenerResponse>>,
    > {
        Box::pin(async {
            Err(openrust::events::EventError::Listener {
                source: Box::new(std::io::Error::other("verified listener rejected")),
            })
        })
    }
}
#[tokio::test]
async fn verification_response_replacements_observe_updated_user_and_event_failure_keeps_saved_email()
 {
    let _serial = SERIAL.lock().await;
    if !mysql() {
        return;
    }
    let server = start(email_settings(), true, |app| {
        app.container()
            .instance::<dyn contracts::VerifyEmailResponse>(Arc::new(UpdatedVerificationResponse))
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    signup(&mut client).await;
    let url = path(client.send("GET", "/verification-link", "", 200).await);
    assert_eq!(
        client.send("GET", &url, "", 200).await.body,
        json!({"verified":true})
    );
    server.close().await;
    let server = start(email_settings(), true, |app| {
        app.container()
            .make::<openrust::events::Dispatcher>()
            .unwrap()
            .listen(
                "Verified",
                Arc::new(RejectVerified),
                openrust::events::Delivery::Sync,
            )
            .unwrap();
    })
    .await;
    let mut client = Client::new(&server);
    signup(&mut client).await;
    let url = path(client.send("GET", "/verification-link", "", 200).await);
    client.send("GET", &url, "", 500).await;
    let row = server
        .database
        .select_one("SELECT email_verified_at FROM fortify_account_users", &[])
        .await
        .unwrap()
        .unwrap();
    assert_ne!(row.get("email_verified_at"), Some(&Value::Null));
    client.send("GET", &url, "", 204).await;
    server.close().await;
}
