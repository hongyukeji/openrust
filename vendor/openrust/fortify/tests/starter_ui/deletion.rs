use super::*;
use openrust::{
    database::eloquent::Model,
    support::facades::{App, Facade as _},
};

async fn server() -> Server {
    let server = start_options("full", false).await;
    resource_cleanup::schema_and_disk(&server).await;
    register_token_user(&server).await;
    server
}
async fn token(server: &Server) -> String {
    let result = token_client_request(server.address, "/api/auth/token", device_login()).await;
    assert_eq!(result.0, 201, "{}", result.2);
    result.2["data"]["token"].as_str().unwrap().into()
}
#[tokio::test]
async fn deletion_requires_proof_then_atomically_removes_user_and_all_tokens() {
    let server = server().await;
    let first = token(&server).await;
    let second = token(&server).await;
    assert_eq!(
        account_request(server.address, &first, "DELETE", "/api/user", json!({}))
            .await
            .0,
        401
    );
    assert_eq!(bearer_request(server.address, &first).await, 200);
    let wrong = confirmation(&server, &first, "avatar").await;
    assert_eq!(
        account_request(
            server.address,
            &first,
            "DELETE",
            "/api/user",
            json!({"proof":wrong})
        )
        .await
        .0,
        401
    );
    let proof = confirmation(&server, &first, "delete").await;
    let result = account_request(
        server.address,
        &first,
        "DELETE",
        "/api/user",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(result.0, 204, "{}", result.1);
    assert_eq!(bearer_request(server.address, &first).await, 401);
    assert_eq!(bearer_request(server.address, &second).await, 401);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let connection = db.connection(None).unwrap();
        assert_eq!(connection.table("users").count("*").await.unwrap(), 0);
        assert_eq!(
            connection
                .table("personal_access_tokens")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .table("user_resource_cleanups")
                .count("*")
                .await
                .unwrap(),
            0
        );
    })
    .await;
}
struct NoOp;
impl openrust_fortify::starter::deletion::DeletesUsers<app::models::User> for NoOp {
    fn delete<'a>(
        &'a self,
        _: &'a mut app::models::User,
        _: &'a mut openrust::database::Transaction,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
}
#[tokio::test]
async fn deletion_rejects_noop_action_and_rolls_back_credential_revocation() {
    let server = server().await;
    let first = token(&server).await;
    openrust_fortify::starter::deletion::delete_users_using::<app::models::User>(
        server._kernel.get_application(),
        Arc::new(NoOp),
    )
    .unwrap();
    let proof = confirmation(&server, &first, "delete").await;
    assert_eq!(
        account_request(
            server.address,
            &first,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        )
        .await
        .0,
        500
    );
    assert_eq!(bearer_request(server.address, &first).await, 200);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let user = app::models::User::find(1).await.unwrap().unwrap();
        assert_eq!(
            user.get_attribute("credential_version"),
            Some(&openrust::database::Value::Integer(0))
        );
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        assert_eq!(
            db.connection(None)
                .unwrap()
                .table("personal_access_tokens")
                .count("*")
                .await
                .unwrap(),
            1
        );
    })
    .await;
}

#[tokio::test]
async fn deletion_returns_receipt_that_survives_user_deletion_and_native_worker_cleanup() {
    let server = avatar::server().await;
    let first = token(&server).await;
    let path = format!("profile-photos/{}.png", "d".repeat(64));
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let disk = openrust::support::facades::Storage::disk(Some("public")).unwrap();
        assert!(disk.put(&path, b"previous-photo".to_vec()).await.unwrap());
        let mut user = app::models::User::find(1).await.unwrap().unwrap();
        user.force_fill(
            [
                ("profile_photo_path".into(), path.clone().into()),
                ("profile_photo_disk".into(), "public".into()),
            ]
            .into(),
        )
        .unwrap();
        user.save().await.unwrap();
    })
    .await;
    let proof = confirmation(&server, &first, "delete").await;
    let result = account_request(
        server.address,
        &first,
        "DELETE",
        "/api/user",
        json!({"proof":proof}),
    )
    .await;
    assert_eq!(result.0, 202, "{}", result.1);
    let ticket = &result.2["meta"]["cleanup"];
    let id = ticket["id"].as_str().unwrap();
    let receipt = ticket["receipt"].as_str().unwrap();
    assert_eq!(bearer_request(server.address, &first).await, 401);
    assert_eq!(
        avatar::receipt(&server, id, Some(receipt), false).await.2["data"]["status"],
        "pending"
    );
    App::set_facade_application(Some(server._kernel.get_application()), async {
        assert_eq!(resource_cleanup::worker().await, 0);
        assert!(
            !openrust::support::facades::Storage::disk(Some("public"))
                .unwrap()
                .exists(&path)
                .await
                .unwrap()
        );
    })
    .await;
    assert_eq!(
        avatar::receipt(&server, id, Some(receipt), false).await.2["data"]["status"],
        "completed"
    );
}

