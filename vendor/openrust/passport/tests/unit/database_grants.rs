//! 原子存储反例；纯行校验覆盖绑定，真实事务覆盖条件消费与全部写入回滚。

use super::*;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
use crate::GrantAuthority;
use serde_json::json;

fn access() -> Result<AccessRecord> {
    Ok(AccessRecord {
        authority: GrantAuthorityBinding::new("test-authority".into())?,
        id: "old-access".into(),
        client_id: "client".into(),
        user_id: "7".into(),
        scopes: vec!["read".into(), "write".into()],
        issued_at: 1_700_000_000,
        expires_at: 1_700_003_600,
    })
}

fn tokens() -> Result<TokenRecords> {
    let mut access = access()?;
    access.id = "new-access".into();
    access.scopes = vec!["read".into()];
    Ok(TokenRecords {
        refresh: Some(RefreshRecord {
            id: "new-refresh".into(),
            access: access.clone(),
            expires_at: 1_800_000_000,
        }),
        access,
    })
}

fn code_row() -> Map<String, Value> {
    Map::from_iter([
        ("id".into(), json!("code")),
        ("client_id".into(), json!("client")),
        ("user_id".into(), json!(7)),
        ("scopes".into(), json!("[\"read\",\"write\"]")),
        ("revoked".into(), json!(false)),
        ("expires_at".into(), json!("2023-11-14 23:13:20")),
    ])
}

#[test]
fn original_scope_binding_is_not_replaced_by_finalized_access_scope() -> Result<()> {
    let row = code_row();
    let original = access()?;
    check_metadata(&row, "code", &original.scopes, original.expires_at)?;
    let narrowed = tokens()?;
    check_new_tokens(
        &narrowed,
        "client",
        "7",
        &original.scopes,
        &original.authority,
    )?;
    assert_eq!(
        check_metadata(&row, "code", &narrowed.access.scopes, original.expires_at)
            .err()
            .map(|error| error.code().to_owned()),
        Some("passport_grant_binding_invalid".into())
    );
    Ok(())
}

#[test]
fn narrowed_tokens_cannot_change_owner_client_or_refresh_access_binding() -> Result<()> {
    let original = access()?;
    for change in 0..6 {
        let mut issued = tokens()?;
        match change {
            0 => issued.access.scopes.push("admin".into()),
            1 => issued.access.user_id = "8".into(),
            2 => issued.access.client_id = "other".into(),
            3 => {
                if let Some(refresh) = &mut issued.refresh {
                    refresh.access.id = "unrelated".into();
                }
            }
            4 => {
                if let Some(refresh) = &mut issued.refresh {
                    refresh.access.scopes.clear();
                }
            }
            _ => issued.access.authority = GrantAuthorityBinding::new("changed-authority".into())?,
        }
        assert_eq!(
            check_new_tokens(
                &issued,
                "client",
                "7",
                &original.scopes,
                &original.authority
            )
            .err()
            .map(|error| error.code().to_owned()),
            Some("passport_grant_binding_invalid".into())
        );
    }
    Ok(())
}

#[test]
fn metadata_and_revocation_fail_closed_without_swallowing_decode_errors() -> Result<()> {
    let mut row = code_row();
    assert!(!revoked(&row)?);
    row.insert("revoked".into(), json!(true));
    assert_eq!(
        check_revoked(&row, "passport_code_revoked")
            .err()
            .map(|error| error.code().to_owned()),
        Some("passport_code_revoked".into())
    );
    for raw in [json!(null), json!("false"), json!(2), json!([])] {
        row.insert("revoked".into(), raw);
        assert!(revoked(&row).is_err());
    }
    row.insert("scopes".into(), json!("not-json"));
    assert!(scopes(&row).is_err());
    row.insert("scopes".into(), json!("[1]"));
    assert!(scopes(&row).is_err());
    row.insert("expires_at".into(), json!("not-a-date"));
    assert!(row_timestamp(&row, "expires_at").is_err());
    assert!(timestamp(i64::MAX).is_err());
    Ok(())
}

#[test]
fn expiry_equality_and_conditional_row_count_follow_contract() -> Result<()> {
    check_expiry(100, 100)?;
    assert_eq!(
        check_expiry(99, 100)
            .err()
            .map(|error| error.code().to_owned()),
        Some("passport_grant_expired".into())
    );
    require_one(1, "passport_code_revoked")?;
    for rows in [0, 2] {
        assert!(require_one(rows, "passport_code_revoked").is_err());
    }
    Ok(())
}

#[test]
fn refresh_uses_its_actual_old_access_not_an_unrelated_row() -> Result<()> {
    let refresh = RefreshRecord {
        id: "refresh".into(),
        access: access()?,
        expires_at: 1_700_003_600,
    };
    let mut row = code_row();
    row.insert("id".into(), json!("refresh"));
    row.insert("access_token_id".into(), json!("old-access"));
    check_refresh_row(&row, &refresh, 1_700_000_000)?;
    row.insert("access_token_id".into(), json!("unrelated-access"));
    assert!(check_refresh_row(&row, &refresh, 1_700_000_000).is_err());
    Ok(())
}

