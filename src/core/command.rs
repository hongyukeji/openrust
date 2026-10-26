//! 应用二进制公开的运行时命令。

use std::fmt::Write as _;

use crate::core::app::App;
use crate::core::error::Result;
use crate::core::plugin::BoxFuture;

/// 可通过 `<app-binary> <name> [args…]` 运行的命名命令。
///
/// 插件在 [`Plugin::build`](crate::Plugin::build) 阶段注册命令；HTTP crate 提供 `serve`，数据库 crate 提供 `migrate` 等。
/// 嵌套命令使用空格分隔单词，例如 `queue work`、`db seed`。
pub trait Command: Send + Sync + 'static {
    /// 在命令行中输入的命令名称。
    fn name(&self) -> &'static str;

    /// `help` 显示的单行说明。
    fn about(&self) -> &'static str;

    /// 命令名后是否可接收参数；默认不接收参数。
    fn accepts_args(&self) -> bool {
        false
    }

    /// 使用剩余参数执行命令。
    fn run<'a>(&'a self, app: &'a App, args: Vec<String>) -> BoxFuture<'a, Result<()>>;
}

/// 列出已注册的命令。
pub(crate) struct HelpCommand;

impl Command for HelpCommand {
    fn name(&self) -> &'static str {
        "help"
    }

    fn about(&self) -> &'static str {
        "List available commands"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut commands: Vec<&dyn Command> = app.commands().collect();
            commands.sort_by_key(|c| c.name());
            let width = commands.iter().map(|c| c.name().len()).max().unwrap_or(0);
            let mut out = format!("{} commands:\n", app.config().app.name);
            for command in commands {
                // 写入 String 不会失败。
                let _ = writeln!(out, "  {:<width$}  {}", command.name(), command.about());
            }
            print(&out);
            Ok(())
        })
    }
}

/// 输出合并后的配置，并隐藏敏感值。
pub(crate) struct ConfigShowCommand;

impl Command for ConfigShowCommand {
    fn name(&self) -> &'static str {
        "config show"
    }

    fn about(&self) -> &'static str {
        "Print the effective configuration with secrets redacted"
    }

    fn run<'a>(&'a self, app: &'a App, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut value: serde_json::Value = app
                .figment()
                .extract()
                .map_err(crate::config::ConfigError::from)?;
            crate::config::redact_secrets(&mut value);
            let rendered = toml::to_string_pretty(&value)
                .unwrap_or_else(|_| serde_json::to_string_pretty(&value).unwrap_or_default());
            print(&rendered);
            Ok(())
        })
    }
}

// 命令是允许并预期向 stdout 输出内容的唯一位置。
#[allow(clippy::print_stdout)]
fn print(text: &str) {
    println!("{text}");
}
