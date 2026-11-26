use super::{HttpError, Method};

/// 每个应用的 HTTP 方法覆写策略，不修改进程级全局设置。
#[derive(Clone, Debug)]
pub struct MethodOverride {
    pub parameter: bool,
    allowed: Option<Vec<Method>>,
}
impl Default for MethodOverride {
    fn default() -> Self {
        Self {
            parameter: true,
            allowed: None,
        }
    }
}
impl MethodOverride {
    pub fn new(parameter: bool, allowed: Option<Vec<Method>>) -> Result<Self, HttpError> {
        if allowed.as_ref().is_some_and(|methods| {
            methods.iter().any(|method| {
                matches!(
                    *method,
                    Method::GET | Method::HEAD | Method::CONNECT | Method::TRACE
                )
            })
        }) {
            return Err(HttpError::InvalidRequest);
        }
        Ok(Self { parameter, allowed })
    }
    pub fn allowed(&self) -> Option<&[Method]> {
        self.allowed.as_deref()
    }
}
