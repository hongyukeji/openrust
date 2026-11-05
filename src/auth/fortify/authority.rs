//! 数据库因子授权：在同一账户锁下校验 Fortify 三字段、代际和单次消费。

use super::{DatabaseTwoFactorState, TwoFactorState};
use crate::{
    Error, Result,
    auth::{AuthUser, FactorManagementContext, UserProvider},
    db::{
        query::{QueryBuilder, RawSql},
        sea_orm::{ConnectionTrait, DatabaseTransaction, DbBackend, Value as SqlValue},
    },
    security::Encryptor,
};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{Arc, OnceLock},
};

const SOURCE: &str = "two_factor_authority_source";
const GENERATION: &str = "two_factor_authority_generation";
const CONSUMED: &str = "two_factor_consumed_codes";
const MANAGEMENT_HASH: &str = "two_factor_management_session_hash";
const MANAGEMENT_GENERATION: &str = "two_factor_management_generation";
const MANAGEMENT_AT: &str = "two_factor_management_authorized_at";

/// authority当前有效因子种类，不把部分状态误当Disabled。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactorAuthorityStatus {
    /// 当前无已绑定因子。
    Disabled,
    /// 现代绑定尚未确认。
    Pending,
    /// 已确认现代因子。
    Modern,
}
/// 显式管理动作；消费恢复码不属于重新生成管理动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FactorAuthorityMutation {
    /// 启用或force替换。
    Enable,
    /// 确认标识；公开replace拒绝，框架通过已验证验证码的受控路径完成。
    Confirm,
    /// 清除三字段并轮换代际。
    Disable,
    /// 重新生成可读恢复码。
    Regenerate,
}
/// 同一账户的当前状态与持久代际，快照绑定原始账户标识。
#[derive(Clone, PartialEq, Eq)]
pub struct FactorAuthoritySnapshot {
    /// 官方三字段密文快照。
    pub state: TwoFactorState,
    /// 每账户持久随机来源，不能按Provider名称推断。
    pub source: String,
    /// 持久随机代际与主因子状态的绑定值；调用方不可解析其结构。
    pub generation: String,
    /// 当前有效状态。
    pub status: FactorAuthorityStatus,
    persisted_generation: String,
    identifier: Value,
    confirmed_storage: Option<String>,
    management_hash: Option<String>,
    management_generation: Option<String>,
    management_at: Option<DateTime<Utc>>,
}
impl fmt::Debug for FactorAuthoritySnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FactorAuthoritySnapshot([REDACTED])")
    }
}
impl FactorAuthoritySnapshot {
    /// 已确认因子要求挑战，待确认的因子按当前确认策略处理。
    #[must_use]
    pub fn challenge_required(&self, confirm: bool) -> bool {
        match self.status {
            FactorAuthorityStatus::Modern => true,
            FactorAuthorityStatus::Pending => !confirm,
            FactorAuthorityStatus::Disabled => false,
        }
    }
}
/// 使用同一账户模型和存储的因子授权服务。
#[derive(Clone)]
pub struct DatabaseFactorAuthority {
    store: DatabaseTwoFactorState,
    management_event: Option<super::actions::ManagementEvent>,
    consumed_ttl_seconds: u64,
    provider_verification: Option<super::TwoFactorCodeVerification>,
    confirmation_config: Option<crate::config::repository::ConfigRepository>,
}
impl fmt::Debug for DatabaseFactorAuthority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseFactorAuthority([REDACTED])")
    }
}
impl DatabaseFactorAuthority {
    /// 显式选择同一账户存储。密文由 Fortify Actions 所选加密器处理。
    #[must_use]
    pub fn new(store: DatabaseTwoFactorState, _encrypter: Encryptor) -> Self {
        Self {
            store,
            management_event: None,
            consumed_ttl_seconds: 150,
            provider_verification: None,
            confirmation_config: None,
        }
    }

    pub(super) fn with_management_event(mut self, event: super::actions::ManagementEvent) -> Self {
        self.management_event = Some(event);
        self
    }

    /// 选择实际验证码窗口；摘要账本覆盖整个相邻时间步有效期，不包含明文。
    pub fn with_verification_window(mut self, window: u64) -> Result<Self> {
        self.consumed_ttl_seconds = window
            .checked_mul(2)
            .and_then(|value| value.checked_add(3))
            .and_then(|value| value.checked_mul(30))
            .ok_or_else(|| invalid("验证码窗口有效期溢出"))?;
        self.provider_verification = None;
        Ok(self)
    }

