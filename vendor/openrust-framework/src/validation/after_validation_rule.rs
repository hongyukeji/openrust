use super::{AfterValidation, ValidationResult, Validator};

/// 后置校验服务；稳定Rust使用显式方法代替对象调用运算符。
pub trait AfterValidationRule: Send + Sync {
    fn after(&self, validator: &mut Validator) -> ValidationResult<()>;
    fn into_callback(self) -> AfterValidation
    where
        Self: Sized + 'static,
    {
        Box::new(move |validator| self.after(validator))
    }
}
impl<T: AfterValidationRule + ?Sized> AfterValidationRule for Box<T> {
    fn after(&self, validator: &mut Validator) -> ValidationResult<()> {
        (**self).after(validator)
    }
}
