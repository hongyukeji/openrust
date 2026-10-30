//! 跨 HTTP、命令与导入共用的字段规则；预验证在构造完整 DTO 前选择既有规则。

mod numeric;
mod path;
mod runner;
#[cfg(test)]
mod tests;

use crate::core::{Error, Result};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

/// 自定义规则的错误；沿用既有 code、message 与参数国际化机制。
pub use validator::ValidationError as RuleError;

/// 原始值的来源；查询字符串的数字适配不扩散到 JSON 或已构造 DTO。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputContext {
    /// JSON 数字与字符串保留各自类型。
    Json,
    /// 查询输入保留字符串，数字规则按现查询反序列化方式检查。
    Query,
    /// 由已构造的 DTO 序列化，不注入或修改 serde 默认值。
    Typed,
}

/// 为一个输入类型声明唯一一份字段规则。
pub trait Validate {
    /// 返回普通请求、预验证与跨协议调用共用的规则。
    fn rules() -> Rules;
}

/// 自定义字段规则读取的上下文；路径是当前实际展开的字段。
#[derive(Clone, Copy)]
pub struct Field<'a> {
    /// 具体字段路径，保留声明中的字面点转义。
    pub attribute: &'a str,
    /// 缺失与显式 null 不混同。
    pub value: Option<&'a Value>,
    /// 当前原始输入；规则不得修改它。
    pub input: &'a Value,
    /// 当前输入来源。
    pub context: InputContext,
}

type CustomRule = dyn for<'a> Fn(Field<'a>) -> std::result::Result<(), RuleError> + Send + Sync;

/// 一个字段规则；内置规则与自定义规则沿相同执行器运行。
#[derive(Clone)]
pub struct Rule(Kind);

#[derive(Clone)]
enum Kind {
    Required,
    Present,
    Nullable,
    String,
    Integer,
    Boolean,
    Array,
    Email,
    Min(i64),
    Max(i64),
    Length(Option<u64>, Option<u64>),
    Range(Option<i64>, Option<i64>),
    Custom(Arc<CustomRule>),
}

impl Rule {
    /// 必须存在且非 null、空白字符串或空集合；0 与 false 是有效值。
    pub fn required() -> Self {
        Self(Kind::Required)
    }
    /// 必须存在，但允许空字符串或 null，由后续类型规则决定是否接受。
    pub fn present() -> Self {
        Self(Kind::Present)
    }
    /// null 时跳过普通规则，不跳过 required。
    pub fn nullable() -> Self {
        Self(Kind::Nullable)
    }
    /// 要求字符串。
    pub fn string() -> Self {
        Self(Kind::String)
    }
    /// 使用 Laravel 非 strict integer 规则：整数值或可由 `FILTER_VALIDATE_INT` 接受的值。
    pub fn integer() -> Self {
        Self(Kind::Integer)
    }
    /// 使用 Laravel boolean 的严格集合：true、false、0、1、字符串 0/1。
    pub fn boolean() -> Self {
        Self(Kind::Boolean)
    }
    /// 要求数组或对象，对应 Laravel 的关联数组输入。
    pub fn array() -> Self {
        Self(Kind::Array)
    }
    /// 使用既有 validator 电子邮件规则，不宣称提供 DNS 等扩展。
    pub fn email() -> Self {
        Self(Kind::Email)
    }
    /// Laravel min：带 integer 规则的数值按大小比较，其余按字符串或集合长度。
    pub fn min(value: i64) -> Self {
        Self(Kind::Min(value))
    }
    /// Laravel max：带 integer 规则的数值按大小比较，其余按字符串或集合长度。
    pub fn max(value: i64) -> Self {
        Self(Kind::Max(value))
    }
    /// 兼容既有 validator length 消息与上下界参数；按 Unicode 字符计数。
    ///
    /// 显式空字符串参与检查，空白不裁剪；不沿用 Laravel 原生普通规则的空串跳过。
    pub fn length(min: Option<u64>, max: Option<u64>) -> Self {
        Self(Kind::Length(min, max))
    }
    /// 兼容既有 validator range 消息与上下界参数；比较整数。
    ///
    /// 显式空串仍为无效数值；查询字符串沿既有 Query 整数适配，JSON/Typed 不扩散转换。
    pub fn range(min: Option<i64>, max: Option<i64>) -> Self {
        Self(Kind::Range(min, max))
    }
    /// 自定义普通规则；未选中、缺失、空字符串或 nullable null 时不调用。
    pub fn custom<F>(callback: F) -> Self
    where
        F: for<'a> Fn(Field<'a>) -> std::result::Result<(), RuleError> + Send + Sync + 'static,
    {
        Self(Kind::Custom(Arc::new(callback)))
    }
}

/// 有序且有界的字段规则集合；构建错误在执行前显式返回，不丢弃声明。
#[derive(Clone, Default)]
#[must_use]
pub struct Rules {
    fields: Vec<(String, Vec<Rule>)>,
    count: usize,
    path_bytes: usize,
    invalid: Option<&'static str>,
}

impl Rules {
    /// 创建空规则集合。
    pub fn new() -> Self {
        Self::default()
    }
    /// 添加字段规则；dot 表示嵌套，`\.` 表示字面点，单独的 `*` 段按输入键展开。
    pub fn field(
        mut self,
        attribute: impl Into<String>,
        rules: impl IntoIterator<Item = Rule>,
    ) -> Self {
        let attribute = attribute.into();
        if self.invalid.is_some() {
            return self;
        }
        if attribute.len() > path::MAX_PATH || self.fields.len() >= path::MAX_FIELDS {
            self.invalid = Some("字段声明数量或路径长度超限");
            return self;
        }
        self.path_bytes = self.path_bytes.saturating_add(attribute.len());
        if self.path_bytes > path::MAX_PATH_BYTES {
            self.invalid = Some("字段声明路径总长度超限");
            return self;
        }
        if self.fields.iter().any(|(name, _)| name == &attribute) {
            self.invalid = Some("字段规则重复声明");
            return self;
        }
        let mut collected = Vec::new();
        for rule in rules {
            self.count += 1;
            if self.count > path::MAX_RULES {
                self.invalid = Some("规则数量超限");
                return self;
            }
            collected.push(rule);
        }
        self.fields.push((attribute, collected));
        self
    }
    /// 对完整原始输入执行所有既有字段规则，不反序列化 DTO。
    pub fn validate(&self, input: &Value, context: InputContext) -> Result<()> {
        runner::run(self, input, None, context)
    }
    /// 展开规则后仅执行匹配的既有键；模式不 trim，`*` 不跨点分段。
    pub fn validate_only(
        &self,
        input: &Value,
        patterns: &[&str],
        context: InputContext,
    ) -> Result<()> {
        runner::run(self, input, Some(patterns), context)
    }
}

/// 对原始值执行类型声明的全部规则；HTTP 与导入不需要维护第二份规则。
pub fn validate_input<T: Validate>(input: &Value, context: InputContext) -> Result<()> {
    T::rules().validate(input, context)
}

/// 校验已经构造的 DTO，包含 serde 默认值，成功返回原值。
pub fn validate<T: Validate + Serialize>(value: T) -> Result<T> {
    let raw = serde_json::to_value(&value).map_err(Error::internal)?;
    validate_input::<T>(&raw, InputContext::Typed)?;
    Ok(value)
}