    // 仅随同一次真实Provider校验传递；不把Provider事实当作账户/Guard认证收据。
    pub(crate) fn with_provider_verification(
        self,
        verification: &super::TwoFactorCodeVerification,
    ) -> Result<Self> {
        let mut selected = self;
        selected.consumed_ttl_seconds = verification
            .replay_lifetime_seconds()
            .ok_or_else(|| invalid("缺少验证码有效期"))?;
        selected.provider_verification = Some(verification.clone());
        Ok(selected)
    }

    /// 先核同实际连接/表、标识字段和原始typed ID，不能借同数字的另一Provider账户。
    #[allow(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "保留统一authority公开异步校验契约，当前来源证明本身为同步操作"
    )]
    pub async fn validate_account(
        &self,
        user: &AuthUser,
        provider: &dyn UserProvider,
    ) -> Result<()> {
        if !provider.uses_database_accounts(self.store.db(), self.store.table())?
            || user.auth_identifier_name() != self.store.identifier_name()
            || user.auth_identifier()? != *self.store.raw_identifier()
        {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
    /// 主库同用户锁下读取；首次使用原子初始化随机来源/代际。
    pub async fn load(&self) -> Result<FactorAuthoritySnapshot> {
        let this = self.clone();
        self.store
            .db()
            .transaction_for_update(move |transaction| {
                Box::pin(async move { this.read_locked(transaction).await })
            })
            .await
    }
    // 直接凭据签发只使用同库 users.id 正整数域；普通模型读取仍支持其他 typed ID。
    pub(crate) async fn load_in_transaction(
        &self,
        db: &crate::db::Db,
        transaction: &DatabaseTransaction,
    ) -> Result<FactorAuthoritySnapshot> {
        if !self.store.db().same_handle(db)
            || self.store.db().table_name(self.store.table())? != db.table_name("users")?
            || self.store.identifier_name() != "id"
            || self
                .store
                .raw_identifier()
                .as_i64()
                .is_none_or(|id| id <= 0)
        {
            return Err(Error::Unauthorized);
        }
        self.read_locked(transaction).await
    }
    pub(crate) fn with_confirmation_config(
        mut self,
        config: crate::config::repository::ConfigRepository,
    ) -> Self {
        self.confirmation_config = Some(config);
        self
    }
    fn current_confirmation(&self, fallback: bool) -> Result<bool> {
        self.confirmation_config
            .as_ref()
            .map_or(Ok(fallback), super::options::confirmation_required)
    }

    pub(crate) async fn authorize_management(
        &self,
        before: &FactorAuthoritySnapshot,
        context: &FactorManagementContext,
        confirm: bool,
    ) -> Result<()> {
        let this = self.clone();
        let expected = before.clone();
        let read_context = context.clone();
        self.management_transaction(before, Some(context), confirm, None, move |transaction| {
            Box::pin(async move {
                let current = this.read_locked(transaction).await?;
                ensure_same(&expected, &current)?;
                this.authorize_context(transaction, &current, &read_context, confirm, None)
                    .await?;
                Ok(current)
            })
        })
        .await
        .map(|_| ())
    }

    // API 的返回快照必须经过全局提交前监听器之后的最后一次检查。
    async fn management_transaction<F>(
        &self,
        before: &FactorAuthoritySnapshot,
        context: Option<&FactorManagementContext>,
        confirm: bool,
        mutation: Option<FactorAuthorityMutation>,
        callback: F,
    ) -> Result<FactorAuthoritySnapshot>
    where
        F: for<'a> FnOnce(
                &'a DatabaseTransaction,
            )
                -> crate::core::BoxFuture<'a, Result<FactorAuthoritySnapshot>>
            + Send,
    {
        let db = self.store.db();
        let Some(context) = context.filter(|context| context.has_api_authorization()) else {
            return db.transaction_for_update(callback).await;
        };
        let authority = self.clone();
        let before = before.clone();
        let context = context.clone();
        let expected = Arc::new(OnceLock::new());
        let captured = expected.clone();
        db.transaction_for_update_checked(
            move |tx| {
                let result = callback(tx);
                Box::pin(async move {
                    let after = result.await?;
                    captured
                        .set(after.clone())
                        .map_err(|_| invalid("管理事务重复产生提交候选快照"))?;
                    Ok(after)
                })
            },
            move |tx| {
                Box::pin(async move {
                    let after = expected
                        .get()
                        .ok_or_else(|| invalid("管理事务缺少提交候选快照"))?;
                    ensure_same(after, &authority.read_locked(tx).await?)?;
                    // Confirm/Disable 已合法清除管理绑定；授权仍针对原快照重核。
                    authority
                        .authorize_context(tx, &before, &context, confirm, mutation)
                        .await
                })
            },
        )
        .await
    }

    async fn authorize_context(
        &self,
        transaction: &DatabaseTransaction,
        snapshot: &FactorAuthoritySnapshot,
        context: &FactorManagementContext,
        confirm: bool,
        mutation: Option<FactorAuthorityMutation>,
    ) -> Result<()> {
        context
            .validate_api_in_transaction(self.store.db(), transaction)
            .await?;
        if context.has_api_authorization() {
            if context.is_api_pending_restart() {
                if snapshot.status != FactorAuthorityStatus::Pending
                    || !context.api_initial_snapshot_matches(&snapshot.source, &snapshot.generation)
                    || mutation.is_some_and(|mutation| mutation != FactorAuthorityMutation::Enable)
                {
                    return Err(Error::Unauthorized);
                }
                return authorize_snapshot(snapshot, context, self.current_confirmation(confirm)?);
            }
            let owns_continuation = snapshot.management_hash.as_deref()
                == Some(context.session_binding())
                && snapshot.management_generation.as_deref() == Some(snapshot.generation.as_str());
            if (!context.api_initial_snapshot_matches(&snapshot.source, &snapshot.generation)
                || (snapshot.status == FactorAuthorityStatus::Pending
                    && snapshot.management_hash.is_some()))
                && !owns_continuation
            {
                return Err(Error::Unauthorized);
            }
        }
        authorize_snapshot(snapshot, context, self.current_confirmation(confirm)?)
    }

    async fn finish_management(
        &self,
        transaction: &DatabaseTransaction,
        before: &FactorAuthoritySnapshot,
        after: &FactorAuthoritySnapshot,
        context: &FactorManagementContext,
        confirm: bool,
        mutation: FactorAuthorityMutation,
    ) -> Result<()> {
        self.authorize_context(transaction, before, context, confirm, Some(mutation))
            .await?;
        if context.has_api_authorization() {
            let event = self.management_event.as_ref().ok_or(Error::Unauthorized)?;
            let user_id = before.identifier.as_i64().ok_or(Error::Unauthorized)?;
            event
                .before_commit(self.store.db(), transaction, mutation, user_id)
                .await?;
            // 监听器可以等待或使用同一事务；提交前重核写入结果、原授权和期限。
            ensure_same(after, &self.read_locked(transaction).await?)?;
            self.authorize_context(transaction, before, context, confirm, Some(mutation))
                .await?;
        }
        Ok(())
    }

    /// 仅在来源、代际和三字段仍匹配时管理。
    /// Confirm须经框架受控确认，以保证验证码消费与确认写入同事务。
    pub async fn replace(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        mutation: FactorAuthorityMutation,
    ) -> Result<FactorAuthoritySnapshot> {
        self.replace_inner(before, replacement, mutation, None, false)
            .await
    }

    pub(crate) async fn replace_with_management(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        mutation: FactorAuthorityMutation,
        context: &FactorManagementContext,
        confirm: bool,
    ) -> Result<FactorAuthoritySnapshot> {
        self.replace_inner(before, replacement, mutation, Some(context), confirm)
            .await
    }

    async fn replace_inner(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        mutation: FactorAuthorityMutation,
        context: Option<&FactorManagementContext>,
        confirm: bool,
    ) -> Result<FactorAuthoritySnapshot> {
        let mutation_context = context.cloned();
        let this = self.clone();
        let expected = before.clone();
        let replacement = replacement.clone();
        self.management_transaction(
            before,
            context,
            confirm,
            Some(mutation),
            move |transaction| {
                Box::pin(async move {
                    let current = this.read_locked(transaction).await?;
                    ensure_same(&expected, &current)?;
                    if let Some(context) = &mutation_context {
                        this.authorize_context(
                            transaction,
                            &current,
                            context,
                            confirm,
                            Some(mutation),
                        )
                        .await?;
                    }
                    validate_state(&replacement)?;
                    match mutation {
                        FactorAuthorityMutation::Confirm => return Err(confirmation_required()),
                        FactorAuthorityMutation::Disable => {
                            if replacement != TwoFactorState::default() {
                                return Err(conflict());
                            }
                        }
                        FactorAuthorityMutation::Regenerate => {
                            if current.state.secret.is_none()
                                || replacement.secret != current.state.secret
                                || replacement.confirmed_at != current.state.confirmed_at
                                || replacement.recovery_codes.is_none()
                            {
                                return Err(conflict());
                            }
                        }
                        FactorAuthorityMutation::Enable => {
                            if replacement.secret.is_none() || replacement.recovery_codes.is_none()
                            {
                                return Err(conflict());
                            }
                        }
                    }
                    let generation = random_identity()?;
                    this.write(transaction, &current, &replacement, &generation)
                        .await?;
                    this.prune_consumed(transaction).await?;
                    let after = this.read_locked(transaction).await?;
                    let grant = mutation_context.as_ref().filter(|_| {
                        matches!(
                            mutation,
                            FactorAuthorityMutation::Enable | FactorAuthorityMutation::Regenerate
                        )
                    });
                    this.set_management(
                        transaction,
                        grant.map(|context| (context.session_binding(), after.generation.as_str())),
                    )
                    .await?;
                    let after = this.read_locked(transaction).await?;
                    if let Some(context) = &mutation_context {
                        this.finish_management(
                            transaction,
                            &current,
                            &after,
                            context,
                            confirm,
                            mutation,
                        )
                        .await?;
                    }
                    Ok(after)
                })
            },
        )
        .await
    }
    /// 实际验证码已在事务外验证；确认与重放登记在同一用户事务内提交。
    pub(crate) async fn confirm_verified(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        verified_code: &str,
    ) -> Result<FactorAuthoritySnapshot> {
        self.confirm_inner(before, replacement, verified_code, None, false)
            .await
    }

    pub(crate) async fn confirm_verified_with_management(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        code: &str,
        context: &FactorManagementContext,
        confirm: bool,
    ) -> Result<FactorAuthoritySnapshot> {
        self.confirm_inner(before, replacement, code, Some(context), confirm)
            .await
    }

    async fn confirm_inner(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        verified_code: &str,
        context: Option<&FactorManagementContext>,
        confirm: bool,
    ) -> Result<FactorAuthoritySnapshot> {
        let mutation_context = context.cloned();
        if verified_code.is_empty() {
            return Err(confirmation_required());
        }
        let this = self.clone();
        let expected = before.clone();
        let replacement = replacement.clone();
        let code = verified_code.to_owned();
        self.management_transaction(
            before,
            context,
            confirm,
            Some(FactorAuthorityMutation::Confirm),
            move |transaction| {
                Box::pin(async move {
                    let current = this.read_locked(transaction).await?;
                    ensure_same(&expected, &current)?;
                    if let Some(context) = &mutation_context {
                        this.authorize_context(
                            transaction,
                            &current,
                            context,
                            confirm,
                            Some(FactorAuthorityMutation::Confirm),
                        )
                        .await?;
                    }
                    validate_state(&replacement)?;
                    if current.state.secret.is_none()
                        || replacement.secret != current.state.secret
                        || replacement.recovery_codes != current.state.recovery_codes
                        || replacement.confirmed_at.is_none()
                    {
                        return Err(conflict());
                    }
                    this.consume_code(transaction, &code).await?;
                    this.write(transaction, &current, &replacement, &random_identity()?)
                        .await?;
                    this.set_management(transaction, None).await?;
                    let after = this.read_locked(transaction).await?;
                    if let Some(context) = &mutation_context {
                        this.finish_management(
                            transaction,
                            &current,
                            &after,
                            context,
                            confirm,
                            FactorAuthorityMutation::Confirm,
                        )
                        .await?;
                    }
                    Ok(after)
                })
            },
        )
        .await
    }

    /// 现代验证已在事务外完成；提交前同锁重核，恢复码替换不转代际。
    pub(crate) async fn consume_modern(
        &self,
        before: &FactorAuthoritySnapshot,
        replacement: Option<&TwoFactorState>,
        verified_code: Option<&str>,
    ) -> Result<FactorAuthoritySnapshot> {
        let this = self.clone();
        let before = before.clone();
        let replacement = replacement.cloned();
        let verified_code = verified_code.map(str::to_owned);
        self.store
            .db()
            .transaction_for_update(move |transaction| {
                Box::pin(async move {
                    let current = this.read_locked(transaction).await?;
                    this.consume_modern_locked(
                        transaction,
                        &before,
                        &current,
                        replacement.as_ref(),
                        verified_code.as_deref(),
                    )
                    .await
                })
            })
            .await
    }

    /// 参与同库 users 凭据事务；调用方须已完成现代验证，返回值尚未提交。
    pub(crate) async fn consume_modern_in_transaction(
        &self,
        db: &crate::db::Db,
        transaction: &DatabaseTransaction,
        before: &FactorAuthoritySnapshot,
        replacement: Option<&TwoFactorState>,
        verified_code: Option<&str>,
    ) -> Result<FactorAuthoritySnapshot> {
        let current = self.load_in_transaction(db, transaction).await?;
        self.consume_modern_locked(transaction, before, &current, replacement, verified_code)
            .await
    }

    // 独立事务保留自定义模型表；借用事务入口先核users域，再复用相同消费边界。
    async fn consume_modern_locked(
        &self,
        transaction: &DatabaseTransaction,
        before: &FactorAuthoritySnapshot,
        current: &FactorAuthoritySnapshot,
        replacement: Option<&TwoFactorState>,
        verified_code: Option<&str>,
    ) -> Result<FactorAuthoritySnapshot> {
        ensure_same(before, current)?;
        if !matches!(
            current.status,
            FactorAuthorityStatus::Modern | FactorAuthorityStatus::Pending
        ) {
            return Err(conflict());
        }
        if let Some(replacement) = replacement {
            if verified_code.is_some()
                || replacement.secret != current.state.secret
                || replacement.confirmed_at != current.state.confirmed_at
                || replacement.recovery_codes.is_none()
                || replacement.recovery_codes == current.state.recovery_codes
            {
                return Err(conflict());
            }
            validate_state(replacement)?;
            self.write(
                transaction,
                current,
                replacement,
                &current.persisted_generation,
            )
            .await?;
        } else {
            let code = verified_code
                .filter(|code| !code.is_empty())
                .ok_or_else(|| invalid("现代验证码消费缺少已验证的验证码"))?;
            self.consume_code(transaction, code).await?;
        }
        self.read_locked(transaction).await
    }

    #[allow(
        clippy::too_many_lines,
        reason = "同一锁内核对账户、三字段及元数据，失败前不初始化来源"
    )]
    async fn read_locked(
        &self,
        transaction: &DatabaseTransaction,
    ) -> Result<FactorAuthoritySnapshot> {
        let query = Self::locked(self.store.query()?.limit(2), transaction)?;
        let rows = query.get().await?;
        let row = match rows.as_slice() {
            [row] => row,
            [] => return Err(Error::Unauthorized),
            _ => return Err(conflict()),
        };
        // 数据库collation/数值强转不得把String("42")或大小写近似ID借成另一账户。
        let identifier_matches = match self.store.raw_identifier() {
            Value::String(expected) => {
                row.try_get::<String>("", self.store.identifier_name())
                    .map_err(Error::internal)?
                    == *expected
            }
            Value::Number(expected) => {
                if let Some(expected) = expected.as_i64() {
                    row.try_get::<i64>("", self.store.identifier_name())
                        .map_err(Error::internal)?
                        == expected
                } else if let Some(expected) = expected.as_u64() {
                    row.try_get::<u64>("", self.store.identifier_name())
                        .map_err(Error::internal)?
                        == expected
                } else {
                    false
                }
            }
            _ => false,
        };
        if !identifier_matches {
            return Err(Error::Unauthorized);
        }
        let columns = row.column_names();
        if [
            SOURCE,
            GENERATION,
            CONSUMED,
            MANAGEMENT_HASH,
            MANAGEMENT_GENERATION,
            MANAGEMENT_AT,
        ]
        .iter()
        .any(|field| !columns.iter().any(|column| column == field))
        {
            return Err(invalid("账户缺少authority元数据字段"));
        }
        let state = super::database::decode_state(row)?;
        validate_state(&state)?;
        let source: Option<String> = row.try_get("", SOURCE).map_err(Error::internal)?;
        let generation: Option<String> = row.try_get("", GENERATION).map_err(Error::internal)?;
        let consumed_text: Option<String> = row.try_get("", CONSUMED).map_err(Error::internal)?;
        drop(decode_consumed(consumed_text.as_deref())?);
        let management_hash: Option<String> =
            row.try_get("", MANAGEMENT_HASH).map_err(Error::internal)?;
        let management_generation: Option<String> = row
            .try_get("", MANAGEMENT_GENERATION)
            .map_err(Error::internal)?;
        let management_at: Option<DateTime<Utc>> =
            row.try_get("", MANAGEMENT_AT).map_err(Error::internal)?;
        match (&management_hash, &management_generation, management_at) {
            (None, None, None) => {}
            (Some(hash), Some(generation), Some(_))
                if valid_identity(hash) && valid_effective_generation(generation) => {}
            _ => return Err(invalid("因子管理授权元数据不完整")),
        }
        let status = if state.secret.is_none() {
            FactorAuthorityStatus::Disabled
        } else if state.confirmed_at.is_none() {
            FactorAuthorityStatus::Pending
        } else {
            FactorAuthorityStatus::Modern
        };
        let (source, generation) = match (source, generation) {
            (Some(source), Some(generation))
                if valid_identity(&source) && valid_identity(&generation) =>
            {
                (source, generation)
            }
            (None, None) if consumed_text.is_none() && management_hash.is_none() => {
                let source = random_identity()?;
                let generation = random_identity()?;
                let affected = self
                    .store
                    .query()?
                    .using_transaction(transaction)
                    .where_null(SOURCE)?
                    .where_null(GENERATION)?
                    .update(vec![
                        (SOURCE, source.clone().into()),
                        (GENERATION, generation.clone().into()),
                    ])
                    .await?;
                if affected != 1 {
                    return Err(conflict());
                }
                (source, generation)
            }
            _ => return Err(invalid("authority来源或代际不完整")),
        };
        let confirmed_storage = if transaction.get_database_backend() == DbBackend::Sqlite {
            row.try_get::<Option<String>>("", "two_factor_confirmed_at")
                .map_err(Error::internal)?
        } else {
            None
        };
        let effective = effective_generation(&generation, &state)?;
        Ok(FactorAuthoritySnapshot {
            state,
            source,
            generation: effective,
            persisted_generation: generation,
            status,
            identifier: self.store.raw_identifier().clone(),
            confirmed_storage,
            management_hash,
            management_generation,
            management_at,
        })
    }

    async fn prune_consumed(&self, transaction: &DatabaseTransaction) -> Result<()> {
        let row = self
            .store
            .query()?
            .using_transaction(transaction)
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        let text: Option<String> = row.try_get("", CONSUMED).map_err(Error::internal)?;
        let mut consumed = decode_consumed(text.as_deref())?;
        let now = u64::try_from(Utc::now().timestamp()).map_err(Error::internal)?;
        consumed.retain(|_, expires| *expires > now);
        let updated = if consumed.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&consumed).map_err(Error::internal)?)
        };
        if updated == text {
            return Ok(());
        }
        self.set_consumed(transaction, updated).await
    }

    async fn set_management(
        &self,
        transaction: &DatabaseTransaction,
        grant: Option<(&str, &str)>,
    ) -> Result<()> {
        let clearing = grant.is_none();
        let expected_change = if clearing {
            let row = self
                .store
                .query()?
                .using_transaction(transaction)
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            row.try_get::<Option<String>>("", MANAGEMENT_HASH)
                .map_err(Error::internal)?
                .is_some()
        } else {
            true
        };
        let (hash, generation, at) = match grant {
            Some((hash, generation)) => (
                Some(hash.to_owned()),
                Some(generation.to_owned()),
                Some(Utc::now()),
            ),
            None => (None, None, None),
        };
        let affected = self
            .store
            .query()?
            .using_transaction(transaction)
            .update(vec![
                (MANAGEMENT_HASH, hash.into()),
                (MANAGEMENT_GENERATION, generation.into()),
                (MANAGEMENT_AT, SqlValue::ChronoDateTimeUtc(at)),
            ])
            .await?;
        if affected == 1 || (!expected_change && affected == 0) {
            Ok(())
        } else {
            Err(conflict())
        }
    }

    async fn consume_code(&self, transaction: &DatabaseTransaction, code: &str) -> Result<()> {
        if let Some(verification) = &self.provider_verification {
            verification.require_code(code)?;
        }
        let row = self
            .store
            .query()?
            .using_transaction(transaction)
            .first()
            .await?
            .ok_or(Error::Unauthorized)?;
        let text: Option<String> = row.try_get("", CONSUMED).map_err(Error::internal)?;
        let mut consumed = decode_consumed(text.as_deref())?;
        let now = u64::try_from(Utc::now().timestamp()).map_err(Error::internal)?;
        consumed.retain(|_, expires| *expires > now);
        let hash = format!("{:x}", Sha256::digest(code.as_bytes()));
        if consumed.contains_key(&hash) {
            return Err(Error::Unauthorized);
        }
        let expires = now
            .checked_add(self.consumed_ttl_seconds)
            .ok_or_else(|| invalid("验证码摘要有效期溢出"))?;
        if i64::try_from(expires)
            .ok()
            .and_then(|expires| DateTime::<Utc>::from_timestamp(expires, 0))
            .is_none()
        {
            return Err(invalid("验证码摘要有效期无效"));
        }
        consumed.insert(hash, expires);
        self.set_consumed(
            transaction,
            Some(serde_json::to_string(&consumed).map_err(Error::internal)?),
        )
        .await
    }

    async fn set_consumed(
        &self,
        transaction: &DatabaseTransaction,
        value: Option<String>,
    ) -> Result<()> {
        let clearing = value.is_none();
        let affected = self
            .store
            .query()?
            .using_transaction(transaction)
            .update(vec![(CONSUMED, value.into())])
            .await?;
        if affected == 1 || (clearing && affected == 0) {
            Ok(())
        } else {
            Err(conflict())
        }
    }

    fn locked<'a>(
        query: QueryBuilder<'static>,
        transaction: &'a DatabaseTransaction,
    ) -> Result<QueryBuilder<'a>> {
        let query = query.using_transaction(transaction);
        if transaction.get_database_backend() == DbBackend::Sqlite {
            Ok(query)
        } else {
            query.lock_for_update()
        }
    }

    async fn write(
        &self,
        transaction: &DatabaseTransaction,
        before: &FactorAuthoritySnapshot,
        replacement: &TwoFactorState,
        generation: &str,
    ) -> Result<()> {
        let backend = transaction.get_database_backend();
        let query = self
            .store
            .query()?
            .where_eq(SOURCE, before.source.clone())?
            .where_eq(GENERATION, before.persisted_generation.clone())?;
        let query = super::database::match_ciphertext(
            query,
            backend,
            "two_factor_secret",
            before.state.secret.as_deref(),
        )?;
        let query = super::database::match_ciphertext(
            query,
            backend,
            "two_factor_recovery_codes",
            before.state.recovery_codes.as_deref(),
        )?;
        let query = match before.state.confirmed_at {
            None => query.where_null("two_factor_confirmed_at")?,
            Some(_) if backend == DbBackend::Sqlite => query.where_raw(
                RawSql::new("CAST(\"two_factor_confirmed_at\" AS BLOB)=CAST(? AS BLOB)"),
                vec![
                    before
                        .confirmed_storage
                        .clone()
                        .ok_or_else(conflict)?
                        .into(),
                ],
            ),
            Some(value) => query.where_eq(
                "two_factor_confirmed_at",
                SqlValue::ChronoDateTimeUtc(Some(value)),
            )?,
        };
        let affected = query
            .using_transaction(transaction)
            .update(vec![
                ("two_factor_secret", replacement.secret.clone().into()),
                (
                    "two_factor_recovery_codes",
                    replacement.recovery_codes.clone().into(),
                ),
                (
                    "two_factor_confirmed_at",
                    SqlValue::ChronoDateTimeUtc(replacement.confirmed_at),
                ),
                (GENERATION, generation.into()),
            ])
            .await?;
        if affected != 1 {
            return Err(conflict());
        }
        Ok(())
    }
}

