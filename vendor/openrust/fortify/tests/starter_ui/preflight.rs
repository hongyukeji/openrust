use super::*;
use openrust::support::facades::{App, Config, Facade as _};

#[tokio::test]
async fn user_routes_reject_invalid_avatar_configuration_before_rewiring_services() {
    let server = start_options("full", false).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let original = Config::get("user").unwrap().unwrap();
        let features = Config::get("fortify.features").unwrap();
        for (key, value) in [
            ("user.avatar.enabled", json!("false")),
            ("user.avatar.disk", json!(12)),
            ("user.avatar.disk", json!("missing-disk")),
            ("user.avatar.max_kilobytes", json!(0)),
            ("user.avatar.max_kilobytes", json!(u64::MAX)),
            ("user.avatar.max_dimension", json!("4096")),
            ("user.avatar.max_dimension", json!(u64::MAX)),
        ] {
            Config::set("user", original.clone()).unwrap();
            Config::set(key, value).unwrap();
            assert!(
                openrust_fortify::starter::client::register_routes::<app::models::User>(app)
                    .is_err(),
                "{key}"
            );
            assert_eq!(Config::get("fortify.features").unwrap(), features);
        }
        Config::set("user", original).unwrap();
        Config::set("filesystems.disks.public.visibility", json!("private")).unwrap();
        assert!(
            openrust_fortify::starter::client::register_routes::<app::models::User>(app)
                .unwrap_err()
                .to_string()
                .contains("公开磁盘")
        );
    })
    .await;
}

#[tokio::test]
async fn user_routes_reject_nontransactional_and_cross_connection_cleanup_queues() {
    let server = start_options("api", false).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let original = Config::get("user").unwrap().unwrap();
        for (key, value) in [
            ("user.cleanup.connection", json!("redis")),
            ("user.cleanup.connection", json!("missing")),
            ("user.cleanup.connection", json!(false)),
            ("user.cleanup.queue", json!(" ")),
            ("user.cleanup.queue", json!("x".repeat(256))),
        ] {
            Config::set("user", original.clone()).unwrap();
            Config::set(key, value).unwrap();
            assert!(
                openrust_fortify::starter::client::register_routes::<app::models::User>(app)
                    .is_err(),
                "{key}"
            );
        }
        Config::set("user", original).unwrap();
        Config::set(
            "database.connections.other",
            json!({"driver":"sqlite","database":":memory:"}),
        )
        .unwrap();
        Config::set("queue.connections.database.connection", json!("other")).unwrap();
        assert!(
            openrust_fortify::starter::client::register_routes::<app::models::User>(app)
                .unwrap_err()
                .to_string()
                .contains("同一具名数据库连接")
        );
    })
    .await;
}

#[tokio::test]
async fn disabling_registration_closes_web_and_token_entry_points_in_both_profiles() {
    for profile in ["full", "api"] {
        let server = start_security_catalog_mail(
            profile,
            false,
            false,
            "api",
            false,
            Some(json!({
                "default":"array","mailers":{"array":{"transport":"array"}},
                "from":{"address":"admin@example.test","name":"验收"},"__user_registration":false
            })),
        )
        .await;
        let mut browser = Client {
            address: server.address,
            cookies: Default::default(),
            csrf: String::new(),
        };
        assert_eq!(browser.send("GET", "/login", "", false).await.0, 200);
        for path in [
            "/register",
            "/auth/google/redirect",
            "/user/passkeys",
            "/user/two-factor-authentication",
        ] {
            assert_eq!(
                browser.send("GET", path, "", false).await.0,
                404,
                "{profile}: {path}"
            );
        }
        let form = r#"{"name":"关闭注册","email":"closed@example.test","password":"long-password-123","password_confirmation":"long-password-123"}"#;
        assert_eq!(browser.send("POST", "/register", form, true).await.0, 404);
        assert_eq!(
            token_client_request(
                server.address,
                "/api/auth/register",
                serde_json::from_str(form).unwrap()
            )
            .await
            .0,
            404
        );
        assert_eq!(
            browser.send("GET", "/forgot-password", "", false).await.0,
            if profile == "full" { 200 } else { 405 }
        );
        assert_eq!(
            token_client_request(
                server.address,
                "/api/auth/forgot-password",
                json!({"email":"closed@example.test"})
            )
            .await
            .0,
            422
        );
        App::set_facade_application(Some(server._kernel.get_application()), async {
            let db = App::make::<openrust::database::DatabaseManager>()
                .await
                .unwrap()
                .connection(None)
                .unwrap();
            assert_eq!(db.table("users").count("*").await.unwrap(), 0);
        })
        .await;
        assert!(server.mailer.sent().unwrap().is_empty());
    }
}
