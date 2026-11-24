use std::sync::Arc;

use serde_json::Map;

use super::{Rule, RuleSet, ValidationResult, Value, rule::Kind};

/// 每个匹配项可返回标量规则，或相对于该项的字段规则。
#[derive(Clone)]
pub enum NestedRuleSet {
    Value(Vec<Rule>),
    Fields(RuleSet),
}
impl From<Vec<Rule>> for NestedRuleSet {
    fn from(rules: Vec<Rule>) -> Self {
        Self::Value(rules)
    }
}
impl From<RuleSet> for NestedRuleSet {
    fn from(rules: RuleSet) -> Self {
        Self::Fields(rules)
    }
}

type Callback = dyn Fn(Option<&Value>, &str, &Map<String, Value>, Option<&Value>) -> ValidationResult<NestedRuleSet>
    + Send
    + Sync;

/// 在验证前编译每项规则，不作为通过／失败的自定义规则运行。
#[derive(Clone)]
pub struct NestedRules {
    callback: CallbackKind,
}
#[derive(Clone)]
enum CallbackKind {
    Arguments(Arc<Callback>),
    Context(Arc<ContextCallback>),
}
type ContextCallback =
    dyn Fn(&NestedRuleContext<'_>) -> ValidationResult<NestedRuleSet> + Send + Sync;

/// 完整只读上下文；原数据保留类型化文件，扁平数据保持当前编译作用域。
pub struct NestedRuleContext<'a> {
    pub value: Option<&'a Value>,
    pub attribute: &'a str,
    pub data: &'a super::Data,
    pub flattened: &'a Map<String, Value>,
    pub parent: Option<&'a Value>,
}
impl NestedRuleContext<'_> {
    pub fn file(&self) -> Option<&crate::http::UploadedFile> {
        self.data.file(self.attribute)
    }
}
impl NestedRules {
    pub fn new<T: Into<NestedRuleSet>>(
        callback: impl Fn(
            Option<&Value>,
            &str,
            &Map<String, Value>,
            Option<&Value>,
        ) -> ValidationResult<T>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            callback: CallbackKind::Arguments(Arc::new(move |value, attribute, data, context| {
                callback(value, attribute, data, context).map(Into::into)
            })),
        }
    }
    pub fn with_context<T: Into<NestedRuleSet>>(
        callback: impl Fn(&NestedRuleContext<'_>) -> ValidationResult<T> + Send + Sync + 'static,
    ) -> Self {
        Self {
            callback: CallbackKind::Context(Arc::new(move |context| {
                callback(context).map(Into::into)
            })),
        }
    }

    pub(crate) fn resolve(
        &self,
        context: &NestedRuleContext<'_>,
    ) -> ValidationResult<NestedRuleSet> {
        match &self.callback {
            CallbackKind::Context(callback) => callback(context),
            CallbackKind::Arguments(callback) => {
                let readable = super::validator::unescape(context.attribute);
                callback(context.value, &readable, context.flattened, context.parent)
            }
        }
    }
}
impl From<NestedRules> for Rule {
    fn from(rule: NestedRules) -> Self {
        Rule::new(Kind::ForEach(rule))
    }
}

impl crate::contracts::validation::CompilableRules for NestedRules {
    fn compile(
        &self,
        attribute: &str,
        value: Option<&Value>,
        data: &Map<String, Value>,
        context: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        let input = super::Data::new(super::path::from_flat(data)?)?;
        Rule::compile(
            attribute,
            self.resolve(&NestedRuleContext {
                value,
                attribute,
                data: &input,
                flattened: data,
                parent: context,
            })?,
            data,
        )
    }
    fn compile_context(&self, context: &NestedRuleContext<'_>) -> ValidationResult<RuleSet> {
        let master = super::Data::new(super::path::from_flat(context.flattened)?)?;
        super::rule_compiler::bind(context.attribute, self.resolve(context)?)?
            .compile_scoped(context.data, &master)
    }
}
