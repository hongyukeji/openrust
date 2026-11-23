use crate::{
    foundation::Precognition,
    http::HttpError,
    support::facades::Validator,
    validation::{Data, RuleSet, ValidatedInput},
};

use super::Request;

impl Request {
    /// 验证当前请求，Precognition 使用同一验证器并在成功后提前返回。
    pub async fn validate(&mut self, rules: RuleSet) -> Result<ValidatedInput, HttpError> {
        let data = Data::from_request(self).await?;
        let rules = if self.is_precognitive() {
            self.filter_precognitive_rules(rules)?
        } else {
            rules
        };
        let mut validator = Validator::make(data, rules)?;
        // 隔离验证器的大 Future 状态，不要求应用提高编译器的布局递归上限。
        let validated = Box::pin(validator.validate_request(self)).await?;
        if self.is_precognitive() {
            (Precognition::after_validation_hook(self))(&validator)?;
        }
        Ok(validated)
    }

    /// 只为验证失败选择错误包，不改写其他错误或 Precognition 的提前响应。
    pub async fn validate_with_bag(
        &mut self,
        bag: &str,
        rules: RuleSet,
    ) -> Result<ValidatedInput, HttpError> {
        self.validate(rules).await.map_err(|mut error| {
            if let HttpError::Validation(exception) = &mut error {
                exception.error_bag(bag);
            }
            error
        })
    }
}