fn authorize_snapshot(
    snapshot: &FactorAuthoritySnapshot,
    context: &FactorManagementContext,
    confirm: bool,
) -> Result<()> {
    if context.identifier() != &snapshot.identifier {
        return Err(Error::Unauthorized);
    }
    if !snapshot.challenge_required(confirm)
        || context.factor_proof().is_some_and(|proof| {
            proof.matches(
                context.identifier(),
                context.guard(),
                context.provider(),
                &snapshot.source,
                &snapshot.generation,
            )
        })
        || (snapshot.management_hash.as_deref() == Some(context.session_binding())
            && snapshot.management_generation.as_deref() == Some(snapshot.generation.as_str())
            && snapshot.management_at.is_some())
    {
        return Ok(());
    }
    Err(Error::Forbidden("当前凭据没有因子管理授权".into()))
}
fn valid_effective_generation(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(random, digest)| valid_identity(random) && valid_identity(digest))
}

fn decode_consumed(text: Option<&str>) -> Result<BTreeMap<String, u64>> {
    use serde::de::{MapAccess, Visitor};
    struct Codes;
    impl<'de> Visitor<'de> for Codes {
        type Value = BTreeMap<String, u64>;
        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("验证码摘要与Unix有效期对象")
        }
        fn visit_map<M: MapAccess<'de>>(
            self,
            mut map: M,
        ) -> std::result::Result<Self::Value, M::Error> {
            let mut result = BTreeMap::new();
            while let Some((hash, expiry)) = map.next_entry::<String, u64>()? {
                if !valid_identity(&hash)
                    || i64::try_from(expiry)
                        .ok()
                        .and_then(|expiry| DateTime::<Utc>::from_timestamp(expiry, 0))
                        .is_none()
                    || result.insert(hash, expiry).is_some()
                {
                    return Err(serde::de::Error::custom("验证码消费账本格式无效"));
                }
            }
            Ok(result)
        }
    }
    let Some(text) = text else {
        return Ok(BTreeMap::new());
    };
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let codes = serde::de::Deserializer::deserialize_map(&mut deserializer, Codes)
        .map_err(|_| invalid("验证码消费账本损坏"))?;
    deserializer
        .end()
        .map_err(|_| invalid("验证码消费账本尾部无效"))?;
    Ok(codes)
}

