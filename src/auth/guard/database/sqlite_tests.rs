//! 防止 Provider 将账户禁用策略、整数主键或密码查询偷偷加入官方默认。
use super::*;
use crate::{
    config::PasswordConfig,
    db::sea_orm::{ConnectionTrait, DbBackend, Statement},
    db::sea_orm_migration::{MigrationTrait, MigratorTrait},
    testing::TestDatabase,
};
use serde_json::{Map, json};

struct EmptyMigrator;
impl MigratorTrait for EmptyMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        Vec::new()
    }
}

fn credentials(values: &[(&str, Value)]) -> Credentials {
    Credentials::new(Map::from_iter(
        values
            .iter()
            .map(|(key, value)| ((*key).into(), value.clone())),
    ))
}

#[tokio::test]
async fn database_provider_queries_tokens_and_rehash_use_the_official_attribute_contract()
-> Result<()> {
    let guard = TestDatabase::sqlite_memory::<EmptyMigrator>().await?;
    let db = Db::default();
    db.initialize(guard.connection()?)?;
    let result: Result<()> = async {
        let passwords = Passwords::new(PasswordConfig { memory_kib: 19_456, iterations: 1, parallelism: 1 })?;
        let hash = passwords.hash("correct secret").await?;
        let table = db.quoted_table_name("guard_accounts")?;
        db.connection()?.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY, username TEXT, password TEXT, remember_token TEXT, disabled_at TEXT)")).await.map_err(Error::internal)?;
        db.table("guard_accounts")?.insert(vec![
            ("id", "tenant:42".into()), ("username", "person".into()),
            ("password", hash.as_str().into()), ("remember_token", "remember-value".into()),
            ("disabled_at", "2026-01-01".into()),
        ]).await?;
        let provider = DatabaseUserProvider::new(db.clone(), passwords, "guard_accounts")?;
        let user = provider.retrieve_by_id(&json!("tenant:42")).await?.ok_or(Error::Unauthorized)?;
        assert_eq!(user.auth_identifier()?, json!("tenant:42"));
        assert!(provider.retrieve_by_token(&json!("tenant:42"), "remember-value").await?.is_some());
        assert!(provider.retrieve_by_token(&json!("tenant:42"), "wrong").await?.is_none());
        assert!(provider.retrieve_by_credentials(&credentials(&[("password", json!("ignored")), ("password_confirmation", json!("ignored"))])).await?.is_none());
        assert!(provider.retrieve_by_credentials(&credentials(&[("username", json!(["absent", "person"])), ("password", json!("not queried"))])).await?.is_some());
        assert!(provider.retrieve_by_credentials(&credentials(&[("username", json!([]))])).await?.is_none());
        assert!(provider.retrieve_by_credentials(&credentials(&[("username", json!("person' OR 1=1 --"))])).await?.is_none());
        assert!(provider.retrieve_by_credentials(&credentials(&[("username OR 1=1", json!("person"))])).await.is_err());
        assert!(provider.validate_credentials(user.as_ref(), &credentials(&[("password", json!("correct secret"))])).await?);
        assert!(!provider.validate_credentials(user.as_ref(), &credentials(&[("password", json!("incorrect"))])).await?);
        assert!(!provider.validate_credentials(user.as_ref(), &Credentials::default()).await?);
        provider.update_remember_token(user.as_ref(), "replacement").await?;
        assert_eq!(user.remember_token()?.as_deref(), Some("remember-value"));
        assert!(provider.retrieve_by_token(&json!("tenant:42"), "replacement").await?.is_some());
        provider.update_remember_token(user.as_ref(), "").await?;
        assert!(provider.retrieve_by_token(&json!("tenant:42"), "").await?.is_none());
        provider.rehash_password_if_required(user.as_ref(), &credentials(&[("password", json!("correct secret"))]), true).await?;
        let row = db.connection()?.query_one_raw(Statement::from_string(DbBackend::Sqlite, format!("SELECT password FROM {table}"))).await.map_err(Error::internal)?.ok_or(Error::Unauthorized)?;
        let replacement: String = row.try_get("", "password").map_err(Error::internal)?;
        assert_ne!(replacement, hash.as_str());
        assert_eq!(user.auth_password()?.as_deref(), Some(hash.as_str()));
        assert_eq!(format!("{provider:?}"), "DatabaseUserProvider([REDACTED])");
        Ok(())
    }.await;
    drop(db);
    let cleanup = guard.close().await;
    result?;
    cleanup
}

