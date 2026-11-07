//! 官方 OAuth 表的原子协议存储增强；不是完整 Eloquent save/events 生命周期。
//!
//! 对照 Passport v13.10.0 Bridge 的 AuthCode/AccessToken/RefreshTokenRepository
//! 及 League 9.4.1 AuthCodeGrant/RefreshTokenGrant。单次消费、统一锁序与提交末检
//! 是 OpenRust 安全增强，不宣称固定上游已经提供同等原子事务。

use std::sync::Arc;

use openrust::{
    Error, Result,
    database::{
        DatabaseTransaction, Databases, Db, DbBackend, QueryBuilder, TransactionContext,
        eloquent::{AttributeCast, attributes_from_row},
        sea_orm::{
            Value as SqlValue,
            prelude::{DateTime, DateTimeUtc},
        },
    },
};
use serde_json::{Map, Value};

use crate::grant_clock::{GrantClock, SystemGrantClock};
#[cfg(test)]
use crate::grant_records::RefreshRecord;
use crate::grant_records::{
    AccessRecord, ClientSnapshot, CodeRecord, DatabaseGrantAuthority, GrantAuthorityBinding,
    GrantOperation, GrantUserKey, GrantValidation, TokenRecords,
};
use crate::protocol_token::RefreshPayload;
use crate::{ClientLookup, ClientLookupContext, ClientModelFactory, ClientRecord};
use openrust::container::Resolver;

// 同一checked事务两阶段的借用参数，不持有或新建连接。
struct RefreshOperation<'a> {
    payload: &'a RefreshPayload,
    expected_access: Option<&'a AccessRecord>,
    snapshot: &'a ClientSnapshot,
    tokens: &'a TokenRecords,
    authority: &'a GrantAuthorityBinding,
    revoke_old_refresh: bool,
}

/// 以同一主库事务操作官方 OAuth 表的默认原子存储。
///
/// 通过所选模型的 `metadata().resolve_connection` 选择唯一 Db，
/// 所有客户端、授权码和令牌查询复用该句柄。
/// 用户权威扩展同样只能使用传入事务；跨库权威需要显式等价保证。
#[derive(Clone)]
pub struct DatabaseGrantRepository {
    db: Db,
    factory: Arc<dyn ClientModelFactory>,
    user_key: GrantUserKey,
    authority: Arc<DatabaseGrantAuthority>,
    clock: Arc<dyn GrantClock>,
    clients: Arc<dyn ClientLookup>,
    resolver: Resolver,
}

impl DatabaseGrantRepository {
    /// 绑定已按客户端模型选定的 Db、主键策略与同事务用户权威。
    ///
    /// 只校验同一表名解析器；不会创建连接、迁移或把声明中的其他连接偷偷打开。
    /// clients及resolver必须来自同一应用当前scoped工厂；手动SDK也须安装公开缓存注册。
    /// Resolver克隆仅含弱Container引用，不证明非root或任意外传依赖的同源性。
    pub fn new(
        databases: &Databases,
        passport_connection: Option<&str>,
        factory: Arc<dyn ClientModelFactory>,
        user_key: GrantUserKey,
        authority: Arc<DatabaseGrantAuthority>,
        clients: Arc<dyn ClientLookup>,
        resolver: Resolver,
    ) -> Result<Self> {
        let db = factory
            .metadata()
            .resolve_connection(databases, passport_connection)?;
        for table in [
            factory.metadata().table.as_str(),
            "oauth_auth_codes",
            "oauth_access_tokens",
            "oauth_refresh_tokens",
        ] {
            db.table_names().resolve(table)?;
        }
        Ok(Self {
            db,
            factory,
            user_key,
            authority,
            clock: Arc::new(SystemGrantClock),
            clients,
            resolver,
        })
    }

