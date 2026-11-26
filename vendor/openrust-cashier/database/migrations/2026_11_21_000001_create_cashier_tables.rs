use openrust::database::{
    DatabaseFuture, DatabaseResult,
    migrations::{Migration, MigrationContext},
};
pub struct CreateCashierTables;
pub(crate) static MIGRATION: CreateCashierTables = CreateCashierTables;
impl Migration for CreateCashierTables {
    fn up<'a>(&'a self, c: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            let mut schema = c.get_schema_builder();
            schema
                .create("cashier_customers", |t| {
                    t.string("id", 64).primary();
                    t.string("remote_key", 64).unique();
                    t.string("remote_id", 255);
                    t.string("status", 16);
                })
                .await?;
            schema
                .create("cashier_subscriptions", |t| {
                    t.string("id", 64).primary();
                    t.string("customer_key", 64).index();
                    t.string("name", 255);
                    t.text("payload");
                })
                .await?;
            schema
                .create("cashier_invoices", |t| {
                    t.string("id", 64).primary();
                    t.string("customer_key", 64).index();
                    t.text("payload");
                })
                .await?;
            for name in ["cashier_events", "cashier_operations"] {
                schema
                    .create(name, |t| {
                        t.string("id", 64).primary();
                        t.string("fingerprint", 64);
                        t.string("status", 16);
                    })
                    .await?;
            }
            Ok(())
        })
    }
    fn down<'a>(&'a self, c: &'a mut MigrationContext) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async move {
            for name in [
                "cashier_operations",
                "cashier_events",
                "cashier_invoices",
                "cashier_subscriptions",
                "cashier_customers",
            ] {
                c.get_schema_builder().drop_if_exists(name).await?;
            }
            Ok(())
        })
    }
}