#[test]
fn configured_user_key_preserves_large_integer_and_text_uuid_semantics() -> Result<()> {
    let value = "9007199254740993";
    assert_eq!(
        GrantUserKey::Integer.binding(value, DbBackend::MySql)?,
        SqlValue::from(9_007_199_254_740_993i64)
    );
    let uuid = "019a4d4b-f21a-7191-bce9-fdc817a165aa";
    assert_eq!(
        GrantUserKey::String.binding(uuid, DbBackend::Postgres)?,
        SqlValue::from(uuid)
    );
    assert!(matches!(
        GrantUserKey::Uuid.binding(uuid, DbBackend::Postgres)?,
        SqlValue::Uuid(Some(_))
    ));
    assert!(
        GrantUserKey::Integer
            .binding(uuid, DbBackend::MySql)
            .is_err()
    );
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct FixedClock(i64);
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl GrantClock for FixedClock {
    fn now(&self) -> Result<i64> {
        Ok(self.0)
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct Migrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl openrust::database::sea_orm_migration::MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct Authority(std::sync::atomic::AtomicBool, Db);
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl<'tx> GrantAuthority<TransactionContext<'tx>> for Authority {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        _context: &'a GrantValidation<'a>,
        _binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.1) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Err(Error::custom("test_authority_rejected", "测试权威拒绝"))
            } else {
                Ok(())
            }
        })
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct RejectFinal(std::sync::atomic::AtomicUsize, Db);
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl<'tx> GrantAuthority<TransactionContext<'tx>> for RejectFinal {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        _context: &'a GrantValidation<'a>,
        _binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.1) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            if self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                Ok(())
            } else {
                Err(Error::custom(
                    "test_final_authority_rejected",
                    "测试提交末检拒绝",
                ))
            }
        })
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct ControlledClock(std::sync::atomic::AtomicI64);
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl GrantClock for ControlledClock {
    fn now(&self) -> Result<i64> {
        Ok(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

// 首次锁后预检有效，只有最终同步读时跨过期限；不依赖sleep或真实墙钟。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct LastTickClock {
    calls: std::sync::atomic::AtomicUsize,
    initial: i64,
    final_tick: i64,
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl GrantClock for LastTickClock {
    fn now(&self) -> Result<i64> {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
            Ok(self.initial)
        } else {
            Ok(self.final_tick)
        }
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct TamperAuthority {
    db: Db,
    calls: std::sync::atomic::AtomicUsize,
    table: &'static str,
    id: String,
    column: Option<&'static str>,
    value: SqlValue,
    advance: Option<(Arc<ControlledClock>, i64)>,
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl<'tx> GrantAuthority<TransactionContext<'tx>> for TamperAuthority {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        _context: &'a GrantValidation<'a>,
        _binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.db) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            let tx = db_context.transaction();
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                if let Some((clock, now)) = &self.advance {
                    clock.0.store(*now, std::sync::atomic::Ordering::SeqCst);
                } else {
                    let binding = if self.table == "oauth_clients"
                        && self.db.backend()? == DbBackend::Postgres
                    {
                        SqlValue::Uuid(Some(
                            uuid::Uuid::parse_str(&self.id).map_err(Error::internal)?,
                        ))
                    } else {
                        self.id.clone().into()
                    };
                    let query = self
                        .db
                        .table(self.table)?
                        .using_transaction(tx)
                        .where_eq("id", binding)?;
                    if let Some(column) = self.column {
                        query.update(vec![(column, self.value.clone())]).await?;
                    } else {
                        query.delete().await?;
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
async fn real_transactions_consume_once_and_roll_back_token_insert_failures() -> Result<()> {
    use openrust::testing::TestDatabase;
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("未执行Passport原子存储网络回归：需要真实MySQL/PostgreSQL测试URL");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&url).await?;
    let result: Result<()> = Box::pin(async {
        let fixture = Box::pin(SqlGrantFixture::new(&database)).await?;
        Box::pin(fixture.ordinary_revocation_reads_match_active_revoked_and_missing_rows()).await?;
        Box::pin(fixture.foreign_context_origin_is_rejected_before_writes()).await?;
        Box::pin(fixture.concurrent_consumption()).await?;
        Box::pin(fixture.insert_collisions()).await?;
        Box::pin(fixture.record_tampering()).await?;
        Box::pin(fixture.client_tampering()).await?;
        Box::pin(fixture.code_clock_advance()).await?;
        Box::pin(fixture.code_last_tick()).await?;
        Box::pin(fixture.authority_rejection()).await?;
        Box::pin(fixture.refresh_matrix()).await?;
        Box::pin(bound_authority_contexts(
            &fixture.db,
            &fixture.repository,
            &fixture.snapshot,
        ))
        .await?;
        Box::pin(fixture.scope_closes_during_final_authority_and_rolls_back()).await
    })
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct SqlGrantFixture {
    db: Db,
    repository: DatabaseGrantRepository,
    owner: super::lookup_test_support::ScopedRepository,
    snapshot: ClientSnapshot,
    authority: Arc<Authority>,
    code: CodeRecord,
    issued: TokenRecords,
    id: Value,
    client_id: String,
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
async fn create_grant_schema(db: &Db) -> Result<()> {
    use openrust::database::sea_orm::ConnectionTrait;
    let backend = db.backend()?;
    let id_type = match backend {
        DbBackend::MySql => "CHAR(36)",
        DbBackend::Postgres => "UUID",
        _ => {
            return Err(Error::custom(
                "test_backend_invalid",
                "此测试需要MySQL/PostgreSQL",
            ));
        }
    };
    let date_type = if backend == DbBackend::MySql {
        "DATETIME"
    } else {
        "TIMESTAMP"
    };
    for sql in [
        format!(
            "CREATE TABLE oauth_clients (id {id_type} PRIMARY KEY, name VARCHAR(80), secret VARCHAR(255), revoked BOOLEAN, redirect_uris TEXT, grant_types TEXT)"
        ),
        format!(
            "CREATE TABLE oauth_auth_codes (id VARCHAR(80) PRIMARY KEY, client_id {id_type}, user_id BIGINT, scopes TEXT, revoked BOOLEAN, expires_at {date_type})"
        ),
        format!(
            "CREATE TABLE oauth_access_tokens (id VARCHAR(80) PRIMARY KEY, client_id {id_type}, user_id BIGINT, scopes TEXT, revoked BOOLEAN, created_at {date_type}, updated_at {date_type}, expires_at {date_type})"
        ),
        format!(
            "CREATE TABLE oauth_refresh_tokens (id VARCHAR(80) PRIMARY KEY, access_token_id VARCHAR(80), revoked BOOLEAN, expires_at {date_type})"
        ),
    ] {
        db.primary_connection()?
            .execute_unprepared(&sql)
            .await
            .map_err(Error::internal)?;
    }
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl SqlGrantFixture {
    async fn new(database: &openrust::testing::TestDatabase) -> Result<Self> {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        create_grant_schema(&db).await?;
        let backend = db.backend()?;
        let factory: Arc<dyn ClientModelFactory> = Arc::new(crate::ClientFactory::default());
        let id = json!("019a4d4b-f21a-7191-bce9-fdc817a165aa");
        db.table("oauth_clients")?
            .insert(vec![
                ("id", factory.identifier_binding(&id, backend)?),
                ("name", "client".into()),
                ("secret", SqlValue::String(None)),
                ("revoked", false.into()),
                ("redirect_uris", "[]".into()),
                (
                    "grant_types",
                    "[\"authorization_code\",\"refresh_token\"]".into(),
                ),
            ])
            .await?;
        let authority = Arc::new(Authority(
            std::sync::atomic::AtomicBool::new(false),
            db.clone(),
        ));
        let owner = super::lookup_test_support::scoped_repository(
            db.clone(),
            factory,
            GrantUserKey::Integer,
            authority.clone(),
            Arc::new(FixedClock(1_700_000_000)),
            None,
        )
        .await?;
        let repository = owner.repository.as_ref().clone();
        let client = repository.client(&id).await?.ok_or_else(binding_invalid)?;
        let snapshot = ClientSnapshot::capture(client.as_ref())?;
        let client_id = snapshot.entity.get_identifier().to_owned();
        let code = CodeRecord {
            authority: access()?.authority,
            id: "code".into(),
            client_id: client_id.clone(),
            user_id: "7".into(),
            scopes: vec!["read".into(), "write".into()],
            expires_at: 1_700_003_600,
        };
        let mut issued = tokens()?;
        issued.access.client_id = client_id.clone();
        if let Some(refresh) = &mut issued.refresh {
            refresh.access = issued.access.clone();
        }
        Ok(Self {
            db,
            repository,
            owner,
            snapshot,
            authority,
            code,
            issued,
            id,
            client_id,
        })
    }

    async fn concurrent_consumption(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            ..
        } = self;
        repository.persist_code(code, snapshot).await?;
        let (left, right) = tokio::join!(
            repository.exchange_code(code, snapshot, issued, 1_700_000_000),
            repository.exchange_code(code, snapshot, issued, 1_700_000_000)
        );
        assert_ne!(left.is_ok(), right.is_ok());
        let failure = left.err().or(right.err()).ok_or_else(binding_invalid)?;
        assert_eq!(failure.code(), "passport_code_revoked");
        assert_eq!(db.table("oauth_access_tokens")?.count().await?, 1);
        Ok(())
    }

    async fn insert_collisions(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            ..
        } = self;
        let mut next_code = code.clone();
        next_code.id = "rollback-code".into();
        repository.persist_code(&next_code, snapshot).await?;
        // 已有access唯一冲突必须原样失败，且此前的code条件消费全部回滚。
        assert!(
            repository
                .exchange_code(&next_code, snapshot, issued, 1_700_000_000)
                .await
                .is_err()
        );
        let row = db
            .table("oauth_auth_codes")?
            .where_eq("id", next_code.id.clone())?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        let mut failed_refresh = issued.clone();
        failed_refresh.access.id = "rolled-back-access".into();
        if let Some(refresh) = &mut failed_refresh.refresh {
            refresh.access = failed_refresh.access.clone();
        }
        // 新access先写成功、refresh唯一冲突时，必须连同新access和code消费全部回滚。
        assert!(
            repository
                .exchange_code(&next_code, snapshot, &failed_refresh, 1_700_000_000)
                .await
                .is_err()
        );
        assert_eq!(
            db.table("oauth_access_tokens")?
                .where_eq("id", "rolled-back-access")?
                .count()
                .await?,
            0
        );
        let row = db
            .table("oauth_auth_codes")?
            .where_eq("id", next_code.id.clone())?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        let mut final_repository = repository.clone();
        final_repository.authority = Some(Arc::new(RejectFinal(
            std::sync::atomic::AtomicUsize::new(0),
            db.clone(),
        )));
        let mut final_code = code.clone();
        final_code.id = "final-rejected-code".into();
        assert_eq!(
            final_repository
                .persist_code(&final_code, snapshot)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("test_final_authority_rejected".into())
        );
        assert_eq!(
            db.table("oauth_auth_codes")?
                .where_eq("id", final_code.id)?
                .count()
                .await?,
            0
        );
        Ok(())
    }

    async fn record_tampering(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            ..
        } = self;
        // 在末检authority回调注入与BeforeCommit监听器等价的同事务篡改；
        // 框架监听器→末检的真实派发顺序另由framework final_check_tests覆盖。
        let attacks: Vec<(&str, Option<&str>, SqlValue)> = vec![
            ("oauth_auth_codes", Some("revoked"), false.into()),
            ("oauth_auth_codes", None, false.into()),
            ("oauth_auth_codes", Some("scopes"), "[]".into()),
            ("oauth_access_tokens", Some("revoked"), true.into()),
            ("oauth_access_tokens", Some("scopes"), "[]".into()),
            ("oauth_access_tokens", Some("user_id"), 8i64.into()),
            ("oauth_access_tokens", None, false.into()),
            ("oauth_refresh_tokens", Some("revoked"), true.into()),
            (
                "oauth_refresh_tokens",
                Some("access_token_id"),
                "unrelated".into(),
            ),
            ("oauth_refresh_tokens", None, false.into()),
        ];
        for (index, (table, column, value)) in attacks.into_iter().enumerate() {
            let mut attacked_code = code.clone();
            attacked_code.id = format!("attacked-code-{index}");
            let mut attacked_tokens = issued.clone();
            attacked_tokens.access.id = format!("attacked-access-{index}");
            if let Some(refresh) = &mut attacked_tokens.refresh {
                refresh.id = format!("attacked-refresh-{index}");
                refresh.access = attacked_tokens.access.clone();
            }
            repository.persist_code(&attacked_code, snapshot).await?;
            let id = match table {
                "oauth_auth_codes" => attacked_code.id.clone(),
                "oauth_access_tokens" => attacked_tokens.access.id.clone(),
                _ => attacked_tokens
                    .refresh
                    .as_ref()
                    .ok_or_else(binding_invalid)?
                    .id
                    .clone(),
            };
            let mut attacked = repository.clone();
            attacked.authority = Some(Arc::new(TamperAuthority {
                db: db.clone(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                table,
                id,
                column,
                value,
                advance: None,
            }));
            assert_eq!(
                attacked
                    .exchange_code(&attacked_code, snapshot, &attacked_tokens, 1_700_000_000)
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("passport_grant_binding_invalid".into())
            );
            let row = db
                .table("oauth_auth_codes")?
                .where_eq("id", attacked_code.id)?
                .first()
                .await?
                .ok_or_else(binding_invalid)?;
            assert!(!revoked(&attributes_from_row(&row)?)?);
            assert_eq!(
                db.table("oauth_access_tokens")?
                    .where_eq("id", attacked_tokens.access.id)?
                    .count()
                    .await?,
                0
            );
        }
        Ok(())
    }

    async fn client_tampering(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            client_id,
            id,
            ..
        } = self;
        for (index, (column, value)) in [
            ("revoked", SqlValue::from(true)),
            ("secret", SqlValue::from("changed-hash")),
        ]
        .into_iter()
        .enumerate()
        {
            let mut attacked_code = code.clone();
            attacked_code.id = format!("client-attacked-code-{index}");
            let mut attacked_tokens = issued.clone();
            attacked_tokens.access.id = format!("client-attacked-access-{index}");
            if let Some(refresh) = &mut attacked_tokens.refresh {
                refresh.id = format!("client-attacked-refresh-{index}");
                refresh.access = attacked_tokens.access.clone();
            }
            repository.persist_code(&attacked_code, snapshot).await?;
            let mut attacked = repository.clone();
            attacked.authority = Some(Arc::new(TamperAuthority {
                db: db.clone(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                table: "oauth_clients",
                id: client_id.clone(),
                column: Some(column),
                value,
                advance: None,
            }));
            assert_eq!(
                attacked
                    .exchange_code(&attacked_code, snapshot, &attacked_tokens, 1_700_000_000)
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("passport_client_changed".into())
            );
            let restored = repository.client(id).await?.ok_or_else(binding_invalid)?;
            assert!(snapshot.matches(&restored)?);
            let row = db
                .table("oauth_auth_codes")?
                .where_eq("id", attacked_code.id)?
                .first()
                .await?
                .ok_or_else(binding_invalid)?;
            assert!(!revoked(&attributes_from_row(&row)?)?);
            assert_eq!(
                db.table("oauth_access_tokens")?
                    .where_eq("id", attacked_tokens.access.id)?
                    .count()
                    .await?,
                0
            );
        }
        Ok(())
    }

    async fn code_clock_advance(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            ..
        } = self;
        for (index, end_time) in [1_700_003_600, 1_700_003_601].into_iter().enumerate() {
            let clock = Arc::new(ControlledClock(std::sync::atomic::AtomicI64::new(
                1_700_000_000,
            )));
            let mut timed = repository.clone();
            timed.clock = clock.clone();
            let mut timed_code = code.clone();
            timed_code.id = format!("timed-code-{index}");
            repository.persist_code(&timed_code, snapshot).await?;
            let mut timed_tokens = issued.clone();
            timed_tokens.access.id = format!("timed-access-{index}");
            if let Some(refresh) = &mut timed_tokens.refresh {
                refresh.id = format!("timed-refresh-{index}");
                refresh.access = timed_tokens.access.clone();
            }
            timed.authority = Some(Arc::new(TamperAuthority {
                db: db.clone(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                table: "oauth_auth_codes",
                id: timed_code.id.clone(),
                column: None,
                value: false.into(),
                advance: Some((clock, end_time)),
            }));
            assert_eq!(
                timed
                    .exchange_code(&timed_code, snapshot, &timed_tokens, 1_700_000_000)
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("passport_grant_expired".into())
            );
            let row = db
                .table("oauth_auth_codes")?
                .where_eq("id", timed_code.id)?
                .first()
                .await?
                .ok_or_else(binding_invalid)?;
            assert!(!revoked(&attributes_from_row(&row)?)?);
            assert_eq!(
                db.table("oauth_access_tokens")?
                    .where_eq("id", timed_tokens.access.id)?
                    .count()
                    .await?,
                0
            );
        }
        Ok(())
    }

    async fn code_last_tick(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            issued,
            ..
        } = self;
        let last_tick = Arc::new(LastTickClock {
            calls: std::sync::atomic::AtomicUsize::new(0),
            initial: 1_700_000_000,
            final_tick: 1_700_003_601,
        });
        let last_timed = repository.clone().with_clock(last_tick.clone());
        // 只有旧code锁后的预检和最终所有异步查询之后各取时一次。
        let mut last_code = code.clone();
        last_code.id = "last-tick-code".into();
        repository.persist_code(&last_code, snapshot).await?;
        let mut last_tokens = issued.clone();
        last_tokens.access.id = "last-tick-access".into();
        if let Some(refresh) = &mut last_tokens.refresh {
            refresh.id = "last-tick-refresh".into();
            refresh.access = last_tokens.access.clone();
        }
        assert_eq!(
            last_timed
                .exchange_code(&last_code, snapshot, &last_tokens, 1_700_000_000)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("passport_grant_expired".into())
        );
        assert_eq!(last_tick.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        let row = db
            .table("oauth_auth_codes")?
            .where_eq("id", last_code.id)?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        assert_eq!(
            db.table("oauth_access_tokens")?
                .where_eq("id", last_tokens.access.id)?
                .count()
                .await?,
            0
        );
        Ok(())
    }

    async fn authority_rejection(&self) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            code,
            authority,
            ..
        } = self;
        authority.0.store(true, std::sync::atomic::Ordering::SeqCst);
        let mut denied_code = code.clone();
        denied_code.id = "denied-code".into();
        assert_eq!(
            repository
                .persist_code(&denied_code, snapshot)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("test_authority_rejected".into())
        );
        assert_eq!(
            db.table("oauth_auth_codes")?
                .where_eq("id", denied_code.id)?
                .count()
                .await?,
            0
        );
        authority
            .0
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn refresh_matrix(&self) -> Result<()> {
        let Self {
            repository,
            snapshot,
            issued,
            ..
        } = self;
        let old_refresh = issued.refresh.clone().ok_or_else(binding_invalid)?;
        repository.revoke_access(&issued.access.id).await?;
        assert!(repository.is_access_revoked(&issued.access.id).await?);
        let mut rotated = issued.clone();
        rotated.access.id = "rotated-access".into();
        if let Some(refresh) = &mut rotated.refresh {
            refresh.id = "rotated-refresh".into();
            refresh.access = rotated.access.clone();
        }
        let payload = RefreshPayload {
            client_id: old_refresh.access.client_id.clone(),
            refresh_token_id: old_refresh.id.clone(),
            access_token_id: old_refresh.access.id.clone(),
            scopes: old_refresh.access.scopes.clone(),
            user_id: old_refresh.access.user_id.clone(),
            expire_time: old_refresh.expires_at,
            authority_binding: old_refresh.access.authority.as_str().to_owned(),
        };
        Box::pin(self.refresh_tampering(&payload, &rotated)).await?;
        Box::pin(self.refresh_clock_advance(&payload, &rotated)).await?;
        // 旧access已撤销仍允许使用未撤销refresh；真实负载不提供旧iat或exp。
        repository
            .rotate_refresh_payload(&payload, snapshot, &rotated, 1_700_000_000, true)
            .await?;
        assert_eq!(
            repository
                .rotate_refresh(&old_refresh, snapshot, &rotated, 1_700_000_000, true)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("passport_refresh_revoked".into())
        );
        assert!(repository.is_access_revoked("not-found").await?);
        Ok(())
    }

    async fn refresh_tampering(
        &self,
        payload: &RefreshPayload,
        rotated: &TokenRecords,
    ) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            ..
        } = self;
        for column in [Some("revoked"), Some("access_token_id"), None] {
            let mut attacked = repository.clone();
            let value = if column == Some("access_token_id") {
                SqlValue::from("unrelated")
            } else {
                false.into()
            };
            attacked.authority = Some(Arc::new(TamperAuthority {
                db: db.clone(),
                calls: std::sync::atomic::AtomicUsize::new(0),
                table: "oauth_refresh_tokens",
                id: payload.refresh_token_id.clone(),
                column,
                value,
                advance: None,
            }));
            assert_eq!(
                attacked
                    .rotate_refresh_payload(payload, snapshot, rotated, 1_700_000_000, true)
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("passport_grant_binding_invalid".into())
            );
            let row = db
                .table("oauth_refresh_tokens")?
                .where_eq("id", payload.refresh_token_id.clone())?
                .first()
                .await?
                .ok_or_else(binding_invalid)?;
            assert!(!revoked(&attributes_from_row(&row)?)?);
            assert_eq!(
                db.table("oauth_access_tokens")?
                    .where_eq("id", rotated.access.id.clone())?
                    .count()
                    .await?,
                0
            );
        }
        Ok(())
    }

    async fn refresh_clock_advance(
        &self,
        payload: &RefreshPayload,
        rotated: &TokenRecords,
    ) -> Result<()> {
        let Self {
            db,
            repository,
            snapshot,
            ..
        } = self;
        let clock = Arc::new(ControlledClock(std::sync::atomic::AtomicI64::new(
            1_700_000_000,
        )));
        let mut timed = repository.clone();
        timed.clock = clock.clone();
        timed.authority = Some(Arc::new(TamperAuthority {
            db: db.clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            table: "oauth_refresh_tokens",
            id: payload.refresh_token_id.clone(),
            column: None,
            value: false.into(),
            advance: Some((clock, payload.expire_time + 1)),
        }));
        assert_eq!(
            timed
                .rotate_refresh_payload(payload, snapshot, rotated, 1_700_000_000, true)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("passport_grant_expired".into())
        );
        let row = db
            .table("oauth_refresh_tokens")?
            .where_eq("id", payload.refresh_token_id.clone())?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        let refresh_tick = Arc::new(LastTickClock {
            calls: std::sync::atomic::AtomicUsize::new(0),
            initial: 1_700_000_000,
            final_tick: payload.expire_time + 1,
        });
        let last_refresh = repository.clone().with_clock(refresh_tick.clone());
        assert_eq!(
            last_refresh
                .rotate_refresh_payload(payload, snapshot, rotated, 1_700_000_000, true)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("passport_grant_expired".into())
        );
        assert_eq!(
            refresh_tick.calls.load(std::sync::atomic::Ordering::SeqCst),
            2
        );
        let row = db
            .table("oauth_refresh_tokens")?
            .where_eq("id", payload.refresh_token_id.clone())?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        assert_eq!(
            db.table("oauth_access_tokens")?
                .where_eq("id", rotated.access.id.clone())?
                .count()
                .await?,
            0
        );
        let expired = repository
            .clone()
            .with_clock(Arc::new(FixedClock(payload.expire_time + 1)));
        assert_eq!(
            expired
                .rotate_refresh_payload(payload, snapshot, rotated, 1_700_000_000, true)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("passport_grant_expired".into())
        );
        Ok(())
    }
}

/// 隔离SQL夹具把不透明证明关联subject/client/purpose/批准范围；不作为生产默认。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct BoundAuthority {
    db: Db,
    phases: Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl<'tx> GrantAuthority<TransactionContext<'tx>> for BoundAuthority {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        context: &'a GrantValidation<'a>,
        binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.db) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            let tx = db_context.transaction();
            let row = self
                .db
                .table("passport_bound_authority_fixture")?
                .using_transaction(tx)
                .where_eq("binding_id", binding.as_str().to_owned())?
                .lock_for_update()?
                .first()
                .await?
                .ok_or_else(bound_rejected)?;
            let row = attributes_from_row(&row)?;
            if row.get("user_id").and_then(Value::as_str) != Some(context.user_id)
                || row.get("client_id").and_then(Value::as_str) != Some(context.client_id)
                || row.get("purpose").and_then(Value::as_str) != Some("oauth_delegation")
            {
                return Err(bound_rejected());
            }
            let approved = scopes(&row)?;
            if !context.scopes.iter().all(|scope| approved.contains(scope)) {
                return Err(bound_rejected());
            }
            // 同一个有效委托可跨正常三个阶段，阶段不作为binding字符串标签比较。
            let phase = match context.operation {
                GrantOperation::Authorize => 1,
                GrantOperation::Redeem => 2,
                GrantOperation::Refresh => 4,
            };
            self.phases
                .fetch_or(phase, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
fn bound_rejected() -> Error {
    Error::custom(
        "fixture_bound_authority_rejected",
        "隔离委托绑定或实际上下文越界",
    )
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
async fn bound_authority_contexts(
    db: &Db,
    repository: &DatabaseGrantRepository,
    snapshot: &ClientSnapshot,
) -> Result<()> {
    let fixture = Box::pin(BoundFixture::new(db, repository, snapshot)).await?;
    Box::pin(fixture.rejected_contexts(db, snapshot)).await?;
    let issued = Box::pin(fixture.narrowed_exchange(db, snapshot)).await?;
    Box::pin(fixture.narrowed_refresh(db, snapshot, &issued)).await
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct BoundFixture {
    bounded: DatabaseGrantRepository,
    storage: Arc<dyn crate::GrantStorage>,
    phases: Arc<std::sync::atomic::AtomicUsize>,
    binding: GrantAuthorityBinding,
    code: CodeRecord,
    other_snapshot: ClientSnapshot,
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
async fn create_other_bound_client(
    db: &Db,
    repository: &DatabaseGrantRepository,
) -> Result<ClientSnapshot> {
    let other = serde_json::json!("019a4d4b-f21a-7191-bce9-fdc817a165ab");
    db.table("oauth_clients")?
        .insert(vec![
            (
                "id",
                repository
                    .factory
                    .identifier_binding(&other, db.backend()?)?,
            ),
            ("name", "other".into()),
            ("revoked", false.into()),
            ("redirect_uris", "[]".into()),
            (
                "grant_types",
                "[\"authorization_code\",\"refresh_token\"]".into(),
            ),
        ])
        .await?;
    let other_record = repository
        .client(&other)
        .await?
        .ok_or_else(binding_invalid)?;
    ClientSnapshot::capture(other_record.as_ref())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl BoundFixture {
    async fn new(
        db: &Db,
        repository: &DatabaseGrantRepository,
        snapshot: &ClientSnapshot,
    ) -> Result<Self> {
        use openrust::database::sea_orm::ConnectionTrait;
        db.primary_connection()?.execute_unprepared("CREATE TABLE passport_bound_authority_fixture (binding_id VARCHAR(80) PRIMARY KEY, user_id VARCHAR(80), client_id VARCHAR(80), purpose VARCHAR(80), scopes TEXT)").await.map_err(Error::internal)?;
        let client_id = snapshot.entity.get_identifier();
        for (binding, purpose) in [
            ("bound-oauth", "oauth_delegation"),
            ("other-purpose", "business_action"),
        ] {
            db.table("passport_bound_authority_fixture")?
                .insert(vec![
                    ("binding_id", binding.into()),
                    ("user_id", "7".into()),
                    ("client_id", client_id.into()),
                    ("purpose", purpose.into()),
                    ("scopes", "[\"read\",\"write\"]".into()),
                ])
                .await?;
        }
        let other_snapshot = create_other_bound_client(db, repository).await?;
        let phases = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut bounded = repository.clone();
        bounded.authority = Some(Arc::new(BoundAuthority {
            db: db.clone(),
            phases: phases.clone(),
        }));
        let storage: Arc<dyn crate::GrantStorage> = Arc::new(bounded.clone());
        assert_eq!(
            storage.current_time().map_err(Error::internal)?,
            1_700_000_000
        );
        assert!(
            storage
                .client_by_protocol(client_id)
                .await
                .map_err(Error::internal)?
                .is_some()
        );
        let binding = GrantAuthorityBinding::new("bound-oauth".into())?;
        let code = CodeRecord {
            id: "bound-code".into(),
            client_id: client_id.to_owned(),
            user_id: "7".into(),
            scopes: vec!["read".into(), "write".into()],
            expires_at: 1_700_003_600,
            authority: binding.clone(),
        };
        Ok(Self {
            bounded,
            storage,
            phases,
            binding,
            code,
            other_snapshot,
        })
    }

    async fn rejected_contexts(&self, db: &Db, snapshot: &ClientSnapshot) -> Result<()> {
        let Self {
            bounded,
            code,
            other_snapshot,
            ..
        } = self;
        for case in 0..4 {
            let mut denied = code.clone();
            denied.id = format!("bound-denied-{case}");
            let current = match case {
                0 => {
                    denied.user_id = "8".into();
                    snapshot
                }
                1 => {
                    denied.client_id = other_snapshot.entity.get_identifier().to_owned();
                    other_snapshot
                }
                2 => {
                    denied.authority = GrantAuthorityBinding::new("other-purpose".into())?;
                    snapshot
                }
                _ => {
                    denied.scopes.push("admin".into());
                    snapshot
                }
            };
            assert_eq!(
                bounded
                    .persist_code(&denied, current)
                    .await
                    .err()
                    .map(|error| error.code().to_owned()),
                Some("fixture_bound_authority_rejected".into())
            );
            assert_eq!(
                db.table("oauth_auth_codes")?
                    .where_eq("id", denied.id)?
                    .count()
                    .await?,
                0
            );
        }
        Ok(())
    }

    async fn narrowed_exchange(&self, db: &Db, snapshot: &ClientSnapshot) -> Result<TokenRecords> {
        let Self {
            bounded,
            code,
            binding,
            ..
        } = self;
        let client_id = snapshot.entity.get_identifier();
        self.storage
            .persist_code(code, snapshot)
            .await
            .map_err(Error::internal)?;
        // 当前委托范围收窄后，原code仍保原同意scope；兑换必须校将签发的最终列表。
        db.table("passport_bound_authority_fixture")?
            .where_eq("binding_id", "bound-oauth")?
            .update(vec![("scopes", "[\"read\"]".into())])
            .await?;
        let mut issued = tokens()?;
        issued.access.id = "bound-access".into();
        issued.access.client_id = client_id.to_owned();
        issued.access.authority = binding.clone();
        if let Some(refresh) = &mut issued.refresh {
            refresh.id = "bound-refresh".into();
            refresh.access = issued.access.clone();
        }
        let mut outside = issued.clone();
        outside.access.id = "bound-outside-access".into();
        outside.access.scopes = vec!["write".into()];
        if let Some(refresh) = &mut outside.refresh {
            refresh.id = "bound-outside-refresh".into();
            refresh.access = outside.access.clone();
        }
        assert_eq!(
            bounded
                .exchange_code(code, snapshot, &outside, 1_700_000_000)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("fixture_bound_authority_rejected".into())
        );
        let row = db
            .table("oauth_auth_codes")?
            .where_eq("id", code.id.clone())?
            .first()
            .await?
            .ok_or_else(binding_invalid)?;
        assert!(!revoked(&attributes_from_row(&row)?)?);
        assert_eq!(
            db.table("oauth_access_tokens")?
                .where_eq("id", outside.access.id)?
                .count()
                .await?,
            0
        );
        self.storage
            .exchange_code(code, snapshot, &issued)
            .await
            .map_err(Error::internal)?;
        Ok(issued)
    }

    async fn narrowed_refresh(
        &self,
        db: &Db,
        snapshot: &ClientSnapshot,
        issued: &TokenRecords,
    ) -> Result<()> {
        let Self {
            storage,
            phases,
            binding,
            ..
        } = self;
        let client_id = snapshot.entity.get_identifier();
        let old_refresh = issued.refresh.as_ref().ok_or_else(binding_invalid)?;
        let payload = RefreshPayload {
            client_id: client_id.to_owned(),
            refresh_token_id: old_refresh.id.clone(),
            access_token_id: issued.access.id.clone(),
            scopes: issued.access.scopes.clone(),
            user_id: "7".into(),
            expire_time: old_refresh.expires_at,
            authority_binding: binding.as_str().to_owned(),
        };
        let mut rotated = issued.clone();
        rotated.access.id = "bound-rotated-access".into();
        rotated.access.scopes.clear();
        if let Some(refresh) = &mut rotated.refresh {
            refresh.id = "bound-rotated-refresh".into();
            refresh.access = rotated.access.clone();
        }
        // refresh末检同样必须用最终空scope，而不是旧access.read；合法缩窄继续通过。
        db.table("passport_bound_authority_fixture")?
            .where_eq("binding_id", "bound-oauth")?
            .update(vec![("scopes", "[]".into())])
            .await?;
        let redemption = crate::RefreshRedemption {
            id: payload.refresh_token_id.clone(),
            client_id: payload.client_id.clone(),
            access_token_id: payload.access_token_id.clone(),
            user_id: payload.user_id.clone(),
            scopes: payload.scopes.clone(),
            expires_at: payload.expire_time,
            authority: payload.authority()?,
        };
        storage
            .rotate_refresh(&redemption, snapshot, &rotated, true)
            .await
            .map_err(Error::internal)?;
        storage
            .revoke_access(&rotated.access.id)
            .await
            .map_err(Error::internal)?;
        assert!(
            storage
                .is_access_revoked(&rotated.access.id)
                .await
                .map_err(Error::internal)?
        );
        assert_eq!(phases.load(std::sync::atomic::Ordering::SeqCst), 7);
        assert_eq!(
            db.table("oauth_access_tokens")?
                .where_eq("id", rotated.access.id)?
                .count()
                .await?,
            1
        );
        Ok(())
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct ScopeClosingAuthority {
    db: Db,
    scope: Arc<openrust::container::Scope>,
    calls: std::sync::atomic::AtomicUsize,
    client_id: String,
    scopes: Vec<String>,
    binding: GrantAuthorityBinding,
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl<'tx> GrantAuthority<TransactionContext<'tx>> for ScopeClosingAuthority {
    fn validate<'a>(
        &'a self,
        db_context: &'a TransactionContext<'tx>,
        context: &'a GrantValidation<'a>,
        binding: &'a GrantAuthorityBinding,
    ) -> openrust::support::BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !db_context.is_from(&self.db) {
                return Err(Error::custom(
                    "fixture_transaction_origin_mismatch",
                    "事务context来源不匹配",
                ));
            }
            if context.operation != GrantOperation::Authorize
                || context.user_id != "7"
                || context.client_id != self.client_id.as_str()
                || context.scopes != self.scopes.as_slice()
                || binding.as_str() != self.binding.as_str()
            {
                return Err(Error::custom(
                    "passport_authority_rejected",
                    "关闭夹具委托不匹配",
                ));
            }
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                self.scope.close();
            }
            Ok(())
        })
    }
}
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl SqlGrantFixture {
    async fn scope_closes_during_final_authority_and_rolls_back(&self) -> Result<()> {
        let mut code = self.code.clone();
        code.id = "scope-close-final-code".to_owned();
        let authority = Arc::new(ScopeClosingAuthority {
            db: self.db.clone(),
            scope: self.owner.scope.clone(),
            calls: std::sync::atomic::AtomicUsize::new(0),
            client_id: code.client_id.clone(),
            scopes: code.scopes.clone(),
            binding: code.authority.clone(),
        });
        let mut repository = self.repository.clone();
        repository.authority = Some(authority.clone());
        let result = repository.persist_code(&code, &self.snapshot).await;
        assert!(matches!(result, Err(error) if error.code() == "scope_closed"));
        assert_eq!(authority.calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(
            self.db
                .table("oauth_auth_codes")?
                .where_eq("id", code.id)?
                .count()
                .await?,
            0
        );
        assert!(
            matches!(repository.is_access_revoked("not-read-after-close").await,
            Err(error) if error.code() == "scope_closed")
        );
        assert!(
            matches!(repository.revoke_access("not-written-after-close").await,
            Err(error) if error.code() == "scope_closed")
        );
        Ok(())
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl SqlGrantFixture {
    async fn foreign_context_origin_is_rejected_before_writes(&self) -> Result<()> {
        // 相同物理连接池也不是同一Db来源；不按URL、backend或连接池等同句柄。
        let foreign = Db::default();
        foreign.initialize(self.db.primary_connection()?)?;
        let mut repository = self.repository.clone();
        repository.authority = Some(Arc::new(Authority(
            std::sync::atomic::AtomicBool::new(false),
            foreign,
        )));
        let mut code = self.code.clone();
        code.id = "foreign-context-code".to_owned();
        assert_eq!(
            repository
                .persist_code(&code, &self.snapshot)
                .await
                .err()
                .map(|error| error.code().to_owned()),
            Some("fixture_transaction_origin_mismatch".to_owned())
        );
        assert_eq!(
            self.db
                .table("oauth_auth_codes")?
                .where_eq("id", code.id)?
                .count()
                .await?,
            0
        );
        Ok(())
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl SqlGrantFixture {
    async fn assert_resource_exchange_and_revocation(
        &self,
        code: &CodeRecord,
        tokens: &TokenRecords,
    ) -> Result<()> {
        use crate::{
            OAuthAccessVerifier, ResourceServer,
            resource_server::tests::{
                assert_unprojected, projected, rejected, resource_request, resource_token,
            },
            test_keys::PUBLIC,
        };
        use openrust::http::axum::{http::StatusCode, response::IntoResponse as _};

        let server = ResourceServer::new(
            Arc::new(OAuthAccessVerifier::from_pem(PUBLIC).map_err(Error::internal)?),
            Arc::new(self.repository.clone()),
        );
        let token = resource_token(
            &tokens.access.id,
            &tokens.access.client_id,
            Some(&tokens.access.user_id),
            &tokens.access.scopes,
        )?;
        let authorization = format!("Bearer {token}");
        let assert_denied = || async {
            let request = resource_request(&[&authorization]).await?;
            let error = rejected(&server, &request).await?;
            assert_eq!(error.into_response().status(), StatusCode::UNAUTHORIZED);
            assert_unprojected(&request)
        };
        assert_denied().await?;
        self.repository
            .exchange_code(code, &self.snapshot, tokens, 1_700_000_000)
            .await?;
        let authorized = resource_request(&[&authorization]).await?;
        server
            .validate_authenticated_request(&authorized)
            .await
            .map_err(Error::internal)?;
        let jti = projected(&authorized, "oauth_access_token_id")?;
        assert_eq!(jti.downcast::<String>()?.as_str(), tokens.access.id);
        assert!(self.repository.is_code_revoked(&code.id).await?);
        assert!(
            !self
                .repository
                .is_refresh_revoked("early-read-refresh")
                .await?
        );
        self.repository.revoke_access(&tokens.access.id).await?;
        assert_denied().await?;
        Ok(())
    }

    async fn ordinary_revocation_reads_match_active_revoked_and_missing_rows(&self) -> Result<()> {
        let mut code = self.code.clone();
        code.id = "early-read-code".to_owned();
        assert!(self.repository.is_code_revoked(&code.id).await?);
        assert!(
            self.repository
                .is_refresh_revoked("early-read-refresh")
                .await?
        );
        self.repository.persist_code(&code, &self.snapshot).await?;
        assert!(!self.repository.is_code_revoked(&code.id).await?);
        self.db
            .table("oauth_auth_codes")?
            .where_eq("id", code.id.clone())?
            .update(vec![("revoked", true.into())])
            .await?;
        assert!(self.repository.is_code_revoked(&code.id).await?);
        self.db
            .table("oauth_auth_codes")?
            .where_eq("id", code.id.clone())?
            .update(vec![("revoked", false.into())])
            .await?;
        let mut tokens = self.issued.clone();
        tokens.access.id = "early-read-access".to_owned();
        if let Some(refresh) = &mut tokens.refresh {
            refresh.id = "early-read-refresh".to_owned();
            refresh.access = tokens.access.clone();
        }
        self.assert_resource_exchange_and_revocation(&code, &tokens)
            .await?;
        // 官方refresh早检只看refresh本行，不由旧access已撤销推断。
        assert!(
            !self
                .repository
                .is_refresh_revoked("early-read-refresh")
                .await?
        );
        self.db
            .table("oauth_refresh_tokens")?
            .where_eq("id", "early-read-refresh")?
            .update(vec![("revoked", true.into())])
            .await?;
        assert!(
            self.repository
                .is_refresh_revoked("early-read-refresh")
                .await?
        );
        self.db
            .table("oauth_refresh_tokens")?
            .where_eq("id", "early-read-refresh")?
            .delete()
            .await?;
        self.db
            .table("oauth_access_tokens")?
            .where_eq("id", tokens.access.id)?
            .delete()
            .await?;
        self.db
            .table("oauth_auth_codes")?
            .where_eq("id", code.id.clone())?
            .delete()
            .await?;
        assert!(self.repository.is_code_revoked(&code.id).await?);
        assert!(
            self.repository
                .is_refresh_revoked("early-read-refresh")
                .await?
        );
        Ok(())
    }
}
