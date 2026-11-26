use super::{ConsoleError, ConsoleQuestionError, configuration};
use crate::console::ConsoleTemplateError;
use serde_json::Value;
/// 问答适配保留配置错误分类、原始编码来源及容量错误的立即终止语义。
pub(super) fn format(template: &str, value: &Value) -> Result<String, ConsoleError> {
    super::super::formatting::format_template(template, value).map_err(|error| match error {
        ConsoleTemplateError::InvalidFormat(message) => configuration(&message),
        ConsoleTemplateError::InvalidEncoding(source) => {
            ConsoleError::from_source(ConsoleQuestionError::InvalidEncoding(source))
        }
        ConsoleTemplateError::Allocation(source) => ConsoleError::from_runtime_source(source),
    })
}
