//! 模型事件到日志服务的转接，以及事务提交后的持久化调度。
use crate::model_snapshot::{build_stored, event_name};
use crate::{ActivityLogger, ActivityRepository, Error, models::concerns::LogsActivity};
use openrust::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult, Transaction,
    eloquent::concerns::{ModelEvent, ModelEventListener, ModelEventResponse},
};
use std::sync::Arc;

// 保留当前保存点标记和最外层提交后的有序写入。
fn save_after_commit(
    transaction: &mut Transaction,
    prepared: crate::PreparedActivity,
    repository: Arc<ActivityRepository>,
    buffer: Option<Arc<crate::ActivityBuffer>>,
) -> DatabaseResult<()> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let committed = Arc::new(AtomicBool::new(false));
    let marker = committed.clone();
    transaction.after_commit(move || {
        Box::pin(async move {
            marker.store(true, Ordering::SeqCst);
            Ok(())
        })
    })?;
    transaction.after_commit_at_level(1, move || {
        Box::pin(async move {
            if committed.load(Ordering::SeqCst) {
                if let Some(buffer) = buffer {
                    buffer.add(prepared);
                } else {
                    repository.save_prepared(prepared).await.map_err(error)?;
                }
            }
            Ok(())
        })
    })
}
pub(crate) struct LogsActivityListener {
    pub(crate) repository: Arc<ActivityRepository>,
}
fn error(error: Error) -> DatabaseError {
    DatabaseError::Unavailable(
        openrust::contracts::container::BindingResolutionError::FactoryFailed {
            service: "Activitylog",
            source: Box::new(error),
        },
    )
}
impl<M: LogsActivity> ModelEventListener<M> for LogsActivityListener {
    fn handle<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        _: &'a Connection,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if let Some(activity) = build_stored(
                model,
                event,
                None,
                &self.repository.default_except_attributes,
            )
            .await
            .map_err(error)?
            {
                let mut logger = ActivityLogger::current()
                    .or_else(|| crate::facades::Activitylog::logger().ok())
                    .unwrap_or_else(|| ActivityLogger::from_repository(self.repository.clone()));
                if let Some(prepared) = logger
                    .prepare_model(activity, |activity| {
                        model.before_activity_logged(activity, event_name(event).unwrap_or(""))
                    })
                    .map_err(error)?
                {
                    if logger.buffer_prepared(prepared.clone()).is_some() {
                        return Ok(ModelEventResponse::Continue);
                    }
                    self.repository
                        .save_prepared(prepared)
                        .await
                        .map_err(error)?;
                }
            }
            Ok(ModelEventResponse::Continue)
        })
    }
    fn handle_in_transaction<'a>(
        &'a self,
        event: ModelEvent,
        model: &'a mut M,
        transaction: &'a mut Transaction,
    ) -> DatabaseFuture<'a, DatabaseResult<ModelEventResponse>> {
        Box::pin(async move {
            if let Some(activity) = build_stored(
                model,
                event,
                Some(&mut *transaction),
                &self.repository.default_except_attributes,
            )
            .await
            .map_err(error)?
            {
                let mut logger = ActivityLogger::current()
                    .or_else(|| crate::facades::Activitylog::logger().ok())
                    .unwrap_or_else(|| ActivityLogger::from_repository(self.repository.clone()));
                if let Some(prepared) = logger
                    .prepare_model(activity, |activity| {
                        model.before_activity_logged(activity, event_name(event).unwrap_or(""))
                    })
                    .map_err(error)?
                {
                    if let Some(buffer) = logger.buffer() {
                        save_after_commit(
                            transaction,
                            prepared,
                            self.repository.clone(),
                            Some(buffer),
                        )?;
                        return Ok(ModelEventResponse::Continue);
                    }
                    if self.repository.uses_connection(&model.get_connection()?) {
                        prepared.log_in(transaction).await?;
                    } else {
                        save_after_commit(transaction, prepared, self.repository.clone(), None)?;
                    }
                }
            }
            Ok(ModelEventResponse::Continue)
        })
    }
}