pub(super) fn login_other_guard(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::contracts::{auth::StatefulGuard as _, session::Session as _};
    Box::pin(async move {
        let user = app::models::User::find(1).await?.unwrap();
        openrust::auth::Auth::session_guard(request, Some("second"))?
            .login(Box::new(user), true)
            .await?;
        request.session_mut()?.put("cart", json!(["retained"]))?;
        Ok(openrust::http::Response::no_content())
    })
}
pub(super) fn guard_state(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    use openrust::contracts::{auth::Guard as _, session::Session as _};
    Box::pin(async move {
        let second = openrust::auth::Auth::session_guard(request, Some("second"))?
            .check()
            .await?;
        let admin = openrust::auth::Auth::session_guard(request, Some("admin"))?
            .check()
            .await?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            json!({"second":second,"admin":admin,"cart":request.session()?.get("cart"),"session_id":request.session()?.get_id()}),
        )
    })
}
#[tokio::test]
async fn deletion_cookie_clears_all_same_user_guards_but_preserves_other_principal_and_cart() {
    let server = server().await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        use openrust::support::facades::{Config, Hash};
        Config::set(
            "auth.guards.second",
            json!({"driver":"session","provider":"users"}),
        )
        .unwrap();
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let c = db.connection(None).unwrap();
        c.get_schema_builder()
            .create("admins", |table| {
                table.big_integer("id").primary();
                table.string("email", 255);
                table.string("password", 255);
                table.string("remember_token", 100).nullable();
            })
            .await
            .unwrap();
        let password = Hash::make("admin-password-123", &openrust::hashing::HashOptions::new())
            .await
            .unwrap();
        c.table("admins")
            .insert(&[[
                ("id".into(), 1.into()),
                ("email".into(), "admin@example.test".into()),
                ("password".into(), password.into()),
            ]
            .into()])
            .await
            .unwrap();
    })
    .await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    browser.send("GET", "/login", "", false).await;
    assert_eq!(
        browser
            .send(
                "POST",
                "/login",
                r#"{"email":"device@example.test","password":"long-password-123","remember":true}"#,
                true
            )
            .await
            .0,
        200
    );
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(
        browser
            .send("POST", "/fixture-other-guard", "{}", true)
            .await
            .0,
        204
    );
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    assert_eq!(
        browser
            .send(
                "POST",
                "/admin-fixture-login",
                r#"{"password":"admin-password-123"}"#,
                true
            )
            .await
            .0,
        204
    );
    assert_eq!(browser.send("GET", "/home", "", false).await.0, 200);
    let before: Value = serde_json::from_str(
        &browser
            .send("GET", "/fixture-guard-state", "", true)
            .await
            .1,
    )
    .unwrap();
    assert_eq!(before["second"], true);
    assert_eq!(before["admin"], true);
    assert_eq!(browser.send("DELETE", "/user", "{}", true).await.0, 423);
    assert_eq!(
        browser
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let csrf = std::mem::take(&mut browser.csrf);
    assert_eq!(browser.send("DELETE", "/user", "{}", true).await.0, 419);
    browser.csrf = csrf;
    assert_eq!(browser.send("DELETE", "/user", "{}", true).await.0, 204);
    let after = browser.send("GET", "/fixture-guard-state", "", true).await;
    assert_eq!(after.0, 200, "{}", after.1);
    let after: Value = serde_json::from_str(&after.1).unwrap();
    assert_eq!(after["second"], false);
    assert_eq!(after["admin"], true);
    assert_eq!(after["cart"], json!(["retained"]));
    assert_ne!(after["session_id"], before["session_id"]);
}

