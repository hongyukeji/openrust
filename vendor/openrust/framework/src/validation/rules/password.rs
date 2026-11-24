use crate::validation::{Rule, RuleFailure, ValidationResult, Value, rule::Kind};

/// 应用级默认密码策略；普通构造仍从显式最小长度开始。
#[derive(Clone)]
pub struct Password {
    pub(in crate::validation) minimum: usize,
    pub(in crate::validation) maximum: Option<usize>,
    pub(in crate::validation) required: bool,
    pub(in crate::validation) sometimes: bool,
    pub(in crate::validation) mixed_case: bool,
    pub(in crate::validation) letters: bool,
    pub(in crate::validation) numbers: bool,
    pub(in crate::validation) symbols: bool,
    pub(in crate::validation) compromised_threshold: Option<i64>,
    pub(in crate::validation) custom_rules: Vec<Rule>,
    configuration_error: Option<crate::validation::rule_compiler::CompilationError>,
}

/// 原生状态视图；规则对象保持类型，不创造新的序列化协议。
#[derive(Clone)]
pub struct PasswordRules {
    pub min: usize,
    pub max: Option<usize>,
    pub mixed_case: bool,
    pub letters: bool,
    pub numbers: bool,
    pub symbols: bool,
    pub uncompromised: bool,
    pub compromised_threshold: i64,
    pub custom_rules: Vec<Rule>,
}
impl Default for Password {
    fn default() -> Self {
        Self::defaults().unwrap_or_else(|error| {
            let mut rule = Self::min(8);
            rule.configuration_error = Some(error.into());
            rule
        })
    }
}
impl Password {
    pub fn new(min: usize) -> Self {
        Self::min(min)
    }
    pub fn min(min: usize) -> Self {
        Self {
            minimum: min.max(1),
            maximum: None,
            required: false,
            sometimes: false,
            mixed_case: false,
            letters: false,
            numbers: false,
            symbols: false,
            compromised_threshold: None,
            custom_rules: Vec::new(),
            configuration_error: None,
        }
    }
    pub fn defaults() -> ValidationResult<Self> {
        use crate::support::facades::Facade as _;
        if crate::support::facades::current_container().is_none() {
            return Ok(Self::min(8));
        }
        crate::validation::facade::Validator::get_facade_root()
            .map(|factory| factory.password_defaults())
            .map_err(|source| crate::validation::ValidationError::Execution {
                source: Box::new(source),
            })
    }
    pub fn set_defaults(
        callback: impl Fn() -> Self + Send + Sync + 'static,
    ) -> ValidationResult<()> {
        use crate::support::facades::Facade as _;
        crate::validation::facade::Validator::get_facade_root()
            .map_err(|source| crate::validation::ValidationError::Execution {
                source: Box::new(source),
            })?
            .set_password_defaults(callback);
        Ok(())
    }
    pub fn max(mut self, max: usize) -> Self {
        self.maximum = Some(max);
        self
    }
    pub fn required() -> Self {
        Self {
            required: true,
            ..Self::default()
        }
    }
    pub fn sometimes() -> Self {
        Self {
            sometimes: true,
            ..Self::default()
        }
    }
    pub fn mixed_case(mut self) -> Self {
        self.mixed_case = true;
        self
    }
    pub fn letters(mut self) -> Self {
        self.letters = true;
        self
    }
    pub fn numbers(mut self) -> Self {
        self.numbers = true;
        self
    }
    pub fn symbols(mut self) -> Self {
        self.symbols = true;
        self
    }
    pub fn uncompromised(mut self, threshold: i64) -> Self {
        self.compromised_threshold = Some(threshold);
        self
    }
    /// 附加规则替换上一批，保留与原策略相同的执行顺序。
    pub fn rules(mut self, rules: impl IntoIterator<Item = Rule>) -> Self {
        self.custom_rules = rules.into_iter().collect();
        self
    }
    pub fn when(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        if condition { callback(self) } else { self }
    }
    pub fn unless(self, condition: bool, callback: impl FnOnce(Self) -> Self) -> Self {
        self.when(!condition, callback)
    }
    pub fn when_else(
        self,
        condition: bool,
        yes: impl FnOnce(Self) -> Self,
        no: impl FnOnce(Self) -> Self,
    ) -> Self {
        if condition { yes(self) } else { no(self) }
    }
    pub fn applied_rules(&self) -> PasswordRules {
        PasswordRules {
            min: self.minimum,
            max: self.maximum,
            mixed_case: self.mixed_case,
            letters: self.letters,
            numbers: self.numbers,
            symbols: self.symbols,
            uncompromised: self.compromised_threshold.is_some(),
            compromised_threshold: self.compromised_threshold.unwrap_or(0),
            custom_rules: self.custom_rules.clone(),
        }
    }
    pub fn to_password_rules_string(&self) -> String {
        let mut rules = vec![format!("minlength: {}", self.minimum)];
        if let Some(max) = self.maximum.filter(|max| *max != 0) {
            rules.push(format!("maxlength: {max}"));
        }
        if self.mixed_case {
            rules.extend(["required: lower".into(), "required: upper".into()]);
        } else if self.letters {
            rules.push("required: lower".into());
        }
        if self.numbers {
            rules.push("required: digit".into());
        }
        if self.symbols {
            rules.push("required: special".into());
        }
        format!("{};", rules.join("; "))
    }
    pub(in crate::validation) fn skip(&self, value: Option<&Value>, nullable: bool) -> bool {
        if value.is_none() && (!self.required || self.sometimes) {
            return true;
        }
        let blank = value.is_none_or(|value| match value {
            Value::Null => true,
            Value::String(value) => value
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty(),
            Value::Array(values) => values.is_empty(),
            Value::Object(values) => values.is_empty(),
            _ => false,
        });
        blank && !self.required && nullable
    }
    pub(in crate::validation) fn complexity_failures(
        &self,
        value: Option<&Value>,
    ) -> Vec<RuleFailure> {
        let mut failures = Vec::new();
        if let Some(value) = value.and_then(Value::as_str) {
            use std::sync::LazyLock;
            static MIXED: LazyLock<regex::Regex> = LazyLock::new(|| {
                regex::Regex::new(r"(\p{Ll}+.*\p{Lu})|(\p{Lu}+.*\p{Ll})")
                    .expect("固定密码规则表达式有效")
            });
            static LETTERS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"\pL").expect("固定密码规则表达式有效"));
            static NUMBERS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"\pN").expect("固定密码规则表达式有效"));
            static SYMBOLS: LazyLock<regex::Regex> = LazyLock::new(|| {
                regex::Regex::new(r"[\pZ\pS\pP]").expect("固定密码规则表达式有效")
            });
            for (enabled, regex, key) in [
                (self.mixed_case, &*MIXED, "mixed"),
                (self.letters, &*LETTERS, "letters"),
                (self.symbols, &*SYMBOLS, "symbols"),
                (self.numbers, &*NUMBERS, "numbers"),
            ] {
                if enabled && !regex.is_match(value) {
                    failures.push(RuleFailure::new(format!("validation.password.{key}")));
                }
            }
        }
        failures
    }
}

impl From<Password> for Rule {
    fn from(rule: Password) -> Self {
        if let Some(error) = &rule.configuration_error {
            return Self::compilable(error.clone());
        }
        Self::new(Kind::Password(rule))
    }
}
impl IntoIterator for &Password {
    type Item = Rule;
    type IntoIter = std::vec::IntoIter<Rule>;
    fn into_iter(self) -> Self::IntoIter {
        let mut rules = Vec::new();
        if self.required {
            rules.push(Rule::required());
        }
        if self.sometimes {
            rules.push(Rule::sometimes());
        }
        rules.extend([Rule::string(), Rule::min(self.minimum as u64)]);
        if let Some(max) = self.maximum.filter(|max| *max != 0) {
            rules.push(Rule::max(max as u64));
        }
        rules.extend(self.custom_rules.iter().cloned());
        rules.into_iter()
    }
}
impl IntoIterator for Password {
    type Item = Rule;
    type IntoIter = std::vec::IntoIter<Rule>;
    fn into_iter(self) -> Self::IntoIter {
        (&self).into_iter()
    }
}
