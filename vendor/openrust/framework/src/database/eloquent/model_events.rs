//! Eloquent 模型事件经应用唯一 Events 派发。

use super::ModelMetadata;
use crate::{
    Result,
    database::DatabaseTransaction,
    events::{Event, Events},
    support::{BoxFuture, DataValue},
};
use std::{any::Any, future::Future};

tokio::task_local! {
    static MODEL_EVENTS_SUPPRESSED: ();
}

/// 在当前异步调用链内临时停止模型事件；应用的其他事件继续派发。
pub async fn without_model_events<F: Future>(work: F) -> F::Output {
    MODEL_EVENTS_SUPPRESSED.scope((), work).await
}

/// 从本次模型实例构造并派发应用声明的映射事件，提供当前事务和 halt 模式。
pub type MappedModelEvent<'m, M> = dyn for<'a> Fn(
        &'a Events,
        &'a M,
        Option<&'a DatabaseTransaction>,
        bool,
    ) -> BoxFuture<'a, Result<DataValue>>
    + Send
    + Sync
    + 'm;

/// 通过同一派发器处理映射的类型事件，保留直到首个非 null 或全部响应的语义。
pub async fn dispatch_mapped_model_event<E: Event>(
    events: &Events,
    event: &E,
    transaction: Option<&DatabaseTransaction>,
    halt: bool,
) -> Result<DataValue> {
    match (transaction, halt) {
        (Some(transaction), true) => events.until_in_transaction(transaction, event).await,
        (Some(transaction), false) => Ok(response_list(
            events.dispatch_in_transaction(transaction, event).await?,
        )),
        (None, true) => events.until(event).await,
        (None, false) => Ok(response_list(events.dispatch(event).await?)),
    }
}

/// 触发模型事件，先处理映射类型事件，再按上游规则回退到具名事件。
///
/// 具名监听器接收传入的同一模型引用。halt 模式的严格 false 可取消调用方的写入；
/// 其他非 null 响应可结束单次派发，但不能当作写入取消。没有派发器时返回 true。
pub async fn fire_model_event<M: Any + Send + Sync>(
    events: Option<&Events>,
    metadata: &ModelMetadata,
    model: &M,
    event: &str,
    halt: bool,
    mapped: Option<&MappedModelEvent<'_, M>>,
) -> Result<DataValue> {
    fire_model_event_in_context(events, metadata, model, event, halt, mapped, None).await
}

pub(super) async fn fire_model_event_in_context<M: Any + Send + Sync>(
    events: Option<&Events>,
    metadata: &ModelMetadata,
    model: &M,
    event: &str,
    halt: bool,
    mapped: Option<&MappedModelEvent<'_, M>>,
    transaction: Option<&DatabaseTransaction>,
) -> Result<DataValue> {
    let Some(events) = events.filter(|_| MODEL_EVENTS_SUPPRESSED.try_with(|()| ()).is_err()) else {
        return Ok(DataValue::Bool(true));
    };
    if let Some(dispatch) = mapped {
        let mut result = dispatch(events, model, transaction, halt).await?;
        if let DataValue::Array(entries) = &mut result {
            entries.retain(|(_, value)| !value.is_null());
        }
        if matches!(result, DataValue::Bool(false)) || !is_empty(&result) {
            return Ok(result);
        }
    }

    let name = format!("eloquent.{event}: {}", metadata.class_name);
    match (transaction, halt) {
        (Some(transaction), true) => {
            events
                .until_named_in_transaction(transaction, &name, model)
                .await
        }
        (Some(transaction), false) => Ok(response_list(
            events
                .dispatch_named_in_transaction(transaction, &name, model)
                .await?,
        )),
        (None, true) => events.until_named(&name, model).await,
        (None, false) => Ok(response_list(events.dispatch_named(&name, model).await?)),
    }
}

fn response_list(responses: Option<Vec<DataValue>>) -> DataValue {
    responses.map_or(DataValue::Null, |values| {
        DataValue::Array(
            (0_i64..)
                .zip(values)
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    })
}

fn is_empty(value: &DataValue) -> bool {
    match value {
        DataValue::Null | DataValue::Bool(false) => true,
        DataValue::Bool(true) | DataValue::Object(_) => false,
        DataValue::Number(value) => value.as_f64() == Some(0.0),
        DataValue::String(value) => value.is_empty() || value == "0",
        DataValue::Array(entries) => entries.is_empty(),
    }
}

#[cfg(test)]
pub(super) async fn test_events(provider: crate::events::EventsServiceProvider) -> Result<Events> {
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder.provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    Ok((*runtime.container().resolve::<Events>().await?).clone())
}

#[cfg(test)]
mod tests;
