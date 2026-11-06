//! 原生 database adapter 与受管 Session 状态机的实际 SQLite 文件库回归。
#![cfg(feature = "db-sqlite")]

use super::DatabaseSessionStore;
use crate::{
    Error, Result,
    core::BoxFuture,
    db::{
        Db,
        sea_orm::{ConnectionTrait, DbBackend, QueryResult, Statement},
        sea_orm_migration::{MigrationTrait, MigratorTrait},
    },
    session::{
        DatabaseSessionHandler, Expiry, MemorySessionStore, Session, SessionId, SessionMigration,
        SessionStore,
    },
    testing::TestDatabase,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};

struct NativeMigrator;
impl MigratorTrait for NativeMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(SessionMigration)]
    }
}

fn check(condition: bool, message: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom("native_session_contract_failed", message))
    }
}

async fn owned<F>(callback: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a Db) -> BoxFuture<'a, Result<()>>,
{
    let guard = TestDatabase::sqlite_file::<NativeMigrator>().await?;
    let db = Db::default();
    let outcome = async {
        db.initialize(guard.connection()?)?;
        callback(&db).await
    }
    .await;
    drop(db);
    // 所有断言使用 Result；普通失败同样显式关闭 owned 数据库，不跳过清理。
    let closed = guard.close().await;
    outcome?;
    closed
}

async fn row(db: &Db, id: &SessionId) -> Result<Option<QueryResult>> {
    let table = db.quoted_table_name("sessions")?;
    db.connection()?
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            format!("SELECT id,payload,last_activity FROM {table} WHERE id=?"),
            [id.as_str().to_owned().into()],
        ))
        .await
        .map_err(Error::internal)
}

fn attributes(row: &QueryResult) -> Result<serde_json::Value> {
    let payload: String = row.try_get("", "payload").map_err(Error::internal)?;
    let bytes = STANDARD.decode(payload).map_err(Error::internal)?;
    serde_json::from_slice(&bytes).map_err(Error::internal)
}

