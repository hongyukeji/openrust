use crate::validation::{
    Rule, RuleFailure, Value,
    rule::{Kind, is_empty},
};

/// A fluent password policy. The default policy requires eight characters.
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
}

impl Default for Password {
    fn default() -> Self {
        Self::min(8)
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Password rule API")]
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
        }
    }
    pub fn max(mut self, max: usize) -> Self {
        self.maximum = (max != 0).then_some(max);
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
    pub fn mixedCase(mut self) -> Self {
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
    pub(in crate::validation) fn failures(
        &self,
        value: Option<&Value>,
        nullable: bool,
    ) -> Vec<RuleFailure> {
        if value.is_none() && (!self.required || self.sometimes) {
            return Vec::new();
        }
        let blank = value.is_none_or(|value| match value {
            Value::Null => true,
            Value::String(value) => value
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .is_empty(),
            Value::Array(value) => value.is_empty(),
            Value::Object(value) => value.is_empty(),
            _ => false,
        });
        if blank && !self.required && nullable {
            return Vec::new();
        }
        let mut failures = Vec::new();
        if self.required && value.is_none_or(is_empty) {
            failures.push(RuleFailure::new("validation.required"));
        } else if let Some(value) = value {
            // The embedded string/min/max rules use ordinary non-implicit validation.
            if !value.as_str().is_some_and(|value| {
                value
                    .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                    .is_empty()
            }) {
                if !value.is_string() {
                    failures.push(RuleFailure::new("validation.string"));
                }
                failures.extend(password_size(value, "min", self.minimum));
                if let Some(max) = self.maximum {
                    failures.extend(password_size(value, "max", max));
                }
            }
        }
        if let Some(value) = value.and_then(Value::as_str) {
            use std::sync::LazyLock;
            static MIXED: LazyLock<regex::Regex> = LazyLock::new(|| {
                regex::Regex::new(r"(\p{Ll}+.*\p{Lu})|(\p{Lu}+.*\p{Ll})").expect("constant regex")
            });
            static LETTERS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"\pL").expect("constant regex"));
            static NUMBERS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"\pN").expect("constant regex"));
            static SYMBOLS: LazyLock<regex::Regex> =
                LazyLock::new(|| regex::Regex::new(r"[\pZ\pS\pP]").expect("constant regex"));
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
        Self::new(Kind::Password(rule))
    }
}

fn password_size(value: &Value, rule: &str, bound: usize) -> Option<RuleFailure> {
    let size = match value {
        Value::Null | Value::Bool(false) => 0,
        Value::Bool(true) => 1,
        Value::Number(number) if number.is_f64() => {
            crate::support::php_float_string(number.as_f64().expect("float")).len()
        }
        Value::Number(number) => number.to_string().len(),
        Value::String(value) => value.chars().count(),
        Value::Array(value) => value.len(),
        Value::Object(value) => value.len(),
    };
    if if rule == "min" {
        size >= bound
    } else {
        size <= bound
    } {
        return None;
    }
    let mut failure = RuleFailure::new(format!("validation.{rule}.string"));
    failure.replacements.insert(rule.into(), bound.to_string());
    Some(failure)
}
