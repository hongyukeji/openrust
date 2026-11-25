use super::*;
use openrust::http::Request;
use openrust::support::facades::{App, Facade as _};

async fn devices_server() -> Server {
    start_security_catalog_mail("full",false,false,"api",false,Some(json!({
        "default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"devices@example.test","name":"Device test"},
        "__user_devices":{"enabled":true}
    }))).await
}
fn browser(server: &Server) -> Client {
    Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    }
}
async fn login(client: &mut Client) {
    assert_eq!(client.send("GET", "/login", "", false).await.0, 200);
    let (status, body) = client
        .send(
            "POST",
            "/login",
            r#"{"email":"device@example.test","password":"long-password-123","remember":true}"#,
            true,
        )
        .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(client.send("GET", "/home", "", false).await.0, 200);
}
async fn sessions(client: &mut Client) -> Value {
    let (status, _, body) = client
        .exchange_origin("GET", "/api/user/sessions", "", true, true)
        .await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str::<Value>(&body).unwrap()["data"].clone()
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
async fn browser_devices_list_and_persistent_revocation_reject_stale_session_writeback_and_remember()
 {
    let server = devices_server().await;
    let mut registrar = browser(&server);
    assert_eq!(registrar.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=registrar.send("POST","/register",r#"{"name":"设备用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("user_browser_sessions").count("*").await.unwrap(),
            1,
            "注册响应轮换会话ID后须登记最终设备"
        );
    })
    .await;
    assert_eq!(
        registrar.send("GET", "/home", "", false).await.0,
        200,
        "注册后的最终会话必须可认证，同时刷新表单CSRF"
    );
    let (status, body) = registrar.send("POST", "/logout", "{}", true).await;
    assert_eq!(status, 204, "{body}");
    let mut first = browser(&server);
    let mut second = browser(&server);
    login(&mut first).await;
    login(&mut second).await;
    let mut stale = Client {
        address: second.address,
        cookies: second.cookies.clone(),
        csrf: second.csrf.clone(),
    };
    let list = sessions(&mut first).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let target = list
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["is_current_device"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(target.len(), 64);
    let app = server._kernel.get_application();
    let rows = App::set_facade_application(Some(app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let rows = db.table("sessions").get().await.unwrap();
        let index = db.table("user_browser_sessions").get().await.unwrap();
        assert!(index.len() >= 2);
        for row in &index {
            let openrust::database::Value::Text(encrypted) = row.get("session_id").unwrap() else {
                panic!("missing encrypted session ID")
            };
            assert!(encrypted.len() > 40);
            assert!(!list.to_string().contains(encrypted));
        }
        for row in &rows {
            if let Some(openrust::database::Value::Text(id)) = row.get("id") {
                assert!(!list.to_string().contains(id));
            }
        }
        rows
    })
    .await;
    assert!(!list.to_string().contains("password_hash"));
    assert!(!list.to_string().contains("_token"));
    let (status, _, body) = first
        .exchange_origin(
            "DELETE",
            &format!("/api/user/sessions/{target}"),
            "{}",
            true,
            true,
        )
        .await;
    assert_eq!(status, 423, "{body}");
    confirm(&mut first).await;
    let (status, _, body) = first
        .exchange_origin(
            "DELETE",
            &format!("/api/user/sessions/{target}"),
            "{}",
            true,
            true,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_empty());
    assert_eq!(first.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(
        second
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
    // 模拟早先已开始的请求在撤销后将旧会话整行写回；设备索引的撤销点必须仍然有效。
    App::set_facade_application(Some(app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        for row in rows {
            let record = row
                .columns()
                .iter()
                .cloned()
                .collect::<openrust::database::query::Record>();
            let id = record["id"].clone();
            db.table("sessions")
                .r#where("id", "=", id)
                .delete()
                .await
                .unwrap();
            db.table("sessions").insert(&[record]).await.unwrap();
        }
    })
    .await;
    assert_eq!(
        stale
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
    let remaining = sessions(&mut first).await;
    assert_eq!(remaining.as_array().unwrap().len(), 1);
    assert_eq!(remaining[0]["is_current_device"], true);
}

#[tokio::test]
async fn browser_devices_others_require_proof_preserve_current_and_do_not_revoke_personal_tokens() {
    let server = devices_server().await;
    let mut first = browser(&server);
    assert_eq!(first.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=first.send("POST","/register",r#"{"name":"设备用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(first.send("GET", "/home", "", false).await.0, 200);
    let mut second = browser(&server);
    login(&mut second).await;
    let (status, _, token) =
        token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(status, 201, "{}", token_failure(&token));
    let token = token["data"]["token"].as_str().unwrap();
    let (status, _, body) = account_request(
        server.address,
        token,
        "GET",
        "/api/user/sessions",
        json!({}),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(
        body["data"]
            .as_array()
            .unwrap()
            .iter()
            .all(|device| device["is_current_device"] == false)
    );
    assert_eq!(
        first
            .exchange_origin("DELETE", "/api/user/sessions", "{}", true, true)
            .await
            .0,
        423
    );
    confirm(&mut first).await;
    let (status, _, body) = first
        .exchange_origin("DELETE", "/api/user/sessions", "{}", true, true)
        .await;
    assert_eq!(status, 204, "{body}");
    assert!(body.is_empty());
    assert_eq!(first.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(
        second
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
    assert_eq!(
        account_request(
            server.address,
            token,
            "GET",
            "/api/user/sessions",
            json!({})
        )
        .await
        .0,
        200,
        "撤销浏览器设备不应撤销独立PAT"
    );
    let remaining = sessions(&mut first).await;
    assert_eq!(remaining.as_array().unwrap().len(), 1);
    assert_eq!(remaining[0]["is_current_device"], true);
    let id = remaining[0]["id"].as_str().unwrap();
    let (status, _, body) = first
        .exchange_origin(
            "DELETE",
            &format!("/api/user/sessions/{id}"),
            "{}",
            true,
            true,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(
        first
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
}

#[tokio::test]
async fn browser_devices_prune_expired_records_is_idempotent_and_preserves_active_records() {
    let server = devices_server().await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let values = |id: &str, expires: i64| {
            [
                ("id".into(), openrust::database::Value::Text(id.repeat(64))),
                (
                    "owner_hash".into(),
                    openrust::database::Value::Text("o".repeat(64)),
                ),
                (
                    "session_id".into(),
                    openrust::database::Value::Text("not-serialized".into()),
                ),
                ("last_seen".into(), openrust::database::Value::Integer(0)),
                (
                    "expires_at".into(),
                    openrust::database::Value::Integer(expires),
                ),
                ("revoked_at".into(), openrust::database::Value::Null),
            ]
            .into()
        };
        db.table("user_browser_sessions")
            .insert(&[values("a", 0), values("b", i64::MAX)])
            .await
            .unwrap();
        for expected in [1, 0] {
            let mut output = openrust::console::Output::default();
            assert_eq!(
                openrust::support::facades::Artisan::call(
                    "auth:prune-browser-sessions",
                    &["--hours=0".into()],
                    &mut output
                )
                .await
                .unwrap(),
                0,
                "{}",
                output.stderr
            );
            assert!(output.stdout.contains(&expected.to_string()));
            assert_eq!(
                db.table("user_browser_sessions").count("*").await.unwrap(),
                1
            );
        }
    })
    .await;
}

struct GrantGate {
    entered: tokio::sync::Notify,
    resume: tokio::sync::Notify,
}
struct HeldResource(app::models::User);
impl From<app::models::User> for HeldResource {
    fn from(user: app::models::User) -> Self {
        Self(user)
    }
}
impl openrust::http::resources::json::JsonResource for HeldResource {
    fn to_array(&self, _: &Request) -> Result<Value, openrust::http::HttpError> {
        openrust_fortify::starter::UserResource::make(&self.0)
    }
    fn prepare_response(
        &mut self,
        request: &Request,
    ) -> Option<openrust::http::HttpFuture<'_, Result<(), openrust::http::HttpError>>> {
        if request.path().trim_start_matches('/') != "api/user/tokens" {
            return None;
        }
        Some(Box::pin(async {
            let gate = App::make::<GrantGate>().await?;
            gate.entered.notify_one();
            gate.resume.notified().await;
            Ok(())
        }))
    }
}
#[tokio::test]
async fn browser_devices_revocation_after_confirmation_claim_prevents_a_delayed_pat_grant() {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    let server = devices_server().await;
    let mut owner = browser(&server);
    assert_eq!(owner.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=owner.send("POST","/register",r#"{"name":"设备用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(owner.send("GET", "/home", "", false).await.0, 200);
    let mut revoked = browser(&server);
    login(&mut revoked).await;
    confirm(&mut owner).await;
    confirm(&mut revoked).await;
    let list = sessions(&mut owner).await;
    let target = list
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["is_current_device"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let gate = Arc::new(GrantGate {
        entered: tokio::sync::Notify::new(),
        resume: tokio::sync::Notify::new(),
    });
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        app.container().instance(gate.clone()).unwrap();
        openrust_fortify::starter::UserResource::use_resource::<app::models::User, HeldResource>(
            app,
        )
        .unwrap();
    })
    .await;
    let task = tokio::spawn(async move {
        revoked
            .exchange_origin(
                "POST",
                "/api/user/tokens",
                r#"{"device_name":"must-not-be-issued"}"#,
                true,
                true,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), gate.entered.notified())
        .await
        .unwrap();
    let (status, _, body) = owner
        .exchange_origin(
            "DELETE",
            &format!("/api/user/sessions/{target}"),
            "{}",
            true,
            true,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    gate.resume.notify_one();
    let (status, _, body) = task.await.unwrap();
    assert_eq!(status, 401, "{body}");
    App::set_facade_application(Some(app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        assert_eq!(
            db.table("personal_access_tokens").count("*").await.unwrap(),
            0,
            "已领取的确认状态不能越过设备撤销点签发PAT"
        );
    })
    .await;
}

#[tokio::test]
async fn browser_devices_shared_session_keeps_an_independent_admin_guard_and_ignores_user_id_metadata()
 {
    let server = devices_server().await;
    let mut first = browser(&server);
    assert_eq!(first.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=first.send("POST","/register",r#"{"name":"设备用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(first.send("GET", "/home", "", false).await.0, 200);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        db.get_schema_builder()
            .create("admins", |table| {
                table.big_integer("id").primary();
                table.string("email", 255);
                table.string("password", 255);
                table.remember_token();
            })
            .await
            .unwrap();
        let hash = openrust::hashing::Hash::make("long-password-123", &Default::default())
            .await
            .unwrap();
        db.table("admins")
            .insert(&[[
                ("id".into(), openrust::database::Value::Integer(99)),
                ("email".into(), "admin@example.test".into()),
                ("password".into(), hash.into()),
            ]
            .into()])
            .await
            .unwrap();
    })
    .await;
    let mut shared = browser(&server);
    login(&mut shared).await;
    let (status, body) = shared
        .send(
            "POST",
            "/admin-fixture-login",
            r#"{"password":"long-password-123"}"#,
            true,
        )
        .await;
    assert_eq!(status, 204, "{body}");
    assert_eq!(
        shared.send("GET", "/admin-fixture-user", "", true).await.0,
        200
    );
    let list = sessions(&mut first).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    let target = list
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["is_current_device"] == false)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let app = server._kernel.get_application();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let openrust::database::Value::Text(cipher) = db
            .table("user_browser_sessions")
            .r#where("id", "=", target.clone())
            .value("session_id")
            .await
            .unwrap()
        else {
            panic!("missing encrypted device record")
        };
        let raw = String::from_utf8(
            app.make::<dyn openrust::contracts::encryption::Encrypter>()
                .await
                .unwrap()
                .decrypt_string(&cipher)
                .unwrap(),
        )
        .unwrap();
        db.table("sessions")
            .r#where("id", "=", raw)
            .update([("user_id".into(), openrust::database::Value::Integer(99))].into())
            .await
            .unwrap();
    })
    .await;
    assert_eq!(
        sessions(&mut first).await.as_array().unwrap().len(),
        2,
        "不能依赖只能存一个Guard的user_id元数据漏掉本人设备"
    );
    confirm(&mut first).await;
    assert_eq!(
        first
            .exchange_origin(
                "DELETE",
                &format!("/api/user/sessions/{target}"),
                "{}",
                true,
                true
            )
            .await
            .0,
        204
    );
    let (status, body) = shared.send("GET", "/admin-fixture-user", "", true).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["id"], 99);
    assert_eq!(
        shared
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
    assert_eq!(
        shared.send("GET", "/admin-fixture-user", "", true).await.0,
        200,
        "只撤销实际web Guard，不能退出共享浏览器中的独立后台身份"
    );
}

#[tokio::test]
async fn browser_devices_html_forms_keep_csrf_recent_confirmation_redirects_and_localized_flash() {
    let server = devices_server().await;
    let mut first = browser(&server);
    assert_eq!(first.send("GET", "/register", "", false).await.0, 200);
    let(status,body)=first.send("POST","/register",r#"{"name":"设备用户","email":"device@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#,true).await;
    assert_eq!(status, 201, "{body}");
    let (status, page) = first.send("GET", "/home", "", false).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("浏览器设备"));
    let mut other = browser(&server);
    login(&mut other).await;
    let (status, page) = first.send("GET", "/user/sessions", "", false).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("当前浏览器"));
    assert!(page.contains("最近活动"));
    assert!(!page.contains("user.browser_sessions"));
    let csrf = std::mem::take(&mut first.csrf);
    assert_eq!(
        form_request(&mut first, "/user/sessions", "_method=DELETE")
            .await
            .0,
        419
    );
    first.csrf = csrf;
    let (status, headers, _) = form_request(&mut first, "/user/sessions", "_method=DELETE").await;
    assert_eq!(status, 302);
    assert!(headers.contains("/user/confirm-password"));
    assert_eq!(
        first
            .send("GET", "/user/confirm-password", "", false)
            .await
            .0,
        200
    );
    let (status, headers, body) = first
        .exchange(
            "POST",
            "/user/confirm-password",
            r#"{"password":"long-password-123"}"#,
            false,
        )
        .await;
    assert_eq!(status, 302, "{body}");
    assert!(headers.to_ascii_lowercase().contains("location:"));
    assert_eq!(first.send("GET", "/user/sessions", "", false).await.0, 200);
    let (status, headers, body) =
        form_request(&mut first, "/user/sessions", "_method=DELETE").await;
    assert_eq!(status, 302, "{body}");
    assert!(headers.contains("/user/sessions"));
    let (status, page) = first.send("GET", "/user/sessions", "", false).await;
    assert_eq!(status, 200, "{page}");
    assert!(page.contains("浏览器设备已更新。"));
    assert!(!page.contains("user.sessions_updated"));
    assert_eq!(
        other
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        401
    );
    assert_eq!(sessions(&mut first).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn browser_devices_reject_unsupported_or_custom_handlers_and_disabled_routes_are_absent() {
    for (driver, replacement) in [
        ("array", false),
        ("file", false),
        ("database", true),
        ("redis", true),
    ] {
        let root = Root(
            std::env::temp_dir().join(format!("device-capability-{}-{driver}", std::process::id())),
        );
        std::fs::create_dir_all(&root.0).unwrap();
        std::fs::write(
            root.0.join("driver.json"),
            serde_json::to_string(driver).unwrap(),
        )
        .unwrap();
        let mut app=Application::configure(root.0.clone()).with_config(|base,_| {
            let driver:Value=serde_json::from_slice(&std::fs::read(base.join("driver.json")).unwrap()).unwrap();
            Ok([
            ("app".into(),json!({"key":"p".repeat(32),"url":"http://localhost","name":"Capability acceptance"})),
            ("user".into(),json!({"profile":"full","devices":{"enabled":true},"avatar":{"enabled":false},"email_verification":false})),
            ("auth".into(),json!({"defaults":{"guard":"web"},"guards":{"web":{"driver":"session","provider":"users"}},"providers":{"users":{"driver":"eloquent","model":"StarterUser"}}})),
            ("database".into(),json!({"default":"capability","connections":{"capability":{"driver":"sqlite","database":":memory:"}},"redis":{"default":{"host":"127.0.0.1","port":6379,"database":0}}})),
            ("queue".into(),json!({"default":"database","connections":{"database":{"driver":"database"}}})),
            ("cache".into(),json!({"default":"array","stores":{"array":{"driver":"array"},"redis":{"driver":"redis","connection":"default"}}})),
            ("session".into(),json!({"driver":driver,"lottery":[0,100]})),
        ].into())}).with_providers(vec![Box::new(openrust_fortify::FortifyServiceProvider)]).create().unwrap();
        app.boot().await.unwrap();
        app.make::<AuthManager>()
            .await
            .unwrap()
            .register_sanctum_model::<app::models::User>("StarterUser");
        assert_eq!(
            app.make::<openrust::session::SessionManager>()
                .await
                .unwrap()
                .get_session_config()["driver"],
            driver
        );
        if replacement {
            app.make::<openrust::session::SessionManager>()
                .await
                .unwrap()
                .extend(driver, |_, _| {
                    Ok(openrust::session::ArraySessionHandler::new(
                        std::time::Duration::from_secs(120),
                    ))
                })
                .unwrap();
        }
        App::set_facade_application(Some(&app), async {
            let error =
                openrust_fortify::starter::client::register_routes::<app::models::User>(&app)
                    .unwrap_err()
                    .to_string();
            assert!(error.contains("user.devices.enabled"), "{driver}: {error}");
        })
        .await;
    }
    let server = start().await;
    let mut guest = browser(&server);
    assert_eq!(guest.send("GET", "/user/sessions", "", false).await.0, 404);
    assert_eq!(
        guest
            .exchange_origin("GET", "/api/user/sessions", "", true, true)
            .await
            .0,
        404
    );
}
