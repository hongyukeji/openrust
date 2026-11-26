use crate::{
    support::MessageBag,
    validation::{ValidatedInput, ValidationFuture, ValidationResult},
};

/// Evaluates rules and exposes their errors and validated data.
pub trait Validator: Send + crate::contracts::support::MessageProvider {
    fn validate(&mut self) -> ValidationFuture<'_, ValidationResult<ValidatedInput>>;
    fn fails(&mut self) -> ValidationFuture<'_, ValidationResult<bool>>;
    fn validated(&self) -> ValidationResult<ValidatedInput>;
    fn errors(&self) -> &MessageBag;
}

// 既有外部Validator实现仍只需实现原错误读取方法。
impl<T: Validator + ?Sized> crate::contracts::support::MessageProvider for T {
    fn get_message_bag(&self) -> &MessageBag {
        self.errors()
    }
}
