//! 从宿主 Db 的逻辑表读取账户并保留标识原始类型。

use super::contracts::{AuthUser, Authenticatable, Credentials, GenericUser, UserProvider};
use crate::{
    Error, Result,
    database::{
        Db,
        eloquent::attributes_from_row,
        query::{QueryBuilder, RawSql},
        sea_orm::{DbBackend, QueryResult, Value as SqlValue},
    },
    hashing::HashOptions,
    support::BoxFuture,
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
    passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
    user_factory: UserFactory,
}

impl fmt::Debug for DatabaseUserProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DatabaseUserProvider([REDACTED])")
    }
}

impl DatabaseUserProvider {
    /// 使用已选择连接的 Db 与逻辑表名；表前缀、标识校验和绑定复用 Db。
    pub fn new(
        db: Db,
        passwords: std::sync::Arc<dyn crate::hashing::Hasher>,
        table: impl Into<String>,
    ) -> Result<Self> {
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

    /// 通过宿主已编译的账户模型解释数据库属性；默认创建 `GenericUser`。
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

    fn query<'cap>(&self) -> Result<QueryBuilder<'static, 'cap>> {
        self.db.table(&self.table)
    }

    fn query_for_user<'cap>(
        &self,
        user: &dyn Authenticatable,
    ) -> Result<QueryBuilder<'static, 'cap>> {
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
                .check(plain.as_bytes(), Some(&encoded), &HashOptions::new())
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
            if !self.passwords.needs_rehash(&encoded, &HashOptions::new())? && !force {
                return Ok(());
            }
            let plain = plaintext(credentials)?.ok_or_else(invalid_credentials)?;
            let field = user.auth_password_name().to_owned();
            // where_eq 先校验模型字段名。附加字节比较，避免文本排序规则忽略编码哈希的大小写。
            let identifier_name = user.auth_identifier_name().to_owned();
            let identifier = scalar(&user.auth_identifier()?)?;
            drop(
                self.db
                    .table(&self.table)?
                    .where_eq(&identifier_name, identifier.clone())?
                    .where_eq(&field, encoded.as_str())?,
            );
            let exact = match self.db.backend()? {
                DbBackend::MySql => format!("CAST(`{field}` AS BINARY) = CAST(? AS BINARY)"),
                DbBackend::Postgres => {
                    format!("convert_to(\"{field}\", 'UTF8') = convert_to(?, 'UTF8')")
                }
                DbBackend::Sqlite => format!("CAST(\"{field}\" AS BLOB) = CAST(? AS BLOB)"),
                _ => {
                    return Err(Error::custom(
                        "unsupported_database_backend",
                        "密码重哈希不支持此数据库后端",
                    ));
                }
            };
            let replacement = self
                .passwords
                .make(plain.as_bytes(), &HashOptions::new())
                .await?;
            let db = self.db.clone();
            let table = self.table.clone();
            self.db
                .transaction(move |transaction| {
                    Box::pin(async move {
                        let query = db
                            .table(&table)?
                            .where_eq(&identifier_name, identifier)?
                            .where_eq(&field, encoded.as_str())?
                            .where_raw(RawSql::new(exact), vec![encoded.as_str().into()]);
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
    attributes_from_row(row).map_err(auth_row_error)
}

fn auth_row_error(error: Error) -> Error {
    // 认证错误沿用既有 code；行解码是服务端失败，不能沿用旧路径误设的 400。
    match error.code() {
        "model_database_row_invalid" => Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth_database_row_invalid",
            "认证账户必须是属性对象",
        ),
        "model_database_attribute_unsupported" => Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "auth_database_attribute_unsupported",
            "认证账户包含无法转换的数据库字段",
        ),
        _ => error,
    }
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

    #[test]
    fn common_row_adapter_keeps_auth_codes_and_reports_all_decode_failures_as_server_errors() {
        for (model_code, auth_code) in [
            ("model_database_row_invalid", "auth_database_row_invalid"),
            (
                "model_database_attribute_unsupported",
                "auth_database_attribute_unsupported",
            ),
        ] {
            let error = auth_row_error(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                model_code,
                "内部适配错误",
            ));
            assert_eq!(error.code(), auth_code);
            assert_eq!(error.status().as_u16(), 500);
        }
        for code in [
            "model_database_attribute_invalid",
            "model_database_attribute_non_finite",
        ] {
            let error = auth_row_error(Error::custom_with_status(
                http::StatusCode::INTERNAL_SERVER_ERROR,
                code,
                "内部适配错误",
            ));
            assert_eq!(error.code(), code);
            assert_eq!(error.status().as_u16(), 500);
        }
    }
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "database/sqlite_tests.rs"]
mod sqlite_tests;