fn effective_generation(generation: &str, state: &TwoFactorState) -> Result<String> {
    // 单次恢复码消费仅更新恢复码密文，其他已验证会话保留原代际。
    let encoded = serde_json::to_vec(&(state.secret.as_deref(), state.confirmed_at))
        .map_err(Error::internal)?;
    Ok(format!("{generation}:{:x}", Sha256::digest(encoded)))
}

fn ensure_same(before: &FactorAuthoritySnapshot, current: &FactorAuthoritySnapshot) -> Result<()> {
    if before != current {
        return Err(conflict());
    }
    Ok(())
}
fn validate_state(state: &TwoFactorState) -> Result<()> {
    if state
        .secret
        .as_ref()
        .is_some_and(|value| value.is_empty() || value == "0")
        || state
            .recovery_codes
            .as_ref()
            .is_some_and(|value| value.is_empty() || value == "0")
        || (state.secret.is_none()
            && (state.recovery_codes.is_some() || state.confirmed_at.is_some()))
    {
        return Err(invalid("现代因子状态不完整或损坏"));
    }
    Ok(())
}
fn valid_identity(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn random_identity() -> Result<String> {
    const DIGITS: &[u8] = b"0123456789abcdef";
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(Error::internal)?;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    Ok(result)
}

fn conflict() -> Error {
    Error::Conflict("因子状态已变化，请重新开始操作".into())
}
fn confirmation_required() -> Error {
    Error::custom_with_status(
        crate::http::axum::http::StatusCode::FORBIDDEN,
        "fortify_verified_confirmation_required",
        "确认必须通过已验证验证码的受控原子操作",
    )
}
fn invalid(message: &'static str) -> Error {
    Error::custom_with_status(
        crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "fortify_authority_state_invalid",
        message,
    )
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "authority/tests.rs"]
mod tests;

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
#[path = "authority/real_server_tests.rs"]
mod real_server_tests;
