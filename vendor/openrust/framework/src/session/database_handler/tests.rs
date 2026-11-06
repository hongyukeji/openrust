//! 固定 Laravel `DatabaseSessionHandler` 的真实数据库契约；不以旧增强store代替。

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, QueryResult, Statement, Value};
use sea_orm_migration::{MigrationTrait, MigratorTrait, SchemaManager};

use super::{DatabaseSessionHandler, DatabaseSessionMetadata};
use crate::{
    Error, Result,
    database::Db,
    support::calendar::{CalendarClock, CalendarInstant, FixedClock},
    testing::TestDatabase,
};

const NOW: i64 = 7_200;
const CUTOFF: i64 = NOW - 60;

struct SessionMigrator;

#[sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for SessionMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(crate::session::SessionMigration)]
    }
}

fn handler(db: Db) -> Result<DatabaseSessionHandler> {
    Ok(
        DatabaseSessionHandler::new(db, Some("sessions".into()), 1)?.with_clock(Arc::new(
            FixedClock::new(CalendarInstant::new(NOW, 999_999)?),
        )),
    )
}

fn marker(backend: DbBackend, index: usize) -> String {
    if backend == DbBackend::Postgres {
        format!("${index}")
    } else {
        "?".into()
    }
}

async fn seed(
    connection: &DatabaseConnection,
    id: &str,
    payload: &str,
    activity: i64,
) -> Result<()> {
    let backend = connection.get_database_backend();
    connection
        .execute_raw(Statement::from_sql_and_values(
            backend,
            format!(
                "INSERT INTO sessions (id,payload,last_activity) VALUES ({},{},{})",
                marker(backend, 1),
                marker(backend, 2),
                marker(backend, 3)
            ),
            [
                id.to_owned().into(),
                payload.to_owned().into(),
                activity.into(),
            ],
        ))
        .await
        .map_err(Error::internal)?;
    Ok(())
}

async fn row(connection: &DatabaseConnection, id: &str) -> Result<Option<QueryResult>> {
    let backend = connection.get_database_backend();
    let activity = if backend == DbBackend::Postgres {
        "CAST(last_activity AS BIGINT)"
    } else {
        "last_activity"
    };
    // 官方 MySQL foreignId 是 unsigned；这里只将固定小整数夹具投影到统一断言类型。
    let user = if backend == DbBackend::MySql {
        "CAST(user_id AS SIGNED)"
    } else {
        "user_id"
    };
    connection.query_one_raw(Statement::from_sql_and_values(backend,
        format!("SELECT id,{user} AS user_id,ip_address,user_agent,payload,{activity} AS last_activity FROM sessions WHERE id={}", marker(backend, 1)),
        [id.to_owned().into()],
    )).await.map_err(Error::internal)
}

fn required(row: Option<QueryResult>) -> Result<QueryResult> {
    row.ok_or_else(|| Error::custom("test_session_row_missing", "Session契约测试缺少预期行"))
}

async fn canonical_schema(connection: &DatabaseConnection) -> Result<()> {
    let schema = SchemaManager::new(connection);
    if connection.get_database_backend() == DbBackend::MySql {
        let row = connection.query_one_raw(Statement::from_string(DbBackend::MySql,
            "SELECT COLUMN_TYPE AS column_kind FROM information_schema.columns WHERE table_schema=DATABASE() AND table_name='sessions' AND column_name='user_id'".to_owned()))
            .await.map_err(Error::internal)?;
        assert_eq!(
            required(row)?
                .try_get::<String>("", "column_kind")
                .map_err(Error::internal)?,
            "bigint unsigned"
        );
    }
    if connection.get_database_backend() == DbBackend::Postgres {
        let row = connection.query_one_raw(Statement::from_string(DbBackend::Postgres,
            "SELECT data_type FROM information_schema.columns WHERE table_schema=current_schema() AND table_name='sessions' AND column_name='last_activity'".to_owned()))
            .await.map_err(Error::internal)?;
        assert_eq!(
            required(row)?
                .try_get::<String>("", "data_type")
                .map_err(Error::internal)?,
            "integer"
        );
    }
    for column in [
        "id",
        "user_id",
        "ip_address",
        "user_agent",
        "payload",
        "last_activity",
    ] {
        assert!(
            schema
                .has_column("sessions", column)
                .await
                .map_err(Error::internal)?,
            "生产SessionMigration缺少{column}"
        );
    }
    for index in ["sessions_user_id_index", "sessions_last_activity_index"] {
        assert!(
            schema
                .has_index("sessions", index)
                .await
                .map_err(Error::internal)?,
            "生产 SessionMigration 缺少固定 Blueprint 索引 {index}"
        );
    }
    seed(connection, "schema-retained", "c2NoZW1h", NOW).await?;
    let names = crate::database::DatabaseTableNames::default();
    names
        .scope(crate::session::SessionGcMigration.down(&schema))
        .await
        .map_err(Error::internal)?;
    assert!(
        !schema
            .has_index("sessions", "sessions_last_activity_index")
            .await
            .map_err(Error::internal)?
    );
    names
        .scope(crate::session::SessionGcMigration.up(&schema))
        .await
        .map_err(Error::internal)?;
    assert!(
        schema
            .has_index("sessions", "sessions_last_activity_index")
            .await
            .map_err(Error::internal)?
    );
    assert!(
        row(connection, "schema-retained").await?.is_some(),
        "原生活动索引回滚/恢复不能丢失会话行"
    );
    for old in ["id_hash", "data", "expires_at", "created_at"] {
        assert!(
            !schema
                .has_column("sessions", old)
                .await
                .map_err(Error::internal)?,
            "原生表不能混用旧增强字段{old}"
        );
    }
    Ok(())
}