#[tokio::test]
async fn same_arc_prototype_gives_each_session_its_own_exists_state_and_native_payload()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let prototype: Arc<dyn SessionStore> = Arc::new(DatabaseSessionStore::new(db.clone()));
            let first = Session::new(None, prototype.clone(), None)?;
            first.start().await?;
            first.put_typed("marker", "A")?;
            first.save().await?;
            let first_id = first.id()?;
            let reader = Session::new(Some(first_id.clone()), prototype.clone(), None)?;
            reader.start().await?;
            check(
                reader.get_typed::<String>("marker")?.as_deref() == Some("A"),
                "真实 A 行未加载",
            )?;
            let second = Session::new(None, prototype, None)?;
            second.start().await?;
            second.put_typed("marker", "B")?;
            let before = OffsetDateTime::now_utc().unix_timestamp();
            second.save().await?;
            let after = OffsetDateTime::now_utc().unix_timestamp();
            let second_id = second.id()?;
            check(first_id != second_id, "独立 Session 意外复用 ID")?;
            let persisted = row(db, &second_id).await?.ok_or_else(|| {
                Error::custom(
                    "missing_native_b",
                    "新 B 必须实际 INSERT，不能由污染的 exists 执行零行 UPDATE",
                )
            })?;
            let stored_id: String = persisted.try_get("", "id").map_err(Error::internal)?;
            check(stored_id == second_id.as_str(), "原生主键不是原始 ID")?;
            let decoded = attributes(&persisted)?;
            check(
                decoded == second.all_json()?,
                "Base64 必须只编码属性，不能额外套身份 envelope",
            )?;
            check(
                decoded.get("id_hash").is_none()
                    && decoded.get("expiry_seconds").is_none()
                    && decoded.get("version").is_none(),
                "原生 payload 混入增强 envelope",
            )?;
            let activity: i64 = persisted
                .try_get("", "last_activity")
                .map_err(Error::internal)?;
            check(
                (before..=after).contains(&activity),
                "last_activity 不是实际保存时的 Unix 秒",
            )?;
            check(row(db, &first_id).await?.is_some(), "B 保存误删旧 A")?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn native_migrate_inserts_new_id_and_destroy_flag_controls_old_row() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let prototype = Arc::new(DatabaseSessionStore::new(db.clone()));
            let session = Session::new(None, prototype, None)?;
            session.start().await?;
            session.put_typed("kept", "migration-data")?;
            session.save().await?;
            let original = session.id()?;
            check(session.migrate(false).await?, "migrate(false) 未成功")?;
            let second = session.id()?;
            check(second != original, "migrate(false) 未换 ID")?;
            session.save().await?;
            check(
                row(db, &original).await?.is_some(),
                "migrate(false) 删除了旧行",
            )?;
            let inserted = row(db, &second)
                .await?
                .ok_or_else(|| Error::custom("missing_migrated_row", "新 ID 必须真实 INSERT"))?;
            check(
                attributes(&inserted)?
                    .get("kept")
                    .and_then(serde_json::Value::as_str)
                    == Some("migration-data"),
                "迁移丢属性",
            )?;
            check(session.migrate(true).await?, "migrate(true) 未成功")?;
            let third = session.id()?;
            check(third != second, "migrate(true) 未换 ID")?;
            check(
                row(db, &second).await?.is_none(),
                "migrate(true) 未立即删除旧 ID",
            )?;
            session.save().await?;
            check(
                row(db, &third).await?.is_some(),
                "migrate(true) 后新 ID 未 INSERT",
            )?;
            check(
                row(db, &original).await?.is_some(),
                "销毁第二 ID 误删更早保留的第一 ID",
            )?;
            check(
                session.get_typed::<String>("kept")?.as_deref() == Some("migration-data"),
                "换 ID 丢属性",
            )?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn native_expired_read_is_empty_without_deleting_and_save_updates_same_id() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let id = SessionId::generate()?;
            let mut handler = DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), 1)?;
            handler.write(id.as_str(), br#"{"old":"expired"}"#).await?;
            let cutoff = OffsetDateTime::now_utc().unix_timestamp() - 3600;
            db.connection()?
                .execute_raw(Statement::from_sql_and_values(
                    DbBackend::Sqlite,
                    "UPDATE sessions SET last_activity=? WHERE id=?",
                    [cutoff.into(), id.as_str().to_owned().into()],
                ))
                .await
                .map_err(Error::internal)?;
            let mut reader = DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), 1)?;
            check(
                reader.read(id.as_str()).await?.is_empty(),
                "原生过期 read 应返回空载荷",
            )?;
            check(reader.exists(), "过期行必须保持 handler exists=true")?;
            check(row(db, &id).await?.is_some(), "过期 read 不得删除原行")?;
            let session = Session::new(
                Some(id.clone()),
                Arc::new(DatabaseSessionStore::new(db.clone()).configured("sessions", 1)?),
                None,
            )?;
            session.start().await?;
            check(
                session.get_typed::<String>("old")?.is_none(),
                "过期属性泄漏给新请求",
            )?;
            session.put_typed("new", "fresh")?;
            session.save().await?;
            check(session.id()? == id, "原生过期会话被强制换 ID")?;
            let persisted = row(db, &id)
                .await?
                .ok_or_else(|| Error::custom("expired_row_missing", "原生 save 应更新过期原行"))?;
            check(
                attributes(&persisted)?
                    .get("new")
                    .and_then(serde_json::Value::as_str)
                    == Some("fresh"),
                "过期原行未写入新属性",
            )?;
            let activity: i64 = persisted
                .try_get("", "last_activity")
                .map_err(Error::internal)?;
            check(activity > cutoff, "save 未更新真实 last_activity")?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn native_zero_row_update_succeeds_and_valid_missing_id_is_not_strongly_revoked() -> Result<()>
{
    owned(|db| {
        Box::pin(async move {
            let prototype: Arc<dyn SessionStore> = Arc::new(DatabaseSessionStore::new(db.clone()));
            let session = Session::new(None, prototype.clone(), None)?;
            session.start().await?;
            session.put_typed("owned", "before-delete")?;
            session.save().await?;
            let id = session.id()?;
            prototype.delete(&id).await?;
            session.put_typed("owned", "zero-row-update")?;
            session.save().await?;
            check(session.id()? == id, "原生零行更新被当成强撤销并换 ID")?;
            check(
                row(db, &id).await?.is_none(),
                "原生 exists=true 的零行 UPDATE 不应偷偷 INSERT",
            )?;
            let missing = SessionId::generate()?;
            let fresh = Session::new(Some(missing.clone()), prototype, None)?;
            fresh.start().await?;
            fresh.put_typed("owned", "valid-missing")?;
            fresh.save().await?;
            check(fresh.id()? == missing, "合法 missing ID 不应套增强撤销语义")?;
            check(
                row(db, &missing).await?.is_some(),
                "合法 missing ID 应正常 INSERT",
            )?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn native_absolute_expiry_and_bound_guarded_transfer_are_rejected_without_mutation()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let native: Arc<dyn SessionStore> = Arc::new(DatabaseSessionStore::new(db.clone()));
            let session = Session::new(None, native.clone(), None)?;
            session.start().await?;
            session.put_typed("owned", "retained-native")?;
            session.save().await?;
            let id = session.id()?;
            let original = session.handler()?;
            let absolute = Expiry::AtDateTime(OffsetDateTime::now_utc() + Duration::hours(1));
            check(
                session
                    .set_expiry(Some(absolute))
                    .err()
                    .is_some_and(|error| error.code() == "session_expiry_unsupported"),
                "原生 driver 不得伪装绝对截止支持",
            )?;
            check(
                Session::new(None, native.clone(), Some(absolute)).is_err(),
                "原生构造应拒绝绝对截止",
            )?;
            let guarded: Arc<dyn SessionStore> = Arc::new(MemorySessionStore::default());
            check(
                session
                    .set_handler(guarded.clone())
                    .err()
                    .is_some_and(|error| error.code() == "session_transfer_unsupported"),
                "已有属性/绑定的 native→guarded 转移必须明确拒绝",
            )?;
            check(
                Arc::ptr_eq(&original, &session.handler()?),
                "被拒绝转移仍修改了原 handler",
            )?;
            check(
                session.id()? == id
                    && session.get_typed::<String>("owned")?.as_deref() == Some("retained-native"),
                "被拒绝的期限/转移丢 ID 或属性",
            )?;
            check(row(db, &id).await?.is_some(), "被拒绝转移删除了原生源行")?;
            check(
                !matches!(session.expiry()?, Some(Expiry::AtDateTime(_))),
                "拒绝绝对截止后仍改写了原期限",
            )?;
            // 同语义的原生 setHandler 仍按上游允许，不一刀切拒绝不同 Arc identity。
            session.set_handler(Arc::new(DatabaseSessionStore::new(db.clone())))?;
            session.save().await?;
            check(
                session.id()? == id && row(db, &id).await?.is_some(),
                "Native→Native 切换破坏原生绑定",
            )?;
            let enhanced = Session::new(None, guarded, None)?;
            enhanced.start().await?;
            enhanced.put_typed("owned", "retained-guarded")?;
            enhanced.save().await?;
            let enhanced_id = enhanced.id()?;
            let enhanced_handler = enhanced.handler()?;
            check(
                enhanced
                    .set_handler(native)
                    .err()
                    .is_some_and(|error| error.code() == "session_transfer_unsupported"),
                "已有绑定的 guarded→native 转移必须明确拒绝",
            )?;
            check(
                Arc::ptr_eq(&enhanced_handler, &enhanced.handler()?)
                    && enhanced.id()? == enhanced_id,
                "被拒绝反向转移修改了原绑定",
            )?;
            check(
                enhanced.get_typed::<String>("owned")?.as_deref() == Some("retained-guarded"),
                "被拒绝反向转移丢属性",
            )?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn closed_native_pool_error_cannot_be_reported_as_written() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let session =
                Session::new(None, Arc::new(DatabaseSessionStore::new(db.clone())), None)?;
            session.start().await?;
            session.put_typed("owned", "initial-persisted")?;
            session.save().await?;
            session.put_typed("owned", "must-not-report-written")?;
            db.connection()?
                .close_by_ref()
                .await
                .map_err(Error::internal)?;
            check(
                session.save().await.is_err(),
                "关闭池的 UPDATE 基础设施错误不能 fake Written",
            )?;
            let missing =
                Session::new(None, Arc::new(DatabaseSessionStore::new(db.clone())), None)?;
            missing.put_typed("owned", "must-not-insert")?;
            check(
                missing.save().await.is_err(),
                "关闭池的新记录读取/INSERT 错误不能 fake Written",
            )?;
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn independent_authorized_read_uses_configured_activity_deadline_and_never_renews()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let store = DatabaseSessionStore::new(db.clone()).configured("sessions", 1)?;
            let session = Session::new(
                None,
                Arc::new(DatabaseSessionStore::new(db.clone()).configured("sessions", 1)?),
                None,
            )?;
            session.start().await?;
            session.put_typed("owned", "native-live")?;
            session.save().await?;
            let id = session.id()?;
            let activity = OffsetDateTime::now_utc().unix_timestamp() - 30;
            db.table("sessions")?
                .where_eq("id", id.as_str())?
                .update(vec![("last_activity", activity.into())])
                .await?;
            let record = store
                .load_authorized(&id, crate::session::DEFAULT_MAX_BYTES)
                .await?
                .ok_or(Error::Unauthorized)?;
            check(
                record.expiry_date.unix_timestamp() == activity + 60,
                "独立授权读取未使用真实活动时间及配置期限",
            )?;
            let after = row(db, &id).await?.ok_or(Error::Unauthorized)?;
            check(
                after
                    .try_get::<i64>("", "last_activity")
                    .map_err(Error::internal)?
                    == activity,
                "独立读取不得续期",
            )?;
            db.table("sessions")?
                .where_eq("id", id.as_str())?
                .update(vec![("last_activity", (activity - 120).into())])
                .await?;
            check(
                store
                    .load_authorized(&id, crate::session::DEFAULT_MAX_BYTES)
                    .await?
                    .is_none(),
                "过期真实记录不得授权",
            )?;
            check(row(db, &id).await?.is_some(), "授权读取不得删除过期行")?;
            store.delete(&id).await?;
            check(
                store
                    .load_authorized(&id, crate::session::DEFAULT_MAX_BYTES)
                    .await?
                    .is_none(),
                "删除后独立授权读取仍返回记录",
            )
        })
    })
    .await
}
