use openrust::{
    config::{ConfigError, Environment},
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        DatabaseError, DatabaseFuture, DatabaseResult, Db,
        migrations::{Migration, MigrationContext, MigrationEntry},
    },
    foundation::Application,
    http::{Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::{ProviderResult, ServiceProvider},
};
use openrust_pay::{
    Amount, DatabaseNotifications, DriverNotification, Error, ExpectedPayment, NotificationOutcome,
    Order, PayDriver, PayManager, Payment, Refund, Result, Scene, facades::Pay,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn configuration(env: &Environment) -> std::result::Result<Value, ConfigError> {
    Ok(json!({"default":"fixture","drivers":{"fixture":{
        "driver":"external_fixture",
        "merchant": env.get_utf8("PAY_FIXTURE_MERCHANT")?.unwrap_or("merchant-original"),
    }}}))
}

pub(super) struct ExternalPayProvider;

impl ServiceProvider for ExternalPayProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container()
                .make::<PayManager>()?
                .extend("external_fixture", |profile| {
                    let name = profile["merchant"]
                        .as_str()
                        .ok_or(Error::Configuration("merchant"))?;
                    Ok(Arc::new(ExternalPay(name.into())))
                })?;
            Ok(())
        })
    }
}

struct ExternalPay(String);

impl PayDriver for ExternalPay {
    fn pay<'a>(&'a self, scene: Scene, order: &'a Order) -> HttpFuture<'a, Result<Payment>> {
        Box::pin(async move {
            if scene != Scene::Scan {
                return Err(Error::InvalidInput("fixture scene"));
            }
            Ok(Payment::QrCode(format!(
                "{}:{}",
                self.0, order.out_trade_no
            )))
        })
    }

    fn find<'a>(&'a self, order: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"merchant":self.0,"order":order})) })
    }

    fn close<'a>(&'a self, order: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"closed":order})) })
    }

    fn refund<'a>(&'a self, refund: &'a Refund) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(json!({"refund":refund.out_refund_no})) })
    }

    fn find_refund<'a>(&'a self, _: &'a str, _: &'a str) -> HttpFuture<'a, Result<Value>> {
        Box::pin(async { Err(Error::InvalidInput("fixture refund lookup")) })
    }

    fn callback(&self, headers: &HeaderMap, body: &[u8]) -> Result<DriverNotification> {
        // 验收夹具只认可固定标记和预期原文，包级用例另外通过 HMAC 验证外部渠道的实际验签。
        if headers
            .get("x-fixture-auth")
            .and_then(|value| value.to_str().ok())
            != Some("valid")
            || body != format!("{}:order-1:fx-tx:150", self.0).as_bytes()
        {
            return Err(Error::Verification);
        }
        DriverNotification::new(
            &self.0,
            "order-1",
            "fx-tx",
            Amount::from_fen(150)?,
            axum::http::Response::builder()
                .status(StatusCode::ACCEPTED)
                .header("content-type", "text/plain")
                .body("accepted by fixture".into())
                .expect("fixture acknowledgment"),
        )
    }
}

fn write_error(error: impl std::error::Error + Send + Sync + 'static) -> HttpError {
    HttpError::Internal {
        source: Box::new(error),
    }
}

pub(super) fn callback(
    request: &mut Request,
) -> HttpFuture<'_, std::result::Result<Response, HttpError>> {
    Box::pin(async move {
        let body = request.body_bytes().await?;
        let gateway = Pay::driver("fixture").map_err(write_error)?;
        let notification = gateway
            .callback(request.headers(), &body)
            .map_err(|_| HttpError::InvalidRequest)?;
        let force_rollback = request.has_header("x-fixture-rollback");
        let outcome = DatabaseNotifications::new(Db::connection(None).map_err(write_error)?)
            .process(
                &notification,
                &ExpectedPayment {
                    out_trade_no: "order-1".into(),
                    amount: Amount::from_fen(150).map_err(write_error)?,
                },
                move |tx, notice| {
                    Box::pin(async move {
                        tx.table("pay_fixture_orders")
                            .insert(&[[("id".into(), notice.out_trade_no().into())]
                                .into_iter()
                                .collect()])
                            .await?;
                        if force_rollback {
                            return Err(DatabaseError::InvalidConfiguration("fixture rollback"));
                        }
                        Ok(())
                    })
                },
            )
            .await
            .map_err(write_error)?;
        let (mut parts, body) = notification.success().into_parts();
        parts.headers.insert(
            "x-fixture-outcome",
            match outcome {
                NotificationOutcome::Processed => "processed",
                NotificationOutcome::Duplicate => "duplicate",
            }
            .parse()
            .expect("fixture header"),
        );
        Ok(Response::from_parts(parts, Body::from(body)))
    })
}

struct Probe(CommandDefinition);

impl Command for Probe {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, std::result::Result<i32, ConsoleError>> {
        Box::pin(async move {
            let gateway = Pay::default_driver().map_err(ConsoleError::from_source)?;
            let payment = gateway
                .pay(
                    Scene::Scan,
                    &Order::new(
                        "order-1",
                        "测试订单",
                        Amount::from_fen(150).map_err(ConsoleError::from_source)?,
                        "https://fixture.example/notify",
                    ),
                )
                .await
                .map_err(ConsoleError::from_source)?;
            let Payment::QrCode(qr) = payment else {
                return Err(ConsoleError::new("支付类型异常"));
            };
            let database = Db::connection(None).map_err(ConsoleError::from_source)?;
            let orders = database
                .table("pay_fixture_orders")
                .count("*")
                .await
                .map_err(ConsoleError::from_source)?;
            let notifications = database
                .table("payment_notifications")
                .count("*")
                .await
                .map_err(ConsoleError::from_source)?;
            output
                .write_raw(&format!(
                    "{}\n",
                    json!({"payment":qr,"orders":orders,"notifications":notifications})
                ))
                .map_err(ConsoleError::from_source)?;
            Ok(0)
        })
    }
}

pub(super) fn register_command(artisan: &mut Artisan) -> std::result::Result<(), ConsoleError> {
    artisan.add_command(Box::new(Probe(CommandDefinition::new(clap::Command::new(
        "drivers:pay",
    )))))
}

pub(super) static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "2026_11_26_000001_create_pay_fixture_orders.rs",
    &CreateOrders,
)];

struct CreateOrders;

impl Migration for CreateOrders {
    fn up<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .create("pay_fixture_orders", |table| {
                    table.string("id", 32).primary();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        context: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            context
                .get_schema_builder()
                .drop_if_exists("pay_fixture_orders")
                .await
        })
    }
}
