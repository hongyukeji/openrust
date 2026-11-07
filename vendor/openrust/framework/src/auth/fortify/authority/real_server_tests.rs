//! 真实 MySQL/PostgreSQL 用户行读取权限、同码竞争与恢复读权限回归。
#![cfg(any(feature = "db-mysql", feature = "db-postgres"))]

use crate::Error;
use crate::Result;
use crate::auth::AuthMigration;
use crate::auth::fortify::DatabaseFactorAuthority;
use crate::auth::fortify::DatabaseTwoFactorState;
use crate::auth::fortify::FactorAuthorityMigration;
use crate::auth::fortify::FactorAuthorityMutation;
use crate::auth::fortify::FactorAuthorityStatus;
use crate::auth::fortify::FortifyTwoFactorMigration;
use crate::auth::fortify::TwoFactorCodeVerification;
use crate::auth::fortify::TwoFactorState;
use crate::database::Db;
use crate::database::sea_orm::ConnectOptions;
use crate::database::sea_orm::ConnectionTrait;
use crate::database::sea_orm::Database;
use crate::database::sea_orm::DatabaseConnection;
use crate::database::sea_orm::DbBackend;
use crate::database::sea_orm::Statement;
use crate::database::sea_orm_migration::MigrationTrait;
use crate::database::sea_orm_migration::MigratorTrait;
use crate::testing::TestDatabase;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::json;
use std::time::Duration;

struct Migrator;
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(AuthMigration),
            Box::new(FortifyTwoFactorMigration::new()),
            Box::new(FactorAuthorityMigration::new()),
        ]
    }
}
fn check(condition: bool, code: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::custom_with_status(
            crate::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            code,
            "数据库ACL回归断言失败（无凭据值）",
        ))
    }
}
fn random_hex(bytes: usize) -> Result<String> {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut random = vec![0_u8; bytes];
    getrandom::fill(&mut random).map_err(Error::internal)?;
    let mut result = String::with_capacity(bytes * 2);
    for byte in random {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(result)
}
fn identifier(value: &str, backend: DbBackend) -> Result<String> {
    check(
        !value.is_empty()
            && value.len() <= 63
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "acl_identifier_invalid",
    )?;
    Ok(match backend {
        DbBackend::Postgres => format!("\"{value}\""),
        DbBackend::MySql => format!("`{value}`"),
        _ => {
            return Err(Error::custom(
                "acl_backend_invalid",
                "ACL回归仅支持MySQL/PostgreSQL",
            ));
        }
    })
}
async fn execute(connection: &DatabaseConnection, sql: &str) -> Result<()> {
    connection
        .execute_unprepared(sql)
        .await
        .map_err(Error::internal)?;
    Ok(())
}
async fn connect(url: &str, backend: DbBackend) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url.to_owned());
    // CREATE ROLE/USER包含本次随机密码；此连接不记录SQL或参数。
    options
        .max_connections(2)
        .min_connections(0)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .sqlx_logging(false);
    #[cfg(feature = "db-mysql")]
    if backend == DbBackend::MySql {
        options
            .map_sqlx_mysql_opts(|options| options.ssl_mode(sqlx::mysql::MySqlSslMode::Required));
        options.map_sqlx_mysql_pool_opts(|pool| {
            pool.after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET time_zone = '+00:00'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
        });
    }
    #[cfg(not(feature = "db-mysql"))]
    let _ = backend;
    Database::connect(options).await.map_err(Error::internal)
}
fn encrypter() -> Result<std::sync::Arc<dyn crate::encryption::ApplicationEncrypter>> {
    crate::auth::fortify::test_field_encrypter(&STANDARD.encode([7_u8; 32]))
}
fn authority(connection: &DatabaseConnection) -> Result<DatabaseFactorAuthority> {
    let db = Db::default();
    db.initialize(connection.clone())?;
    Ok(DatabaseFactorAuthority::new(DatabaseTwoFactorState::new(
        db,
        "users",
        "id",
        json!(42),
    )?))
}
async fn role_exists(admin: &DatabaseConnection, backend: DbBackend, role: &str) -> Result<bool> {
    let sql = match backend {
        DbBackend::Postgres => "SELECT COUNT(*) AS count FROM pg_catalog.pg_roles WHERE rolname=$1",
        DbBackend::MySql => "SELECT COUNT(*) AS count FROM mysql.user WHERE User=? AND Host='%'",
        _ => {
            return Err(Error::custom(
                "acl_backend_invalid",
                "ACL回归仅支持MySQL/PostgreSQL",
            ));
        }
    };
    let row = admin
        .query_one_raw(Statement::from_sql_and_values(backend, sql, [role.into()]))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    Ok(row.try_get::<i64>("", "count").map_err(Error::internal)? != 0)
}
async fn assert_role_identity(
    connection: &DatabaseConnection,
    backend: DbBackend,
    role: &str,
) -> Result<()> {
    let sql = if backend == DbBackend::Postgres {
        "SELECT CURRENT_USER AS identity"
    } else {
        "SELECT CURRENT_USER() AS identity"
    };
    let row = connection
        .query_one_raw(Statement::from_string(backend, sql))
        .await
        .map_err(Error::internal)?
        .ok_or(Error::Unauthorized)?;
    let actual: String = row.try_get("", "identity").map_err(Error::internal)?;
    let expected = if backend == DbBackend::MySql {
        format!("{role}@%")
    } else {
        role.to_owned()
    };
    check(actual == expected, "acl_role_connection_identity")
}

