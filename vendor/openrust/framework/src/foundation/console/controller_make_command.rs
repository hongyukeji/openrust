use std::{
    borrow::Cow,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

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
    let mut syntax = Syntax::new("make:controller")
        .about("创建 HTTP 控制器")
        .arg(Arg::new("name").required(true).help("控制器类型名称"))
        .arg(
            Arg::new("type")
                .long("type")
                .value_name("TEMPLATE")
                .help("选择控制器模板类型"),
        );
    for (key, short, help) in [
        ("invokable", Some('i'), "生成单动作控制器"),
        ("resource", Some('r'), "生成资源控制器"),
        ("singleton", Some('s'), "生成单例资源控制器"),
        ("api", None, "省略创建和编辑表单动作"),
        ("creatable", None, "允许单例资源创建与删除"),
        ("force", Some('f'), "覆盖已有控制器文件"),
    ] {
        let flag = Arg::new(key)
            .long(key)
            .help(help)
            .action(ArgAction::SetTrue);
        syntax = syntax.arg(if let Some(short) = short {
            flag.short(short)
        } else {
            flag
        });
    }
    artisan.add_command(Box::new(ControllerMakeCommand(CommandDefinition::new(
        syntax,
    ))))
}

struct ControllerMakeCommand(CommandDefinition);

impl Command for ControllerMakeCommand {
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
            let root = app.base_path().to_owned();
            let path = self.get_path(app, name(input), input)?;
            let relative = path
                .strip_prefix(&root)
                .expect("控制器路径位于应用目录")
                .to_owned();
            let force = input.matches().get_flag("force");
            let template = self.get_stub(app, input)?;
            let result = tokio::task::spawn_blocking(move || {
                check_target(&root, &path)?;
                write_generated(&path, template.as_bytes(), force)
            })
            .await
            .map_err(ConsoleError::from_source)?;

            match result {
                Ok(()) => {
                    output.info(&format!("控制器 [{}] 创建成功。", relative.display()));
                    output
                        .stdout
                        .push_str("请在父模块中声明新模块，再到服务提供者与路由中注册控制器。\n");
                }
                Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
                    output.stderr.push_str("控制器已存在。\n");
                }
                Err(error) => return Err(super::source_edit::file_error(&relative, error)),
            }
            Ok(0)
        })
    }
}

impl GeneratorCommand for ControllerMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let (root, parts) = relative_name(name);
        let mut path = app.base_path().join(root);
        path.extend(name_parts(parts)?.map(snake_case));
        Ok(path.with_extension("rs"))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let (_, relative) = relative_name(name(input));
        let class = name_parts(relative)?.last().expect("名称至少包含类型");
        let template = template(input)?;
        let source = match template {
            Template::BuiltIn(stub) => stub
                .load(app.base_path())
                .map_err(ConsoleError::from_source)?,
            Template::Custom(kind, api) => {
                let suffix = if api { ".api" } else { "" };
                let path = app
                    .base_path()
                    .join("stubs")
                    .join(format!("controller.{kind}{suffix}.rs.stub"));
                Cow::Owned(super::source_edit::read(&path)?)
            }
        };
        let creatable = input.matches().get_flag("creatable");
        let actions = match template {
            Template::BuiltIn(stub) => methods(stub, creatable),
            Template::Custom(..) => String::new(),
        };
        Ok(Cow::Owned(
            source
                .replace("{{class}}", class)
                .replace("{{actions}}", &actions),
        ))
    }
}

#[derive(Clone, Copy)]
enum Template<'a> {
    BuiltIn(Stub),
    Custom(&'a str, bool),
}

fn template(input: &Input) -> Result<Template<'_>, ConsoleError> {
    let matches = input.matches();
    let api = matches.get_flag("api");
    let stub = if let Some(kind) = matches.get_one::<String>("type") {
        let api = api && !matches.get_flag("invokable");
        match (kind.as_str(), api) {
            ("plain", false) => Stub::PlainController,
            ("invokable", false) => Stub::InvokableController,
            ("api", false) => Stub::ApiController,
            ("singleton", false) => Stub::SingletonController,
            ("singleton", true) | ("singleton.api", false) => Stub::ApiSingletonController,
            _ if kind.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || [b'-', b'_'].contains(&byte))
            }) =>
            {
                return Ok(Template::Custom(kind, api));
            }
            _ => {
                return Err(ConsoleError::new(
                    "控制器模板类型只能包含字母、数字、点、连字符和下划线。",
                ));
            }
        }
    } else if matches.get_flag("invokable") {
        Stub::InvokableController
    } else if matches.get_flag("singleton") {
        if api {
            Stub::ApiSingletonController
        } else {
            Stub::SingletonController
        }
    } else if api {
        Stub::ApiController
    } else if matches.get_flag("resource") {
        Stub::ResourceController
    } else {
        Stub::PlainController
    };
    Ok(Template::BuiltIn(stub))
}

fn methods(stub: Stub, creatable: bool) -> String {
    let names: &[&str] = match stub {
        Stub::InvokableController => &["invoke"],
        Stub::ResourceController => &[
            "index", "create", "store", "show", "edit", "update", "destroy",
        ],
        Stub::ApiController => &["index", "store", "show", "update", "destroy"],
        Stub::SingletonController => &["create", "store", "show", "edit", "update", "destroy"],
        Stub::ApiSingletonController => &["store", "show", "update", "destroy"],
        _ => &[],
    };
    let singleton = matches!(
        stub,
        Stub::SingletonController | Stub::ApiSingletonController
    );
    names
        .iter()
        .map(|name| {
            let status =
                if singleton && !creatable && matches!(*name, "create" | "store" | "destroy") {
                    "NOT_FOUND"
                } else {
                    "NOT_IMPLEMENTED"
                };
            include_str!("stubs/controller.method.rs.stub")
                .replace("{{action}}", name)
                .replace("{{status}}", status)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn name(input: &Input) -> &str {
    input
        .matches()
        .get_one::<String>("name")
        .expect("名称由 Clap 校验")
}

fn relative_name(name: &str) -> (&'static str, &str) {
    let name = name.trim_start_matches(['/', '\\']);
    match name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        Some(relative) => ("app", relative),
        None => ("app/http/controllers", name),
    }
}

fn name_parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    let parts = source_name_parts(
        name,
        "请使用 PascalCase 控制器类型名，例如 Admin/ReportController。",
    )?
    .collect::<Vec<_>>();
    for &component in &parts {
        syn::parse_str::<syn::Ident>(&snake_case(component)).map_err(|_| {
            ConsoleError::new(format!("控制器路径使用了 Rust 保留字 {component}。"))
        })?;
    }
    Ok(parts.into_iter())
}

fn check_target(root: &Path, path: &Path) -> io::Result<()> {
    let mut current = root.to_owned();
    for component in path
        .parent()
        .unwrap_or(root)
        .strip_prefix(root)
        .expect("控制器目标位于应用下")
        .components()
    {
        current.push(component);
        match current.symlink_metadata() {
            Ok(metadata) if !metadata.is_dir() => {
                return Err(io::Error::other(format!(
                    "{} 不是实际目录",
                    current.display()
                )));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => break,
            Err(error) => return Err(error),
        }
    }
    if path.with_extension("").join("mod.rs").try_exists()? {
        return Err(io::Error::other("已定义同名目录模块"));
    }
    Ok(())
}
