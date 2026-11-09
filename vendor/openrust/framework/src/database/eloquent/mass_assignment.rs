//! 批量赋值沿同一字段保护及 setter 管线执行。

use super::{ModelAttributes, ModelMetadata};
use crate::{Databases, Error, Result};
use serde_json::{Map, Value};
use std::future::Future;

tokio::task_local! {
    static UNGUARDED: ();
}

/// 严格拒绝或默认全保护时的处理回调；正常返回后继续处理。
pub type DiscardedAttributeViolation<'a> =
    dyn Fn(&ModelAttributes, &[String], &Error) -> Result<()> + Send + Sync + 'a;

/// 批量赋值所需的应用服务和拒绝策略。
///
/// 独立属性快照可用默认策略；若字段实际需要查询 Schema，则必须提供应用服务。
#[derive(Default)]
pub struct MassAssignment<'a> {
    /// 当前应用的数据库服务，需检查 Schema 时必填。
    pub databases: Option<&'a Databases>,
    /// 遇到被丢弃的属性时报告错误或调用处理回调。
    pub prevent_silent_discard: bool,
    /// 可选的拒绝回调；参数依次为属性存储、被拒字段及对应错误。
    pub discarded_attribute_violation: Option<&'a DiscardedAttributeViolation<'a>>,
    /// 可选的应用 HashManager；普通和强制赋值均复用同一服务。
    #[cfg(feature = "hashing")]
    pub hashing: Option<&'a crate::hashing::HashManager>,
}

impl<'a> MassAssignment<'a> {
    /// 使用默认拒绝策略及当前应用的数据库服务。
    pub fn new(databases: &'a Databases) -> Self {
        Self {
            databases: Some(databases),
            ..Self::default()
        }
    }

    fn reject(
        &self,
        attributes: &ModelAttributes,
        metadata: &ModelMetadata,
        names: &[String],
    ) -> Result<()> {
        let error = Error::custom_with_status(
            http::StatusCode::INTERNAL_SERVER_ERROR,
            "model_mass_assignment",
            format!(
                "模型 [{}] 的字段 [{}] 不允许批量赋值",
                metadata.class_name,
                names.join(", ")
            ),
        );
        match self.discarded_attribute_violation {
            Some(handle) => handle(attributes, names, &error),
            None => Err(error),
        }
    }
}

/// 当前异步调用链是否临时关闭批量赋值保护。
pub fn is_unguarded() -> bool {
    UNGUARDED.try_with(|()| ()).is_ok()
}

/// 在当前 future 内临时关闭批量赋值保护；嵌套继承，结束或取消后恢复。
///
/// 单独派生的 Tokio 任务不会继承此调用链的临时设置。
pub async fn unguarded<F: Future>(work: F) -> F::Output {
    UNGUARDED.scope((), work).await
}

impl ModelAttributes {
    /// 依输入顺序执行批量赋值。已有 setter/cast 生效，后续失败时先前赋值保留。
    pub async fn fill(
        &self,
        metadata: &ModelMetadata,
        input: Map<String, Value>,
        policy: &MassAssignment<'_>,
    ) -> Result<&Self> {
        let guarded = !is_unguarded();
        let totally_guarded = guarded && metadata.totally_guarded();
        let filter = guarded && !metadata.get_fillable().is_empty();
        let mut filtered = Vec::new();
        for (name, value) in input {
            // 预筛按数组键完全相等，独立于后续 isFillable 宽松比较。
            if filter && !metadata.get_fillable().contains(&name) {
                if policy.prevent_silent_discard {
                    filtered.push(name);
                }
                continue;
            }
            if self
                .is_fillable_for_assignment(metadata, policy.databases, &name)
                .await?
            {
                self.set_attribute_inner(
                    metadata,
                    &name,
                    value,
                    None,
                    #[cfg(feature = "hashing")]
                    policy.hashing,
                )
                .await?;
            } else if totally_guarded || policy.prevent_silent_discard {
                policy.reject(self, metadata, &[name])?;
            }
        }
        if !filtered.is_empty() {
            policy.reject(self, metadata, &filtered)?;
        }
        Ok(self)
    }

    /// 临时解除保护并调用同一个批量赋值入口。
    pub async fn force_fill(
        &self,
        metadata: &ModelMetadata,
        input: Map<String, Value>,
        policy: &MassAssignment<'_>,
    ) -> Result<&Self> {
        unguarded(self.fill(metadata, input, policy)).await
    }
}

#[cfg(test)]
mod tests;