    /// 覆盖权威时钟，供可信应用的统一时钟与确定性验证使用。
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn GrantClock>) -> Self {
        self.clock = clock;
        self
    }

    // 同步clock不证明scope仍开放；异步入口另主动解析原scoped私有cache。
    pub(crate) fn current_time(&self) -> Result<i64> {
        self.clock.now()
    }

    pub(crate) async fn client_by_protocol(
        &self,
        identifier: &str,
    ) -> Result<Option<ClientRecord>> {
        let context = ClientLookupContext::from(&self.resolver);
        context.ensure_open().await?;
        self.clients
            .find_active_by_protocol(&context, identifier)
            .await
    }

    async fn ensure_open(&self) -> Result<()> {
        ClientLookupContext::from(&self.resolver)
            .ensure_open()
            .await
    }

    #[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
    pub(crate) async fn client(&self, identifier: &Value) -> Result<Option<ClientRecord>> {
        let context = ClientLookupContext::from(&self.resolver);
        context.ensure_open().await?;
        self.clients.find(&context, identifier).await
    }

    pub(crate) async fn persist_code(
        &self,
        code: &CodeRecord,
        client: &ClientSnapshot,
    ) -> Result<()> {
        self.ensure_open().await?;
        check_identity(&code.client_id, client)?;
        let work = self.clone();
        let final_check = self.clone();
        let code = code.clone();
        let final_code = code.clone();
        let snapshot = client.clone();
        let final_snapshot = client.clone();
        self.db
            .transaction_for_update_checked_context(
                move |db_context| {
                    Box::pin(async move {
                        let tx = db_context.transaction();
                        work.check_client(tx, &snapshot).await?;
                        let context = GrantValidation {
                            user_id: &code.user_id,
                            client_id: snapshot.entity.get_identifier(),
                            scopes: &code.scopes,
                            operation: GrantOperation::Authorize,
                        };
                        work.authority
                            .validate(&db_context, &context, &code.authority)
                            .await?;
                        work.db
                            .table("oauth_auth_codes")?
                            .using_transaction(tx)
                            .insert(work.code_values(&code, &snapshot)?)
                            .await?;
                        Ok(())
                    })
                },
                move |db_context| {
                    Box::pin(async move {
                        let tx = db_context.transaction();
                        let context = GrantValidation {
                            user_id: &final_code.user_id,
                            client_id: final_snapshot.entity.get_identifier(),
                            scopes: &final_code.scopes,
                            operation: GrantOperation::Authorize,
                        };
                        final_check
                            .final_check(
                                &db_context,
                                &final_snapshot,
                                &context,
                                &final_code.authority,
                            )
                            .await?;
                        final_check
                            .final_code(tx, &final_code, &final_snapshot, false)
                            .await?;
                        final_check.ensure_open().await?;
                        final_check.check_final_times(Some(final_code.expires_at), None, None)
                    })
                },
            )
            .await
    }

    pub(crate) async fn exchange_code(
        &self,
        code: &CodeRecord,
        client: &ClientSnapshot,
        tokens: &TokenRecords,
        _now: i64,
    ) -> Result<()> {
        self.ensure_open().await?;
        check_identity(&code.client_id, client)?;
        check_new_tokens(
            tokens,
            &code.client_id,
            &code.user_id,
            &code.scopes,
            &code.authority,
        )?;
        let work = self.clone();
        let final_check = self.clone();
        let code = code.clone();
        let tokens = tokens.clone();
        let final_code = code.clone();
        let final_tokens = tokens.clone();
        let snapshot = client.clone();
        let final_snapshot = client.clone();
        self.db
            .transaction_for_update_checked_context(
                move |db_context| {
                    Box::pin(async move {
                        let tx = db_context.transaction();
                        work.check_client(tx, &snapshot).await?;
                        let row = work
                            .locked(tx, "oauth_auth_codes")?
                            .where_eq("id", code.id.clone())?
                            .first()
                            .await?
                            .ok_or_else(|| failure("passport_code_revoked"))?;
                        let row = attributes_from_row(&row)?;
                        check_revoked(&row, "passport_code_revoked")?;
                        work.check_code_row(&row, &code, &snapshot, work.current_time()?)?;
                        let context = GrantValidation {
                            user_id: &code.user_id,
                            client_id: snapshot.entity.get_identifier(),
                            scopes: &tokens.access.scopes,
                            operation: GrantOperation::Redeem,
                        };
                        work.authority
                            .validate(&db_context, &context, &code.authority)
                            .await?;
                        let affected = work
                            .code_cas(tx, &code, &snapshot, &row)?
                            .update(vec![("revoked", true.into())])
                            .await?;
                        require_one(affected, "passport_code_revoked")?;
                        work.insert_tokens(tx, &tokens, &snapshot).await
                    })
                },
                move |db_context| {
                    Box::pin(async move {
                        let tx = db_context.transaction();
                        let context = GrantValidation {
                            user_id: &final_code.user_id,
                            client_id: final_snapshot.entity.get_identifier(),
                            scopes: &final_tokens.access.scopes,
                            operation: GrantOperation::Redeem,
                        };
                        final_check
                            .final_check(
                                &db_context,
                                &final_snapshot,
                                &context,
                                &final_code.authority,
                            )
                            .await?;
                        final_check
                            .final_code(tx, &final_code, &final_snapshot, true)
                            .await?;
                        final_check
                            .final_tokens(tx, &final_tokens, &final_snapshot)
                            .await?;
                        final_check.ensure_open().await?;
                        final_check.check_final_times(
                            Some(final_code.expires_at),
                            None,
                            Some(&final_tokens),
                        )
                    })
                },
            )
            .await
    }

    #[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
    pub(crate) async fn rotate_refresh(
        &self,
        refresh: &RefreshRecord,
        client: &ClientSnapshot,
        tokens: &TokenRecords,
        now: i64,
        revoke_old_refresh: bool,
    ) -> Result<()> {
        let payload = RefreshPayload {
            client_id: refresh.access.client_id.clone(),
            refresh_token_id: refresh.id.clone(),
            access_token_id: refresh.access.id.clone(),
            scopes: refresh.access.scopes.clone(),
            user_id: refresh.access.user_id.clone(),
            expire_time: refresh.expires_at,
            authority_binding: refresh.access.authority.as_str().to_owned(),
        };
        self.rotate_refresh_inner(
            &payload,
            Some(&refresh.access),
            client,
            tokens,
            now,
            revoke_old_refresh,
        )
        .await
    }

    pub(crate) async fn rotate_refresh_payload(
        &self,
        payload: &RefreshPayload,
        client: &ClientSnapshot,
        tokens: &TokenRecords,
        now: i64,
        revoke_old_refresh: bool,
    ) -> Result<()> {
        self.rotate_refresh_inner(payload, None, client, tokens, now, revoke_old_refresh)
            .await
    }

    async fn rotate_refresh_inner(
        &self,
        payload: &RefreshPayload,
        expected_access: Option<&AccessRecord>,
        client: &ClientSnapshot,
        tokens: &TokenRecords,
        _now: i64,
        revoke_old_refresh: bool,
    ) -> Result<()> {
        self.ensure_open().await?;
        check_identity(&payload.client_id, client)?;
        let authority = payload.authority()?;
        check_new_tokens(
            tokens,
            &payload.client_id,
            &payload.user_id,
            &payload.scopes,
            &authority,
        )?;
        let final_authority = authority.clone();
        let work = self.clone();
        let final_check = self.clone();
        let payload = payload.clone();
        let final_payload = payload.clone();
        let expected_access = expected_access.cloned();
        let tokens = tokens.clone();
        let final_tokens = tokens.clone();
        let snapshot = client.clone();
        let final_snapshot = client.clone();
        self.db
            .transaction_for_update_checked_context(
                move |db_context| {
                    Box::pin(async move {
                        let operation = RefreshOperation {
                            payload: &payload,
                            expected_access: expected_access.as_ref(),
                            snapshot: &snapshot,
                            tokens: &tokens,
                            authority: &authority,
                            revoke_old_refresh,
                        };
                        work.refresh_in_transaction(&db_context, &operation).await
                    })
                },
                move |db_context| {
                    Box::pin(async move {
                        let operation = RefreshOperation {
                            payload: &final_payload,
                            expected_access: None,
                            snapshot: &final_snapshot,
                            tokens: &final_tokens,
                            authority: &final_authority,
                            revoke_old_refresh,
                        };
                        final_check.finalize_refresh(&db_context, &operation).await
                    })
                },
            )
            .await
    }

    async fn refresh_in_transaction(
        &self,
        db_context: &TransactionContext<'_>,
        operation: &RefreshOperation<'_>,
    ) -> Result<()> {
        let tx = db_context.transaction();
        let payload = operation.payload;
        let snapshot = operation.snapshot;
        let tokens = operation.tokens;
        let authority = operation.authority;
        let revoke_old_refresh = operation.revoke_old_refresh;
        self.check_client(tx, snapshot).await?;
        let row = self
            .required_row(
                tx,
                "oauth_refresh_tokens",
                &payload.refresh_token_id,
                "passport_refresh_revoked",
            )
            .await?;
        check_revoked(&row, "passport_refresh_revoked")?;
        check_refresh_payload_row(&row, payload)?;
        check_expiry(payload.expire_time, self.current_time()?)?;
        let access_row = self
            .required_row(
                tx,
                "oauth_access_tokens",
                &payload.access_token_id,
                "passport_grant_binding_invalid",
            )
            .await?;
        // 只核对密文实际携带的字段；旧access的iat/exp不在固定刷新负载中。
        self.check_refresh_access(&access_row, payload, snapshot)?;
        if let Some(expected) = operation.expected_access {
            self.check_access_row(&access_row, expected, snapshot)?;
        }
        let context = GrantValidation {
            user_id: &payload.user_id,
            client_id: snapshot.entity.get_identifier(),
            scopes: &tokens.access.scopes,
            operation: GrantOperation::Refresh,
        };
        self.authority
            .validate(db_context, &context, authority)
            .await?;
        if revoke_old_refresh {
            let affected = self
                .db
                .table("oauth_refresh_tokens")?
                .using_transaction(tx)
                .where_eq("id", payload.refresh_token_id.clone())?
                .where_eq("revoked", false)?
                .where_eq("access_token_id", payload.access_token_id.clone())?
                .where_eq("expires_at", timestamp(payload.expire_time)?)?
                .update(vec![("revoked", true.into())])
                .await?;
            require_one(affected, "passport_refresh_revoked")?;
        }
        // 显式关闭refresh撤销允许重复使用；仍由行锁串行化此次事务。
        self.db
            .table("oauth_access_tokens")?
            .using_transaction(tx)
            .where_eq("id", payload.access_token_id.clone())?
            .update(vec![("revoked", true.into())])
            .await?;
        self.insert_tokens(tx, tokens, snapshot).await
    }

    async fn finalize_refresh(
        &self,
        db_context: &TransactionContext<'_>,
        operation: &RefreshOperation<'_>,
    ) -> Result<()> {
        let tx = db_context.transaction();
        let payload = operation.payload;
        let snapshot = operation.snapshot;
        let tokens = operation.tokens;
        let authority = operation.authority;
        let revoke_old_refresh = operation.revoke_old_refresh;
        let context = GrantValidation {
            user_id: &payload.user_id,
            client_id: snapshot.entity.get_identifier(),
            scopes: &tokens.access.scopes,
            operation: GrantOperation::Refresh,
        };
        self.final_check(db_context, snapshot, &context, authority)
            .await?;
        let row = self
            .required_row(
                tx,
                "oauth_refresh_tokens",
                &payload.refresh_token_id,
                "passport_grant_binding_invalid",
            )
            .await?;
        check_refresh_payload_row(&row, payload)?;
        require_revoked(&row, revoke_old_refresh)?;
        let old_access = self
            .required_row(
                tx,
                "oauth_access_tokens",
                &payload.access_token_id,
                "passport_grant_binding_invalid",
            )
            .await?;
        self.check_refresh_access(&old_access, payload, snapshot)?;
        require_revoked(&old_access, true)?;
        self.final_tokens(tx, tokens, snapshot).await?;
        self.ensure_open().await?;
        self.check_final_times(None, Some(payload.expire_time), Some(tokens))
    }

    /// 普通读路由上的授权码早检；最终主库事务仍独立复核。
    pub async fn is_code_revoked(&self, id: &str) -> Result<bool> {
        self.ensure_open().await?;
        self.db
            .table("oauth_auth_codes")?
            .where_eq("id", id.to_owned())?
            .where_eq("revoked", false)?
            .doesnt_exist()
            .await
    }

    /// 普通读路由上的refresh早检；不通过旧access撤销状态推断。
    pub async fn is_refresh_revoked(&self, id: &str) -> Result<bool> {
        self.ensure_open().await?;
        self.db
            .table("oauth_refresh_tokens")?
            .where_eq("id", id.to_owned())?
            .where_eq("revoked", false)?
            .doesnt_exist()
            .await
    }

    /// 从主库查询access撤销状态；不存在视为已撤销，数据库或解码错误原样传播。
    ///
    /// 与官方Repository一致，此方法不代替资源服务对签名、期限及claims的验证。
    pub async fn is_access_revoked(&self, identifier: &str) -> Result<bool> {
        self.ensure_open().await?;
        let query = self
            .db
            .table("oauth_access_tokens")?
            .where_eq("id", identifier.to_owned())?;
        let row = self.db.with_primary(query.first()).await??;
        match row {
            None => Ok(true),
            Some(row) => revoked(&attributes_from_row(&row)?),
        }
    }

    /// 在同一主库事务中按client→access锁序撤销access；不存在时成功返回。
    ///
    /// 此为原子协议存储增强，不宣称已实现完整Eloquent模型事件生命周期。
    pub async fn revoke_access(&self, identifier: &str) -> Result<()> {
        self.ensure_open().await?;
        // 预读只确定锁序中的 client；锁定后重验关联，不能信任预读状态。
        let query = self
            .db
            .table("oauth_access_tokens")?
            .where_eq("id", identifier.to_owned())?;
        let Some(row) = self.db.with_primary(query.first()).await?? else {
            return Ok(());
        };
        let row = attributes_from_row(&row)?;
        let client_id = row.get("client_id").cloned().ok_or_else(binding_invalid)?;
        let work = self.clone();
        let final_check = self.clone();
        let id = identifier.to_owned();
        self.db
            .transaction_for_update_checked_context(
                move |db_context| {
                    Box::pin(async move {
                        let tx = db_context.transaction();
                        work.load_client(tx, &client_id).await?;
                        let Some(row) = work
                            .locked(tx, "oauth_access_tokens")?
                            .where_eq("id", id.clone())?
                            .first()
                            .await?
                        else {
                            return Ok(());
                        };
                        let row = attributes_from_row(&row)?;
                        work.check_client_binding(&row, &client_id)?;
                        work.db
                            .table("oauth_access_tokens")?
                            .using_transaction(tx)
                            .where_eq("id", id)?
                            .update(vec![("revoked", true.into())])
                            .await?;
                        Ok(())
                    })
                },
                move |_db_context| Box::pin(async move { final_check.ensure_open().await }),
            )
            .await
    }

    fn locked<'a>(&self, tx: &'a DatabaseTransaction, table: &str) -> Result<QueryBuilder<'a, 'a>> {
        let query = self.db.table(table)?.using_transaction(tx);
        if self.db.backend()? == DbBackend::Sqlite {
            Ok(query)
        } else {
            query.lock_for_update()
        }
    }

    async fn load_client(
        &self,
        tx: &DatabaseTransaction,
        identifier: &Value,
    ) -> Result<Option<ClientRecord>> {
        let metadata = self.factory.metadata();
        let row = self
            .locked(tx, &metadata.table)?
            .where_eq(
                &metadata.key_name,
                self.factory
                    .identifier_binding(identifier, self.db.backend()?)?,
            )?
            .first()
            .await?;
        row.map(|row| self.factory.new_from_builder(attributes_from_row(&row)?))
            .transpose()
    }

    async fn check_client(
        &self,
        tx: &DatabaseTransaction,
        expected: &ClientSnapshot,
    ) -> Result<()> {
        let current = self
            .load_client(tx, &expected.identifier)
            .await?
            .ok_or_else(|| failure("passport_client_changed"))?;
        if !expected.matches(&current)? {
            return Err(failure("passport_client_changed"));
        }
        Ok(())
    }

    async fn final_check(
        &self,
        db_context: &TransactionContext<'_>,
        expected: &ClientSnapshot,
        context: &GrantValidation<'_>,
        binding: &GrantAuthorityBinding,
    ) -> Result<()> {
        let tx = db_context.transaction();
        self.ensure_open().await?;
        // 框架 checked 事务保证此处位于全部直接 BeforeCommit 监听器之后。
        self.authority
            .validate(db_context, context, binding)
            .await?;
        // 扩展契约要求只读；仍在回调之后重读客户端，拒绝可观察的安全状态改写。
        self.check_client(tx, expected).await
    }

    async fn required_row(
        &self,
        tx: &DatabaseTransaction,
        table: &str,
        id: &str,
        missing_code: &'static str,
    ) -> Result<Map<String, Value>> {
        let row = self
            .locked(tx, table)?
            .where_eq("id", id.to_owned())?
            .first()
            .await?
            .ok_or_else(|| failure(missing_code))?;
        attributes_from_row(&row)
    }

    async fn final_code(
        &self,
        tx: &DatabaseTransaction,
        code: &CodeRecord,
        snapshot: &ClientSnapshot,
        consumed: bool,
    ) -> Result<()> {
        let row = self
            .required_row(
                tx,
                "oauth_auth_codes",
                &code.id,
                "passport_grant_binding_invalid",
            )
            .await?;
        self.check_owner(&row, &code.user_id, snapshot)?;
        check_metadata(&row, &code.id, &code.scopes, code.expires_at)?;
        require_revoked(&row, consumed)
    }

    async fn final_tokens(
        &self,
        tx: &DatabaseTransaction,
        tokens: &TokenRecords,
        snapshot: &ClientSnapshot,
    ) -> Result<()> {
        if let Some(refresh) = &tokens.refresh {
            let row = self
                .required_row(
                    tx,
                    "oauth_refresh_tokens",
                    &refresh.id,
                    "passport_grant_binding_invalid",
                )
                .await?;
            let expected = RefreshPayload {
                client_id: refresh.access.client_id.clone(),
                refresh_token_id: refresh.id.clone(),
                access_token_id: refresh.access.id.clone(),
                scopes: refresh.access.scopes.clone(),
                user_id: refresh.access.user_id.clone(),
                expire_time: refresh.expires_at,
                authority_binding: refresh.access.authority.as_str().to_owned(),
            };
            check_refresh_payload_row(&row, &expected)?;
            require_revoked(&row, false)?;
        }
        let row = self
            .required_row(
                tx,
                "oauth_access_tokens",
                &tokens.access.id,
                "passport_grant_binding_invalid",
            )
            .await?;
        self.check_access_row(&row, &tokens.access, snapshot)?;
        require_revoked(&row, false)
    }

    // 必须位于最终异步查询之后；从这里返回到框架开始commit前不再await。
    // 不承诺数据库网络commit完成时仍未跨过期限。
    fn check_final_times(
        &self,
        code_expiry: Option<i64>,
        refresh_expiry: Option<i64>,
        tokens: Option<&TokenRecords>,
    ) -> Result<()> {
        let now = self.current_time()?;
        for expires in [code_expiry, refresh_expiry].into_iter().flatten() {
            check_expiry(expires, now)?;
        }
        if let Some(tokens) = tokens {
            if tokens.access.expires_at <= now {
                return Err(failure("passport_grant_expired"));
            }
            if let Some(refresh) = &tokens.refresh {
                check_expiry(refresh.expires_at, now)?;
            }
        }
        Ok(())
    }

    fn check_refresh_access(
        &self,
        row: &Map<String, Value>,
        payload: &RefreshPayload,
        snapshot: &ClientSnapshot,
    ) -> Result<()> {
        self.check_owner(row, &payload.user_id, snapshot)?;
        if scalar(row, "id")? != payload.access_token_id || scopes(row)? != payload.scopes {
            return Err(binding_invalid());
        }
        Ok(())
    }

    fn check_client_binding(&self, row: &Map<String, Value>, expected: &Value) -> Result<()> {
        let actual = row.get("client_id").ok_or_else(binding_invalid)?;
        if self
            .factory
            .identifier_binding(actual, self.db.backend()?)?
            != self
                .factory
                .identifier_binding(expected, self.db.backend()?)?
        {
            return Err(binding_invalid());
        }
        Ok(())
    }

    fn check_owner(
        &self,
        row: &Map<String, Value>,
        user: &str,
        snapshot: &ClientSnapshot,
    ) -> Result<()> {
        self.check_client_binding(row, &snapshot.identifier)?;
        let actual = scalar(row, "user_id")?;
        if self.user_key.binding(&actual, self.db.backend()?)?
            != self.user_key.binding(user, self.db.backend()?)?
        {
            return Err(binding_invalid());
        }
        Ok(())
    }

    fn check_code_row(
        &self,
        row: &Map<String, Value>,
        code: &CodeRecord,
        snapshot: &ClientSnapshot,
        now: i64,
    ) -> Result<()> {
        self.check_owner(row, &code.user_id, snapshot)?;
        check_metadata(row, &code.id, &code.scopes, code.expires_at)?;
        check_expiry(code.expires_at, now)
    }

    fn check_access_row(
        &self,
        row: &Map<String, Value>,
        access: &AccessRecord,
        snapshot: &ClientSnapshot,
    ) -> Result<()> {
        self.check_owner(row, &access.user_id, snapshot)?;
        check_metadata(row, &access.id, &access.scopes, access.expires_at)?;
        if row_timestamp(row, "created_at")? != timestamp(access.issued_at)? {
            return Err(binding_invalid());
        }
        Ok(())
    }

    fn code_cas<'a>(
        &self,
        tx: &'a DatabaseTransaction,
        code: &CodeRecord,
        snapshot: &ClientSnapshot,
        row: &Map<String, Value>,
    ) -> Result<QueryBuilder<'a, 'a>> {
        let query = self
            .db
            .table("oauth_auth_codes")?
            .using_transaction(tx)
            .where_eq("id", code.id.clone())?
            .where_eq("revoked", false)?
            .where_eq(
                "client_id",
                self.factory
                    .identifier_binding(&snapshot.identifier, self.db.backend()?)?,
            )?
            .where_eq(
                "user_id",
                self.user_key.binding(&code.user_id, self.db.backend()?)?,
            )?
            .where_eq("expires_at", timestamp(code.expires_at)?)?;
        match row.get("scopes") {
            Some(Value::Null) => query.where_null("scopes"),
            Some(Value::String(raw)) => query.where_eq("scopes", raw.clone()),
            _ => Err(binding_invalid()),
        }
    }

    fn code_values(
        &self,
        code: &CodeRecord,
        snapshot: &ClientSnapshot,
    ) -> Result<Vec<(&'static str, SqlValue)>> {
        Ok(vec![
            ("id", code.id.clone().into()),
            (
                "user_id",
                self.user_key.binding(&code.user_id, self.db.backend()?)?,
            ),
            (
                "client_id",
                self.factory
                    .identifier_binding(&snapshot.identifier, self.db.backend()?)?,
            ),
            ("scopes", encoded_scopes(&code.scopes)?.into()),
            ("revoked", false.into()),
            ("expires_at", timestamp(code.expires_at)?.into()),
        ])
    }

    async fn insert_tokens(
        &self,
        tx: &DatabaseTransaction,
        tokens: &TokenRecords,
        snapshot: &ClientSnapshot,
    ) -> Result<()> {
        let access = &tokens.access;
        self.db
            .table("oauth_access_tokens")?
            .using_transaction(tx)
            .insert(vec![
                ("id", access.id.clone().into()),
                (
                    "user_id",
                    self.user_key.binding(&access.user_id, self.db.backend()?)?,
                ),
                (
                    "client_id",
                    self.factory
                        .identifier_binding(&snapshot.identifier, self.db.backend()?)?,
                ),
                ("scopes", encoded_scopes(&access.scopes)?.into()),
                ("revoked", false.into()),
                ("created_at", timestamp(access.issued_at)?.into()),
                ("updated_at", timestamp(access.issued_at)?.into()),
                ("expires_at", timestamp(access.expires_at)?.into()),
            ])
            .await?;
        if let Some(refresh) = &tokens.refresh {
            self.db
                .table("oauth_refresh_tokens")?
                .using_transaction(tx)
                .insert(vec![
                    ("id", refresh.id.clone().into()),
                    ("access_token_id", access.id.clone().into()),
                    ("revoked", false.into()),
                    ("expires_at", timestamp(refresh.expires_at)?.into()),
                ])
                .await?;
        }
        Ok(())
    }
}

