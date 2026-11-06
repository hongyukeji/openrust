//! 现代因子安全门的真实后端当前读、权限错误和父事务存活回归。
use super::{
    require_unbound_fortify_account,
    tests::{NoMigrations, add_column, create_user, owned, schema_contract},
};
use crate::{
    Error, Result,
    database::{
        Db,
        sea_orm::{
            ConnectionTrait, Database, DbBackend, IsolationLevel, Statement, TransactionTrait,
        },
    },
    testing::TestDatabase,
};

#[tokio::test]
#[ignore = "需要真实MySQL/PostgreSQL及OPENRUST_TEST_DATABASE_URL，缺少环境必须失败"]
async fn modern_schema_absence_partial_assets_and_parent_transaction_use_real_backend() -> Result<()>
{
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    owned(TestDatabase::create::<NoMigrations>(&url).await?, |db| {
        Box::pin(schema_contract(db))
    })
    .await
}

async fn modern_user(db: &Db) -> Result<()> {
    create_user(db).await?;
    for column in [
        "two_factor_secret",
        "two_factor_recovery_codes",
        "two_factor_confirmed_at",
    ] {
        add_column(db, column).await?;
    }
    Ok(())
}
#[tokio::test]
#[ignore = "需要真实MySQL/PostgreSQL，校验已有普通快照不能绕过当前锁读"]
async fn current_locked_factor_read_cannot_reuse_an_earlier_clear_snapshot() -> Result<()> {
    let url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    owned(TestDatabase::create::<NoMigrations>(&url).await?, |db| {
        Box::pin(async move {
            modern_user(&db).await?;
            let connection = db.connection()?;
            let backend = db.backend()?;
            let isolation = if backend == DbBackend::MySql {
                IsolationLevel::RepeatableRead
            } else {
                IsolationLevel::ReadCommitted
            };
            let transaction = connection
                .begin_with_config(Some(isolation), None)
                .await
                .map_err(Error::internal)?;
            let snapshot = transaction
                .query_one_raw(Statement::from_string(
                    backend,
                    "SELECT two_factor_secret FROM users WHERE id=42",
                ))
                .await
                .map_err(Error::internal)?
                .ok_or(Error::Unauthorized)?;
            assert!(
                snapshot
                    .try_get::<Option<String>>("", "two_factor_secret")
                    .map_err(Error::internal)?
                    .is_none()
            );
            db.table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![(
                    "two_factor_secret",
                    "protected-current-value".into(),
                )])
                .await?;
            let outcome: Result<()> = async {
                assert!(
                    require_unbound_fortify_account(&db, &transaction, 42)
                        .await
                        .err()
                        .is_some_and(|error| error.code() == "auth_fortify_factor_proof_required")
                );
                transaction
                    .execute_unprepared("SELECT 1")
                    .await
                    .map_err(Error::internal)?;
                Ok(())
            }
            .await;
            let ended = transaction.rollback().await.map_err(Error::internal);
            outcome?;
            ended?;
            let row = db
                .table("users")?
                .first()
                .await?
                .ok_or(Error::Unauthorized)?;
            assert_eq!(
                row.try_get::<String>("", "two_factor_secret")
                    .map_err(Error::internal)?,
                "protected-current-value"
            );
            Ok(())
        })
    })
    .await
}

