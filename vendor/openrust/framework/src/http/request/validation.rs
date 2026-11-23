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
        self.validate_with(rules, Default::default(), Default::default())
            .await
    }

    /// 为本次请求指定消息和字段显示名，保留预验证与错误渲染管线。
    pub async fn validate_with(
        &mut self,
        rules: RuleSet,
        messages: std::collections::BTreeMap<String, String>,
        attributes: std::collections::BTreeMap<String, String>,
    ) -> Result<ValidatedInput, HttpError> {
        let data = Data::from_request(self).await?;
        let rules = if self.is_precognitive() {
            self.filter_precognitive_rules(rules)?
        } else {
            rules
        };
        let mut validator = Validator::make_with(data, rules, messages, attributes)?;
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
        self.validate_with_bag_and_options(bag, rules, Default::default(), Default::default())
            .await
    }

    /// 具名错误包与自定义消息使用同一验证流程。
    pub async fn validate_with_bag_and_options(
        &mut self,
        bag: &str,
        rules: RuleSet,
        messages: std::collections::BTreeMap<String, String>,
        attributes: std::collections::BTreeMap<String, String>,
    ) -> Result<ValidatedInput, HttpError> {
        self.validate_with(rules, messages, attributes)
            .await
            .map_err(|mut error| {
                if let HttpError::Validation(exception) = &mut error {
                    exception.error_bag(bag);
                }
                error
            })
    }
}
