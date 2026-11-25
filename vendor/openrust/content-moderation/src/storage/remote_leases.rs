use openrust::database::{DatabaseError, Transaction};

use super::integer;
use crate::VideoReview;

const LEASES: &str = "moderation_remote_leases";
const CAPACITY_RECHECK_SECONDS: i64 = 5;

/// 送审账号的额度记录已被锁定，按当前写入连接读取远端在途租约。
pub(super) async fn next_capacity_at(
    tx: &mut Transaction,
    account: &str,
    maximum: u16,
    now: i64,
) -> Result<Option<i64>, DatabaseError> {
    tx.table(LEASES)
        .r#where("account_key", "=", account)
        .r#where("expires_at", "<=", now)
        .delete()
        .await?;
    let current = tx
        .table(LEASES)
        .r#where("account_key", "=", account)
        .r#where("expires_at", ">", now)
        .order_by("expires_at", "asc")
        .select(["expires_at"])
        .limit(u64::from(maximum))
        .lock_for_update()
        .get()
        .await?;
    if current.len() < usize::from(maximum) {
        return Ok(None);
    }
    let earliest = current
        .first()
        .and_then(|row| integer(row, "expires_at"))
        .ok_or(DatabaseError::InvalidQuery("审核远端租约无效"))?;
    Ok(Some(earliest.min(now + CAPACITY_RECHECK_SECONDS)))
}

pub(super) async fn open(
    tx: &mut Transaction,
    account: &str,
    review: &VideoReview,
) -> Result<(), DatabaseError> {
    tx.table(LEASES)
        .insert(&[[
            ("check_id".into(), review.check_id().into()),
            ("account_key".into(), account.into()),
            ("expires_at".into(), review.deadline_at().into()),
        ]
        .into_iter()
        .collect()])
        .await?;
    Ok(())
}

pub(super) async fn release(tx: &mut Transaction, check: &str) -> Result<(), DatabaseError> {
    tx.table(LEASES)
        .r#where("check_id", "=", check)
        .delete()
        .await?;
    Ok(())
}
