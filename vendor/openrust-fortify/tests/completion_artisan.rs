use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::auth::Authenticatable,
    database::{Db, Value, eloquent::Model},
    events::Dispatcher,
    foundation::{Application, configuration::Routing},
    support::facades::Artisan,
    testing::TestApp,
};
use openrust_fortify::{Fortify, FortifyServiceProvider, actions::CompletePasswordReset};
use serde_json::json;
#[path = "../../../app/models/mod.rs"]
mod models;

struct Complete(CommandDefinition);
impl Command for Complete {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        _: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let fortify = app.make::<Fortify>().await.unwrap();
            fortify.register_password_reset::<models::User>();
            let connection = Db::connection(None).unwrap();
            connection
                .get_schema_builder()
                .create("users", |table| {
                    table.big_integer("id").primary();
                    table.string("email", 255);
                    table.string("remember_token", 100).nullable();
                    table.timestamps();
                })
                .await
                .unwrap();
            connection
                .table("users")
                .insert(&[[
                    ("id".into(), Value::Integer(7)),
                    ("email".into(), "member@example.test".into()),
                    ("remember_token".into(), "old-token".into()),
                ]
                .into()])
                .await
                .unwrap();
            let mut user = models::User::query_on(&connection)
                .find(7_i64)
                .await
                .unwrap()
                .unwrap();
            let events = app
                .make::<Dispatcher>()
                .await
                .unwrap()
                .fake(&["PasswordReset"]);
            let action = CompletePasswordReset::new(fortify);
            action.handle(&mut user).await.unwrap();
            let token = user.get_remember_token().unwrap().to_owned();
            assert_eq!(token.len(), 60);
            assert_ne!(token, "old-token");
            assert_eq!(
                connection
                    .table("users")
                    .r#where("id", "=", 7_i64)
                    .value("remember_token")
                    .await
                    .unwrap(),
                Value::Text(token)
            );
            events.assert_dispatched_once("PasswordReset");
            connection.get_schema_builder().drop("users").await.unwrap();
            assert!(action.handle(&mut user).await.is_err());
            events.assert_dispatched_once("PasswordReset");
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(Complete(CommandDefinition::new(
        clap::Command::new("fortify:complete-probe"),
    ))))
}
#[tokio::test]
async fn native_artisan_completes_reset_after_persistence_and_propagates_failure() {
    let mut app = TestApp::sqlite(|builder| {
        builder
            .with_config(|_, _| Ok([("fortify".into(), json!({"features":[]}))].into()))
            .with_providers(vec![Box::new(FortifyServiceProvider)])
            .with_routing(Routing::default().commands(commands))
    })
    .unwrap();
    app.artisan(["fortify:complete-probe"])
        .await
        .assert_successful();
}
