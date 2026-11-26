use std::ffi::OsString;

use crate::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::{Application, BootstrapError},
};

/// Bootstraps Artisan, dispatches commands, and terminates the application.
pub trait Kernel: Send + Sync {
    fn bootstrap(&mut self) -> ConsoleFuture<'_, Result<(), BootstrapError>>;
    fn get_application(&self) -> &Application;
    fn all(&self) -> Vec<&CommandDefinition>;
    fn handle<'a>(
        &'a mut self,
        arguments: &'a [OsString],
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, i32>;
    fn terminate<'a>(&'a mut self, arguments: &'a [OsString], status: i32)
    -> ConsoleFuture<'a, ()>;
    fn register_command(&mut self, command: Box<dyn Command>) -> Result<(), ConsoleError>;
    fn register_isolatable_command(
        &mut self,
        command: Box<dyn Isolatable>,
    ) -> Result<(), ConsoleError> {
        self.register_command(Box::new(crate::console::IsolatedCommand::new(command)))
    }
    fn call<'a>(
        &'a mut self,
        command: &'a str,
        input: &'a Input,
        output: Option<&'a mut Output>,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>>;
    fn output(&mut self) -> String;
}

/// 标记可隔离命令，并声明依赖本次输入的锁标识及有效期。
/// 使用控制台／门面的隔离注册入口自动添加`--isolated`。
pub trait Isolatable: Command {
    fn isolatable_id(&self, _input: &Input) -> Result<Option<String>, ConsoleError> {
        Ok(None)
    }
    fn isolation_lock_expires_at(&self, _input: &Input) -> Result<crate::cache::Ttl, ConsoleError> {
        Ok(crate::cache::Ttl::Seconds(3600))
    }
}

/// 声明必填参数的自动提示及提示完成钩子。
pub trait PromptsForMissingInput: Command {
    fn prompt_for_missing_arguments_using(
        &self,
    ) -> std::collections::BTreeMap<String, crate::console::MissingInputPrompt> {
        Default::default()
    }
    fn after_prompting_for_missing_arguments<'a>(
        &'a self,
        _app: &'a Application,
        _input: &'a mut Input,
        _output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
        Box::pin(async { Ok(()) })
    }
}