async fn raw_ids_expiry_and_exists(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    seed(&connection, "empty", "", NOW).await?;
    seed(&connection, "expired", "b2xk", CUTOFF - 1).await?;
    seed(&connection, "equal-cutoff", "ZXF1YWw=", CUTOFF).await?;
    let mut native = handler(db.clone())?;
    assert!(native.open() && native.close());
    assert_eq!(native.read("missing").await?, Vec::<u8>::new());
    assert!(!native.exists());
    assert!(!native.validate_id("missing").await?);
    assert!(native.validate_id("expired").await?);
    assert!(!native.exists(), "validate_id不能改变请求exists状态");
    assert_eq!(native.read("empty").await?, Vec::<u8>::new());
    assert!(native.exists());
    native.set_exists(false);
    assert_eq!(native.read("expired").await?, Vec::<u8>::new());
    assert!(native.exists());
    assert!(
        row(&connection, "expired").await?.is_some(),
        "read过期不能删行"
    );
    native.set_exists(false);
    assert_eq!(native.read("equal-cutoff").await?, b"equal");
    assert!(native.write("expired", b"restored").await?);
    assert_eq!(handler(db.clone())?.read("expired").await?, b"restored");

    let mut native = handler(db.clone())?;
    let raw = "raw/id-is-not-a-40-character-SessionId";
    assert!(native.write(raw, b"first").await?);
    assert!(native.exists());
    assert_eq!(
        required(row(&connection, raw).await?)?
            .try_get::<String>("", "id")
            .map_err(Error::internal)?,
        raw
    );
    assert!(
        native
            .write("second-id", b"lost-by-upstream-update-zero")
            .await?
    );
    assert!(
        row(&connection, "second-id").await?.is_none(),
        "exists跨ID保留，UPDATE零行仍true"
    );
    assert_eq!(native.read("another-missing-id").await?, Vec::<u8>::new());
    assert!(native.exists(), "read缺失不能自动reset既有exists");
    native.set_exists(false);
    assert!(native.write("second-id", b"created-after-reset").await?);
    assert!(row(&connection, "second-id").await?.is_some());
    assert!(native.destroy("second-id").await?);
    assert!(native.destroy("second-id").await?);
    assert!(native.exists(), "destroy不修改exists");
    assert!(row(&connection, "second-id").await?.is_none());
    Ok(())
}

