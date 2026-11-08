//! 由应用代码填充的精确名称请求属性。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::container::ParameterValue;
use crate::{Error, Result};

/// 当前请求共享的类型化属性袋。克隆和重新提取 `RequestInfo` 共享变更。
///
/// 键是精确字符串。查询参数、表单、请求头和路由参数独立存取。
#[derive(Clone, Default)]
pub struct RequestAttributes(Arc<AttributeState>);

#[derive(Default)]
struct AttributeState {
    values: Mutex<BTreeMap<String, ParameterValue>>,
}

impl Drop for AttributeState {
    fn drop(&mut self) {
        let values = self
            .values
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, value) in std::mem::take(values) {
            crate::foundation::lifecycle::disposal::dispose(value, None);
        }
    }
}

impl RequestAttributes {
    /// 读取属性。`None` 表示键不存在，显式 null 保存在 `Some` 中。
    pub fn get(&self, key: &str) -> Result<Option<ParameterValue>> {
        Ok(self.lock()?.get(key).cloned())
    }

    /// 设置属性，返回原值。返回的对象已离开属性袋的内部锁。
    pub fn insert(
        &self,
        key: impl Into<String>,
        value: ParameterValue,
    ) -> Result<Option<ParameterValue>> {
        let key = key.into();
        let previous = self.lock()?.insert(key, value);
        Ok(previous)
    }

    /// 删除属性，返回原值。
    pub fn remove(&self, key: &str) -> Result<Option<ParameterValue>> {
        let previous = self.lock()?.remove(key);
        Ok(previous)
    }

    fn lock(&self) -> Result<MutexGuard<'_, BTreeMap<String, ParameterValue>>> {
        self.0
            .values
            .lock()
            .map_err(|_| Error::custom("http_request_attributes_poisoned", "请求属性上下文不可用"))
    }
}

#[cfg(test)]
mod tests {
    use axum::extract::FromRequestParts as _;
    use axum::http::Request;

    use super::{ParameterValue, Result};
    use crate::http::RequestInfo;

    #[tokio::test]
    async fn request_info_reuses_typed_attributes_independently_of_http_input() -> Result<()> {
        let request = Request::builder()
            .uri("/example?tag=from-query")
            .header("tag", "from-header")
            .body(())
            .map_err(crate::Error::internal)?;
        let (mut parts, ()) = request.into_parts();
        let first = RequestInfo::from_request_parts(&mut parts, &()).await?;
        assert!(first.attributes().get("tag")?.is_none());
        assert!(
            first
                .attributes()
                .insert("tag", ParameterValue::null())?
                .is_none()
        );
        assert!(
            first
                .attributes()
                .insert(" tag", ParameterValue::new(7_u8))?
                .is_none()
        );

        let second = RequestInfo::from_request_parts(&mut parts, &()).await?;
        let previous = second
            .attributes()
            .insert("tag", ParameterValue::new(42_u8))?;
        assert!(previous.is_some_and(|value| value.is_null()));
        let removed = first.clone().attributes().remove("tag")?.ok_or_else(|| {
            crate::Error::custom("test_request_attribute_missing", "共享请求属性未写入")
        })?;
        assert_eq!(*removed.downcast::<u8>()?, 42);
        assert!(second.attributes().get("tag")?.is_none());
        assert!(second.attributes().get(" tag")?.is_some());
        Ok(())
    }
}
