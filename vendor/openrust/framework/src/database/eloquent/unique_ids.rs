//! 模型插入前的唯一 ID 生成，沿用属性访问器和赋值管线。

use super::{AttributeCast, ModelAttributes, ModelMetadata};
use crate::Result;
use serde_json::Value;

/// 生成符合 `HasUuids` 契约的 UUID v7 字符串。
#[cfg(feature = "db-uuid")]
pub fn new_uuid_v7() -> String {
    uuid::Uuid::now_v7().to_string()
}

impl ModelAttributes {
    /// 按模型声明顺序为 PHP 空值的属性生成 ID。
    ///
    /// 非 null 属性按 PHP 的 `empty` 属性访问顺序再次调用 getter，已有非空值保持。
    /// 返回 `None` 的生成器沿用基础生成器的 null 语义。此操作只写内存属性。
    pub async fn set_unique_ids(
        &self,
        metadata: &ModelMetadata,
        mut generate: impl FnMut() -> Result<Option<String>>,
    ) -> Result<&Self> {
        for name in &metadata.unique_id_columns {
            if !matches!(name.as_str(), "" | "0")
                && self
                    .get_attribute(metadata, name)?
                    .is_some_and(|value| !value.is_null())
                && AttributeCast::Boolean.read(self.get_attribute(metadata, name)?)?
                    == Some(Value::Bool(true))
            {
                continue;
            }
            let id = generate()?.map_or(Value::Null, Value::String);
            self.set_attribute(metadata, name, id).await?;
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests;
