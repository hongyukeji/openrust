mod support;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{Application, configuration::Routing},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider, facades::Artisan},
    testing::TestApp,
};
use openrust_cashier::{Billable, BillingStore, Client, Error, Provider, WebhookOutcome};
use serde_json::json;
use std::sync::Arc;
use support::{Db, Server, event, subscription};
struct Fixtures(Vec<(Client, BillingStore)>);
struct Register(Arc<Fixtures>);
impl ServiceProvider for Register {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().instance(self.0.clone())?;
            Ok(())
        })
    }
}
struct BillingCommand(CommandDefinition);
impl Command for BillingCommand {
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
            let fixtures = app.make::<Fixtures>().await.unwrap();
            for (client, store) in &fixtures.0 {
                let billable =
                    Billable::new(client.clone(), store.clone(), "users:artisan").unwrap();
                billable.attach_customer("cus_1").await.unwrap();
                let value = store
                    .execute(
                        client,
                        http::Method::POST,
                        "/subscriptions",
                        &json!({}),
                        "artisan:operation",
                    )
                    .await
                    .unwrap();
                assert_eq!(value["id"], "sub_1");
                if client.provider() == Provider::Paddle {
                    assert!(matches!(
                        store
                            .execute(
                                client,
                                http::Method::POST,
                                "/subscriptions",
                                &json!({}),
                                "artisan:operation"
                            )
                            .await,
                        Err(Error::OutcomeUnknown)
                    ));
                }
                assert_eq!(
                    store
                        .sync_subscription(client, "users:artisan", "sub_1")
                        .await
                        .unwrap()
                        .status,
                    "active"
                );
                let kind = if client.provider() == Provider::Stripe {
                    "customer.subscription.updated"
                } else {
                    "subscription.updated"
                };
                let (headers, body) = event(
                    client,
                    "evt_artisan",
                    kind,
                    subscription(client.provider(), "active"),
                    0,
                );
                let verified = client.verify_webhook(&headers, &body).unwrap();
                assert_eq!(
                    store.handle_webhook(client, &verified).await.unwrap(),
                    WebhookOutcome::Processed
                );
                assert_eq!(
                    store.handle_webhook(client, &verified).await.unwrap(),
                    WebhookOutcome::Duplicate
                );
            }
            Ok(0)
        })
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(BillingCommand(CommandDefinition::new(
        clap::Command::new("billing:probe"),
    ))))
}
#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn native_artisan_keeps_remote_operations_and_webhook_transactions() {
    let db = Db::new(true).await;
    let stripe = Server::new(Provider::Stripe).await;
    let paddle = Server::new(Provider::Paddle).await;
    let fixtures = Arc::new(Fixtures(vec![
        (stripe.client(), db.store.clone()),
        (paddle.client(), db.store.clone()),
    ]));
    let mut app = TestApp::create(|builder| {
        builder
            .with_providers(vec![Box::new(Register(fixtures))])
            .with_routing(Routing::default().commands(commands))
    })
    .unwrap();
    app.artisan(["billing:probe"]).await.assert_successful();
    drop(app);
    db.migrator
        .rollback(None, &[], Default::default())
        .await
        .unwrap();
}
