use std::{ffi::OsString, sync::Arc};

use crate::foundation::Application;

use super::{ConsoleError, ConsoleFuture, Input, Output};

pub(super) type Prompt =
    dyn Fn(&clap::ArgMatches) -> Result<Vec<OsString>, ConsoleError> + Send + Sync;

/// Describes a command's name, aliases, arguments, options, and help.
#[derive(Clone)]
pub struct CommandDefinition {
    pub(super) syntax: Arc<clap::Command>,
    pub(super) isolated: bool,
    pub(super) missing_argument: Option<(&'static str, Arc<Prompt>)>,
}

impl CommandDefinition {
    pub fn new(syntax: clap::Command) -> Self {
        Self {
            syntax: Arc::new(syntax),
            isolated: false,
            missing_argument: None,
        }
    }

    pub(crate) fn with_missing_argument_prompt(
        mut self,
        argument: &'static str,
        prompt: impl Fn(&clap::ArgMatches) -> Result<Vec<OsString>, ConsoleError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.missing_argument = Some((argument, Arc::new(prompt)));
        self
    }

    /// Enables the optional `--isolated` switch for this command.
    pub fn isolatable(mut self) -> Self {
        if !self.isolated {
            let syntax = Arc::make_mut(&mut self.syntax);
            *syntax = std::mem::take(syntax).arg(
                clap::Arg::new("isolated")
                    .long("isolated")
                    .num_args(0..=1)
                    .default_missing_value("0")
                    .value_parser(clap::value_parser!(i32))
                    .help("Do not run the command if another instance is already running"),
            );
            self.isolated = true;
        }
        self
    }

    pub fn name(&self) -> &str {
        self.syntax.get_name()
    }

    pub fn parse(&self, arguments: &[OsString]) -> Result<Input, ConsoleError> {
        let command = self.name().to_owned();
        let matches = self
            .syntax
            .as_ref()
            .clone()
            .try_get_matches_from(
                std::iter::once(OsString::from(&command)).chain(arguments.iter().cloned()),
            )
            .map_err(ConsoleError::from_source)?;
        Ok(Input {
            shutdown: None,
            interaction: Default::default(),
            stream: None,
            syntax: self.syntax.clone(),
            parsing_syntax: self.syntax.clone(),
            invoked_name: None,
            matches,
        })
    }
}

/// An Artisan command with a definition shared by dispatch, list, and help.
pub trait Command: Send + Sync {
    /// 隔离注册适配器提供标记；普通命令沿用定义中的可选开关。
    fn isolation(&self) -> Option<&dyn crate::contracts::console::Isolatable> {
        None
    }

    fn prompting(&self) -> Option<&dyn crate::contracts::console::PromptsForMissingInput> {
        None
    }

    fn definition(&self) -> &CommandDefinition;

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>;
}

/// A closure-based Artisan command registered by the application.
#[derive(Clone)]
pub struct ClosureCommand {
    pub(super) definition: CommandDefinition,
    pub(super) callback: Arc<CommandCallback>,
}

impl ClosureCommand {
    pub(crate) fn registration_key(&self) -> std::sync::Weak<clap::Command> {
        Arc::downgrade(&self.definition.syntax)
    }

    /// Sets the command description shown by Artisan help and list.
    pub fn purpose(&mut self, description: &'static str) -> &mut Self {
        let syntax = self.definition.syntax.as_ref().clone().about(description);
        self.definition = CommandDefinition::new(syntax);
        self
    }
}

impl Command for ClosureCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        (self.callback)(app, input, output)
    }
}

/// 闭包命令复用正式Command执行作用域，可显式解析所属应用的依赖。
pub type CommandCallback = dyn for<'a> Fn(
        &'a Application,
        &'a Input,
        &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>
    + Send
    + Sync;

/// 为捕获状态的异步闭包提供明确返回类型，不增加第二层执行器。
#[derive(Clone)]
pub struct AsyncCommandCallback(Arc<CommandCallback>);
impl AsyncCommandCallback {
    pub fn new(
        callback: impl for<'a> Fn(
            &'a Application,
            &'a Input,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(callback))
    }
}
impl IntoCommandCallback<AsyncCommandCallback> for AsyncCommandCallback {
    fn into_command_callback(self) -> Arc<CommandCallback> {
        self.0
    }
}

/// 按参数形状转换闭包，不改变已有仅输出回调的调用方式。
pub trait IntoCommandCallback<Arguments>: Send + Sync + 'static {
    fn into_command_callback(self) -> Arc<CommandCallback>;
}
impl<F> IntoCommandCallback<()> for F
where
    F: Fn(&mut Output) + Send + Sync + 'static,
{
    fn into_command_callback(self) -> Arc<CommandCallback> {
        Arc::new(move |_, _, output| {
            self(output);
            Box::pin(async { Ok(0) })
        })
    }
}
impl<F> IntoCommandCallback<(Input,)> for F
where
    F: Fn(&Input, &mut Output) -> Result<i32, ConsoleError> + Send + Sync + 'static,
{
    fn into_command_callback(self) -> Arc<CommandCallback> {
        Arc::new(move |_, input, output| {
            let result = self(input, output);
            Box::pin(async { result })
        })
    }
}
impl<F> IntoCommandCallback<(Application, Input)> for F
where
    F: for<'a> Fn(
            &'a Application,
            &'a Input,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>
        + Send
        + Sync
        + 'static,
{
    fn into_command_callback(self) -> Arc<CommandCallback> {
        Arc::new(self)
    }
}

impl<F, Service: ?Sized + Send + Sync + 'static> IntoCommandCallback<(Arc<Service>, Input)> for F
where
    F: for<'a> Fn(
            Arc<Service>,
            &'a Input,
            &'a mut Output,
        ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>
        + Send
        + Sync
        + 'static,
{
    fn into_command_callback(self) -> Arc<CommandCallback> {
        let callback = Arc::new(self);
        Arc::new(move |app, input, output| {
            let callback = callback.clone();
            Box::pin(async move {
                let service = app
                    .make::<Service>()
                    .await
                    .map_err(ConsoleError::from_source)?;
                callback(service, input, output).await
            })
        })
    }
}
