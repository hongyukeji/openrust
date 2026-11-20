use openrust::database::Connection;
use openrust_cashier::{Billable, BillingStore, Client, Price, Proration, Result};
use serde_json::json;

pub async fn stripe(
    client: Client,
    connection: Connection,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let user = Billable::new(client, BillingStore::new(connection), "users:42")?;
    user.createOrGetCustomer(json!({"email":"buyer@example.com"}), "customer:42")
        .await?;
    let session=user.newSubscription("default",vec![Price::new("price_monthly",1)?])?.trialDays(14)?.withCoupon("coupon_welcome")?.checkout(json!({"success_url":"https://shop.example/success?session_id={CHECKOUT_SESSION_ID}","cancel_url":"https://shop.example/account"}),"subscription:42:first").await?;
    let _ = session;
    let subscription = user
        .subscription("default")
        .await?
        .ok_or("missing subscription")?;
    let subscription = subscription
        .updateQuantity(3, None, Proration::Invoice, "seats:42:v2")
        .await?;
    let subscription = subscription.cancel("cancel:42:v1").await?;
    if subscription.onGracePeriod() {
        subscription.resume("resume:42:v1").await?;
    }
    Ok(())
}
pub async fn paddle(client: Client, connection: Connection) -> Result<serde_json::Value> {
    let user = Billable::new(client, BillingStore::new(connection), "users:42")?;
    user.createOrGetCustomer(json!({"email":"buyer@example.com"}), "customer:42")
        .await?;
    user.newSubscription("default", vec![Price::new("pri_monthly", 1)?])?
        .withCoupon("dsc_welcome")?
        .checkout(json!({}), "subscription:42:first")
        .await
}
pub async fn webhook(
    client: Client,
    store: BillingStore,
    headers: http::HeaderMap,
    raw_body: Vec<u8>,
) -> Result<http::Response<String>> {
    let event = client.verifyWebhook(&headers, &raw_body)?;
    store
        .process(&client, &event, |tx, event| {
            Box::pin(async move {
                let _ = (tx, event);
                Ok(())
            })
        })
        .await?;
    Ok(event.success())
}
fn main() {}
