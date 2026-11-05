//! 固定 Laravel `EnvironmentCommand` 的应用环境查询（Laravel framework，MIT）。

use crate::core::{App, BoxFuture, Command, Result};

/// 显示已启动应用绑定的精确环境名称，不重新读取进程环境。
pub(crate) struct EnvironmentCommand;

impl Command for EnvironmentCommand {
    fn name(&self) -> &'static str {
        "env"
    }

    fn about(&self) -> &'static str {
        "Display the current framework environment"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            super::print(&message(app));
            Ok(())
        })
    }
}

fn message(app: &App) -> String {
    format!("The application environment is [{}].", app.environment())
}

#[cfg(test)]
#[path = "environment_tests.rs"]
mod tests;
