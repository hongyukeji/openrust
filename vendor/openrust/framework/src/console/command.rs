//! 应用二进制公开的运行时命令。

use std::{any::Any, fmt::Write as _};

use crate::core::error::Result;
use crate::foundation::application::Application;
use crate::support::BoxFuture;

mod environment;
pub(crate) use environment::EnvironmentCommand;

/// 可通过 `<app-binary> <name> [args…]` 运行的命名命令。
///
/// Provider 在 `boot` 阶段注册命令；HTTP 模块提供 `serve`，数据库模块提供 `migrate` 等。
/// 嵌套命令使用空格分隔单词，例如 `queue work`、`db seed`。
pub trait Command: Any + Send + Sync + 'static {
    /// 在命令行中输入的命令名称。
    fn name(&self) -> &'static str;

    /// `help` 显示的单行说明。
    fn about(&self) -> &'static str;

    /// 命令名后是否可接收参数；默认不接收参数。
    fn accepts_args(&self) -> bool {
        false
    }

    /// 使用剩余参数执行命令。
    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>>;
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

    fn run<'a>(&'a self, app: &'a Application, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut commands: Vec<_> = app.commands().collect();
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

    fn run<'a>(&'a self, app: &'a Application, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
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

/// 清除精确选择的配置缓存文件。
pub(crate) struct ConfigClearCommand;

impl Command for ConfigClearCommand {
    fn name(&self) -> &'static str {
        "config:clear"
    }

    fn about(&self) -> &'static str {
        "Clear the compiled configuration"
    }

    fn run<'a>(&'a self, app: &'a Application, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            crate::config::compiled::clear(app.config_cache_path()?)?;
            print("Configuration cache cleared");
            Ok(())
        })
    }
}

/// 重新引导同一应用类型，再保存最终配置仓储的完整有序值。
pub(crate) struct ConfigCacheCommand<A> {
    pub(crate) console_environment: Option<String>,
    pub(crate) marker: std::marker::PhantomData<A>,
}

impl<A: crate::foundation::ApplicationDefinition> Command for ConfigCacheCommand<A> {
    fn name(&self) -> &'static str {
        "config:cache"
    }

    fn about(&self) -> &'static str {
        "Compile the configuration for the next startup"
    }

    fn run<'a>(&'a self, app: &'a Application, _args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if A::config().has_typed() {
                return Err(crate::config::ConfigError::Invalid(
                    "已编译配置含运行期 Rust 类型，不能创建配置缓存".to_owned(),
                )
                .into());
            }
            crate::config::compiled::clear(app.config_cache_path()?)?;
            let fresh = crate::foundation::lifecycle::bootstrap_for_cache::<A>(
                app.config_cache_path()?.to_path_buf(),
                self.console_environment.as_deref(),
            )
            .await?;
            let result = fresh
                .config_repository()
                .all()
                .map_err(crate::core::Error::from);
            let path = fresh.config_cache_path().map(std::path::Path::to_path_buf);
            let shutdown = fresh
                .shutdown(tokio::time::Instant::now() + std::time::Duration::from_secs(5))
                .await;
            match (result, path, shutdown) {
                (Err(error), _, _) | (Ok(_), Ok(_), Err(error)) => Err(error),
                (_, Err(error), _) => Err(error.into()),
                (Ok(value), Ok(path), Ok(())) => {
                    crate::config::compiled::write(&path, &value)?;
                    print("Configuration cached");
                    Ok(())
                }
            }
        })
    }
}

// 命令是允许并预期向 stdout 输出内容的唯一位置。
#[allow(clippy::print_stdout)]
/// 将命令输出写入标准输出；外部命令复用同一输出入口。
pub fn print(text: &str) {
    println!("{text}");
}
