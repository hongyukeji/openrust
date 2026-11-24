use crate::validation::{ValidationResult, ValidatorHandle};

/// 在规则执行前注入当前验证器的安全可变句柄。
pub trait ValidatorAwareRule: Send + Sync {
    fn set_validator(&mut self, validator: ValidatorHandle) -> ValidationResult<()>;
}
