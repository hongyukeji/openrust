use super::{ConsoleError, ConsoleFuture};
use serde_json::Value;
use std::{fmt, sync::Arc, time::Duration};

mod choice;
mod error_format;
mod input;
pub use choice::{ChoiceKey, ChoiceLabel, ChoiceQuestion};
mod io;
mod reader;
pub(super) use input::Interaction;
pub use input::{ConsoleInputStream, ConsoleReader};

/// 回答的规范化或校验回调，在当前命令的异步作用域内运行。
#[derive(Clone)]
pub struct QuestionCallback(
    Arc<dyn Fn(Value) -> ConsoleFuture<'static, Result<Value, ConsoleError>> + Send + Sync>,
);
impl QuestionCallback {
    pub fn new(
        callback: impl Fn(Value) -> ConsoleFuture<'static, Result<Value, ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }
    pub fn sync(
        callback: impl Fn(Value) -> Result<Value, ConsoleError> + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |value| {
            let result = callback(value);
            Box::pin(async move { result })
        })
    }
    pub async fn call(&self, value: Value) -> Result<Value, ConsoleError> {
        (self.0)(value).await
    }
}
type Complete =
    dyn Fn(String) -> ConsoleFuture<'static, Result<Vec<String>, ConsoleError>> + Send + Sync;

/// 补全回调接收当前输入，异步查询仍使用当前应用容器。
#[derive(Clone)]
pub struct CompletionCallback(Arc<Complete>);
impl CompletionCallback {
    pub fn new(
        callback: impl Fn(String) -> ConsoleFuture<'static, Result<Vec<String>, ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }
    pub fn values(values: Vec<String>) -> Self {
        let values = Arc::new(values);
        Self::new(move |_| {
            let values = values.clone();
            Box::pin(async move { Ok((*values).clone()) })
        })
    }
    /// 单次迭代器在首次补全时才消费，后续读取共享已缓存结果。
    pub fn iter<I>(values: I) -> Self
    where
        I: IntoIterator<Item = String>,
        I::IntoIter: Send + 'static,
    {
        let pending = Arc::new(tokio::sync::Mutex::new(Some(values.into_iter())));
        let cache = Arc::new(tokio::sync::OnceCell::<Vec<String>>::new());
        Self::new(move |_| {
            let pending = pending.clone();
            let cache = cache.clone();
            Box::pin(async move {
                let values = cache
                    .get_or_init(|| async {
                        pending
                            .lock()
                            .await
                            .take()
                            .map(Iterator::collect)
                            .unwrap_or_default()
                    })
                    .await;
                Ok(values.clone())
            })
        })
    }
    /// 字符串键映射同时提供键和值，保持声明顺序及重复项。
    pub fn pairs(values: Vec<(String, String)>) -> Self {
        Self::values(
            values
                .iter()
                .map(|(key, _)| key.clone())
                .chain(values.iter().map(|(_, value)| value.clone()))
                .collect(),
        )
    }
    pub fn sync(
        callback: impl Fn(String) -> Result<Vec<String>, ConsoleError> + Send + Sync + 'static,
    ) -> Self {
        Self::new(move |value| {
            let result = callback(value);
            Box::pin(async move { result })
        })
    }
    pub async fn call(&self, input: String) -> Result<Vec<String>, ConsoleError> {
        (self.0)(input).await
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleQuestionError {
    MissingInput,
    Interrupted,
    TimedOut,
    HiddenUnavailable,
    InvalidConfiguration(String),
    InvalidAnswer(String),
    InvalidEncoding(std::string::FromUtf8Error),
}
impl fmt::Display for ConsoleQuestionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingInput => f.write_str("没有可读取的输入"),
            Self::Interrupted => f.write_str("输入已中断"),
            Self::TimedOut => f.write_str("等待输入超时"),
            Self::HiddenUnavailable => f.write_str("当前输入无法隐藏，且已禁止明文回退"),
            Self::InvalidEncoding(error) => write!(f, "错误模板生成了非 UTF-8 文本：{error}"),
            Self::InvalidConfiguration(message) | Self::InvalidAnswer(message) => {
                f.write_str(message)
            }
        }
    }
}
impl std::error::Error for ConsoleQuestionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidEncoding(error) => Some(error),
            _ => None,
        }
    }
}
fn configuration(message: &str) -> ConsoleError {
    ConsoleError::from_source(ConsoleQuestionError::InvalidConfiguration(
        message.to_owned(),
    ))
}

