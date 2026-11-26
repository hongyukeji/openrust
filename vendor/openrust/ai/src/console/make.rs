use std::{borrow::Cow, fs, io::ErrorKind, path::PathBuf};

use openrust::{
    console::{
        Command, CommandDefinition, ConsoleError, ConsoleFuture, GeneratorCommand, Input, Output,
        generator_command::{snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
    support::facades::Artisan,
};
use serde_json::Value;

const AGENT: (&str, &str) = (
    "stubs/agent.rs.stub",
    include_str!("../../stubs/agent.rs.stub"),
);
const STRUCTURED: (&str, &str) = (
    "stubs/structured-agent.rs.stub",
    include_str!("../../stubs/structured-agent.rs.stub"),
);
const TOOL: (&str, &str) = (
    "stubs/tool.rs.stub",
    include_str!("../../stubs/tool.rs.stub"),
);

const MIDDLEWARE: (&str, &str) = (
    "stubs/agent-middleware.rs.stub",
    include_str!("../../stubs/agent-middleware.rs.stub"),
);

pub(crate) const STUBS: &[(&str, &[u8])] = &[
    (AGENT.0, AGENT.1.as_bytes()),
    (STRUCTURED.0, STRUCTURED.1.as_bytes()),
    (TOOL.0, TOOL.1.as_bytes()),
    (MIDDLEWARE.0, MIDDLEWARE.1.as_bytes()),
];

pub(super) fn register() -> Result<(), ConsoleError> {
    for (kind, signature) in [
        (
            Kind::Agent,
            "make:agent {name? : Agent 类型名称} {--f|force : 覆盖已有文件} {--s|structured : 使用结构化输出模板}",
        ),
        (
            Kind::Tool,
            "make:tool {name? : 工具类型名称} {--f|force : 覆盖已有文件}",
        ),
        (
            Kind::Middleware,
            "make:agent-middleware {name? : 中间件类型名称} {--f|force : 覆盖已有文件}",
        ),
    ] {
        Artisan::add(Box::new(Make {
            kind,
            definition: CommandDefinition::from_signature(signature)?.with_help_namespace("ai"),
        }))?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Kind {
    Agent,
    Tool,
    Middleware,
}

struct Make {
    kind: Kind,
    definition: CommandDefinition,
}

impl Command for Make {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let force = input.matches().get_flag("force");
            let structured_option =
                matches!(self.kind, Kind::Agent) && input.matches().get_flag("structured");
            let mut structured = structured_option;
            let name = match input.matches().get_one::<String>("name") {
                Some(name) => name.clone(),
                None if input.option("no-interaction") == Some(Value::Bool(true)) => {
                    return Err(ConsoleError::new("请指定要创建的类型名称"));
                }
                None => {
                    let mut io = input.io(output);
                    let name = io
                        .ask("类型名称")
                        .await?
                        .filter(|name| !name.trim().is_empty())
                        .ok_or_else(|| ConsoleError::new("请指定要创建的类型名称"))?;
                    if matches!(self.kind, Kind::Agent) && !structured && !force {
                        structured = io.confirm_with("Agent 是否返回结构化输出？", false).await?;
                    }
                    name
                }
            };
            let path = self.get_path(app, &name, input)?;
            let relative = path.strip_prefix(app.base_path()).unwrap_or(&path);
            let template = if structured == structured_option {
                self.get_stub(app, input)?
            } else {
                load(app, STRUCTURED.0, STRUCTURED.1)?
            };
            let (_, name) = location(self.kind, &name);
            let class = parts(name)?.last().expect("已验证类型名称");
            let template = template
                .replace("{{class}}", class)
                .replace("{{tool}}", &snake_case(class));
            if path
                .with_extension("")
                .join("mod.rs")
                .try_exists()
                .map_err(ConsoleError::from_source)?
            {
                return Err(ConsoleError::new("已定义同名模块"));
            }
            match write_generated(&path, template.as_bytes(), force) {
                Ok(()) => {
                    output.info(&format!("已创建 [{}]。", relative.display()));
                    output
                        .stdout
                        .push_str("请在父模块中声明此模块，然后重新编译。\n");
                }
                Err(error) if !force && error.kind() == ErrorKind::AlreadyExists => {
                    output.stderr.push_str("源文件已存在。\n");
                }
                Err(error) => return Err(ConsoleError::from_source(error)),
            }
            Ok(0)
        })
    }
}

impl GeneratorCommand for Make {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        let (root, name) = location(self.kind, name);
        let mut path = app.base_path().join(root);
        path.extend(parts(name)?.map(snake_case));
        Ok(path.with_extension("rs"))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let (file, fallback) =
            self.stub(matches!(self.kind, Kind::Agent) && input.matches().get_flag("structured"));
        load(app, file, fallback)
    }
}

impl Make {
    fn stub(&self, structured: bool) -> (&'static str, &'static str) {
        match self.kind {
            Kind::Agent if structured => STRUCTURED,
            Kind::Agent => AGENT,
            Kind::Tool => TOOL,
            Kind::Middleware => MIDDLEWARE,
        }
    }
}

fn location(kind: Kind, name: &str) -> (&'static str, &str) {
    let name = name.trim_start_matches(['/', '\\']);
    if let Some(name) = name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        return ("app", name);
    }
    let root = match kind {
        Kind::Agent => "app/ai/agents",
        Kind::Tool => "app/ai/tools",
        Kind::Middleware => "app/ai/middleware",
    };
    (root, name)
}

fn parts(name: &str) -> Result<impl Iterator<Item = &str>, ConsoleError> {
    source_name_parts(name, "请使用 PascalCase 类型名称，例如 CatalogAgent。")
}

fn load(
    app: &Application,
    file: &str,
    fallback: &'static str,
) -> Result<Cow<'static, str>, ConsoleError> {
    match fs::read_to_string(app.base_path().join(file)) {
        Ok(template) => Ok(Cow::Owned(template)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(Cow::Borrowed(fallback)),
        Err(error) => Err(ConsoleError::from_source(error)),
    }
}
