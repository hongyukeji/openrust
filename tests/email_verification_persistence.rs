use openrust::{
    container::Callback,
    contracts::auth::MustVerifyEmail,
    database::{DatabaseManager, Value, eloquent::Model},
    testing::TestApp,
};
use serde_json::json;

#[path = "../app/models/mod.rs"]
mod models;

#[tokio::test]
async fn stock_user_keeps_conditional_verification_on_its_named_connection() {
    let mut fixture = TestApp::create(|builder| {
        builder.with_config(|_, _| {
            let driver = std::env::var("LAYERING_TEST_DRIVER").unwrap_or("sqlite".into());
            let mut database = match driver.as_str() {
                "sqlite" => json!({"driver":"sqlite","database":":memory:"}),
                "mysql" => json!({"url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap()}),
                "pgsql" => json!({"url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap()}),
                _ => panic!("未知验收数据库"),
            };
            database["prefix"] = json!("layer_");
            Ok([(
                "database".into(),
                json!({
                    "default":"unrelated",
                    "connections":{
                        "unrelated":{"driver":"sqlite","database":":memory:"},
                        "verification":database
                    }
                }),
            )]
            .into())
        })
    })
    .unwrap();
    fixture.boot().await.unwrap();
    let connection = fixture
        .app()
        .make::<DatabaseManager>()
        .await
        .unwrap()
        .connection(Some("verification"))
        .unwrap();
    connection
        .get_schema_builder()
        .create("users", |table| {
            table.big_integer("id").primary();
            table.string("email", 255);
            table.timestamp("email_verified_at", None).nullable();
            table.timestamp("updated_at", None).nullable();
        })
        .await
        .unwrap();
    connection
        .table("users")
        .insert(&[[
            ("id".into(), Value::Integer(7)),
            ("email".into(), "Member@example.test".into()),
        ]
        .into()])
        .await
        .unwrap();

    fixture
        .app()
        .call(
            Callback::new(&[], |_| async {
                let mut user =
                    models::User::new_instance_on(&connection, Default::default()).unwrap();
                user.force_fill(
                    [
                        (models::User::ID.into(), Value::Integer(7)),
                        (models::User::EMAIL.into(), "Member@example.test".into()),
                    ]
                    .into(),
                )
                .unwrap();
                assert!(user.mark_email_as_verified().await.unwrap());
                assert!(user.has_verified_email());
                let verified = user.state().get_attributes().clone();
                assert!(!user.mark_email_as_verified().await.unwrap());
                assert_eq!(user.state().get_attributes(), &verified);
                let row = connection.table("users").first().await.unwrap().unwrap();
                assert_eq!(
                    row.get(models::User::EMAIL_VERIFIED_AT),
                    user.get_attribute(models::User::EMAIL_VERIFIED_AT)
                );
                assert!(user.mark_email_as_unverified().await.unwrap());
                assert!(!user.has_verified_email());
                assert!(!user.mark_email_as_unverified().await.unwrap());

                // 大小写不同的旧邮箱不能验证或撤销当前邮箱。
                user.set_attribute(models::User::EMAIL, "member@example.test");
                let stale = user.state().get_attributes().clone();
                assert!(!user.mark_email_as_verified().await.unwrap());
                assert_eq!(user.state().get_attributes(), &stale);
                user.set_attribute(models::User::EMAIL, "Member@example.test");
                assert!(user.mark_email_as_verified().await.unwrap());
                user.set_attribute(models::User::EMAIL, "member@example.test");
                let stale_verified = user.state().get_attributes().clone();
                assert!(!user.mark_email_as_unverified().await.unwrap());
                assert_eq!(user.state().get_attributes(), &stale_verified);

                user.set_attribute(models::User::ID, 99_i64);
                assert!(!user.mark_email_as_unverified().await.unwrap());
                connection.get_schema_builder().drop("users").await.unwrap();
                let before_error = user.state().get_attributes().clone();
                assert!(user.mark_email_as_verified().await.is_err());
                assert_eq!(user.state().get_attributes(), &before_error);
                Ok(())
            }),
            Default::default(),
        )
        .await
        .unwrap();
}