/// 一次问答的定义；输入流由 Input 持有，回调和显示不建立第二套命令容器。
pub struct Question {
    label: String,
    default: Option<Value>,
    hidden: bool,
    hidden_fallback: bool,
    completion: Option<CompletionCallback>,
    validator: Option<QuestionCallback>,
    normalizer: Option<QuestionCallback>,
    trimmable: bool,
    multiline: bool,
    timeout: Option<Duration>,
    attempts: Option<usize>,
    confirmation: bool,
    choice: Option<choice::ChoiceState>,
}
impl Clone for Question {
    fn clone(&self) -> Self {
        let mut question = Self {
            label: self.label.clone(),
            default: self.default.clone(),
            hidden: self.hidden,
            hidden_fallback: self.hidden_fallback,
            completion: self.completion.clone(),
            validator: self.validator.clone(),
            normalizer: self.normalizer.clone(),
            trimmable: self.trimmable,
            multiline: self.multiline,
            timeout: self.timeout,
            attempts: self.attempts,
            confirmation: self.confirmation,
            choice: self
                .choice
                .as_ref()
                .map(choice::ChoiceState::clone_for_question),
        };
        if question
            .choice
            .as_ref()
            .is_some_and(|choice| choice.default_validator)
        {
            question.install_choice_validator();
        }
        question
    }
}
impl Question {
    pub fn new(label: impl Into<String>, default: Option<Value>) -> Result<Self, ConsoleError> {
        if default
            .as_ref()
            .is_some_and(|value| value.is_array() || value.is_object())
        {
            return Err(configuration("问题默认值必须为字符串、布尔、数字或 null"));
        }
        Ok(Self {
            label: label.into(),
            default: default.filter(|value| !value.is_null()),
            hidden: false,
            hidden_fallback: true,
            completion: None,
            validator: None,
            normalizer: None,
            trimmable: true,
            multiline: false,
            timeout: None,
            attempts: None,
            confirmation: false,
            choice: None,
        })
    }
    pub fn get_question(&self) -> &str {
        &self.label
    }
    pub fn get_default(&self) -> Option<&Value> {
        self.default.as_ref()
    }
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }
    pub fn set_multiline(&mut self, value: bool) -> &mut Self {
        self.multiline = value;
        self
    }
    pub fn get_timeout(&self) -> Option<Duration> {
        self.timeout
    }
    pub fn set_timeout(&mut self, timeout: Option<Duration>) -> &mut Self {
        self.timeout = timeout;
        self
    }
    pub fn is_hidden(&self) -> bool {
        self.hidden
    }
    pub fn set_hidden(&mut self, value: bool) -> Result<&mut Self, ConsoleError> {
        if self.completion.is_some() {
            return Err(configuration("隐藏问题不能同时使用自动补全"));
        }
        self.hidden = value;
        Ok(self)
    }
    pub fn is_hidden_fallback(&self) -> bool {
        self.hidden_fallback
    }
    pub fn set_hidden_fallback(&mut self, value: bool) -> &mut Self {
        self.hidden_fallback = value;
        self
    }
    pub async fn get_autocompleter_values(&self) -> Result<Option<Vec<String>>, ConsoleError> {
        match &self.completion {
            Some(callback) => callback.call(String::new()).await.map(Some),
            None => Ok(None),
        }
    }
    pub fn set_autocompleter_values(
        &mut self,
        values: Option<Vec<String>>,
    ) -> Result<&mut Self, ConsoleError> {
        self.set_autocompleter_callback(values.map(CompletionCallback::values))
    }
    pub fn set_autocompleter_iter<I>(&mut self, values: I) -> Result<&mut Self, ConsoleError>
    where
        I: IntoIterator<Item = String>,
        I::IntoIter: Send + 'static,
    {
        self.set_autocompleter_callback(Some(CompletionCallback::iter(values)))
    }
    pub fn set_autocompleter_pairs(
        &mut self,
        values: Vec<(String, String)>,
    ) -> Result<&mut Self, ConsoleError> {
        self.set_autocompleter_callback(Some(CompletionCallback::pairs(values)))
    }
    pub fn get_autocompleter_callback(&self) -> Option<&CompletionCallback> {
        self.completion.as_ref()
    }
    pub fn set_autocompleter_callback(
        &mut self,
        callback: Option<CompletionCallback>,
    ) -> Result<&mut Self, ConsoleError> {
        if self.hidden && callback.is_some() {
            return Err(configuration("隐藏问题不能同时使用自动补全"));
        }
        self.completion = callback;
        Ok(self)
    }
    pub fn set_validator(&mut self, callback: Option<QuestionCallback>) -> &mut Self {
        if let Some(choice) = &mut self.choice {
            choice.default_validator = false;
        }
        self.validator = callback;
        self
    }
    pub fn get_validator(&self) -> Option<&QuestionCallback> {
        self.validator.as_ref()
    }
    pub fn set_max_attempts(&mut self, attempts: Option<usize>) -> Result<&mut Self, ConsoleError> {
        if attempts == Some(0) {
            return Err(configuration("最大尝试次数必须为正整数"));
        }
        self.attempts = attempts;
        Ok(self)
    }
    pub fn get_max_attempts(&self) -> Option<usize> {
        self.attempts
    }
    pub fn set_normalizer(&mut self, callback: QuestionCallback) -> &mut Self {
        self.normalizer = Some(callback);
        self
    }
    pub fn get_normalizer(&self) -> Option<&QuestionCallback> {
        self.normalizer.as_ref()
    }
    pub fn is_trimmable(&self) -> bool {
        self.trimmable
    }
    pub fn set_trimmable(&mut self, value: bool) -> &mut Self {
        self.trimmable = value;
        if let Some(choice) = &self.choice {
            choice
                .trimmable
                .store(value, std::sync::atomic::Ordering::Relaxed);
        }
        self
    }
    async fn default_answer(&self) -> Result<Value, ConsoleError> {
        let default = self.default.clone().unwrap_or(Value::Null);
        if default.is_null() {
            return Ok(default);
        }
        if let Some(validator) = &self.validator {
            validator.call(default).await
        } else if let Some(choice) = &self.choice {
            Ok(choice.unchecked_default(default, self.trimmable))
        } else {
            Ok(default)
        }
    }
}

