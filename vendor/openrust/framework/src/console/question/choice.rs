use super::{ConsoleError, ConsoleQuestionError, Question, QuestionCallback, configuration};
use serde_json::Value;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// 显式将 Display 标签保存为问答文本快照；关联选择仍返回原声明键。
pub struct ChoiceLabel<T>(pub T);
impl<T: fmt::Display> From<ChoiceLabel<T>> for Value {
    fn from(label: ChoiceLabel<T>) -> Self {
        Value::String(label.0.to_string())
    }
}

/// 保留数字索引与字符串关联键；选择结果不会按声明位置重新编号。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceKey {
    Integer(i64),
    String(String),
}
impl fmt::Display for ChoiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Integer(value) => value.fmt(f),
            Self::String(value) => value.fmt(f),
        }
    }
}
impl From<i64> for ChoiceKey {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}
impl From<String> for ChoiceKey {
    fn from(value: String) -> Self {
        match value.parse::<i64>() {
            Ok(number) if number.to_string() == value => Self::Integer(number),
            _ => Self::String(value),
        }
    }
}
impl From<&str> for ChoiceKey {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl ChoiceKey {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::String(value) => value.clone().into(),
            Value::Null => Self::String(String::new()),
            _ => Self::Integer(crate::support::php_integer_cast(value)),
        }
    }
}
#[derive(Clone)]
pub(super) struct ChoiceState {
    pub entries: Arc<Vec<(ChoiceKey, Value)>>,
    pub multiple: bool,
    pub prompt: String,
    pub error: String,
    pub default_validator: bool,
    pub trimmable: Arc<AtomicBool>,
}
impl ChoiceState {
    pub fn clone_for_question(&self) -> Self {
        let mut state = self.clone();
        state.trimmable = Arc::new(AtomicBool::new(self.trimmable.load(Ordering::Relaxed)));
        state
    }
    fn invalid(&self, value: &Value) -> ConsoleError {
        match super::error_format::format(&self.error, value) {
            Ok(message) => ConsoleError::from_source(ConsoleQuestionError::InvalidAnswer(message)),
            Err(error) => error,
        }
    }
    fn associative(&self) -> bool {
        self.entries
            .iter()
            .any(|(key, _)| matches!(key, ChoiceKey::String(_)))
    }
    pub(super) fn lookup(&self, value: &Value) -> Option<&Value> {
        let key = ChoiceKey::from_value(value);
        self.entries
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, value)| value)
    }
    fn validate(&self, selected: Value, trim: bool) -> Result<Value, ConsoleError> {
        let selected = if self.multiple {
            let string = crate::support::input_scalars::string(selected.clone());
            if string.is_empty() || string.split(',').any(str::is_empty) {
                return Err(self.invalid(&selected));
            }
            string
                .split(',')
                .map(|value| Value::String(value.to_owned()))
                .collect::<Vec<_>>()
        } else {
            vec![selected]
        };
        let mut results = Vec::new();
        for mut value in selected {
            if trim {
                value = Value::String(
                    crate::support::input_scalars::string(value)
                        .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                        .to_owned(),
                );
            }
            let exact = self
                .entries
                .iter()
                .filter(|(_, choice)| *choice == value)
                .map(|(key, _)| key.to_string())
                .collect::<Vec<_>>();
            if exact.len() > 1 {
                let message = super::super::localization::text(
                    "question.choice_ambiguous",
                    "答案不唯一，请使用以下键之一：:choices",
                )
                .replace(":choices", &format!("\"{}\"", exact.join("\" or \"")));
                return Err(ConsoleError::from_source(
                    ConsoleQuestionError::InvalidAnswer(message),
                ));
            }
            let found = self
                .entries
                .iter()
                .find(|(_, choice)| crate::support::php_loose_value_equal(&value, choice));
            let result = if self.associative() {
                found
                    .map(|(key, _)| Value::String(key.to_string()))
                    .or_else(|| {
                        self.lookup(&value)
                            .map(|_| Value::String(ChoiceKey::from_value(&value).to_string()))
                    })
            } else {
                found
                    .map(|(_, choice)| choice.clone())
                    .or_else(|| self.lookup(&value).cloned())
            };
            match result {
                Some(Value::Bool(false)) | None => return Err(self.invalid(&value)),
                Some(result) => results.push(result),
            }
        }
        Ok(if self.multiple {
            Value::Array(results)
        } else {
            results.into_iter().next().unwrap_or(Value::Null)
        })
    }
    pub fn default_label(&self, default: &Value) -> String {
        if self.multiple {
            crate::support::input_scalars::string(default.clone())
                .split(',')
                .map(|part| {
                    self.lookup(&Value::String(
                        part.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                            .to_owned(),
                    ))
                    .cloned()
                    .map(crate::support::input_scalars::string)
                    .unwrap_or_default()
                })
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            crate::support::input_scalars::string(self.lookup(default).unwrap_or(default).clone())
        }
    }
    pub fn unchecked_default(&self, default: Value, trim: bool) -> Value {
        if self.multiple {
            Value::Array(
                crate::support::input_scalars::string(default)
                    .split(',')
                    .map(|value| {
                        let value = Value::String(
                            if trim {
                                value.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                            } else {
                                value
                            }
                            .to_owned(),
                        );
                        self.lookup(&value).cloned().unwrap_or(value)
                    })
                    .collect(),
            )
        } else {
            self.lookup(&default).cloned().unwrap_or(default)
        }
    }
}
impl Question {
    pub(super) fn install_choice_validator(&mut self) {
        let Some(state) = self.choice.as_mut() else {
            return;
        };
        state.default_validator = true;
        let state = state.clone();
        self.validator = Some(QuestionCallback::sync(move |value| {
            state.validate(value, state.trimmable.load(Ordering::Relaxed))
        }));
    }
}

