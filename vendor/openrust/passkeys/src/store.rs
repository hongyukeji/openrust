use crate::{PasskeyModel, PasskeyRecord};
use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult, Transaction, Value, eloquent::Attributes,
};
use std::marker::PhantomData;
pub(crate) trait PasskeyStore: Send + Sync {
    fn name(&self) -> &'static str;
    fn purge_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        user: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>>;
    fn records_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<PasskeyRecord>>>;
    fn create_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        attributes: Attributes,
        user: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<PasskeyRecord>>;
    fn find(&self, key: Value) -> DatabaseFuture<'_, DatabaseResult<Option<PasskeyRecord>>>;
    fn records(
        &self,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<PasskeyRecord>>>;
    fn exists(
        &self,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<bool>>;
    fn create(
        &self,
        attributes: Attributes,
        user: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<PasskeyRecord>>;
}
pub(crate) struct Store<M>(PhantomData<fn() -> M>);
impl<M> Default for Store<M> {
    fn default() -> Self {
        Self(PhantomData)
    }
}
impl<M: PasskeyModel> PasskeyStore for Store<M> {
    fn name(&self) -> &'static str {
        std::any::type_name::<M>()
    }
    fn purge_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        user: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<u64>> {
        Box::pin(async move {
            let model = M::from_state(Default::default());
            let connection = model.get_connection()?;
            let table = model.get_table();
            if !transaction.belongs_to(&connection) {
                if !connection.get_schema_builder().has_table(table).await? {
                    return Ok(0);
                }
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥清理与用户必须使用同一连接事务",
                ));
            }
            if !transaction.get_schema_builder().has_table(table).await? {
                return Ok(0);
            }
            let key_name = model.get_key_name();
            let mut deleted = 0;
            loop {
                let mut query = M::query_on(&connection)
                    .use_write_pdo()
                    .without_global_scopes()
                    .with_trashed();
                query.r#where("user_id", "=", user.clone());
                if matches!(connection.get_driver_name(), "mysql" | "mariadb") {
                    let owner = crate::contracts::scalar(&user)
                        .ok_or(DatabaseError::InvalidQuery("通行密钥用户主键无效"))?;
                    query.where_nested(|conditions| {
                        conditions.where_raw(
                            "BINARY CAST(user_id AS CHAR) = BINARY ?",
                            vec![owner.into()],
                        )
                    });
                }
                let models = query
                    .lock_for_update()
                    .limit(100)
                    .get_in_transaction(transaction)
                    .await?;
                if models.is_empty() {
                    return Ok(deleted);
                }
                let keys = models
                    .iter()
                    .map(|record| {
                        record
                            .get_raw_original(key_name)
                            .cloned()
                            .ok_or(DatabaseError::InvalidQuery("通行密钥缺少真实主键"))
                    })
                    .collect::<DatabaseResult<Vec<_>>>()?;
                for mut record in models {
                    if crate::contracts::scalar(
                        record
                            .get_raw_original("user_id")
                            .ok_or(DatabaseError::InvalidQuery("通行密钥缺少用户归属"))?,
                    ) != crate::contracts::scalar(&user)
                        || record.get_key_name() != key_name
                        || record.get_key() != record.get_raw_original(key_name)
                    {
                        return Err(DatabaseError::InvalidQuery(
                            "通行密钥模型改变了真实主键或归属",
                        ));
                    }
                    if !record.force_delete_in_transaction(transaction).await? {
                        return Err(DatabaseError::InvalidQuery(
                            "通行密钥模型取消注销清理，须回滚调用方事务",
                        ));
                    }
                    deleted += 1;
                }
                if transaction
                    .table(table)
                    .where_in(key_name, keys)
                    .exists()
                    .await?
                {
                    return Err(DatabaseError::InvalidQuery(
                        "通行密钥模型未完成物理清理，须回滚调用方事务",
                    ));
                }
            }
        })
    }
    fn records_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<PasskeyRecord>>> {
        Box::pin(async move {
            let connection = M::from_state(Default::default()).get_connection()?;
            if !transaction.belongs_to(&connection) {
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥与用户必须使用同一连接事务",
                ));
            }
            let mut query = M::query_on(&connection).use_write_pdo();
            query.r#where(column, "=", value.clone());
            if matches!(connection.get_driver_name(), "mysql" | "mariadb") {
                query.where_nested(|conditions| {
                    conditions.where_raw(
                        &format!("BINARY CAST({column} AS CHAR) = BINARY ?"),
                        vec![value],
                    )
                });
            }
            Ok(query
                .lock_for_update()
                .get_in_transaction(transaction)
                .await?
                .into_iter()
                .map(PasskeyRecord::new)
                .collect())
        })
    }
    fn create_in_transaction<'a>(
        &'a self,
        transaction: &'a mut Transaction,
        attributes: Attributes,
        user: Value,
    ) -> DatabaseFuture<'a, DatabaseResult<PasskeyRecord>> {
        Box::pin(async move {
            let mut model = M::new_instance(attributes)?;
            if !transaction.belongs_to(&model.get_connection()?) {
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥与用户必须使用同一连接事务",
                ));
            }
            model.force_fill([("user_id".into(), user.clone())].into())?;
            if !model.save_in_transaction(transaction).await? {
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥模型取消保存，须回滚调用方事务",
                ));
            }
            if !model.state().exists() {
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥模型未完成持久化，须回滚调用方事务",
                ));
            }
            model.refresh_for_update(transaction).await?;
            if model.get_key().is_none()
                || model.get_key() != model.get_raw_original(model.get_key_name())
                || model
                    .get_attribute("user_id")
                    .and_then(crate::contracts::scalar)
                    != crate::contracts::scalar(&user)
                || model
                    .get_raw_original("user_id")
                    .and_then(crate::contracts::scalar)
                    != crate::contracts::scalar(&user)
            {
                return Err(DatabaseError::InvalidQuery(
                    "通行密钥模型改变了真实主键或归属",
                ));
            }
            Ok(PasskeyRecord::new(model))
        })
    }
    fn find(&self, key: Value) -> DatabaseFuture<'_, DatabaseResult<Option<PasskeyRecord>>> {
        Box::pin(async move { Ok(M::find(key).await?.map(PasskeyRecord::new)) })
    }
    fn records(
        &self,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<Vec<PasskeyRecord>>> {
        Box::pin(async move {
            let mut query = M::query()?;
            query.r#where(column, "=", value);
            Ok(query
                .get()
                .await?
                .into_iter()
                .map(PasskeyRecord::new)
                .collect())
        })
    }
    fn exists(
        &self,
        column: &'static str,
        value: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<bool>> {
        Box::pin(async move {
            let mut query = M::query()?;
            query.r#where(column, "=", value);
            Ok(query.first().await?.is_some())
        })
    }
    fn create(
        &self,
        attributes: Attributes,
        user: Value,
    ) -> DatabaseFuture<'_, DatabaseResult<PasskeyRecord>> {
        Box::pin(async move {
            let mut model = M::new_instance(attributes)?;
            model.force_fill([("user_id".into(), user)].into())?;
            model.save().await?;
            Ok(PasskeyRecord::new(model))
        })
    }
}
