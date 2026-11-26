use openrust::database::{DatabaseError, Transaction};

use super::{ModerationStore, digest, integer, remote_leases};
use crate::{
    ProfileRevision, VideoReview,
    rate_policy::{AccountRates, RateOperation},
};

const BUCKETS: &str = "moderation_account_rate_buckets";

pub(super) const RESERVATION_STALE: &str = "内容审核状态已变化，释放本次速率额度";

#[derive(Clone)]
pub(crate) struct RateBudget {
    id: String,
    operation: RateOperation,
    maximum: i64,
    window_seconds: i64,
}

impl ModerationStore {
    pub(crate) fn rate_budget(
        &self,
        profile: &ProfileRevision,
        operation: RateOperation,
        rates: AccountRates,
    ) -> RateBudget {
        RateBudget {
            id: digest(&format!(
                "{}\n{}\n{}\n{}",
                self.application_key,
                profile.driver(),
                profile.account(),
                operation.name(),
            )),
            operation,
            maximum: rates.maximum(operation),
            window_seconds: rates.window_seconds,
        }
    }
}

impl RateBudget {
    // 相同账户和操作通过唯一键接入同一数据库事务。只有提交领取成功才消耗额度。
    pub(super) async fn reserve(
        &self,
        tx: &mut Transaction,
        now: i64,
        in_flight: Option<u16>,
    ) -> Result<Option<i64>, DatabaseError> {
        tx.table(BUCKETS)
            .upsert(
                &[[
                    ("id".into(), self.id.as_str().into()),
                    ("operation".into(), self.operation.name().into()),
                    ("used".into(), 0.into()),
                    ("reset_at".into(), 0.into()),
                ]
                .into_iter()
                .collect()],
                &["id"],
                Some(&["id"]),
            )
            .await?;
        let invalid = || DatabaseError::InvalidQuery("内容审核账户速率记录无效");
        let row = tx
            .table(BUCKETS)
            .r#where("id", "=", self.id.as_str())
            .select(["used", "reset_at"])
            .lock_for_update()
            .first()
            .await?
            .ok_or_else(invalid)?;
        let used = integer(&row, "used")
            .filter(|&used| used >= 0)
            .ok_or_else(invalid)?;
        let reset_at = integer(&row, "reset_at").ok_or_else(invalid)?;
        let (used, reset_at) = if reset_at <= now {
            (1, now.checked_add(self.window_seconds).ok_or_else(invalid)?)
        } else if used >= self.maximum {
            return Ok(Some(reset_at));
        } else {
            (used + 1, reset_at)
        };
        if let Some(maximum) = in_flight
            && let Some(available) =
                remote_leases::next_capacity_at(tx, &self.id, maximum, now).await?
        {
            return Ok(Some(available));
        }
        let affected = tx
            .table(BUCKETS)
            .r#where("id", "=", self.id.as_str())
            .update(
                [
                    ("used".into(), used.into()),
                    ("reset_at".into(), reset_at.into()),
                ]
                .into_iter()
                .collect(),
            )
            .await?;
        if affected != 1 {
            return Err(invalid());
        }
        Ok(None)
    }

    pub(super) async fn open_remote(
        &self,
        tx: &mut Transaction,
        review: &VideoReview,
    ) -> Result<(), DatabaseError> {
        remote_leases::open(tx, &self.id, review).await
    }
}