#[tokio::test]
async fn deletion_revokes_database_device_index_before_post_middleware_can_restore_it() {
    let server=start_security_catalog_mail("full",false,false,"api",false,Some(json!({
        "default":"array","mailers":{"array":{"transport":"array"}},"from":{"address":"devices@example.test","name":"Delete test"},"__user_devices":{"enabled":true}
    }))).await;
    resource_cleanup::schema_and_disk(&server).await;
    register_token_user(&server).await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    browser.send("GET", "/login", "", false).await;
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
    browser.send("GET", "/home", "", false).await;
    assert_eq!(
        browser
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let response = browser.send("DELETE", "/user", "{}", true).await;
    assert_eq!(response.0, 204, "{}", response.1);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let connection = db.connection(None).unwrap();
        assert_eq!(
            connection
                .table("user_browser_sessions")
                .where_null("revoked_at")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert!(
            connection
                .table("user_browser_sessions")
                .count("*")
                .await
                .unwrap()
                > 0
        );
    })
    .await;
    assert_eq!(browser.send("GET", "/home", "", true).await.0, 401);
}

#[tokio::test]
async fn deletion_foreign_dependency_refusal_rolls_back_tokens_and_preserves_photo() {
    let server = avatar::server().await;
    let first = token(&server).await;
    let path = format!("profile-photos/{}.png", "e".repeat(64));
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let conn = db.connection(None).unwrap();
        conn.get_schema_builder()
            .create("user_dependencies", |table| {
                table.unsigned_big_integer("user_id");
                table
                    .foreign(&["user_id"], None)
                    .references("id")
                    .on("users")
                    .restrict_on_delete();
            })
            .await
            .unwrap();
        conn.table("user_dependencies")
            .insert(&[[("user_id".into(), 1.into())].into()])
            .await
            .unwrap();
        assert!(
            openrust::support::facades::Storage::disk(Some("public"))
                .unwrap()
                .put(&path, b"existing-photo".to_vec())
                .await
                .unwrap()
        );
        let mut user = app::models::User::find(1).await.unwrap().unwrap();
        user.force_fill(
            [
                ("profile_photo_path".into(), path.clone().into()),
                ("profile_photo_disk".into(), "public".into()),
            ]
            .into(),
        )
        .unwrap();
        user.save().await.unwrap();
    })
    .await;
    let proof = confirmation(&server, &first, "delete").await;
    assert_eq!(
        account_request(
            server.address,
            &first,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        )
        .await
        .0,
        500
    );
    assert_eq!(bearer_request(server.address, &first).await, 200);
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let user = app::models::User::find(1).await.unwrap().unwrap();
        assert_eq!(
            user.get_attribute("credential_version"),
            Some(&openrust::database::Value::Integer(0))
        );
        let db = server
            ._kernel
            .get_application()
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap();
        let conn = db.connection(None).unwrap();
        assert_eq!(
            conn.table("user_resource_cleanups")
                .count("*")
                .await
                .unwrap(),
            0
        );
        assert_eq!(conn.table("jobs").count("*").await.unwrap(), 0);
        assert!(
            openrust::support::facades::Storage::disk(Some("public"))
                .unwrap()
                .exists(&path)
                .await
                .unwrap()
        );
    })
    .await;
}

