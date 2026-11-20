use crate::{PasskeyModel, PasskeyRecord};
use openrust::database::{DatabaseFuture, DatabaseResult, Value, eloquent::Attributes};
use std::marker::PhantomData;
pub(crate) trait PasskeyStore: Send + Sync {
    fn name(&self) -> &'static str;
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
            let mut model = M::newInstance(attributes)?;
            model.forceFill([("user_id".into(), user)].into())?;
            model.save().await?;
            Ok(PasskeyRecord::new(model))
        })
    }
}