pub struct ConfirmationQuestion(Question);
impl ConfirmationQuestion {
    pub fn new(
        label: impl Into<String>,
        default: bool,
        true_answer_regex: &str,
    ) -> Result<Self, ConsoleError> {
        let regex = crate::validation::compile_regex(true_answer_regex)
            .map_err(ConsoleError::from_source)?;
        let mut question = Question::new(label, Some(Value::Bool(default)))?;
        question.confirmation = true;
        question.set_normalizer(QuestionCallback::sync(move |value| {
            if value.is_boolean() {
                return Ok(value);
            }
            if value.is_array() || value.is_object() {
                return Err(configuration("确认答案必须为标量值"));
            }
            let text = crate::support::input_scalars::string(value);
            Ok(Value::Bool(
                (default && text.is_empty())
                    || ((!text.is_empty() && (default || text != "0"))
                        && regex
                            .is_match(text.as_bytes())
                            .map_err(ConsoleError::from_source)?),
            ))
        }));
        Ok(Self(question))
    }
    pub fn as_question(&self) -> &Question {
        &self.0
    }
    pub fn as_question_mut(&mut self) -> &mut Question {
        &mut self.0
    }
}
impl From<ConfirmationQuestion> for Question {
    fn from(question: ConfirmationQuestion) -> Self {
        question.0
    }
}