// 默认适配器只委托同一Db仓储；trait本身不替代事务、权威或提交边界证据。
impl crate::GrantStorage for DatabaseGrantRepository {
    fn current_time(&self) -> crate::GrantStorageResult<i64> {
        DatabaseGrantRepository::current_time(self).map_err(crate::GrantStorageError::Unavailable)
    }

    fn client_by_protocol<'a>(
        &'a self,
        identifier: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<Option<ClientRecord>>> {
        Box::pin(async move {
            DatabaseGrantRepository::client_by_protocol(self, identifier)
                .await
                .map_err(crate::GrantStorageError::from_framework)
        })
    }

    fn persist_code<'a>(
        &'a self,
        code: &'a CodeRecord,
        client: &'a ClientSnapshot,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async move {
            DatabaseGrantRepository::persist_code(self, code, client)
                .await
                .map_err(crate::GrantStorageError::from_framework)
        })
    }

    fn exchange_code<'a>(
        &'a self,
        code: &'a CodeRecord,
        client: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async move {
            let now = DatabaseGrantRepository::current_time(self)
                .map_err(crate::GrantStorageError::Unavailable)?;
            DatabaseGrantRepository::exchange_code(self, code, client, tokens, now)
                .await
                .map_err(crate::GrantStorageError::from_framework)
        })
    }

    fn rotate_refresh<'a>(
        &'a self,
        refresh: &'a crate::RefreshRedemption,
        client: &'a ClientSnapshot,
        tokens: &'a TokenRecords,
        revoke_old: bool,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async move {
            // 仅还原引擎已认证DTO的原字段，不猜测负载没有的旧access iat/exp。
            let payload = RefreshPayload {
                client_id: refresh.client_id().to_owned(),
                refresh_token_id: refresh.id().to_owned(),
                access_token_id: refresh.access_token_id().to_owned(),
                user_id: refresh.user_id().to_owned(),
                scopes: refresh.scopes().to_vec(),
                expire_time: refresh.expires_at(),
                authority_binding: refresh.authority().as_str().to_owned(),
            };
            let now = DatabaseGrantRepository::current_time(self)
                .map_err(crate::GrantStorageError::Unavailable)?;
            DatabaseGrantRepository::rotate_refresh_payload(
                self, &payload, client, tokens, now, revoke_old,
            )
            .await
            .map_err(crate::GrantStorageError::from_framework)
        })
    }

    fn is_code_revoked<'a>(
        &'a self,
        id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async move {
            DatabaseGrantRepository::is_code_revoked(self, id)
                .await
                .map_err(crate::GrantStorageError::Unavailable)
        })
    }

    fn is_refresh_revoked<'a>(
        &'a self,
        id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async move {
            DatabaseGrantRepository::is_refresh_revoked(self, id)
                .await
                .map_err(crate::GrantStorageError::Unavailable)
        })
    }

    fn is_access_revoked<'a>(
        &'a self,
        id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<bool>> {
        Box::pin(async move {
            DatabaseGrantRepository::is_access_revoked(self, id)
                .await
                .map_err(crate::GrantStorageError::from_framework)
        })
    }

    fn revoke_access<'a>(
        &'a self,
        id: &'a str,
    ) -> openrust::support::BoxFuture<'a, crate::GrantStorageResult<()>> {
        Box::pin(async move {
            DatabaseGrantRepository::revoke_access(self, id)
                .await
                .map_err(crate::GrantStorageError::from_framework)
        })
    }
}

