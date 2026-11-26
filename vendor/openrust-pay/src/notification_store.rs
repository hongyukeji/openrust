//! 通知收件箱的事务内存储操作；事务及业务回调由通知流程协调。
use openrust::database::{DatabaseResult, Transaction, Value};

pub(crate) async fn claim(
    tx: &mut Transaction,
    key: &str,
    fingerprint: &str,
) -> DatabaseResult<bool> {
    let record = [
        ("id".into(), Value::from(key)),
        ("fingerprint".into(), fingerprint.into()),
        ("status".into(), "pending".into()),
    ]
    .into_iter()
    .collect();
    // 无变化的 upsert 在读取前取得写锁，避免 SQLite 锁升级及 MySQL 的共享锁升级。
    tx.table("payment_notifications")
        .upsert(&[record], &["id"], Some(&["id"]))
        .await?;
    let stored = tx
        .table("payment_notifications")
        .r#where("id", "=", key)
        .lock_for_update()
        .value("fingerprint")
        .await?;
    Ok(stored == Value::from(fingerprint))
}

pub(crate) async fn is_complete(tx: &mut Transaction, key: &str) -> DatabaseResult<bool> {
    let status = tx
        .table("payment_notifications")
        .r#where("id", "=", key)
        .value("status")
        .await?;
    Ok(status == Value::from("done"))
}

pub(crate) async fn complete(tx: &mut Transaction, key: &str) -> DatabaseResult<()> {
    tx.table("payment_notifications")
        .r#where("id", "=", key)
        .update([("status".into(), "done".into())].into_iter().collect())
        .await?;
    Ok(())
}
