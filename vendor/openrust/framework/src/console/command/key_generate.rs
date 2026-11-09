//! 应用密钥生成和配置写入入口。

use std::io::{self, IsTerminal as _};

use crate::config::AppConfig;
use crate::console::Command;
use crate::foundation::Application;
use crate::support::BoxFuture;
use crate::{Error, Result};

#[path = "key_file.rs"]
mod key_file;

pub(crate) struct KeyGenerateCommand;

#[derive(Default, PartialEq, Eq)]
enum Action {
    #[default]
    Write,
    Show,
    Help,
}

#[derive(Default)]
struct Options {
    action: Action,
    force: bool,
    no_interaction: bool,
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Self::default();
        for arg in args {
            match arg.as_str() {
                "--show" => {
                    if options.action != Action::Help {
                        options.action = Action::Show;
                    }
                }
                "--force" => options.force = true,
                "--no-interaction" | "-n" => options.no_interaction = true,
                "--help" | "-h" => options.action = Action::Help,
                _ => {
                    return Err(Error::custom(
                        "app_key_command_arguments",
                        "key:generate 包含未知参数",
                    ));
                }
            }
        }
        Ok(options)
    }

    fn needs_confirmation(&self, app: &AppConfig) -> bool {
        app.env.is_production()
            && app.key.as_deref().is_some_and(|key| !key.is_empty())
            && !self.force
    }
}

impl Command for KeyGenerateCommand {
    fn name(&self) -> &'static str {
        "key:generate"
    }

    fn about(&self) -> &'static str {
        "生成并设置应用密钥"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("[--show] [--force] [--no-interaction|-n]")
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let options = Options::parse(&args)?;
            if options.action == Action::Help {
                super::print_help(self);
                return Ok(());
            }
            let repository = app.config_repository();
            let config: AppConfig = repository.section("app")?;
            let key = key_file::generate_key(&config.cipher)?;
            if options.action == Action::Show {
                super::print(&key);
                return Ok(());
            }
            if options.needs_confirmation(&config) && !confirm_production(options.no_interaction)? {
                super::print("已取消命令。");
                return Ok(());
            }
            let (dotenv_path, key_variable, external_key_present) =
                app.key_generation_environment()?;
            let staged = key_file::stage_key_file(
                dotenv_path,
                key_variable,
                config.key.as_deref().unwrap_or_default(),
                &key,
                external_key_present,
            )?;
            repository.persist_application_key(&config, key, move || staged.commit())?;
            super::print("应用密钥已设置。");
            Ok(())
        })
    }
}

fn confirm_production(no_interaction: bool) -> Result<bool> {
    if no_interaction || !io::stdin().is_terminal() {
        return Ok(false);
    }
    super::print("应用处于生产环境。是否继续替换现有密钥？[y/N]");
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|_| Error::custom("app_key_confirmation_failed", "无法读取生产环境确认输入"))?;
    let answer = answer.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Environment;

    #[test]
    fn production_confirmation_only_applies_to_existing_key_without_force() {
        let mut config = AppConfig {
            env: Environment::Production,
            ..AppConfig::default()
        };
        let options = Options::default();
        assert!(!options.needs_confirmation(&config));
        config.key = Some("synthetic-old".to_owned());
        assert!(options.needs_confirmation(&config));
        assert!(
            !Options {
                force: true,
                ..Options::default()
            }
            .needs_confirmation(&config)
        );
        config.env = Environment::Local;
        assert!(!options.needs_confirmation(&config));
    }
}
