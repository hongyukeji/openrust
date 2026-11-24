use crate::validation::{NestedRuleContext, RuleSet, ValidationResult, Value};
use serde_json::Map;

/// 将当前属性的声明展开为字段规则，发生在字段验证之前。
pub trait CompilableRules: Send + Sync {
    fn compile(
        &self,
        attribute: &str,
        value: Option<&Value>,
        data: &Map<String, Value>,
        context: Option<&Value>,
    ) -> ValidationResult<RuleSet>;
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        self.compile(
            context.attribute,
            context.value,
            context.flattened,
            context.parent,
        )
    }
}
