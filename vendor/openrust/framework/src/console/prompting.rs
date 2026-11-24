use std::{ffi::OsString, sync::Arc};

use super::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output};
use crate::{
    contracts::console::{Isolatable, PromptsForMissingInput},
    foundation::Application,
};

type PromptCallback = dyn for<'a> Fn(
        &'a Application,
        &'a mut Input,
        &'a mut Output,
    ) -> ConsoleFuture<'a, Result<Vec<OsString>, ConsoleError>>
    + Send
    + Sync;

/// 必填参数的文本问题或异步自定义输入。
pub enum MissingInputPrompt {
    Text { label: String, placeholder: String },
    Callback(Arc<PromptCallback>),
}
impl MissingInputPrompt {
    pub fn text(label: impl Into<String>, placeholder: impl Into<String>) -> Self {
        Self::Text {
            label: label.into(),
            placeholder: placeholder.into(),
        }
    }
    pub fn custom(
        callback: impl for<'a> Fn(
            &'a Application,
            &'a mut Input,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<Vec<OsString>, ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self::Callback(Arc::new(callback))
    }
}

/// 在类型擦除前显式保留可组合的命令Trait；执行仍使用原注册管线。
pub struct CommandRegistration<C> {
    command: Arc<C>,
    definition: CommandDefinition,
    prompting: Option<Arc<dyn PromptsForMissingInput>>,
    isolation: Option<Arc<dyn Isolatable>>,
}
impl<C: Command + 'static> CommandRegistration<C> {
    pub fn new(command: C) -> Self {
        let definition = command.definition().clone();
        Self {
            command: Arc::new(command),
            definition,
            prompting: None,
            isolation: None,
        }
    }
    pub fn prompting(mut self) -> Self
    where
        C: PromptsForMissingInput,
    {
        self.prompting = Some(self.command.clone());
        self
    }
    pub fn isolatable(mut self) -> Self
    where
        C: Isolatable,
    {
        self.definition = self.definition.isolatable();
        self.isolation = Some(self.command.clone());
        self
    }
}
impl<C: Command + 'static> Command for CommandRegistration<C> {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }
    fn prompting(&self) -> Option<&dyn PromptsForMissingInput> {
        self.prompting
            .as_deref()
            .or_else(|| self.command.prompting())
    }
    fn isolation(&self) -> Option<&dyn Isolatable> {
        self.isolation
            .as_deref()
            .or_else(|| self.command.isolation())
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        self.command.handle(app, input, output)
    }
}

pub(super) fn relaxed(mut syntax: clap::Command) -> clap::Command {
    let required: Vec<_> = syntax
        .get_arguments()
        .filter(|arg| {
            arg.is_required_set() && arg.get_long().is_none() && arg.get_short().is_none()
        })
        .map(|arg| arg.get_id().clone())
        .collect();
    for id in required {
        syntax = syntax.mut_arg(id, |arg| arg.required(false));
    }
    syntax
}

pub(super) async fn prompt(
    command: &dyn PromptsForMissingInput,
    app: &Application,
    input: &mut Input,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    let missing: Vec<_> = input
        .syntax
        .get_arguments()
        .filter(|arg| {
            arg.is_required_set() && arg.get_long().is_none() && arg.get_short().is_none()
        })
        .filter(|arg| {
            input
                .argument(arg.get_id().as_str())
                .is_none_or(|value| value.is_null() || value.as_array().is_some_and(Vec::is_empty))
        })
        .map(|arg| {
            (
                arg.get_id().to_string(),
                arg.get_help().map(ToString::to_string),
            )
        })
        .collect();
    if input.option("no-interaction") != Some(serde_json::Value::Bool(true)) && !missing.is_empty()
    {
        for (key, description) in missing {
            let question = command
                .prompt_for_missing_arguments_using()
                .remove(&key)
                .unwrap_or_else(|| {
                    MissingInputPrompt::text(
                        format!("请输入{}：", description.unwrap_or_else(|| key.clone())),
                        "",
                    )
                });
            let answer = match question {
                MissingInputPrompt::Callback(callback) => callback(app, input, output).await?,
                MissingInputPrompt::Text { label, placeholder } => {
                    if !output.streams_to_terminal() || !::console::Term::stderr().is_term() {
                        return Err(ConsoleError::new(format!(
                            "参数{key}需要交互输入；请显式传参或提供自定义回调"
                        )));
                    }
                    let label = if placeholder.is_empty() {
                        format!("{label} ")
                    } else {
                        format!("{label} [{placeholder}] ")
                    };
                    let key = key.clone();
                    let visible = !output.suppressed;
                    let value = tokio::task::spawn_blocking(move || {
                        let term = ::console::Term::stderr();
                        loop {
                            if visible {
                                term.write_str(&label).map_err(ConsoleError::from_source)?;
                            }
                            let answer = term.read_line().map_err(ConsoleError::from_source)?;
                            if !answer.is_empty() && answer != "0" {
                                return Ok::<_, ConsoleError>(answer);
                            }
                            if visible {
                                term.write_line(&format!("必须提供参数：{key}"))
                                    .map_err(ConsoleError::from_source)?;
                            }
                        }
                    })
                    .await
                    .map_err(ConsoleError::from_source)??;
                    vec![value.into()]
                }
            };
            input.set_argument(&key, answer)?;
        }
        command
            .after_prompting_for_missing_arguments(app, input, output)
            .await?;
    }
    input.validate_prompted(app.version())
}
