use openrust::database::Connection;

pub async fn select(sql: &str) {
    tokio::task::yield_now().await;
    openrust::support::facades::Db::select(sql, &[])
        .await
        .unwrap();
}

pub fn register_nested_query(connection: &Connection) {
    connection.before_executing(|sql, _, connection, _| {
        Box::pin(async move {
            if sql == "SELECT 77 AS outer_caller" {
                connection.select("SELECT 78 AS inner_caller", &[]).await?;
            }
            Ok(())
        })
    });
}

pub async fn select_in_transaction(
    transaction: &mut openrust::database::Transaction,
    sql: &str,
) -> openrust::database::DatabaseResult<()> {
    tokio::task::yield_now().await;
    transaction.select(sql, &[]).await?;
    Ok(())
}