/// 选择题复用同一问答、重试、补全、取消及输出管线。
#[derive(Clone)]
pub struct ChoiceQuestion(Question);
impl ChoiceQuestion {
    pub fn new<T: Into<Value>>(
        label: impl Into<String>,
        choices: impl IntoIterator<Item = T>,
        default: Option<Value>,
    ) -> Result<Self, ConsoleError> {
        let entries = choices
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Ok((
                    ChoiceKey::Integer(i64::try_from(index).map_err(ConsoleError::from_source)?),
                    value.into(),
                ))
            })
            .collect::<Result<Vec<_>, ConsoleError>>()?;
        Self::from_pairs(label, entries, default)
    }
    pub fn from_pairs<K: Into<ChoiceKey>, V: Into<Value>>(
        label: impl Into<String>,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
    ) -> Result<Self, ConsoleError> {
        let mut entries: Vec<(ChoiceKey, Value)> = Vec::new();
        for (key, value) in choices {
            let key = match key.into() {
                ChoiceKey::String(value) => value.into(),
                key => key,
            };
            let value = value.into();
            if value.is_array() || value.is_object() || value.is_null() {
                return Err(configuration("选择项必须为字符串、数字或布尔值"));
            }
            if let Some((_, existing)) = entries.iter_mut().find(|(candidate, _)| *candidate == key)
            {
                *existing = value;
            } else {
                entries.push((key, value));
            }
        }
        if entries.is_empty() {
            return Err(configuration("选择题至少需要一个选项"));
        }
        let mut question = Question::new(label, default)?;
        let associative = entries
            .iter()
            .any(|(key, _)| matches!(key, ChoiceKey::String(_)));
        let mut completion = Vec::new();
        if associative {
            completion.extend(entries.iter().map(|(key, _)| key.to_string()));
        }
        completion.extend(
            entries
                .iter()
                .map(|(_, value)| crate::support::input_scalars::string(value.clone())),
        );
        question.set_autocompleter_values(Some(completion))?;
        question.choice = Some(ChoiceState {
            entries: Arc::new(entries),
            multiple: false,
            prompt: " > ".into(),
            error: super::super::localization::text("question.choice_invalid", "选项“%s”无效"),
            default_validator: true,
            trimmable: Arc::new(AtomicBool::new(question.trimmable)),
        });
        question.install_choice_validator();
        Ok(Self(question))
    }
    pub(super) fn remap_default_value(&mut self) {
        let Some(default) = self.0.default.as_ref() else {
            return;
        };
        let key = ChoiceKey::from_value(default);
        let choice = self
            .get_choices()
            .iter()
            .rev()
            .find(|(_, value)| {
                (value.is_string() || value.as_i64().is_some())
                    && ChoiceKey::from_value(value) == key
            })
            .map(|(key, _)| match key {
                ChoiceKey::Integer(value) => Value::from(*value),
                ChoiceKey::String(value) => Value::String(value.clone()),
            });
        if let Some(default) = choice {
            self.0.default = Some(default);
        }
    }
    pub fn get_choices(&self) -> &[(ChoiceKey, Value)] {
        self.0
            .choice
            .as_ref()
            .expect("选择题包含定义")
            .entries
            .as_slice()
    }
    pub fn set_multiselect(&mut self, multiple: bool) -> &mut Self {
        self.0.choice.as_mut().expect("选择题包含定义").multiple = multiple;
        self.0.install_choice_validator();
        self
    }
    pub fn is_multiselect(&self) -> bool {
        self.0.choice.as_ref().expect("选择题包含定义").multiple
    }
    pub fn get_prompt(&self) -> &str {
        &self.0.choice.as_ref().expect("选择题包含定义").prompt
    }
    pub fn set_prompt(&mut self, prompt: impl Into<String>) -> &mut Self {
        self.0.choice.as_mut().expect("选择题包含定义").prompt = prompt.into();
        self
    }
    pub fn set_error_message(&mut self, message: impl Into<String>) -> &mut Self {
        self.0.choice.as_mut().expect("选择题包含定义").error = message.into();
        self.0.install_choice_validator();
        self
    }
    pub fn as_question(&self) -> &Question {
        &self.0
    }
    pub fn as_question_mut(&mut self) -> &mut Question {
        &mut self.0
    }
}
impl std::ops::Deref for ChoiceQuestion {
    type Target = Question;
    fn deref(&self) -> &Question {
        &self.0
    }
}
impl std::ops::DerefMut for ChoiceQuestion {
    fn deref_mut(&mut self) -> &mut Question {
        &mut self.0
    }
}
impl From<ChoiceQuestion> for Question {
    fn from(value: ChoiceQuestion) -> Self {
        value.0
    }
}