pub(super) async fn ask(
    style: &mut super::OutputStyle<'_>,
    question: &Question,
) -> Result<Value, ConsoleError> {
    if !style.question_input().is_interactive() {
        return question.default_answer().await;
    }
    let mut attempt = 0;
    loop {
        tokio::task::yield_now().await;
        let mut session = reader::prepare(style, question).await?;
        style.prepare_question()?;
        let label = super::formatting::Formatter::escape_trailing_backslash(&question.label);
        let label = if question.multiline {
            let keys = if cfg!(windows) {
                "<comment>Ctrl+Z</comment> → <comment>Enter</comment>"
            } else {
                "<comment>Ctrl+D</comment>"
            };
            let hint = super::localization::text("question.multiline", "按 :keys 继续")
                .replace(":keys", keys);
            format!("{label} ({hint})")
        } else {
            label
        };
        let label = if question.confirmation {
            format!(
                " <info>{label} (yes/no)</info> [<comment>{}</comment>]:",
                if question
                    .default
                    .as_ref()
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    "yes"
                } else {
                    "no"
                }
            )
        } else if let Some(value) = question.default.as_ref().filter(|value| !value.is_null()) {
            let value = question_default_label(question, value);
            format!(
                " <info>{label}</info> [<comment>{}</comment>]:",
                super::formatting::Formatter::escape(&value)
            )
        } else {
            format!(" <info>{label}</info>:")
        };
        style.writeln(label.as_str(), super::OutputOptions::default())?;
        if let Some(choice) = &question.choice {
            let width = choice
                .entries
                .iter()
                .map(|(key, _)| choice_key_width(&key.to_string()))
                .max()
                .unwrap_or(0);
            for (key, value) in choice.entries.iter() {
                let key = key.to_string();
                let padding = " ".repeat(width.saturating_sub(choice_key_width(&key)));
                style.writeln(
                    format!(
                        "  [<comment>{key}{padding}</comment>] {}",
                        crate::support::input_scalars::string(value.clone())
                    )
                    .as_str(),
                    super::OutputOptions::default(),
                )?;
            }
        }
        let prompt = question
            .choice
            .as_ref()
            .map_or(" > ", |choice| choice.prompt.as_str());
        let output = style.get_output();
        let rendered_prompt = output
            .formatter
            .clone()
            .format(prompt, output.is_decorated())
            .map_err(ConsoleError::from_source)?;
        style.write(prompt, false, super::OutputOptions::default())?;
        let shutdown = style.question_input().shutdown().cloned();
        let timeout = question
            .timeout
            .map(|duration| {
                tokio::time::Instant::now()
                    .checked_add(duration)
                    .ok_or_else(|| configuration("问题超时超出当前计时器范围"))
            })
            .transpose()?;
        let result = {
            let read = reader::read(style, question, &mut session, &rendered_prompt);
            tokio::select! {
                result=read => result,
                ()=async { match timeout { Some(deadline)=>tokio::time::sleep_until(deadline).await, None=>std::future::pending().await } } => Err(ConsoleError::from_source(ConsoleQuestionError::TimedOut)),
                ()=async { match shutdown { Some(shutdown)=>shutdown.cancelled().await, None=>std::future::pending().await } } => Err(ConsoleError::from_source(ConsoleQuestionError::Interrupted)),
            }
        };
        drop(session);
        let mut answer = match result {
            Ok(Some(answer)) => answer,
            Ok(None) => {
                return exhausted(
                    style,
                    question,
                    ConsoleError::from_source(ConsoleQuestionError::MissingInput),
                )
                .await;
            }
            Err(error) => {
                if std::error::Error::source(&error).is_some_and(|source| {
                    source.downcast_ref::<ConsoleQuestionError>()
                        == Some(&ConsoleQuestionError::TimedOut)
                }) {
                    return exhausted(style, question, error).await;
                }
                return Err(error);
            }
        };
        if question.hidden {
            style.writeln("", super::OutputOptions::default())?;
        }
        if question.trimmable {
            answer = answer
                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                .to_owned();
        }
        let answer = if answer.is_empty() {
            question.default.clone().unwrap_or(Value::Null)
        } else {
            Value::String(answer)
        };
        let answer = if let Some(normalizer) = &question.normalizer {
            normalizer.call(answer).await
        } else {
            Ok(answer)
        };
        let answer = match answer {
            Ok(answer) => {
                if let Some(validator) = &question.validator {
                    validator.call(answer).await
                } else {
                    Ok(answer)
                }
            }
            Err(error) => Err(error),
        };
        match answer {
            Ok(answer) => {
                if style.question_input().is_interactive() {
                    style.new_line(1)?;
                }
                return Ok(answer);
            }
            Err(error) => {
                if error.is_runtime() || error.dump_output().is_some() {
                    return Err(error);
                }
                if std::error::Error::source(&error).is_some_and(|source| {
                    matches!(
                        source.downcast_ref::<ConsoleQuestionError>(),
                        Some(ConsoleQuestionError::MissingInput | ConsoleQuestionError::TimedOut)
                    )
                }) {
                    return exhausted(style, question, error).await;
                }
                if error.command_exit_code() == 130
                    || std::error::Error::source(&error).is_some_and(|source| {
                        matches!(
                            source.downcast_ref::<ConsoleQuestionError>(),
                            Some(
                                ConsoleQuestionError::InvalidConfiguration(_)
                                    | ConsoleQuestionError::InvalidEncoding(_)
                                    | ConsoleQuestionError::HiddenUnavailable
                            )
                        )
                    })
                {
                    return Err(error);
                }
                attempt += 1;
                if question.validator.is_none()
                    || question.attempts.is_some_and(|max| attempt >= max)
                {
                    return Err(error);
                }
                style.question_validation_error(&error.to_string())?;
            }
        }
    }
}
async fn exhausted(
    style: &mut super::OutputStyle<'_>,
    question: &Question,
    error: ConsoleError,
) -> Result<Value, ConsoleError> {
    style.question_input().mark_input_exhausted();
    let answer = question.default_answer().await?;
    if answer.is_null() {
        Err(error)
    } else {
        Ok(answer)
    }
}

impl std::ops::Deref for ConfirmationQuestion {
    type Target = Question;
    fn deref(&self) -> &Question {
        &self.0
    }
}
impl std::ops::DerefMut for ConfirmationQuestion {
    fn deref_mut(&mut self) -> &mut Question {
        &mut self.0
    }
}

fn question_default_label(question: &Question, value: &Value) -> String {
    question.choice.as_ref().map_or_else(
        || crate::support::input_scalars::string(value.clone()),
        |choice| choice.default_label(value),
    )
}

fn choice_key_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                1
            } else {
                let mut bytes = [0; 4];
                ::console::measure_text_width(character.encode_utf8(&mut bytes))
            }
        })
        .sum()
}
