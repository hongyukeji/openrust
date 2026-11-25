use std::sync::Arc;

use openrust::{
    container::{Container, ServiceKey},
    contracts::{container::Container as _, foundation::Application as _},
    database::{
        Transaction,
        query::{Expression, UpdateValue},
    },
    support::facades::{App, Facade as _},
};

use crate::{
    Error, Result,
    delivery_store::{hash, number},
};

const BUCKETS: &str = "webhook_rate_buckets";

#[derive(Clone, Copy)]
pub struct DeliveryLimit {
    count: u32,
    seconds: u32,
}

impl DeliveryLimit {
    pub fn new(count: u32, seconds: u32) -> Result<Self> {
        if count == 0 || !(1..=86_400).contains(&seconds) {
            return Err(Error::InvalidConfig("delivery.limit"));
        }
        Ok(Self { count, seconds })
    }
}

#[derive(Clone, Copy, Default)]
pub struct DeliveryLimits {
    /// 范围内的所有实际发送，包括直接 URL、自动重试与人工重放。
    pub sends_per_scope: Option<DeliveryLimit>,
    /// 每个具名端点独立计数，端点配置版本不改变其计数键。
    pub sends_per_endpoint: Option<DeliveryLimit>,
    /// 范围内成功登记的人工重放。
    pub replays_per_scope: Option<DeliveryLimit>,
}

pub fn register_delivery_limits(container: &Container, limits: DeliveryLimits) -> Result<()> {
    container
        .instance(Arc::new(limits))
        .map(|_| ())
        .map_err(|_| Error::InvalidConfig("delivery.limits"))
}

pub(crate) fn current() -> Result<DeliveryLimits> {
    let Ok(app) = App::get_facade_root() else {
        return Ok(DeliveryLimits::default());
    };
    let container = app.container();
    if !container.has(ServiceKey::typed::<DeliveryLimits>()) {
        return Ok(DeliveryLimits::default());
    }
    container
        .make::<DeliveryLimits>()
        .map(|limits| *limits)
        .map_err(|_| Error::InvalidConfig("delivery.limits"))
}

/// 在调用者的数据库事务内保留额度。所有发送先锁范围再锁端点。
pub(crate) async fn consume(
    tx: &mut Transaction,
    scope_key: &str,
    kind: &str,
    endpoint: &str,
    limit: DeliveryLimit,
    now: i64,
) -> Result<()> {
    let id = hash(format!("webhook-limit:{scope_key}:{kind}:{endpoint}").as_bytes());
    tx.table(BUCKETS)
        .upsert(
            &[[
                ("id".into(), id.as_str().into()),
                ("scope_key".into(), scope_key.into()),
                ("kind".into(), kind.into()),
                ("used".into(), 0.into()),
                ("reset_at".into(), 0.into()),
            ]
            .into_iter()
            .collect()],
            &["id"],
            Some(&["id"]),
        )
        .await?;
    let expires = now
        .checked_add(i64::from(limit.seconds))
        .ok_or(Error::InvalidRecord)?;
    let reset = tx
        .table(BUCKETS)
        .r#where("id", "=", id.as_str())
        .r#where("reset_at", "<=", now)
        .update(
            [
                ("used".into(), 1.into()),
                ("reset_at".into(), expires.into()),
            ]
            .into_iter()
            .collect(),
        )
        .await?;
    if reset == 1 {
        return Ok(());
    }
    let changed = tx
        .table(BUCKETS)
        .r#where("id", "=", id.as_str())
        .r#where("reset_at", ">", now)
        .r#where("used", "<", i64::from(limit.count))
        .update_with(
            [(
                "used".into(),
                UpdateValue::Operand(Expression::new("used + 1").into()),
            )]
            .into_iter()
            .collect(),
        )
        .await?;
    if changed == 1 {
        return Ok(());
    }
    let row = tx
        .table(BUCKETS)
        .r#where("id", "=", id.as_str())
        .select(["reset_at"])
        .lock_for_update()
        .first()
        .await?
        .ok_or(Error::InvalidRecord)?;
    Err(Error::RateLimited {
        retry_at: number(&row, "reset_at")?,
    })
}