async fn binary_relaxed_decode_and_bound_ids(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    let binary = [0_u8, 255, 128, b'a', 0, b'\n'];
    let id = "x' OR 1=1 --";
    assert!(handler(db.clone())?.write(id, &binary).await?);
    assert_eq!(handler(db.clone())?.read(id).await?, binary);
    let stored = required(row(&connection, id).await?)?;
    assert_eq!(
        stored
            .try_get::<String>("", "payload")
            .map_err(Error::internal)?,
        "AP+AYQAK"
    );
    assert_eq!(
        stored
            .try_get::<String>("", "id")
            .map_err(Error::internal)?,
        id
    );
    assert!(
        row(&connection, "empty").await?.is_some(),
        "SQL注入ID不能改写其他行"
    );
    // 固定PHP8.5非严格base64_decode探针8例，期望来自原始回执而非Rust编码器。
    for (index, (encoded, decoded)) in [
        ("a=Gk=", b"hi".as_slice()),
        ("aG=k", b"hi".as_slice()),
        ("====YQ==", b"a".as_slice()),
        ("a", b"".as_slice()),
        ("aGk=YQ==", &[0x68, 0x69, 0x18]),
        ("YQ===", b"a".as_slice()),
        ("YWJj=ZGVm", b"abcdef".as_slice()),
        ("aGk===", b"hi".as_slice()),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("relaxed-{index}");
        seed(&connection, &id, encoded, NOW).await?;
        assert_eq!(
            handler(db.clone())?.read(&id).await?,
            decoded,
            "PHP非严格解码probe {index}"
        );
    }
    assert!(handler(db.clone())?.destroy(id).await?);
    assert!(
        row(&connection, "empty").await?.is_some(),
        "绑定DELETE不能扩散至其他行"
    );
    Ok(())
}

async fn metadata_omission_null_and_unicode(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    let mut native = handler(db.clone())?.with_metadata(DatabaseSessionMetadata {
        user_id: Some(7_i64.into()),
        ip_address: Some(Some("2001:db8::1".into())),
        user_agent: Some("汉🙂".repeat(300)),
    });
    assert!(native.write("metadata", b"one").await?);
    let stored = required(row(&connection, "metadata").await?)?;
    assert_eq!(
        stored
            .try_get::<Option<i64>>("", "user_id")
            .map_err(Error::internal)?,
        Some(7)
    );
    assert_eq!(
        stored
            .try_get::<Option<String>>("", "ip_address")
            .map_err(Error::internal)?,
        Some("2001:db8::1".into())
    );
    assert_eq!(
        stored
            .try_get::<String>("", "user_agent")
            .map_err(Error::internal)?,
        "汉🙂".repeat(250)
    );
    native.set_metadata(DatabaseSessionMetadata::default());
    assert!(native.write("metadata", b"two").await?);
    let stored = required(row(&connection, "metadata").await?)?;
    assert_eq!(
        stored
            .try_get::<Option<i64>>("", "user_id")
            .map_err(Error::internal)?,
        Some(7)
    );
    assert_eq!(
        stored
            .try_get::<Option<String>>("", "ip_address")
            .map_err(Error::internal)?,
        Some("2001:db8::1".into())
    );
    native.set_metadata(DatabaseSessionMetadata {
        user_id: Some(Value::BigInt(None)),
        ip_address: Some(None),
        user_agent: None,
    });
    assert!(native.write("metadata", b"three").await?);
    let stored = required(row(&connection, "metadata").await?)?;
    assert_eq!(
        stored
            .try_get::<Option<i64>>("", "user_id")
            .map_err(Error::internal)?,
        None
    );
    assert_eq!(
        stored
            .try_get::<Option<String>>("", "ip_address")
            .map_err(Error::internal)?,
        None
    );
    assert_eq!(
        stored
            .try_get::<String>("", "user_agent")
            .map_err(Error::internal)?,
        "汉🙂".repeat(250)
    );
    native.set_metadata(DatabaseSessionMetadata {
        user_agent: Some(String::new()),
        ..Default::default()
    });
    assert!(native.write("metadata", b"four").await?);
    assert_eq!(
        required(row(&connection, "metadata").await?)?
            .try_get::<String>("", "user_agent")
            .map_err(Error::internal)?,
        ""
    );
    Ok(())
}

