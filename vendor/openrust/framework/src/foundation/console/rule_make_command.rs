use std::{borrow::Cow, io::ErrorKind, path::PathBuf};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("make:rule")
        .about("创建验证规则类型")
        .arg(Arg::new("name").required(true).help("规则类型名称"))
        .arg(
            Arg::new("force")
                .long("force")
                .short('f')
                .help("覆盖已有规则文件")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("implicit")
                .long("implicit")
                .short('i')
                .help("生成隐式验证规则")
                .action(ArgAction::SetTrue),
        );
    artisan.add_command(Box::new(RuleMakeCommand(CommandDefinition::new(syntax))))
}

struct RuleMakeCommand(CommandDefinition);

impl Command for RuleMakeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let path = self.get_path(app, name(input), input)?;
            let relative = path
                .strip_prefix(app.base_path())
                .unwrap_or(&path)
                .to_owned();
            let force = input.matches().get_flag("force");
            let template = self.get_stub(app, input)?;
            let result = tokio::task::spawn_blocking(move || {
                let module = path.with_extension("").join("mod.rs");
                if module.try_exists()? {
                    return Err(std::io::Error::other(format!(
                        "{} 已定义同名模块",
                        module.display()
                    )));
                }
                write_generated(&path, template.as_bytes(), force)
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))?;
            match result {
                Ok(()) => {}
                // 与通用生成器一致，重名时报告冲突并保留成功退出码。
                Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
                    output.stderr.push_str("验证规则已存在。\n");
                    return Ok(0);
                }
                Err(error) => {
                    return Err(ConsoleError::new(format!(
                        "{}: {error}",
                        relative.display()
                    )));
                }
            }
            output.info(&format!("验证规则 [{}] 创建成功。", relative.display()));
            output
                .stdout
                .push_str("请在父模块的 mod.rs 中声明规则模块，然后重新编译。\n");
            Ok(0)
        })
    }
}

impl GeneratorCommand for RuleMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let (directory, name) = rule_path(name);
        let mut path = app.base_path().join(directory);
        path.extend(parts(name)?.map(snake_case));
        Ok(path.with_extension("rs"))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let (_, name) = rule_path(name(input));
        let class = parts(name)?.last().expect("已校验规则名称");
        let template = stub(input)
            .load(app.base_path())
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        Ok(Cow::Owned(template.replace("{{class}}", class)))
    }
}

fn name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("名称由 Clap 校验")
}

fn rule_path(name: &str) -> (&'static str, &str) {
    let name = name.trim_start_matches(['/', '\\']);
    match name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        Some(name) => ("app", name),
        None => ("app/rules", name),
    }
}

fn parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    source_name_parts(name, "请使用 PascalCase 规则类型名，例如 Uppercase。")
}

fn stub(input: &Input) -> Stub {
    if input.matches().get_flag("implicit") {
        Stub::ImplicitRule
    } else {
        Stub::Rule
    }
}