fn check_identity(client: &str, snapshot: &ClientSnapshot) -> Result<()> {
    if client != snapshot.entity.get_identifier() {
        return Err(binding_invalid());
    }
    Ok(())
}

fn check_new_tokens(
    tokens: &TokenRecords,
    client: &str,
    user: &str,
    original_scopes: &[String],
    authority: &GrantAuthorityBinding,
) -> Result<()> {
    let access = &tokens.access;
    if access.authority.as_str() != authority.as_str()
        || access.client_id != client
        || access.user_id != user
        || !access
            .scopes
            .iter()
            .all(|scope| original_scopes.contains(scope))
    {
        return Err(binding_invalid());
    }
    if let Some(refresh) = &tokens.refresh {
        let bound = &refresh.access;
        if bound.authority.as_str() != access.authority.as_str()
            || bound.id != access.id
            || bound.client_id != access.client_id
            || bound.user_id != access.user_id
            || bound.scopes != access.scopes
            || bound.issued_at != access.issued_at
            || bound.expires_at != access.expires_at
        {
            return Err(binding_invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
fn check_refresh_row(row: &Map<String, Value>, refresh: &RefreshRecord, now: i64) -> Result<()> {
    let payload = RefreshPayload {
        client_id: refresh.access.client_id.clone(),
        refresh_token_id: refresh.id.clone(),
        access_token_id: refresh.access.id.clone(),
        scopes: refresh.access.scopes.clone(),
        user_id: refresh.access.user_id.clone(),
        expire_time: refresh.expires_at,
        authority_binding: refresh.access.authority.as_str().to_owned(),
    };
    check_refresh_payload_row(row, &payload)?;
    check_expiry(payload.expire_time, now)
}

fn check_refresh_payload_row(row: &Map<String, Value>, payload: &RefreshPayload) -> Result<()> {
    if scalar(row, "id")? != payload.refresh_token_id
        || scalar(row, "access_token_id")? != payload.access_token_id
        || row_timestamp(row, "expires_at")? != timestamp(payload.expire_time)?
    {
        return Err(binding_invalid());
    }
    Ok(())
}

fn require_revoked(row: &Map<String, Value>, expected: bool) -> Result<()> {
    if revoked(row)? == expected {
        Ok(())
    } else {
        Err(binding_invalid())
    }
}

fn check_metadata(
    row: &Map<String, Value>,
    id: &str,
    expected_scopes: &[String],
    expires: i64,
) -> Result<()> {
    if scalar(row, "id")? != id
        || scopes(row)? != expected_scopes
        || row_timestamp(row, "expires_at")? != timestamp(expires)?
    {
        return Err(binding_invalid());
    }
    Ok(())
}

fn check_expiry(expires: i64, now: i64) -> Result<()> {
    // 固定 League 使用 expire_time < time()，同秒边界仍有效。
    if expires < now {
        Err(failure("passport_grant_expired"))
    } else {
        Ok(())
    }
}

fn scalar(row: &Map<String, Value>, key: &str) -> Result<String> {
    match AttributeCast::String.read(row.get(key).cloned())? {
        Some(Value::String(value)) => Ok(value),
        _ => Err(binding_invalid()),
    }
}

fn scopes(row: &Map<String, Value>) -> Result<Vec<String>> {
    match row.get("scopes") {
        Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(raw)) => serde_json::from_str(raw).map_err(Error::internal),
        _ => Err(binding_invalid()),
    }
}

fn encoded_scopes(scopes: &[String]) -> Result<String> {
    serde_json::to_string(scopes).map_err(Error::internal)
}

fn revoked(row: &Map<String, Value>) -> Result<bool> {
    match row.get("revoked") {
        Some(Value::Bool(value)) => Ok(*value),
        Some(Value::Number(value)) if value.as_i64() == Some(0) => Ok(false),
        Some(Value::Number(value)) if value.as_i64() == Some(1) => Ok(true),
        _ => Err(binding_invalid()),
    }
}

fn check_revoked(row: &Map<String, Value>, code: &'static str) -> Result<()> {
    if revoked(row)? {
        Err(failure(code))
    } else {
        Ok(())
    }
}

fn timestamp(seconds: i64) -> Result<DateTime> {
    DateTimeUtc::from_timestamp(seconds, 0)
        .map(|date| date.naive_utc())
        .ok_or_else(binding_invalid)
}

fn row_timestamp(row: &Map<String, Value>, key: &str) -> Result<DateTime> {
    let raw = row
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(binding_invalid)?;
    DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| DateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f"))
        .map_err(Error::internal)
}

fn require_one(affected: u64, code: &'static str) -> Result<()> {
    if affected == 1 {
        Ok(())
    } else {
        Err(failure(code))
    }
}

fn binding_invalid() -> Error {
    failure("passport_grant_binding_invalid")
}

fn failure(code: &'static str) -> Error {
    Error::custom_with_status(
        openrust::http::axum::http::StatusCode::BAD_REQUEST,
        code,
        "OAuth授权状态或持久绑定无效",
    )
}

#[cfg(test)]
#[path = "database_grants_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_flow_tests.rs"]
mod engine_flow_tests;

#[cfg(test)]
#[path = "lookup_test_support.rs"]
mod lookup_test_support;

#[cfg(test)]
#[path = "lookup_binding_tests.rs"]
mod lookup_binding_tests;