#[tokio::test]
#[ignore = "需要真实MySQL/PostgreSQL及OPENRUST_TEST_DATABASE_URL，管理员必须能创建/删除自有测试数据库角色；不允许权限不足时skip"]
#[allow(
    clippy::too_many_lines,
    reason = "真实ACL对照及finally清理必须保留每阶段角色与池的所有权"
)]
async fn account_acl_denial_cannot_hide_a_confirmed_factor_or_allow_code_replay() -> Result<()> {
    let admin_url = crate::test_database_environment::database_url().map_err(Error::internal)?;
    let role = format!("or_acl_{}", random_hex(8)?);
    let password = random_hex(32)?;
    let database = TestDatabase::create::<Migrator>(&admin_url).await?;
    let mut admin_pool = None;
    let mut role_pool = None;
    let mut created = false;
    let mut stage = "parse_database";
    let outcome:Result<()>=async {
        let parsed=url::Url::parse(database.url()).map_err(Error::internal)?;
        let backend=match parsed.scheme(){"postgres"|"postgresql"=>DbBackend::Postgres,"mysql"=>DbBackend::MySql,_=>return Err(Error::custom("acl_backend_invalid","ACL回归仅支持MySQL/PostgreSQL"))};
        let database_name=parsed.path().trim_start_matches('/');
        let database_id=identifier(database_name,backend)?;let role_id=identifier(&role,backend)?;
        stage = "admin_connect";
        let admin=connect(database.url(),backend).await?;admin_pool=Some(admin.clone());
        stage = "role_inventory";
        check(!role_exists(&admin,backend,&role).await?,"acl_role_name_not_owned")?;
        stage = "create_role";
        match backend {
            DbBackend::Postgres=>execute(&admin,&format!("CREATE ROLE {role_id} LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOREPLICATION NOBYPASSRLS PASSWORD '{password}'")).await?,
            DbBackend::MySql=>execute(&admin,&format!("CREATE USER '{role}'@'%' IDENTIFIED BY '{password}'")).await?,
            _=>return Err(Error::custom("acl_backend_invalid","ACL回归仅支持MySQL/PostgreSQL")),
        }
        stage = "grant_users";
        created=true;
        match backend {
            DbBackend::Postgres=>{
                execute(&admin,"REVOKE CREATE ON SCHEMA public FROM PUBLIC").await?;
                execute(&admin,&format!("REVOKE TEMPORARY ON DATABASE {database_id} FROM PUBLIC")).await?;
                execute(&admin,&format!("GRANT CONNECT ON DATABASE {database_id} TO {role_id}")).await?;
                execute(&admin,&format!("GRANT USAGE ON SCHEMA public TO {role_id}")).await?;
                execute(&admin,&format!("GRANT SELECT,UPDATE ON TABLE public.users TO {role_id}")).await?;
            }
            DbBackend::MySql=>execute(&admin,&format!("GRANT SELECT,UPDATE ON {database_id}.`users` TO '{role}'@'%'")).await?,
            _=>return Err(Error::custom("acl_backend_invalid","ACL回归仅支持MySQL/PostgreSQL")),
        }
        stage = "insert_user";
        execute(&admin,"INSERT INTO users(id,email,password_hash,created_at,updated_at) VALUES(42,'acl-fixture@example.test','synthetic-not-a-login-hash',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)").await?;
        let mut role_url=parsed.clone();role_url.set_username(&role).map_err(|()|Error::custom("acl_role_url_invalid","临时角色连接参数无法设置"))?;
        role_url.set_password(Some(&password)).map_err(|()|Error::custom("acl_role_url_invalid","临时角色连接参数无法设置"))?;
        stage = "restricted_connect";
        let restricted=connect(role_url.as_str(),backend).await?;role_pool=Some(restricted.clone());
        stage = "restricted_identity";
        assert_role_identity(&restricted,backend,&role).await?;
        let restricted_authority=authority(&restricted)?;
        stage = "authority_baseline";
        let admin_authority=authority(&admin)?;
        let empty = restricted_authority.load().await?;
        check(empty.status == FactorAuthorityStatus::Disabled, "acl_new_account_disabled")?;
        let crypto = encrypter()?;
        let pending = restricted_authority.replace(&empty, &TwoFactorState {
            secret: Some(crypto.encrypt_serialized_string(b"JBSWY3DPEHPK3PXP")?),
            recovery_codes: Some(crypto.encrypt_serialized_string(br#"["fixture-code"]"#)?),
            confirmed_at: None,
        }, FactorAuthorityMutation::Enable).await?;
        let mut setup = pending.state.clone();
        setup.confirmed_at = Some(chrono::Utc::now());
        let confirmed = restricted_authority.confirm_verified(&pending, &setup, "123456").await?;
        check(admin_authority.load().await? == confirmed, "acl_admin_confirmed_control")?;
        stage = "revoke_users_read";
        let (revoke, grant) = if backend == DbBackend::Postgres {
            (format!("REVOKE SELECT ON TABLE public.users FROM {role_id}"), format!("GRANT SELECT ON TABLE public.users TO {role_id}"))
        } else {
            (format!("REVOKE SELECT ON {database_id}.`users` FROM '{role}'@'%'"), format!("GRANT SELECT ON {database_id}.`users` TO '{role}'@'%'"))
        };
        execute(&admin, &revoke).await?;
        check(restricted_authority.load().await.is_err(), "acl_denied_account_failed_closed")?;
        check(admin_authority.load().await? == confirmed, "acl_admin_retained_after_revoke")?;
        execute(&admin, &grant).await?;
        check(restricted_authority.load().await? == confirmed, "acl_regrant_same_account")?;
        stage = "independent_connections_consume";
        let fact = TwoFactorCodeVerification::verified("654321", 1, 1, 1)?;
        let admin_authority = admin_authority.with_provider_verification(&fact)?;
        let restricted_authority = restricted_authority.with_provider_verification(&fact)?;
        let (first, second) = tokio::join!(
            admin_authority.consume_modern(&confirmed, None, Some("654321")),
            restricted_authority.consume_modern(&confirmed, None, Some("654321"))
        );
        let ((Ok(winner), Err(loser)) | (Err(loser), Ok(winner))) = (first, second) else {
            return Err(Error::Conflict("同验证码必须仅成功一次".into()));
        };
        check(loser.status() == http::StatusCode::UNAUTHORIZED, "acl_nonce_replay_denied")?;
        check(winner == confirmed, "acl_nonce_preserved_generation")?;
        Ok(())
    }.await;
    if outcome.is_err() {
        eprintln!("ACL回归失败阶段：{stage}");
    }
    // finally：无论测试哪阶段失败，先释放角色池，再撤自有权限/角色，最后关闭自有库。
    let mut cleanup_error = None;
    if let Some(pool) = role_pool.take()
        && let Err(error) = pool.close().await
    {
        cleanup_error = Some(Error::internal(error));
    }
    if created && let Some(admin) = &admin_pool {
        let backend = admin.get_database_backend();
        match identifier(&role, backend) {
            Ok(role_id) => {
                if backend == DbBackend::Postgres
                    && let Err(error) = execute(admin, &format!("DROP OWNED BY {role_id}")).await
                {
                    cleanup_error.get_or_insert(error);
                }
                let drop_role = if backend == DbBackend::Postgres {
                    format!("DROP ROLE {role_id}")
                } else {
                    format!("DROP USER '{role}'@'%'")
                };
                if let Err(error) = execute(admin, &drop_role).await {
                    cleanup_error.get_or_insert(error);
                }
                let removed = async {
                    check(
                        !role_exists(admin, backend, &role).await?,
                        "acl_owned_role_removed",
                    )
                }
                .await;
                if let Err(error) = removed {
                    cleanup_error.get_or_insert(error);
                }
            }
            Err(error) => {
                cleanup_error.get_or_insert(error);
            }
        }
    }
    if let Some(pool) = admin_pool.take()
        && let Err(error) = pool.close().await
    {
        cleanup_error.get_or_insert(Error::internal(error));
    }
    let closed = database.close().await;
    if let Some(error) = cleanup_error {
        return Err(error);
    }
    closed?;
    outcome
}
