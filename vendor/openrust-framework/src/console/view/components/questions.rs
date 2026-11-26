// 第三方许可见 LICENSE。
use crate::console::{
    ChoiceKey, ChoiceQuestion, CompletionCallback, ConfirmationQuestion, ConsoleError, Question,
    QuestionHelper, QuestionHelperScope, QuestionStyle,
};
use serde_json::Value;
use std::sync::Arc;
macro_rules! question_component {
    ($name:ident) => {
        pub struct $name<'a, T: QuestionStyle + ?Sized> {
            output: &'a mut T,
        }
        impl<'a, T: QuestionStyle + ?Sized> $name<'a, T> {
            pub fn new(output: &'a mut T) -> Self {
                Self { output }
            }
        }
    };
}
question_component!(Ask);
question_component!(AskWithCompletion);
question_component!(Choice);
question_component!(Confirm);
question_component!(Secret);
async fn ask(
    output: &mut (impl QuestionStyle + ?Sized),
    question: &Question,
) -> Result<Value, ConsoleError> {
    QuestionHelperScope::new(output, Arc::new(QuestionHelper::new()))
        .ask_question(question)
        .await
}
impl<T: QuestionStyle + ?Sized> Ask<'_, T> {
    pub async fn render(&mut self, label: &str) -> Result<Value, ConsoleError> {
        self.render_with(label, None, false).await
    }
    pub async fn render_with(
        &mut self,
        label: &str,
        default: Option<Value>,
        multiline: bool,
    ) -> Result<Value, ConsoleError> {
        let mut scope = QuestionHelperScope::new(self.output, Arc::new(QuestionHelper::new()));
        let mut question = Question::new(label, default)?;
        question.set_multiline(multiline);
        scope.ask_question(&question).await
    }
}
impl<T: QuestionStyle + ?Sized> AskWithCompletion<'_, T> {
    pub async fn render<S: Into<String>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = S>,
    ) -> Result<Value, ConsoleError> {
        self.render_with(label, choices, None).await
    }
    pub async fn render_with<S: Into<String>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = S>,
        default: Option<Value>,
    ) -> Result<Value, ConsoleError> {
        let mut question = Question::new(label, default)?;
        question.set_autocompleter_values(Some(choices.into_iter().map(Into::into).collect()))?;
        ask(self.output, &question).await
    }
    pub async fn render_with_callback(
        &mut self,
        label: &str,
        choices: CompletionCallback,
        default: Option<Value>,
    ) -> Result<Value, ConsoleError> {
        let mut question = Question::new(label, default)?;
        question.set_autocompleter_callback(Some(choices))?;
        ask(self.output, &question).await
    }
}
impl<T: QuestionStyle + ?Sized> Confirm<'_, T> {
    pub async fn render(&mut self, label: &str) -> Result<bool, ConsoleError> {
        self.render_with(label, false).await
    }
    pub async fn render_with(&mut self, label: &str, default: bool) -> Result<bool, ConsoleError> {
        let question = ConfirmationQuestion::new(label, default, "/^y/i")?;
        ask(self.output, question.as_question())
            .await?
            .as_bool()
            .ok_or_else(|| ConsoleError::new("确认问题必须返回布尔值"))
    }
}
impl<T: QuestionStyle + ?Sized> Secret<'_, T> {
    pub async fn render(&mut self, label: &str) -> Result<Value, ConsoleError> {
        self.render_with(label, true).await
    }
    pub async fn render_with(
        &mut self,
        label: &str,
        fallback: bool,
    ) -> Result<Value, ConsoleError> {
        let mut question = Question::new(label, None)?;
        question.set_hidden(true)?.set_hidden_fallback(fallback);
        ask(self.output, &question).await
    }
}
impl<T: QuestionStyle + ?Sized> Choice<'_, T> {
    pub async fn render<V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = V>,
    ) -> Result<Value, ConsoleError> {
        self.render_with(label, choices, None, None, false).await
    }
    pub async fn render_with<V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = V>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut scope = QuestionHelperScope::new(self.output, Arc::new(QuestionHelper::new()));
        let mut question = choice_question(label, choices, default)?;
        question.set_max_attempts(attempts)?;
        question.set_multiselect(multiple);
        scope.ask_question(question.as_question()).await
    }
    pub async fn render_pairs_with<K: Into<ChoiceKey>, V: Into<Value>>(
        &mut self,
        label: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
        attempts: Option<usize>,
        multiple: bool,
    ) -> Result<Value, ConsoleError> {
        let mut scope = QuestionHelperScope::new(self.output, Arc::new(QuestionHelper::new()));
        let mut question = choice_question_pairs(label, choices, default)?;
        question.set_max_attempts(attempts)?;
        question.set_multiselect(multiple);
        scope.ask_question(question.as_question()).await
    }
    pub fn get_choice_question<V: Into<Value>>(
        &self,
        label: &str,
        choices: impl IntoIterator<Item = V>,
        default: Option<Value>,
    ) -> Result<ChoiceQuestion, ConsoleError> {
        choice_question(label, choices, default)
    }
    pub fn get_choice_question_pairs<K: Into<ChoiceKey>, V: Into<Value>>(
        &self,
        label: &str,
        choices: impl IntoIterator<Item = (K, V)>,
        default: Option<Value>,
    ) -> Result<ChoiceQuestion, ConsoleError> {
        choice_question_pairs(label, choices, default)
    }
    pub fn is_assoc(&self, entries: &[(ChoiceKey, Value)]) -> bool {
        is_assoc(entries)
    }
}
fn choice_question<V: Into<Value>>(
    label: &str,
    choices: impl IntoIterator<Item = V>,
    default: Option<Value>,
) -> Result<ChoiceQuestion, ConsoleError> {
    let entries = choices
        .into_iter()
        .enumerate()
        .map(|(i, value)| {
            Ok((
                ChoiceKey::Integer(i64::try_from(i).map_err(ConsoleError::from_source)?),
                value.into(),
            ))
        })
        .collect::<Result<Vec<_>, ConsoleError>>()?;
    choice_question_pairs(label, entries, default)
}
fn choice_question_pairs<K: Into<ChoiceKey>, V: Into<Value>>(
    label: &str,
    choices: impl IntoIterator<Item = (K, V)>,
    default: Option<Value>,
) -> Result<ChoiceQuestion, ConsoleError> {
    ChoiceQuestion::from_pairs_with_associative(label, choices, default, is_assoc)
}
fn is_assoc(entries: &[(ChoiceKey, Value)]) -> bool {
    entries.iter().enumerate().any(
        |(i, (key, _))| !matches!(key,ChoiceKey::Integer(value) if usize::try_from(*value)==Ok(i)),
    )
}
