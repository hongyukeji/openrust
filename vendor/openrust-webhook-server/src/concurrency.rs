use std::num::NonZeroU32;

use openrust::database::{Transaction, Value};

use crate::{
    DeliveryConcurrencyLimits, Error, Result,
    delivery_store::{ATTEMPTS, number},
    limits,
};

impl DeliveryConcurrencyLimits {
    pub(crate) fn applies(self, endpoint: bool) -> bool {
        self.sends_per_scope.is_some() || endpoint && self.sends_per_endpoint.is_some()
    }

    pub(crate) async fn lock(self, transaction: &mut Transaction, scope_key: &str) -> Result<()> {
        limits::lock_bucket(transaction, scope_key, "inflight", "").await?;
        Ok(())
    }

    pub(crate) async fn acquire(
        self,
        transaction: &mut Transaction,
        scope_key: &str,
        endpoint_id: Option<&str>,
        token: &str,
        now: i64,
        lease_until: i64,
    ) -> Result<()> {
        if let Some(maximum) = self.sends_per_scope {
            check_capacity(transaction, scope_key, None, maximum, now).await?;
        }
        if let Some((endpoint_id, maximum)) = endpoint_id.zip(self.sends_per_endpoint) {
            check_capacity(transaction, scope_key, Some(endpoint_id), maximum, now).await?;
        }
        let changed = transaction
            .table(ATTEMPTS)
            .r#where("id", "=", token)
            .where_null("finished_at")
            .where_null("send_lease_until")
            .update(
                [
                    ("send_scope_key".into(), scope_key.into()),
                    (
                        "send_endpoint_id".into(),
                        endpoint_id.map_or(Value::Null, Into::into),
                    ),
                    ("send_lease_until".into(), lease_until.into()),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        if changed != 1 {
            return Err(Error::LeaseLost);
        }
        Ok(())
    }
}

async fn check_capacity(
    transaction: &mut Transaction,
    scope_key: &str,
    endpoint_id: Option<&str>,
    maximum: NonZeroU32,
    now: i64,
) -> Result<()> {
    let mut active = transaction
        .table(ATTEMPTS)
        .r#where("send_scope_key", "=", scope_key)
        .where_null("finished_at")
        .r#where("send_lease_until", ">", now);
    if let Some(endpoint_id) = endpoint_id {
        active = active.r#where("send_endpoint_id", "=", endpoint_id);
    }
    // 当前读在 MySQL 默认的可重复读事务中也能看到前一领取者提交的名额。
    let full = active
        .select(["send_lease_until"])
        .order_by("send_lease_until", "desc")
        .offset(u64::from(maximum.get()) - 1)
        .lock_for_update()
        .first()
        .await?;
    if let Some(occupied) = full {
        let expires_at = number(&occupied, "send_lease_until")?;
        return Err(Error::RateLimited {
            retry_at: expires_at.min(now.saturating_add(1)),
        });
    }
    Ok(())
}