struct RefusesDeletion;
impl openrust_fortify::starter::deletion::DeletesUsers<app::models::User> for RefusesDeletion {
    fn preflight<'a>(
        &'a self,
        _: &'a app::models::User,
        _: &'a mut openrust::database::Transaction,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        Box::pin(async {
            Err(openrust::http::HttpError::AuthorizationDenied("业务依赖拒绝注销".into()).into())
        })
    }
    fn delete<'a>(
        &'a self,
        _: &'a mut app::models::User,
        _: &'a mut openrust::database::Transaction,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        Box::pin(async { panic!("预检拒绝后不能调用删除Action") })
    }
}
#[tokio::test]
async fn deletion_action_preflight_failure_never_revokes_credentials() {
    let server = server().await;
    let first = token(&server).await;
    openrust_fortify::starter::deletion::delete_users_using::<app::models::User>(
        server._kernel.get_application(),
        Arc::new(RefusesDeletion),
    )
    .unwrap();
    let proof = confirmation(&server, &first, "delete").await;
    assert_eq!(
        account_request(
            server.address,
            &first,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        )
        .await
        .0,
        403
    );
    assert_eq!(bearer_request(server.address, &first).await, 200);
}
#[tokio::test]
async fn deletion_html_form_returns_public_no_store_receipt_page_after_removing_account() {
    let server = avatar::server().await;
    App::set_facade_application(Some(server._kernel.get_application()), async {
        let path = format!("profile-photos/{}.png", "f".repeat(64));
        assert!(
            openrust::support::facades::Storage::disk(Some("public"))
                .unwrap()
                .put(&path, b"existing-photo".to_vec())
                .await
                .unwrap()
        );
        let mut user = app::models::User::find(1).await.unwrap().unwrap();
        user.force_fill(
            [
                ("profile_photo_path".into(), path.into()),
                ("profile_photo_disk".into(), "public".into()),
            ]
            .into(),
        )
        .unwrap();
        user.save().await.unwrap();
    })
    .await;
    let mut browser = Client {
        address: server.address,
        cookies: Default::default(),
        csrf: String::new(),
    };
    browser.send("GET", "/login", "", false).await;
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
    browser.send("GET", "/home", "", false).await;
    assert_eq!(
        form_request(&mut browser, "/user", "_method=DELETE")
            .await
            .0,
        302
    );
    assert_eq!(
        browser
            .send(
                "POST",
                "/user/confirm-password",
                r#"{"password":"long-password-123"}"#,
                true
            )
            .await
            .0,
        201
    );
    let response = form_request(&mut browser, "/user", "_method=DELETE").await;
    assert_eq!(response.0, 202, "{}", response.2);
    assert!(response.1.contains("cache-control: no-store"));
    assert!(response.1.contains("referrer-policy: no-referrer"));
    assert!(response.2.contains("账户已注销"));
    assert!(response.2.contains("data-receipt="));
    assert!(response.2.contains("X-Operation-Receipt"));
    assert!(!response.2.contains("?receipt="));
    assert_eq!(browser.send("GET", "/home", "", true).await.0, 401);
}

#[tokio::test]
async fn deletion_concurrent_requests_with_one_proof_have_only_one_committed_winner() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "delete").await;
    let (a, b) = tokio::join!(
        account_request(
            server.address,
            &token,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        ),
        account_request(
            server.address,
            &token,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        )
    );
    let mut statuses = [a.0, b.0];
    statuses.sort();
    assert_eq!(statuses, [204, 401]);
}
struct ExpiresAfterDelete {
    clock: Arc<openrust::support::DateFactory>,
    at: jiff::Timestamp,
}
impl openrust_fortify::starter::deletion::DeletesUsers<app::models::User> for ExpiresAfterDelete {
    fn delete<'a>(
        &'a self,
        user: &'a mut app::models::User,
        tx: &'a mut openrust::database::Transaction,
    ) -> openrust::auth::AuthFuture<'a, openrust::auth::AuthResult<()>> {
        use openrust_fortify::starter::deletion::DeleteUser;
        Box::pin(async move {
            DeleteUser.delete(user, tx).await?;
            self.clock
                .set_test_now(Some(openrust::support::Date::from_utc(
                    self.at,
                    jiff::tz::TimeZone::UTC,
                )));
            Ok(())
        })
    }
}
#[tokio::test]
async fn deletion_rechecks_proof_after_action_and_rolls_back_if_it_expired() {
    let server = server().await;
    let token = token(&server).await;
    let proof = confirmation(&server, &token, "delete").await;
    let app = server._kernel.get_application();
    let clock = app.make::<openrust::support::DateFactory>().await.unwrap();
    let at = App::set_facade_application(Some(app), async {
        openrust::support::facades::Date::now(None)
            .unwrap()
            .as_date_time()
            .timestamp()
            .checked_add(jiff::SignedDuration::from_secs(10801))
            .unwrap()
    })
    .await;
    openrust_fortify::starter::deletion::delete_users_using::<app::models::User>(
        app,
        Arc::new(ExpiresAfterDelete {
            clock: clock.clone(),
            at,
        }),
    )
    .unwrap();
    assert_eq!(
        account_request(
            server.address,
            &token,
            "DELETE",
            "/api/user",
            json!({"proof":proof})
        )
        .await
        .0,
        401
    );
    clock.set_test_now(None);
    assert_eq!(bearer_request(server.address, &token).await, 200);
}