fn acl_error() -> Error {
    Error::custom(
        "factor_gate_acl_fixture_failed",
        "因子权限隔离夹具失败（连接参数与临时密码不输出）",
    )
}
async fn provision_acl(db: &Db, name: &str, password: &str, database: &str) -> Result<()> {
    let connection = db.connection()?;
    let statements = match db.backend()? {
        DbBackend::MySql => vec![
            format!("CREATE USER '{name}'@'%' IDENTIFIED BY '{password}'"),
            format!(
                "GRANT SELECT(id,password_hash), UPDATE(password_hash) ON `{database}`.`users` TO '{name}'@'%'"
            ),
        ],
        DbBackend::Postgres => vec![
            format!("CREATE ROLE {name} LOGIN PASSWORD '{password}'"),
            format!("GRANT CONNECT ON DATABASE \"{database}\" TO {name}"),
            format!("GRANT USAGE ON SCHEMA public TO {name}"),
            format!("GRANT SELECT(id,password_hash), UPDATE(password_hash) ON users TO {name}"),
        ],
        _ => return Err(acl_error()),
    };
    for statement in statements {
        connection
            .execute_unprepared(&statement)
            .await
            .map_err(|_| acl_error())?;
    }
    Ok(())
}
async fn remove_acl(db: &Db, name: &str, database: &str) -> Result<()> {
    let connection = db.connection()?;
    let statements = match db.backend()? {
        DbBackend::MySql => vec![format!("DROP USER IF EXISTS '{name}'@'%'")],
        DbBackend::Postgres => vec![
            format!("REVOKE CONNECT ON DATABASE \"{database}\" FROM {name}"),
            format!("DROP OWNED BY {name}"),
            format!("DROP ROLE IF EXISTS {name}"),
        ],
        _ => return Err(acl_error()),
    };
    for statement in statements {
        connection
            .execute_unprepared(&statement)
            .await
            .map_err(|_| acl_error())?;
    }
    Ok(())
}
async fn restricted_probe(url: &str) -> Result<()> {
    let connection = Database::connect(url).await.map_err(|_| acl_error())?;
    let db = Db::default();
    let outcome: Result<()> = async {
        db.initialize(connection.clone())?;
        let transaction = connection.begin().await.map_err(Error::internal)?;
        let result = require_unbound_fortify_account(&db, &transaction, 42).await;
        // 权限错误绝不能映射成“未安装”、partial schema或可签发身份。
        let rejected = matches!(result, Err(Error::Internal(_)));
        let alive = transaction
            .execute_unprepared("SELECT 1")
            .await
            .map_err(Error::internal);
        let ended = transaction.rollback().await.map_err(Error::internal);
        if !rejected {
            return Err(acl_error());
        }
        alive?;
        ended?;
        Ok(())
    }
    .await;
    drop(db);
    let closed = connection.close().await.map_err(Error::internal);
    outcome?;
    closed
}
#[tokio::test]
#[ignore = "需要真实MySQL/PostgreSQL及创建/回收临时受限用户权限；不跳过ACL错误"]
async fn real_column_acl_denial_is_not_missing_schema_and_preserves_parent_transaction()
-> Result<()> {
    let admin = crate::test_database_environment::database_url().map_err(Error::internal)?;
    let database = TestDatabase::create::<NoMigrations>(&admin).await?;
    let raw = database.url().to_owned();
    owned(database, move |db| {
        Box::pin(async move {
            modern_user(&db).await?;
            db.table("users")?
                .where_eq("id", 42_i64)?
                .update(vec![("two_factor_secret", "protected-acl-value".into())])
                .await?;
            let mut nonce = [0_u8; 16];
            getrandom::fill(&mut nonce).map_err(Error::internal)?;
            let name = format!(
                "or_gate_{:x}",
                u64::from_le_bytes(nonce[..8].try_into().map_err(Error::internal)?)
            );
            let password = format!(
                "temporary_{:x}",
                u64::from_le_bytes(nonce[8..].try_into().map_err(Error::internal)?)
            );
            let mut url = url::Url::parse(&raw).map_err(|_| acl_error())?;
            let database = url.path().trim_start_matches('/').to_owned();
            if !database
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(acl_error());
            }
            let prepared = provision_acl(&db, &name, &password, &database).await;
            let outcome: Result<()> = async {
                prepared?;
                url.set_username(&name).map_err(|()| acl_error())?;
                url.set_password(Some(&password))
                    .map_err(|()| acl_error())?;
                restricted_probe(url.as_str()).await
            }
            .await;
            let removed = remove_acl(&db, &name, &database).await;
            outcome?;
            removed
        })
    })
    .await
}
