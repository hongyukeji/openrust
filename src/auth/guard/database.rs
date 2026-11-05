//! Laravel `DatabaseUserProvider` 对应：使用宿主 Db、逻辑表与原始类型标识。

use super::contracts::{AuthUser, Authenticatable, Credentials, GenericUser, UserProvider};
use crate::{
    Error, Result,
    auth::{EncodedPasswordHash, Passwords},
    core::BoxFuture,
    db::{
        Db,
        query::{QueryBuilder, RawSql},
        sea_orm::{DbBackend, FromQueryResult, QueryResult, Value as SqlValue},
    },
};
use serde_json::{Map, Value};
use std::{fmt, sync::Arc};
use subtle::ConstantTimeEq;

type UserFactory = Arc<dyn Fn(Map<String, Value>) -> Result<AuthUser> + Send + Sync>;

/// 通过同一 Db 查询机制检索通用属性账户，不附加业务禁用或租户条件。
#[derive(Clone)]
pub struct DatabaseUserProvider {
    db: Db,
    table: String,
    passwords: Passwords,
    user_factory: UserFactory,
}

impl fmt::Debug for DatabaseUserProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseUserProvider([REDACTED])")
    }
}

impl DatabaseUserProvider {
    /// 使用已选择连接的 Db 与逻辑表名；表前缀、标识校验和绑定复用 Db。
    pub fn new(db: Db, passwords: Passwords, table: impl Into<String>) -> Result<Self> {
        let table = table.into();
        db.table_name(&table)?;
        Ok(Self {
            db,
            table,
            passwords,
            user_factory: Arc::new(|attributes| {
                Ok(Arc::new(GenericUser::new(attributes)) as AuthUser)
            }),
        })
    }

    /// 通过宿主已编译的账户模型解释数据库属性；默认仍创建 Laravel `GenericUser`。
    ///
    /// 工厂在完整行转换和漏列检查后调用，其错误原样传播。此扩展不提供
    /// Eloquent 模型生命周期，也不会改变数据库 Provider 按 `id` 检索的默认规则。
    #[must_use]
    pub fn with_user_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn(Map<String, Value>) -> Result<AuthUser> + Send + Sync + 'static,
    {
        self.user_factory = Arc::new(factory);
        self
    }

    fn user_from_row(&self, row: &QueryResult) -> Result<AuthUser> {
        (self.user_factory)(row_attributes(row)?)
    }

    fn query(&self) -> Result<QueryBuilder<'static>> {
        self.db.table(&self.table)
    }

    fn query_for_user(&self, user: &dyn Authenticatable) -> Result<QueryBuilder<'static>> {
        self.query()?.where_eq(
            user.auth_identifier_name(),
            scalar(&user.auth_identifier()?)?,
        )
    }
}

