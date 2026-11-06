//! 同一应用命令生命周期中的模块状态操作；不另建启动器或状态仓储。

use std::fmt::Write as _;

use openrust::console::Command;
use openrust::core::Error;
use openrust::core::Result;
use openrust::foundation::Application;
use openrust::foundation::BootContext;
use openrust::support::BoxFuture;

use super::{Module, ModuleRepository};

pub(crate) fn register_commands(app: &mut BootContext<'_>) {
    // 裸内核可不装模块清单；只向实际绑定了同一管理仓储的应用暴露管理命令。
    if !app.is_bound::<ModuleRepository>() {
        return;
    }
    app.command(ChangeStatusCommand { active: true });
    app.command(ChangeStatusCommand { active: false });
    app.command(ListCommand);
}

struct ChangeStatusCommand {
    active: bool,
}

impl Command for ChangeStatusCommand {
    fn name(&self) -> &'static str {
        if self.active {
            "module:enable"
        } else {
            "module:disable"
        }
    }

    fn about(&self) -> &'static str {
        if self.active {
            "持久启用指定模块或 --all，下次应用启动生效"
        } else {
            "持久禁用指定模块或 --all，不卸载当前应用"
        }
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let repository = app.state().resolve::<ModuleRepository>().await?;
            repository.ensure_writable()?;
            let modules = selected_modules(&repository, args)?;
            // 具体 enable/disable 动作对应上游 void 方法：异常中断，前项已提交不回滚。
            for module in modules {
                if self.active {
                    module.enable().await?;
                } else {
                    module.disable().await?;
                }
                openrust::console::command::print(&format!(
                    "模块 {} 已{}，下次启动使用新状态",
                    module.name(),
                    if self.active { "启用" } else { "禁用" }
                ));
            }
            Ok(())
        })
    }
}

fn selected_modules(repository: &ModuleRepository, args: Vec<String>) -> Result<Vec<Module>> {
    let mut all = false;
    let mut names = Vec::new();
    let mut positional = false;
    for value in args {
        match value.as_str() {
            "--" if !positional => positional = true,
            "--all" | "-a" if !positional => all = true,
            "--no-interaction" | "-n" if !positional => {}
            value if !positional && value.starts_with('-') => {
                return Err(Error::custom("module_command_argument", "模块启停选项无效"));
            }
            _ => names.push(value),
        }
    }
    if all {
        return Ok(repository.all());
    }
    if names.is_empty() {
        return Err(Error::custom(
            "module_name_required",
            "请提供模块名称或 --all",
        ));
    }
    // 保留重复项和输入顺序；先解析全部身份，防止拼错第二项时已经写入第一项。
    names
        .iter()
        .map(|name| repository.find_or_fail(name))
        .collect()
}

struct ListCommand;

impl Command for ListCommand {
    fn name(&self) -> &'static str {
        "module:list"
    }
    fn about(&self) -> &'static str {
        "显示模块状态、名称、路径与优先级"
    }
    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let repository = app.state().resolve::<ModuleRepository>().await?;
            let (only, direction) = list_options(args)?;
            let snapshot = repository.snapshot()?;
            let mut modules: Vec<_> = repository
                .all()
                .into_iter()
                .filter(|module| match only.as_str() {
                    "enabled" | "priority" => snapshot.has_status(module.name(), true),
                    "disabled" => snapshot.has_status(module.name(), false),
                    _ => true,
                })
                .collect();
            if only == "priority" {
                let direction = direction.ok_or_else(|| {
                    Error::custom(
                        "module_command_argument",
                        "priority 排序的 direction 不能为 null",
                    )
                })?;
                super::priority::sort_repository_priority(
                    &mut modules,
                    Module::priority,
                    direction == "desc",
                )?;
            }
            let mut output = String::from("状态 / 名称\t路径 / 优先级\n");
            for module in modules {
                let status = if snapshot.is_enabled(module.name()) {
                    "Enabled"
                } else {
                    "Disabled"
                };
                let _ = writeln!(
                    output,
                    "[{status}] {}\t{} [{}]",
                    module.name(),
                    module.path().display(),
                    super::priority::priority_label(module.priority())?
                );
            }
            openrust::console::command::print(output.trim_end());
            Ok(())
        })
    }
}

fn list_options(args: Vec<String>) -> Result<(String, Option<String>)> {
    let mut only = String::new();
    let mut direction = Some("asc".to_owned());
    let mut args = args.into_iter().peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--no-interaction" | "-n" => {}
            "--only" | "-o" => {
                only = args
                    .next_if(|value| !value.starts_with('-'))
                    .unwrap_or_default();
            }
            "--direction" | "-d" => {
                direction = args.next_if(|value| !value.starts_with('-'));
            }
            value if value.starts_with("--only=") => value[7..].clone_into(&mut only),
            value if value.starts_with("-o") && value.len() > 2 => {
                value[2..]
                    .strip_prefix('=')
                    .unwrap_or(&value[2..])
                    .clone_into(&mut only);
            }
            value if value.starts_with("-d") && value.len() > 2 => {
                direction = Some(
                    value[2..]
                        .strip_prefix('=')
                        .unwrap_or(&value[2..])
                        .to_owned(),
                );
            }
            value if value.starts_with("--direction=") => direction = Some(value[12..].to_owned()),
            _ => return Err(Error::custom("module_command_argument", "模块列表选项无效")),
        }
    }
    Ok((only, direction))
}

#[cfg(test)]
mod tests {
    use super::list_options;

    // 可选 only 的空值、紧凑短参数和 direction 不应相互吞掉。
    #[test]
    fn list_options_preserve_optional_only_and_compact_arguments() -> openrust::core::Result<()> {
        let args = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect();
        assert_eq!(
            list_options(Vec::new())?,
            (String::new(), Some("asc".into()))
        );
        assert_eq!(
            list_options(args(&["--only", "--direction", "desc"]))?,
            (String::new(), Some("desc".into()))
        );
        assert_eq!(
            list_options(args(&["-opriority", "-ddesc", "-n"]))?,
            ("priority".into(), Some("desc".into()))
        );
        assert_eq!(
            list_options(args(&["--only=disabled", "--direction=other"]))?,
            ("disabled".into(), Some("other".into()))
        );
        assert_eq!(list_options(args(&["--direction"]))?, (String::new(), None));
        assert!(list_options(args(&["--unknown"])).is_err());
        Ok(())
    }
}