struct AlternateModel {
    attributes: GenericUser,
    token_column: &'static str,
}
impl AlternateModel {
    fn secret(&self, column: &str) -> Result<Option<String>> {
        match self.attributes.attribute(column)? {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => Ok(Some(value)),
            _ => Err(Error::custom(
                "test_model_secret_invalid",
                "模型秘密字段类型无效",
            )),
        }
    }
}
impl Authenticatable for AlternateModel {
    fn auth_identifier_name(&self) -> &'static str {
        "id"
    }
    fn auth_identifier(&self) -> Result<Value> {
        self.attributes.auth_identifier()
    }
    fn auth_password_name(&self) -> &'static str {
        "alternate_hash"
    }
    fn auth_password(&self) -> Result<Option<String>> {
        self.secret(self.auth_password_name())
    }
    fn remember_token_name(&self) -> &str {
        self.token_column
    }
    fn remember_token(&self) -> Result<Option<String>> {
        self.secret(self.remember_token_name())
    }
    fn set_remember_token(&self, token: Option<String>) -> Result<()> {
        self.attributes.set_attribute(
            self.remember_token_name(),
            token.map_or(Value::Null, Value::String),
        )
    }
}

#[tokio::test]
async fn selected_user_factory_controls_real_retrieval_and_model_secret_columns() -> Result<()> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let guard = TestDatabase::sqlite_memory::<EmptyMigrator>().await?;
    let db = Db::default();
    db.initialize(guard.connection()?)?;
    let result: Result<()> = async {
        let passwords = Passwords::new(PasswordConfig { memory_kib: 19_456, iterations: 1, parallelism: 1 })?;
        let hash = passwords.hash("model-password").await?;
        let table = db.quoted_table_name("model_accounts")?;
        db.connection()?.execute_unprepared(&format!("CREATE TABLE {table} (id TEXT PRIMARY KEY, username TEXT, password TEXT, alternate_hash TEXT, remember_token TEXT, alternate_token TEXT)")).await.map_err(Error::internal)?;
        db.table("model_accounts")?.insert(vec![
            ("id", "model:uuid".into()), ("username", "model-user".into()),
            ("password", "must-remain-unchanged".into()), ("alternate_hash", hash.as_str().into()),
            ("remember_token", "default-token-unchanged".into()), ("alternate_token", "model-token".into()),
        ]).await?;
        let factory_calls = Arc::new(AtomicUsize::new(0));
        let provider = DatabaseUserProvider::new(db.clone(), passwords.clone(), "model_accounts")?.with_user_factory({
            let calls = Arc::clone(&factory_calls);
            move |attributes| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Arc::new(AlternateModel { attributes: GenericUser::new(attributes), token_column: "alternate_token" }) as AuthUser)
            }
        });
        let user = provider.retrieve_by_id(&json!("model:uuid")).await?.ok_or(Error::Unauthorized)?;
        assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
        assert_eq!(user.auth_password_name(), "alternate_hash");
        assert_eq!(user.auth_password()?.as_deref(), Some(hash.as_str()));
        assert_eq!(user.remember_token_name(), "alternate_token");
        assert_eq!(user.remember_token()?.as_deref(), Some("model-token"));
        let found = provider.retrieve_by_credentials(&credentials(&[("username", json!("model-user"))])).await?.ok_or(Error::Unauthorized)?;
        assert_eq!(factory_calls.load(Ordering::SeqCst), 2);
        assert_eq!(found.auth_password_name(), "alternate_hash");
        let input = credentials(&[("password", json!("model-password"))]);
        assert!(provider.validate_credentials(user.as_ref(), &input).await?);
        provider.rehash_password_if_required(user.as_ref(), &input, true).await?;
        provider.update_remember_token(user.as_ref(), "new-model-token").await?;
        assert!(provider.retrieve_by_token(&json!("model:uuid"), "new-model-token").await?.is_some());
        let row = db.table("model_accounts")?.first().await?.ok_or(Error::Unauthorized)?;
        let persisted_hash: String = row.try_get("", "alternate_hash").map_err(Error::internal)?;
        assert_ne!(persisted_hash, hash.as_str());
        assert!(passwords.verify("model-password", &EncodedPasswordHash::parse(persisted_hash)?).await?);
        assert_eq!(row.try_get::<String>("", "password").map_err(Error::internal)?, "must-remain-unchanged");
        assert_eq!(row.try_get::<String>("", "remember_token").map_err(Error::internal)?, "default-token-unchanged");
        assert_eq!(row.try_get::<String>("", "alternate_token").map_err(Error::internal)?, "new-model-token");
        let invalid_field_user = AlternateModel {
            attributes: GenericUser::new(Map::from_iter([("id".into(), json!("model:uuid"))])),
            token_column: "alternate_token; DROP TABLE accounts",
        };
        assert!(provider.update_remember_token(&invalid_field_user, "must-not-write").await.is_err());
        let rejected = provider.with_user_factory(|_| Err(Error::custom("model_factory_rejected", "模型拒绝属性")));
        match rejected.retrieve_by_id(&json!("model:uuid")).await {
            Ok(_) => return Err(Error::custom("test_factory_error_missing", "模型工厂错误未传播")),
            Err(error) => assert_eq!(error.code(), "model_factory_rejected"),
        }
        Ok(())
    }.await;
    drop(db);
    let cleanup = guard.close().await;
    result?;
    cleanup
}
