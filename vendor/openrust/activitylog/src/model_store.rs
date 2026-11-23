//! 自定义 Eloquent 活动模型的注册与持久化适配。

/// 通过原生容器注册到配置指定的 `activity_model` 名称。
pub struct ActivityModel {
    pub(crate) table: &'static str,
    pub(crate) connection: Option<String>,
    pub(crate) save: for<'a> fn(
        &'a openrust::database::Connection,
        &'a crate::PreparedActivity,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<crate::ActivityRecord>,
    >,
    pub(crate) save_in: for<'a> fn(
        &'a crate::PreparedActivity,
        &'a mut openrust::database::Transaction,
    ) -> openrust::database::DatabaseFuture<
        'a,
        openrust::database::DatabaseResult<crate::ActivityRecord>,
    >,
}
impl ActivityModel {
    pub fn of<M: openrust::database::eloquent::Model>() -> Self {
        Self {
            table: M::from_state(Default::default()).get_table(),
            connection: M::from_state(Default::default())
                .get_connection_name()
                .map(str::to_owned),
            save: save_model::<M>,
            save_in: save_model_in::<M>,
        }
    }
}
fn prepared_model<M: openrust::database::eloquent::Model>(
    connection: &openrust::database::Connection,
    activity: &crate::PreparedActivity,
) -> openrust::database::DatabaseResult<M> {
    let mut model = M::new_instance_on(connection, Default::default())?;
    for (key, value) in activity.row() {
        model.set_attribute(&key, value);
    }
    Ok(model)
}
fn save_model<'a, M: openrust::database::eloquent::Model>(
    connection: &'a openrust::database::Connection,
    activity: &'a crate::PreparedActivity,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = prepared_model::<M>(connection, activity)?;
        model.save().await?;
        crate::repository::decode_attributes(model.state().get_attributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
fn save_model_in<'a, M: openrust::database::eloquent::Model>(
    activity: &'a crate::PreparedActivity,
    tx: &'a mut openrust::database::Transaction,
) -> openrust::database::DatabaseFuture<'a, openrust::database::DatabaseResult<crate::ActivityRecord>>
{
    Box::pin(async move {
        let mut model = prepared_model::<M>(tx.get_connection(), activity)?;
        model.save_in_transaction(tx).await?;
        crate::repository::decode_attributes(model.state().get_attributes()).map_err(|_| {
            openrust::database::DatabaseError::InvalidQuery("Invalid activity model attributes")
        })
    })
}
