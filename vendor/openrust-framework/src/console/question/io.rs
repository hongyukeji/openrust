use super::super::{CommandIo, ConsoleError, OutputStyle};
use super::{
    ChoiceKey, ChoiceQuestion, CompletionCallback, ConfirmationQuestion, Question, QuestionCallback,
};
use serde_json::Value;

impl OutputStyle<'_> {
    pub async fn ask(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        self.ask_with(label, None).await
    }
    pub async fn ask_with(
        &mut self,
        label: &str,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        let question = Question::new(label, default.map(|text| Value::String(text.to_owned())))?;
        let answer = self.ask_question(&question).await?;
        Ok(answer.as_str().map(str::to_owned))
    }
    pub async fn confirm(&mut self, label: &str) -> Result<bool, ConsoleError> {
        self.confirm_with(label, true).await
    }
    pub async fn confirm_with(&mut self, label: &str, default: bool) -> Result<bool, ConsoleError> {
        let question = ConfirmationQuestion::new(label, default, "/^y/i")?;
        self.ask_question(question.as_question())
            .await?
            .as_bool()
            .ok_or_else(|| super::configuration("确认问题必须返回布尔值"))
    }
    /// 带校验器的样式问答保留校验器返回值，不强制转换成字符串。
    pub async fn ask_with_validator(
        &mut self,
        label: &str,
        default: Option<&str>,
        validator: Option<QuestionCallback>,
    ) -> Result<Value, ConsoleError> {
        let mut question =
            Question::new(label, default.map(|text| Value::String(text.to_owned())))?;
        question.set_validator(validator);
        self.ask_question(&question).await
    }
    pub async fn ask_hidden(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        let answer = self.ask_hidden_with(label, None).await?;
        Ok(answer.as_str().map(str::to_owned))
    }
    pub async fn ask_hidden_with(
        &mut self,
        label: &str,
        validator: Option<QuestionCallback>,
    ) -> Result<Value, ConsoleError> {
        let mut question = Question::new(label, None)?;
        question.set_hidden(true)?.set_validator(validator);
        self.ask_question(&question).await
    }
    pub async fn ask_with_completion(
        &mut self,
        label: &str,
        choices: CompletionCallback,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        let mut question =
            Question::new(label, default.map(|text| Value::String(text.to_owned())))?;
        question.set_autocompleter_callback(Some(choices))?;
        let answer = self.ask_question(&question).await?;
        Ok(answer.as_str().map(str::to_owned))
    }
}
impl CommandIo<'_> {
    pub async fn ask(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        self.get_output().ask(label).await
    }
    pub async fn ask_with(
        &mut self,
        label: &str,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        self.get_output().ask_with(label, default).await
    }
    pub async fn confirm(&mut self, label: &str) -> Result<bool, ConsoleError> {
        self.confirm_with(label, false).await
    }
    pub async fn confirm_with(&mut self, label: &str, default: bool) -> Result<bool, ConsoleError> {
        self.get_output().confirm_with(label, default).await
    }
    pub async fn secret(&mut self, label: &str) -> Result<Option<String>, ConsoleError> {
        self.secret_with(label, true).await
    }
    pub async fn secret_with(
        &mut self,
        label: &str,
        fallback: bool,
    ) -> Result<Option<String>, ConsoleError> {
        let mut question = Question::new(label, None)?;
        question.set_hidden(true)?.set_hidden_fallback(fallback);
        let answer = self.get_output().ask_question(&question).await?;
        Ok(answer.as_str().map(str::to_owned))
    }
    pub async fn ask_with_completion(
        &mut self,
        label: &str,
        choices: CompletionCallback,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        self.get_output()
            .ask_with_completion(label, choices, default)
            .await
    }
    pub async fn anticipate(
        &mut self,
        label: &str,
        choices: CompletionCallback,
        default: Option<&str>,
    ) -> Result<Option<String>, ConsoleError> {
        self.ask_with_completion(label, choices, default).await
    }
}

impl OutputStyle<'_> {
    pub async fn choice<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
    ) -> Result<Value, ConsoleError> {
        self.choice_with(label, choices, None, false).await
    }
    pub async fn choice_with<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
        default: Option<Value>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut question = ChoiceQuestion::new(label, choices, default)?;
        question.remap_default_value();
        question.set_multiselect(multiple);
        self.ask_question(question.as_question()).await
    }
    pub async fn choice_pairs_with<K: Into<ChoiceKey>, V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut question = ChoiceQuestion::from_pairs(label, choices, default)?;
        question.remap_default_value();
        question.set_multiselect(multiple);
        self.ask_question(question.as_question()).await
    }
}
impl CommandIo<'_> {
    pub async fn choice<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
    ) -> Result<Value, ConsoleError> {
        self.choice_with(label, choices, None, None, false).await
    }
    pub async fn choice_with<T: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = T>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut question = ChoiceQuestion::new(label, choices, default)?;
        question.set_multiselect(multiple);
        question.set_max_attempts(attempts)?;
        self.get_output().ask_question(question.as_question()).await
    }
    pub async fn choice_pairs_with<K: Into<ChoiceKey>, V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut question = ChoiceQuestion::from_pairs(label, choices, default)?;
        question.set_multiselect(multiple);
        question.set_max_attempts(attempts)?;
        self.get_output().ask_question(question.as_question()).await
    }
}
