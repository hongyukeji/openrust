// 第三方许可见 ../LICENSE.symfony 与 ../view/components/LICENSE。
use super::{Question, choice_key_width, question_default_label};
use crate::console::{
    AsOutput, ConsoleError, Input, OutputOptions, OutputStyle, view::components::TwoColumnDetail,
};
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// 提示和重试错误的同步扩展点；写入提示并返回格式化后的末尾提示，供原生行编辑重绘。
pub trait QuestionHelperBehavior: Send + Sync {
    fn get_name(&self) -> &str {
        "question"
    }
    fn ensure_ends_with_punctuation(&self, value: &str) -> String {
        if matches!(value.as_bytes().last(), Some(b'?' | b':' | b'!' | b'.')) {
            value.to_owned()
        } else {
            format!("{value}:")
        }
    }
    fn write_prompt(
        &self,
        output: &mut OutputStyle<'_>,
        question: &Question,
    ) -> Result<String, ConsoleError> {
        component_prompt(self, output, question)
    }
    /// styled_output对应原调用者的样式类型；直接借入的普通流不使用错误块。
    fn write_error(
        &self,
        output: &mut OutputStyle<'_>,
        error: &ConsoleError,
        styled_output: bool,
    ) -> Result<(), ConsoleError> {
        if styled_output {
            output.question_validation_error(&error.to_string())
        } else {
            output.writeln(
                format!("<error>{error}</error>").as_str(),
                OutputOptions::default(),
            )
        }
    }
}
#[derive(Default, Clone, Debug)]
#[non_exhaustive]
pub struct QuestionHelper;
impl QuestionHelperBehavior for QuestionHelper {}
#[derive(Default, Clone, Debug)]
#[non_exhaustive]
pub struct DefaultQuestionHelper;
impl QuestionHelperBehavior for DefaultQuestionHelper {
    fn write_prompt(
        &self,
        output: &mut OutputStyle<'_>,
        question: &Question,
    ) -> Result<String, ConsoleError> {
        default_prompt(output, question)
    }
}
macro_rules! helper_api {
    ($name:ident) => {
        impl $name {
            pub const fn new() -> Self {
                Self
            }
            pub const fn get_name(&self) -> &'static str {
                "question"
            }
            /// 原生输入从不调用stty；沿用已确认的原生终端与补全机制。
            pub fn disable_stty() {}
            pub async fn ask(
                &self,
                input: &Input,
                output: &mut (impl AsOutput + ?Sized),
                question: &Question,
            ) -> Result<serde_json::Value, ConsoleError> {
                let mut selected = output.as_output();
                let styled = !selected.parents.is_empty();
                let mut style = OutputStyle::from_reference(input, selected.for_error_style());
                style
                    .ask_question_using(question, Arc::new(self.clone()), false, styled)
                    .await
            }
        }
    };
}
helper_api!(QuestionHelper);
helper_api!(DefaultQuestionHelper);
impl QuestionHelper {
    pub fn ensure_ends_with_punctuation(&self, value: &str) -> String {
        QuestionHelperBehavior::ensure_ends_with_punctuation(self, value)
    }
}
/// 问答组件借用的样式契约；沿用构造时的原输入与帮助器，不复制流位置。
pub trait QuestionStyle: AsOutput + Send {
    fn ask_question<'a>(
        &'a mut self,
        question: &'a Question,
    ) -> crate::console::ConsoleFuture<'a, Result<serde_json::Value, ConsoleError>>;
    fn get_question_helper(&self) -> Option<Arc<dyn QuestionHelperBehavior>>;
    fn set_question_helper(&mut self, helper: Option<Arc<dyn QuestionHelperBehavior>>);
}
/// 临时替换同一样式的帮助器；错误、panic或Future取消时恢复原实例。
pub struct QuestionHelperScope<'scope, T: QuestionStyle + ?Sized> {
    style: &'scope mut T,
    previous: Option<Arc<dyn QuestionHelperBehavior>>,
}
impl<'scope, T: QuestionStyle + ?Sized> QuestionHelperScope<'scope, T> {
    pub fn new(style: &'scope mut T, helper: Arc<dyn QuestionHelperBehavior>) -> Self {
        let previous = style
            .get_question_helper()
            .unwrap_or_else(|| Arc::new(DefaultQuestionHelper));
        style.set_question_helper(Some(helper));
        Self {
            style,
            previous: Some(previous),
        }
    }
}
impl<T: QuestionStyle + ?Sized> Deref for QuestionHelperScope<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.style
    }
}
impl<T: QuestionStyle + ?Sized> DerefMut for QuestionHelperScope<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.style
    }
}
impl<T: QuestionStyle + ?Sized> Drop for QuestionHelperScope<'_, T> {
    fn drop(&mut self) {
        self.style.set_question_helper(self.previous.take());
    }
}
impl<'output> OutputStyle<'output> {
    pub fn get_question_helper(&self) -> Option<Arc<dyn QuestionHelperBehavior>> {
        self.question_helper.clone()
    }
    pub fn set_question_helper(&mut self, helper: Option<Arc<dyn QuestionHelperBehavior>>) {
        self.question_helper = helper;
    }
    pub fn using_question_helper(
        &mut self,
        helper: Arc<dyn QuestionHelperBehavior>,
    ) -> QuestionHelperScope<'_, Self> {
        QuestionHelperScope::new(self, helper)
    }
}
impl QuestionStyle for OutputStyle<'_> {
    fn ask_question<'a>(
        &'a mut self,
        question: &'a Question,
    ) -> crate::console::ConsoleFuture<'a, Result<serde_json::Value, ConsoleError>> {
        Box::pin(OutputStyle::ask_question(self, question))
    }
    fn get_question_helper(&self) -> Option<Arc<dyn QuestionHelperBehavior>> {
        OutputStyle::get_question_helper(self)
    }
    fn set_question_helper(&mut self, helper: Option<Arc<dyn QuestionHelperBehavior>>) {
        OutputStyle::set_question_helper(self, helper)
    }
}
fn multiline_hint() -> String {
    let keys = if cfg!(windows) {
        "<comment>Ctrl+Z</comment> → <comment>Enter</comment>"
    } else {
        "<comment>Ctrl+D</comment>"
    };
    crate::console::localization::text("question.multiline", "按 :keys 继续").replace(":keys", keys)
}
fn default_prompt(
    style: &mut OutputStyle<'_>,
    question: &Question,
) -> Result<String, ConsoleError> {
    let label = crate::console::formatting::Formatter::escape_trailing_backslash(&question.label);
    let label = if question.multiline {
        format!("{label} ({})", multiline_hint())
    } else {
        label
    };
    let label = if question.confirmation {
        format!(
            " <info>{label} (yes/no)</info> [<comment>{}</comment>]:",
            if question
                .default
                .as_ref()
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                "yes"
            } else {
                "no"
            }
        )
    } else if let Some(value) = question.default.as_ref().filter(|v| !v.is_null()) {
        let value = question_default_label(question, value);
        format!(
            " <info>{label}</info> [<comment>{}</comment>]:",
            crate::console::formatting::Formatter::escape(&value)
        )
    } else {
        format!(" <info>{label}</info>:")
    };
    style.writeln(label.as_str(), OutputOptions::default())?;
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
                OutputOptions::default(),
            )?;
        }
    }
    let prompt = question
        .choice
        .as_ref()
        .map_or(" > ", |choice| choice.prompt.as_str());
    write_suffix(style, prompt)
}
fn component_prompt<H: QuestionHelperBehavior + ?Sized>(
    helper: &H,
    style: &mut OutputStyle<'_>,
    question: &Question,
) -> Result<String, ConsoleError> {
    let text = crate::console::formatting::Formatter::escape_trailing_backslash(&question.label);
    let text = helper.ensure_ends_with_punctuation(&text);
    let mut text = format!("  <fg=default;options=bold>{text}</></>");
    if question.multiline {
        text.push_str(&format!(" ({})", multiline_hint()));
    }
    let text = match question.default.as_ref() {
        None => format!("<info>{text}</info>"),
        Some(value) if question.confirmation => format!(
            "<info>{text} (yes/no)</info> [<comment>{}</comment>]",
            if value.as_bool().unwrap_or(false) {
                "yes"
            } else {
                "no"
            }
        ),
        Some(value) => {
            let value = question
                .choice
                .as_ref()
                .and_then(|c| c.lookup(value))
                .unwrap_or(value);
            let value = crate::support::input_scalars::string(value.clone());
            format!(
                "<info>{text}</info> [<comment>{}</comment>]",
                crate::console::formatting::Formatter::escape(&value)
            )
        }
    };
    style.writeln(text.as_str(), OutputOptions::default())?;
    if let Some(choice) = &question.choice {
        for (key, value) in choice.entries.iter() {
            let value = crate::support::input_scalars::string(value.clone());
            let key = key.to_string();
            TwoColumnDetail::new(style).render(&value, Some(&key))?;
        }
    }
    let prompt = "<options=bold>❯ </>";
    write_suffix(style, prompt)
}

fn write_suffix(style: &mut OutputStyle<'_>, prompt: &str) -> Result<String, ConsoleError> {
    let output = style.get_output();
    let rendered = output
        .formatter()
        .clone()
        .format(prompt, output.is_decorated())
        .map_err(ConsoleError::from_source)?;
    style.write(prompt, false, OutputOptions::default())?;
    Ok(rendered)
}

crate::console::helper::impl_helper_tools!(QuestionHelper);
crate::console::helper::impl_helper_tools!(DefaultQuestionHelper);
