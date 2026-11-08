//! 模型时间戳更新及异步调用链内的临时禁用。

use super::{ModelAttributes, ModelDate, ModelMetadata};
use crate::{
    Result,
    support::{CalendarContext, DateConstruction, InputDate},
};
use std::future::Future;

tokio::task_local! {
    static SUPPRESSED_TIMESTAMP_MODELS: Vec<String>;
}

pub(super) fn fresh_timestamp(calendar: &CalendarContext) -> Result<InputDate> {
    calendar.make_date(DateConstruction::Now)?.ok_or_else(|| {
        crate::Error::custom(
            "model_timestamp_unavailable",
            "应用日期工厂没有返回当前时刻",
        )
    })
}

/// 在当前异步调用链禁用给定模型类的自动时间戳；嵌套范围累加，结束或取消后恢复。
pub async fn without_timestamps_on<F: Future>(classes: &[&str], work: F) -> F::Output {
    let mut ignored = SUPPRESSED_TIMESTAMP_MODELS
        .try_with(Clone::clone)
        .unwrap_or_default();
    ignored.extend(classes.iter().map(|name| (*name).to_owned()));
    SUPPRESSED_TIMESTAMP_MODELS.scope(ignored, work).await
}

impl ModelMetadata {
    /// 当前持久化调用是否自动维护模型时间戳。
    pub fn uses_timestamps(&self) -> bool {
        self.timestamps
            && !SUPPRESSED_TIMESTAMP_MODELS
                .try_with(|classes| classes.contains(&self.class_name))
                .unwrap_or_default()
    }
}

impl ModelAttributes {
    /// 依序维护尚未手工改动的更新时间与新模型创建时间。
    ///
    /// 两列共用一次应用时钟读取，经过普通 setter 管线。调用者可先用
    /// [`ModelMetadata::uses_timestamps`] 判断持久化流程是否自动调用此方法。
    pub async fn update_timestamps(
        &self,
        metadata: &ModelMetadata,
        exists: bool,
        calendar: &CalendarContext,
    ) -> Result<&Self> {
        let time = ModelDate::new(fresh_timestamp(calendar)?, false);
        for name in [
            metadata.updated_at_column.as_deref(),
            metadata.created_at_column.as_deref().filter(|_| !exists),
        ]
        .into_iter()
        .flatten()
        {
            if !self.is_dirty(metadata, &[name])? {
                self.set_attribute_value(metadata, name, time.clone().into())
                    .await?;
            }
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