async fn budgets_before_and_after_base64(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    let mut zero = handler(db.clone())?.with_max_bytes(0)?;
    assert!(zero.write("zero-budget", b"").await?);
    assert_eq!(zero.read("zero-budget").await?, Vec::<u8>::new());
    assert!(zero.write("zero-budget", b"a").await.is_err());
    seed(&connection, "zero-padding", "====", NOW).await?;
    assert!(
        handler(db.clone())?
            .with_max_bytes(0)?
            .read("zero-padding")
            .await
            .is_err()
    );
    let mut small = handler(db.clone())?.with_max_bytes(1)?;
    assert!(small.write("one-byte", b"a").await?);
    assert!(small.write("one-byte", b"ab").await.is_err());
    assert_eq!(handler(db.clone())?.read("one-byte").await?, b"a");
    seed(&connection, "encoded-too-long", "YQ===", NOW).await?;
    assert!(
        handler(db.clone())?
            .with_max_bytes(1)?
            .read("encoded-too-long")
            .await
            .is_err(),
        "解码前encoded预算必须生效"
    );
    seed(&connection, "decoded-too-long", "AAAA", NOW).await?;
    assert!(
        handler(db.clone())?
            .with_max_bytes(1)?
            .read("decoded-too-long")
            .await
            .is_err(),
        "encoded长度合法仍要检查decoded预算"
    );
    assert!(
        handler(db.clone())?
            .with_max_bytes(crate::config::SessionConfig::MAX_BYTES + 1)
            .is_err()
    );
    let mut missing_table = DatabaseSessionHandler::new(db.clone(), None, 1)?;
    assert!(
        missing_table.read("absent").await.is_err(),
        "None表名不能偷偷fallback到sessions"
    );
    assert!(DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), i64::MAX).is_err());
    Ok(())
}

struct StepClock(AtomicUsize);

async fn server_error_classification(db: &Db) -> Result<()> {
    let expected = || Error::custom("test_expected_session_error", "预期数据库Session错误");
    let mut missing = DatabaseSessionHandler::new(db.clone(), None, 1)?;
    let error = missing.read("absent").await.err().ok_or_else(expected)?;
    assert_eq!(error.code(), "session_database_table_missing");
    assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    let error = DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), i64::MAX)
        .err()
        .ok_or_else(expected)?;
    assert_eq!(error.code(), "session_database_config_invalid");
    assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    let mut unavailable = DatabaseSessionHandler::new(Db::default(), Some("sessions".into()), 1)?;
    let error = unavailable
        .read("absent")
        .await
        .err()
        .ok_or_else(expected)?;
    assert_eq!(error.code(), "session_database_unavailable");
    assert_eq!(error.status(), http::StatusCode::INTERNAL_SERVER_ERROR);
    Ok(())
}
impl CalendarClock for StepClock {
    fn now(&self) -> Result<CalendarInstant> {
        let offset = self.0.fetch_add(1, Ordering::SeqCst);
        CalendarInstant::new(NOW + i64::try_from(offset).map_err(Error::internal)?, 0)
    }
}

async fn payload_time_precedes_read_and_gc_uses_seconds(db: &Db) -> Result<()> {
    let connection = db.connection()?;
    seed(&connection, "write-clock", "b2xk", CUTOFF).await?;
    let clock = Arc::new(StepClock(AtomicUsize::new(0)));
    let mut native = DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), 1)?
        .with_clock(clock.clone());
    assert!(native.write("write-clock", b"new").await?);
    assert_eq!(
        clock.0.load(Ordering::SeqCst),
        2,
        "write先取payload时间，未知exists再read取时"
    );
    assert_eq!(
        required(row(&connection, "write-clock").await?)?
            .try_get::<i64>("", "last_activity")
            .map_err(Error::internal)?,
        NOW
    );
    // 与其他场景隔离GC计数，参数是秒而非handler的minutes。
    connection
        .execute_unprepared("DELETE FROM sessions")
        .await
        .map_err(Error::internal)?;
    seed(&connection, "gc-before", "YQ==", NOW - 11).await?;
    seed(&connection, "gc-equal", "YQ==", NOW - 10).await?;
    seed(&connection, "gc-after", "YQ==", NOW - 9).await?;
    let native = handler(db.clone())?;
    assert_eq!(native.gc(10).await?, 2);
    assert!(row(&connection, "gc-before").await?.is_none());
    assert!(row(&connection, "gc-equal").await?.is_none());
    assert!(row(&connection, "gc-after").await?.is_some());
    assert_eq!(native.gc(10).await?, 0);
    Ok(())
}

