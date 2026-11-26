use crate::validation::{Data, ValidationResult};

/// 在规则执行前注入本次验证的原始类型化数据。
pub trait DataAwareRule: Send + Sync {
    fn set_data(&mut self, data: &Data) -> ValidationResult<()>;
}
