use openrust::database::Connection;
use openrust_pay::{
    Amount, DatabaseNotifications, ExpectedPayment, Order, Payment, Result, Scene, facades::Pay,
};

async fn start_payment() -> Result<Payment> {
    let order = Order::new(
        "order_001",
        "Coffee",
        Amount::from_fen(1234)?,
        "https://shop.example/pay/notify",
    );
    Pay::driver("wechat")?.pay(Scene::Scan, &order).await
}

async fn handle_success(
    connection: Connection,
    headers: &http::HeaderMap,
    raw_body: &[u8],
    expected: ExpectedPayment,
) -> Result<http::Response<String>> {
    let notification = Pay::driver("wechat")?.callback(headers, raw_body)?;
    DatabaseNotifications::new(connection)
        .process(&notification, &expected, |tx, notice| {
            Box::pin(async move {
                // Resolve and validate the business order's state using this same transaction.
                tx.table("orders")
                    .r#where("number", "=", notice.out_trade_no())
                    .r#where("status", "=", "pending")
                    .update([("status".into(), "paid".into())].into_iter().collect())
                    .await?;
                Ok(())
            })
        })
        .await?;
    Ok(notification.success())
}
fn main() {
    // These handlers require the application's trusted merchant and business order context.
    let _ = start_payment;
    let _ = handle_success;
}
