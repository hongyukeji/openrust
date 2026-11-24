//! 无浏览器会话客户端的短期证明，使用所属用户连接持久化并原子消费。
use openrust::{
    auth::{AuthIdentifier, AuthResult},
    database::{Connection, Value},
    support::{Str, facades::Date},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

pub(crate) const TABLE: &str = "user_auth_challenges";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Pending {
    pub user: AuthIdentifier,
    pub owner: String,
    pub purpose: String,
    pub credential: String,
    pub factor: String,
    pub token: Option<AuthIdentifier>,
    pub payload: serde_json::Value,
}

pub(crate) struct Challenge {
    pub key: Zeroizing<String>,
    pub expires_at: i64,
}

pub(crate) struct ChallengeStore {
    connection: Connection,
}

fn now() -> AuthResult<i64> {
    Ok(Date::now(None)
        .map_err(|error| openrust::auth::AuthError::Backend(Box::new(error)))?
        .as_date_time()
        .timestamp()
        .as_second())
}

fn digest(raw: &str) -> Option<String> {
    if raw.len() != 64 || !raw.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, raw.as_bytes());
    Some(
        hash.as_ref()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

pub(crate) fn owner_key(owner: &str, user: &AuthIdentifier) -> AuthResult<String> {
    let payload = serde_json::to_vec(&(owner, user))?;
    let hash = aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &payload);
    Ok(hash
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

impl ChallengeStore {
    pub fn new(connection: Connection) -> Self {
        Self { connection }
    }

    pub async fn create(&self, pending: &Pending, lifetime: i64) -> AuthResult<Challenge> {
        let now = now()?;
        let expires_at = now.checked_add(lifetime).filter(|_| lifetime > 0).ok_or(
            openrust::auth::AuthError::Configuration("认证证明期限必须为有效正整数秒"),
        )?;
        jiff::Timestamp::from_second(expires_at)
            .map_err(|_| openrust::auth::AuthError::Configuration("认证证明期限超出支持范围"))?;
        let key = Zeroizing::new(Str::random(64));
        let id = digest(&key).expect("原生随机字符串满足认证证明长度和字符要求");
        self.connection
            .table(TABLE)
            .insert(&[[
                ("id".into(), id.into()),
                ("purpose".into(), pending.purpose.as_str().into()),
                (
                    "owner_hash".into(),
                    owner_key(&pending.owner, &pending.user)?.into(),
                ),
                ("payload".into(), serde_json::to_string(pending)?.into()),
                ("created_at".into(), now.into()),
                ("expires_at".into(), expires_at.into()),
                ("consumed_at".into(), Value::Null),
            ]
            .into()])
            .await?;
        Ok(Challenge { key, expires_at })
    }

    pub async fn resolve(&self, raw: &str, purpose: &str) -> AuthResult<Option<Pending>> {
        let Some(id) = digest(raw) else {
            return Ok(None);
        };
        let row = self
            .connection
            .table(TABLE)
            .use_write_pdo()
            .r#where("id", "=", id)
            .r#where("purpose", "=", purpose)
            .where_null("consumed_at")
            .r#where("expires_at", ">", now()?)
            .first()
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let Some(Value::Text(payload)) = row.get("payload") else {
            return Err(openrust::auth::AuthError::Configuration(
                "认证证明缺少持久化载荷",
            ));
        };
        let pending: Pending = serde_json::from_str(payload)?;
        if pending.purpose != purpose {
            return Ok(None);
        }
        Ok(Some(pending))
    }

    /// 在授予权限前消费；后续失败须重新认证，不会留下可重放的授权证明。
    pub async fn consume(&self, raw: &str, pending: &Pending) -> AuthResult<Option<i64>> {
        let Some(id) = digest(raw) else {
            return Ok(None);
        };
        let mut transaction = self.connection.begin_transaction().await?;
        // SQLite不支持SELECT FOR UPDATE，先取得写锁；其他方言直接锁定读取。
        if self.connection.get_driver_name() == "sqlite" {
            transaction
                .table(TABLE)
                .r#where("id", "=", id.as_str())
                .update([("id".into(), id.as_str().into())].into())
                .await?;
        }
        let row = transaction
            .table(TABLE)
            .r#where("id", "=", id.as_str())
            .lock_for_update()
            .first()
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let now = now()?;
        let expires = match row.get("expires_at") {
            Some(Value::Integer(value)) => *value,
            Some(Value::Unsigned(value)) => i64::try_from(*value).map_err(|_| {
                openrust::auth::AuthError::Configuration("认证证明期限超出整数范围")
            })?,
            _ => return Err(openrust::auth::AuthError::Configuration("认证证明期限无效")),
        };
        let stored = match row.get("payload") {
            Some(Value::Text(value)) => serde_json::from_str::<serde_json::Value>(value)?,
            _ => {
                return Err(openrust::auth::AuthError::Configuration(
                    "认证证明缺少持久化载荷",
                ));
            }
        };
        // 以JSON值精确比较，避免数据库不区分大小写的排序规则放宽证明绑定。
        if stored != serde_json::to_value(pending)?
            || expires <= now
            || row.get("purpose") != Some(&Value::Text(pending.purpose.clone()))
            || row.get("consumed_at") != Some(&Value::Null)
        {
            return Ok(None);
        }
        let affected = transaction
            .table(TABLE)
            .r#where("id", "=", id)
            .where_null("consumed_at")
            .update([("consumed_at".into(), now.into())].into())
            .await?;
        transaction.commit().await?;
        Ok((affected == 1).then_some(expires))
    }

    pub async fn prune(&self, retention: i64) -> AuthResult<u64> {
        let deadline = now()?
            .checked_sub(retention)
            .filter(|_| retention >= 0)
            .ok_or(openrust::auth::AuthError::Configuration(
                "认证证明保留期限必须为非负整数秒",
            ))?;
        Ok(self
            .connection
            .table(TABLE)
            .r#where("expires_at", "<=", deadline)
            .delete()
            .await?)
    }
}

pub(crate) async fn prune_for<M: openrust::database::eloquent::Model>(
    retention: i64,
) -> AuthResult<u64> {
    ChallengeStore::new(M::from_state(Default::default()).get_connection()?)
        .prune(retention)
        .await
}