impl UserProvider for DatabaseUserProvider {
    fn uses_database_accounts(&self, db: &Db, table: &str) -> Result<bool> {
        Ok(self.db.same_handle(db) && self.db.table_name(&self.table)? == db.table_name(table)?)
    }
    fn retrieve_by_id<'a>(
        &'a self,
        identifier: &'a Value,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let row = self
                .query()?
                .where_eq("id", scalar(identifier)?)?
                .first()
                .await?;
            row.map(|row| self.user_from_row(&row)).transpose()
        })
    }

    fn retrieve_by_token<'a>(
        &'a self,
        identifier: &'a Value,
        token: &'a str,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let Some(user) = self.retrieve_by_id(identifier).await? else {
                return Ok(None);
            };
            let matches = user.remember_token()?.is_some_and(|stored| {
                !stored.is_empty()
                    && stored.len() == token.len()
                    && bool::from(stored.as_bytes().ct_eq(token.as_bytes()))
            });
            Ok(matches.then_some(user))
        })
    }

    fn update_remember_token<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        token: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.query_for_user(user)?
                .update(vec![(user.remember_token_name(), token.into())])
                .await?;
            Ok(())
        })
    }

    fn retrieve_by_credentials<'a>(
        &'a self,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<Option<AuthUser>>> {
        Box::pin(async move {
            let mut query = self.query()?;
            let mut has_predicates = false;
            for (key, value) in credentials
                .attributes()
                .iter()
                .filter(|(key, _)| !key.contains("password"))
            {
                has_predicates = true;
                query = match value {
                    Value::Array(values) => query
                        .where_in(key, values.iter().map(scalar).collect::<Result<Vec<_>>>()?)?,
                    Value::Null => query.where_null(key)?,
                    _ => query.where_eq(key, scalar(value)?)?,
                };
            }
            if !has_predicates {
                return Ok(None);
            }
            query
                .first()
                .await?
                .map(|row| self.user_from_row(&row))
                .transpose()
        })
    }

    fn validate_credentials<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let Some(plain) = plaintext(credentials)? else {
                return Ok(false);
            };
            let Some(encoded) = user.auth_password()? else {
                return Ok(false);
            };
            self.passwords
                .verify(plain, &EncodedPasswordHash::parse(encoded)?)
                .await
        })
    }

    fn rehash_password_if_required<'a>(
        &'a self,
        user: &'a dyn Authenticatable,
        credentials: &'a Credentials,
        force: bool,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let encoded = user.auth_password()?.ok_or_else(invalid_credentials)?;
            let encoded = EncodedPasswordHash::parse(encoded)?;
            if !self.passwords.needs_rehash(&encoded)? && !force {
                return Ok(());
            }
            let plain = plaintext(credentials)?.ok_or_else(invalid_credentials)?;
            let field = user.auth_password_name().to_owned();
            // where_eq 先校验模型字段名。附加字节比较，避免文本排序规则忽略 PHC 大小写。
            let query = self
                .query_for_user(user)?
                .where_eq(&field, encoded.as_str())?;
            let exact = match self.db.backend()? {
                DbBackend::MySql => format!("CAST(`{field}` AS BINARY) = CAST(? AS BINARY)"),
                DbBackend::Postgres => {
                    format!("convert_to(\"{field}\", 'UTF8') = convert_to($1, 'UTF8')")
                }
                DbBackend::Sqlite => format!("CAST(\"{field}\" AS BLOB) = CAST(? AS BLOB)"),
                _ => {
                    return Err(Error::custom(
                        "unsupported_database_backend",
                        "密码重哈希不支持此数据库后端",
                    ));
                }
            };
            let query = query.where_raw(RawSql::new(exact), vec![encoded.as_str().into()]);
            let replacement = self.passwords.hash(plain).await?;
            self.db
                .transaction(move |transaction| {
                    Box::pin(async move {
                        let affected = query
                            .using_transaction(transaction)
                            .update(vec![(&field, replacement.as_str().into())])
                            .await?;
                        if affected == 1 {
                            Ok(())
                        } else {
                            Err(Error::Unauthorized)
                        }
                    })
                })
                .await
        })
    }
}

fn row_attributes(row: &QueryResult) -> Result<Map<String, Value>> {
    let Value::Object(attributes) = Value::from_query_result(row, "").map_err(Error::internal)?
    else {
        return Err(Error::custom(
            "auth_database_row_invalid",
            "认证账户必须是属性对象",
        ));
    };
    // SeaORM 对不支持的列可能省略属性；不能悄悄丢失账户数据。
    if row
        .column_names()
        .iter()
        .any(|column| !attributes.contains_key(column))
    {
        return Err(Error::custom(
            "auth_database_attribute_unsupported",
            "认证账户包含无法转换的数据库字段",
        ));
    }
    Ok(attributes)
}

fn plaintext(credentials: &Credentials) -> Result<Option<&str>> {
    match credentials.get("password") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(invalid_credentials()),
    }
}

fn scalar(value: &Value) -> Result<SqlValue> {
    match value {
        Value::Null => Ok(SqlValue::String(None)),
        Value::String(value) => Ok(value.clone().into()),
        Value::Bool(value) => Ok((*value).into()),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(value.into())
            } else if let Some(value) = value.as_u64() {
                Ok(value.into())
            } else if let Some(value) = value.as_f64() {
                Ok(value.into())
            } else {
                Err(invalid_credentials())
            }
        }
        Value::Array(_) | Value::Object(_) => Err(invalid_credentials()),
    }
}

fn invalid_credentials() -> Error {
    Error::custom("auth_credentials_invalid", "认证凭据字段类型无效或缺失")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn bound_scalars_preserve_strings_and_unsigned_identifiers() -> Result<()> {
        assert_eq!(scalar(&json!("tenant:42"))?, SqlValue::from("tenant:42"));
        assert_eq!(scalar(&json!(u64::MAX))?, SqlValue::from(u64::MAX));
        assert!(scalar(&json!({"sql": "1 OR 1=1"})).is_err());
        assert!(scalar(&json!([1, 2])).is_err());
        Ok(())
    }

    #[test]
    fn missing_password_is_distinct_from_an_empty_password() -> Result<()> {
        assert_eq!(plaintext(&Credentials::default())?, None);
        assert_eq!(
            plaintext(&Credentials::new(Map::from_iter([(
                "password".into(),
                json!("")
            )])))?,
            Some("")
        );
        assert!(
            plaintext(&Credentials::new(Map::from_iter([(
                "password".into(),
                json!(123)
            )])))
            .is_err()
        );
        Ok(())
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "database/sqlite_tests.rs"]
mod sqlite_tests;
