//! 第三方身份只保存稳定主体及本地归属，不保存供应商访问令牌或按邮箱自动联结。
use openrust::{
    auth::{AuthError, AuthIdentifier, AuthResult},
    contracts::auth::Authenticatable,
    database::{Connection, Row, Transaction, Value, eloquent::Model},
    support::{Str, facades::Date},
};
use serde::{Deserialize, Serialize};

pub(super) const TABLE: &str = "user_social_identities";

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Identity {
    pub provider: String,
    pub subject: String,
    pub owner: String,
    /// 本地Eloquent真实主键，不能使用可变的登录邮箱等认证标识。
    pub user: AuthIdentifier,
    pub generation: String,
}

pub(super) fn key(provider: &str, subject: &str) -> AuthResult<String> {
    if provider.is_empty()
        || provider.len() > 64
        || !provider
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        || subject.is_empty()
        || subject.len() > 255
    {
        return Err(AuthError::Configuration("第三方身份的供应商名称或主体无效"));
    }
    let bytes = serde_json::to_vec(&(provider, subject))?;
    let mut hash = aws_lc_rs::digest::Context::new(&aws_lc_rs::digest::SHA256);
    hash.update(b"user-starter:socialite:v1\0");
    hash.update(&bytes);
    Ok(hash
        .finish()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(super) fn user_key<M: Model>(user: &M) -> AuthResult<AuthIdentifier> {
    AuthIdentifier::try_from(user.get_key().ok_or(AuthError::InvalidIdentity)?)
}

impl Identity {
    pub fn id(&self) -> AuthResult<String> {
        key(&self.provider, &self.subject)
    }
    pub fn owner_hash(&self) -> AuthResult<String> {
        super::super::challenges::owner_key(&self.owner, &self.user)
    }
    pub fn belongs_to<M: Model + Authenticatable>(&self, user: &M) -> AuthResult<bool> {
        Ok(self.owner == user.get_morph_class()?
            && serde_json::to_value(&self.user)? == serde_json::to_value(user_key(user)?)?)
    }
    pub(super) fn from_row(row: &Row) -> AuthResult<Self> {
        let Some(Value::Text(payload)) = row.get("payload") else {
            return Err(AuthError::Configuration("第三方身份缺少归属记录"));
        };
        let record: Self = serde_json::from_str(payload)?;
        if record.generation.len() != 64
            || !record.generation.bytes().all(|b| b.is_ascii_alphanumeric())
            || row.get("id") != Some(&Value::Text(record.id()?))
            || row.get("owner_hash") != Some(&Value::Text(record.owner_hash()?))
            || row.get("provider") != Some(&Value::Text(record.provider.clone()))
        {
            return Err(AuthError::Configuration("第三方身份归属不一致"));
        }
        Ok(record)
    }
}

pub(super) async fn find(
    connection: &Connection,
    provider: &str,
    subject: &str,
) -> AuthResult<Option<Identity>> {
    connection
        .table(TABLE)
        .use_write_pdo()
        .r#where("id", "=", key(provider, subject)?)
        .first()
        .await?
        .as_ref()
        .map(Identity::from_row)
        .transpose()
}

pub(super) async fn for_user<M: Model + Authenticatable>(user: &M) -> AuthResult<Vec<Identity>> {
    let owner = super::super::challenges::owner_key(user.get_morph_class()?, &user_key(user)?)?;
    user.get_connection()?
        .table(TABLE)
        .use_write_pdo()
        .r#where("owner_hash", "=", owner)
        .order_by("provider", "asc")
        .get()
        .await?
        .iter()
        .map(Identity::from_row)
        .collect()
}

fn validate_transaction<M: Model + Authenticatable>(
    user: &M,
    transaction: &Transaction,
) -> AuthResult<()> {
    if !transaction.belongs_to(&user.get_connection()?)
        || !user.state().exists()
        || user.get_key().is_none()
        || user.get_key() != user.get_raw_original(user.get_key_name())
    {
        return Err(AuthError::Configuration(
            "第三方身份必须使用已持久化用户的同一事务与真实主键",
        ));
    }
    Ok(())
}

pub(super) async fn current<M: Model + Authenticatable>(
    user: &M,
    transaction: &mut Transaction,
    id: &str,
    generation: &str,
    provider: &str,
) -> AuthResult<bool> {
    validate_transaction(user, transaction)?;
    let Some(row) = transaction
        .table(TABLE)
        .r#where("id", "=", id)
        .first()
        .await?
    else {
        return Ok(false);
    };
    let record = Identity::from_row(&row)?;
    Ok(record.provider == provider
        && record.belongs_to(user)?
        && super::super::client::same_binding(&record.generation, generation))
}

/// 调用方已持有共同用户写锁；重新绑定生成新版本，旧登录挑战不会因同主体重绑而复活。
pub(super) async fn link<M: Model + Authenticatable>(
    user: &M,
    transaction: &mut Transaction,
    provider: &str,
    subject: &str,
) -> AuthResult<Identity> {
    validate_transaction(user, transaction)?;
    let id = key(provider, subject)?;
    if let Some(row) = transaction
        .table(TABLE)
        .r#where("id", "=", id.as_str())
        .first()
        .await?
    {
        let record = Identity::from_row(&row)?;
        return if record.belongs_to(user)? {
            Ok(record)
        } else {
            Err(super::super::client::failed("provider", "user.provider_already_linked").into())
        };
    }
    let record = Identity {
        provider: provider.into(),
        subject: subject.into(),
        owner: user.get_morph_class()?.into(),
        user: user_key(user)?,
        generation: Str::random(64),
    };
    if transaction
        .table(TABLE)
        .r#where("owner_hash", "=", record.owner_hash()?)
        .r#where("provider", "=", provider)
        .exists()
        .await?
    {
        return Err(
            super::super::client::failed("provider", "user.provider_already_linked").into(),
        );
    }
    let now = Date::now(None)
        .map_err(|e| AuthError::Backend(Box::new(e)))?
        .as_date_time()
        .timestamp()
        .as_second();
    transaction
        .table(TABLE)
        .insert(&[[
            ("id".into(), id.clone().into()),
            ("provider".into(), provider.into()),
            ("owner_hash".into(), record.owner_hash()?.into()),
            ("payload".into(), serde_json::to_string(&record)?.into()),
            ("created_at".into(), now.into()),
        ]
        .into()])
        .await
        .map_err(|error| -> AuthError {
            if matches!(&error, openrust::database::DatabaseError::Driver { source, .. }
                if source.as_database_error().is_some_and(|error| error.is_unique_violation()))
            {
                super::super::client::failed("provider", "user.provider_already_linked").into()
            } else {
                error.into()
            }
        })?;
    let stored = transaction
        .table(TABLE)
        .r#where("id", "=", id)
        .first()
        .await?
        .ok_or(AuthError::Configuration("第三方身份没有持久化"))?;
    let stored = Identity::from_row(&stored)?;
    if stored.generation != record.generation || !stored.belongs_to(user)? {
        return Err(AuthError::Configuration("第三方身份写入结果不一致"));
    }
    Ok(stored)
}

/// 最后认证方式检查由调用方在同一用户锁中完成；本层只删除核定归属的记录。
pub(super) async fn unlink<M: Model + Authenticatable>(
    user: &M,
    transaction: &mut Transaction,
    provider: &str,
) -> AuthResult<bool> {
    validate_transaction(user, transaction)?;
    let owner = super::super::challenges::owner_key(user.get_morph_class()?, &user_key(user)?)?;
    let rows = transaction
        .table(TABLE)
        .r#where("owner_hash", "=", owner.as_str())
        .r#where("provider", "=", provider)
        .get()
        .await?;
    for row in &rows {
        if !Identity::from_row(row)?.belongs_to(user)? {
            return Err(AuthError::Configuration("第三方身份归属不一致"));
        }
    }
    Ok(transaction
        .table(TABLE)
        .r#where("owner_hash", "=", owner)
        .r#where("provider", "=", provider)
        .delete()
        .await?
        > 0)
}

pub(super) async fn purge<M: Model + Authenticatable>(
    user: &M,
    transaction: &mut Transaction,
) -> AuthResult<()> {
    validate_transaction(user, transaction)?;
    let owner = super::super::challenges::owner_key(user.get_morph_class()?, &user_key(user)?)?;
    transaction
        .table(TABLE)
        .r#where("owner_hash", "=", owner)
        .delete()
        .await?;
    Ok(())
}