async fn common_contract(fixture: &TestDatabase) -> Result<()> {
    let db = Db::default();
    db.initialize(fixture.connection()?)?;
    eprintln!("native_session_contract phase=canonical_schema");
    canonical_schema(&db.connection()?).await?;
    eprintln!("native_session_contract phase=raw_ids_expiry_exists");
    raw_ids_expiry_and_exists(&db).await?;
    eprintln!("native_session_contract phase=binary_relaxed_decode");
    binary_relaxed_decode_and_bound_ids(&db).await?;
    eprintln!("native_session_contract phase=metadata");
    metadata_omission_null_and_unicode(&db).await?;
    eprintln!("native_session_contract phase=budgets");
    budgets_before_and_after_base64(&db).await?;
    eprintln!("native_session_contract phase=clock_gc");
    server_error_classification(&db).await?;
    payload_time_precedes_read_and_gc_uses_seconds(&db).await
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn canonical_handler_contract_uses_owned_real_sqlite_file() -> Result<()> {
    let fixture = TestDatabase::sqlite_file::<SessionMigrator>().await?;
    let outcome = common_contract(&fixture).await;
    if let Err(error) = &outcome {
        eprintln!(
            "native_session_contract source={:?}",
            std::error::Error::source(error)
        );
    }
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

// 显式入口：缺凭据是错误，不把未执行包装成成功skip；同一契约逻辑用于真实MySQL/PG。
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
#[ignore = "显式设置OPENRUST_TEST_DATABASE_URL后运行真实服务契约"]
async fn canonical_handler_contract_uses_explicit_real_server_database() -> Result<()> {
    let admin = crate::test_database_environment::database_url().map_err(Error::internal)?;
    let backend = crate::database::DatabaseBackend::from_url(&admin)?;
    assert!(matches!(
        backend,
        crate::database::DatabaseBackend::PostgreSql | crate::database::DatabaseBackend::MySql
    ));
    let fixture = TestDatabase::create::<SessionMigrator>(&admin).await?;
    let outcome = common_contract(&fixture).await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn insert_nonunique_query_failure_falls_back_to_update() -> Result<()> {
    let fixture = TestDatabase::sqlite_file::<SessionMigrator>().await?;
    let outcome: Result<()> = async {
        let connection = fixture.connection()?;
        // RAISE(FAIL)保留本语句此前写入；AFTER trigger把payload改成old后报非unique错误。
        // handler必须真正fallback UPDATE才能把它改回new，不能忽略错误或仅捕获唯一冲突。
        connection.execute_unprepared("CREATE TRIGGER reject_session_insert AFTER INSERT ON sessions WHEN NEW.id = 'trigger-fallback' BEGIN UPDATE sessions SET payload = 'b2xk' WHERE id = NEW.id; SELECT RAISE(FAIL, 'controlled nonunique insert failure'); END").await.map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        let mut native = handler(db.clone())?;
        native.set_exists(false);
        assert!(native.write("trigger-fallback", b"new").await?);
        assert!(native.exists());
        assert_eq!(handler(db)?.read("trigger-fallback").await?, b"new");
        connection.execute_unprepared("DROP TRIGGER reject_session_insert").await.map_err(Error::internal)?;
        Ok(())
    }.await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn reads_use_primary_even_when_same_id_has_different_replica_payload() -> Result<()> {
    let primary = TestDatabase::sqlite_file::<SessionMigrator>().await?;
    let replica = match TestDatabase::sqlite_file::<SessionMigrator>().await {
        Ok(replica) => replica,
        Err(error) => {
            let _ = primary.close().await;
            return Err(error);
        }
    };
    let outcome: Result<()> = async {
        seed(&primary.connection()?, "writer-read", "cHJpbWFyeQ==", NOW).await?;
        seed(&replica.connection()?, "writer-read", "cmVwbGljYQ==", NOW).await?;
        let db = Db::default();
        db.initialize_with_replicas(primary.connection()?, vec![replica.connection()?])?;
        assert_eq!(handler(db)?.read("writer-read").await?, b"primary");
        Ok(())
    }
    .await;
    let replica_cleanup = replica.close().await;
    let primary_cleanup = primary.close().await;
    outcome?;
    replica_cleanup?;
    primary_cleanup
}

#[cfg(feature = "db-mysql")]
async fn mysql_signal_insert_case(
    db: &Db,
    connection: &DatabaseConnection,
    trigger: &'static str,
    sql_state: &'static str,
    number: u16,
    fallback: bool,
) -> Result<()> {
    let create = format!(
        "CREATE TRIGGER {trigger} BEFORE INSERT ON sessions FOR EACH ROW SIGNAL SQLSTATE '{sql_state}' SET MYSQL_ERRNO={number}, MESSAGE_TEXT='controlled session insert error'"
    );
    connection
        .execute_unprepared(&create)
        .await
        .map_err(Error::internal)?;
    let outcome: Result<()> = async {
        let id = format!("mysql-signal-{number}");
        let mut native = handler(db.clone())?;
        native.set_exists(false);
        let result = native.write(&id, b"opaque-native-payload").await;
        if fallback {
            if !result? {
                return Err(Error::custom(
                    "mysql_session_fallback_not_true",
                    "普通非唯一 QueryException 必须回退零行 UPDATE 并返回 true",
                ));
            }
            if !native.exists() {
                return Err(Error::custom(
                    "mysql_session_fallback_exists_missing",
                    "普通 QueryException 回退后必须设置 exists=true",
                ));
            }
        } else {
            if result.is_ok() {
                return Err(Error::custom(
                    "mysql_session_infrastructure_swallowed",
                    "MYSQL_ERRNO 1053 不得被零行 UPDATE 伪装成功",
                ));
            }
            if native.exists() {
                return Err(Error::custom(
                    "mysql_session_infrastructure_changed_exists",
                    "基础设施写入失败不得改变 exists=false",
                ));
            }
        }
        if row(connection, &id).await?.is_some() {
            return Err(Error::custom(
                "mysql_session_signal_unexpected_row",
                "BEFORE INSERT SIGNAL 后不得留下 Session 行",
            ));
        }
        Ok(())
    }
    .await;
    // 断言以 Result 返回，写入/验证失败也先尝试 DROP，再回报原错误；fixture 仍会关闭。
    let cleanup = connection
        .execute_unprepared(&format!("DROP TRIGGER {trigger}"))
        .await
        .map(|_| ())
        .map_err(Error::internal);
    outcome?;
    cleanup
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
#[ignore = "必须设置真实MySQL OPENRUST_TEST_DATABASE_URL，验证SQLSTATE与MYSQL_ERRNO分类"]
async fn mysql_native_handler_distinguishes_infrastructure_number_from_query_sqlstate() -> Result<()>
{
    let admin = crate::test_database_environment::database_url().map_err(Error::internal)?;
    if crate::database::DatabaseBackend::from_url(&admin)?
        != crate::database::DatabaseBackend::MySql
    {
        return Err(Error::custom(
            "mysql_session_fixture_backend_required",
            "此真实契约入口必须选择 MySQL，不能跳过",
        ));
    }
    let fixture = TestDatabase::create::<SessionMigrator>(&admin).await?;
    let outcome: Result<()> = async {
        let connection = fixture.connection()?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        canonical_schema(&connection).await?;
        // HY000 是 SQLSTATE，1053 才是基础设施分类所需的 MySQL number。
        mysql_signal_insert_case(
            &db,
            &connection,
            "session_infrastructure_1053",
            "HY000",
            1053,
            false,
        )
        .await?;
        // 非唯一、普通查询错误仍保持固定 Laravel 的 INSERT-error→UPDATE 行为。
        mysql_signal_insert_case(&db, &connection, "session_query_1644", "45000", 1644, true).await
    }
    .await;
    let cleanup = fixture.close().await;
    outcome?;
    cleanup
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn authorized_read_uses_same_row_deadline_and_controlled_clock_without_renewal() -> Result<()>
{
    let database = TestDatabase::sqlite_file::<SessionMigrator>().await?;
    let outcome: Result<()> = async {
        let connection = database.connection()?;
        let db = Db::default();
        db.initialize(connection.clone())?;
        seed(&connection, "authorized-clock", "e30=", NOW - 30).await?;
        let mut reader = handler(db.clone())?;
        let (bytes, deadline) = reader
            .read_with_deadline("authorized-clock")
            .await?
            .ok_or(Error::Unauthorized)?;
        assert_eq!(bytes, b"{}");
        assert_eq!(deadline.unix_timestamp(), NOW + 30);
        assert_eq!(
            required(row(&connection, "authorized-clock").await?)?
                .try_get::<i64>("", "last_activity")
                .map_err(Error::internal)?,
            NOW - 30
        );
        let mut expired =
            DatabaseSessionHandler::new(db.clone(), Some("sessions".into()), 1)?.with_clock(
                Arc::new(FixedClock::new(CalendarInstant::new(NOW + 30, 0)?)),
            );
        assert!(
            expired
                .read_with_deadline("authorized-clock")
                .await?
                .is_none()
        );
        assert!(row(&connection, "authorized-clock").await?.is_some());
        connection
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "DELETE FROM sessions WHERE id=?",
                ["authorized-clock".into()],
            ))
            .await
            .map_err(Error::internal)?;
        assert!(
            reader
                .read_with_deadline("authorized-clock")
                .await?
                .is_none()
        );
        Ok(())
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
