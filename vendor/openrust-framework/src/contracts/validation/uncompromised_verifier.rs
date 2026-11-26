use crate::validation::{ValidationFuture, ValidationResult, Value};

/// 查询泄漏范围；显式参数替代动态数据数组，阈值为严格大于比较。
pub trait UncompromisedVerifier: Send + Sync {
    fn verify<'a>(
        &'a self,
        value: &'a Value,
        threshold: i64,
    ) -> ValidationFuture<'a, ValidationResult<bool>>;
}
