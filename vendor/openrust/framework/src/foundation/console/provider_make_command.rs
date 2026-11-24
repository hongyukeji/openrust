use std::{
    fs, io,
    path::{Path, PathBuf},
};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

use super::source_edit::{file_error, lock_manifest, read, replace};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ProviderMakeCommand(CommandDefinition::new(
        Syntax::new("make:provider")
            .about("Create a new service provider class")
            .arg(
                Arg::new("name")
                    .required(true)
                    .help("The name of the provider"),
            )
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Replace an existing provider file"),
            ),
    ))))
}

struct ProviderMakeCommand(CommandDefinition);

impl Command for ProviderMakeCommand {
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
            let args = input.matches();
            let name = args
                .get_one::<String>("name")
                .expect("name required by Clap")
                .clone();
            let force = args.get_flag("force");
            let path = tokio::task::spawn_blocking(move || generate(&root, &name, force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))??;
            output.info(&format!(
                "Provider [{}] created and registered successfully.",
                path.display()
            ));
            Ok(0)
        })
    }
}

fn generate(root: &Path, name: &str, force: bool) -> Result<PathBuf, ConsoleError> {
    let parts: Vec<_> = source_name_parts(
        name,
        "Use a PascalCase provider name, such as ReportServiceProvider.",
    )?
    .collect();
    let class = parts.last().expect("validated name");
    let modules: Vec<_> = parts.iter().map(|name| snake_case(name)).collect();
    let identifiers: Vec<_> = modules
        .iter()
        .map(|module| module_identifier(module))
        .collect();
    let _manifest = lock_manifest(root)?;
    let mut directory = PathBuf::from("app/providers");
    let mut changes = Vec::new();
    for (index, (module, identifier)) in modules.iter().zip(&identifiers).enumerate() {
        let path = root.join(&directory).join("mod.rs");
        if index + 1 < modules.len()
            && root
                .join(&directory)
                .join(format!("{module}.rs"))
                .try_exists()
                .map_err(|error| file_error(&path, error))?
        {
            return Err(ConsoleError::new(format!(
                "{} already has a file module named {module}.",
                directory.display()
            )));
        }
        if let Some(change) = module_declaration(&path, identifier, index == 0)? {
            changes.push(change);
        }
        directory.push(module);
    }
    let relative = directory.with_extension("rs");
    let source = root.join(&relative);
    let directory_module = root.join(&directory).join("mod.rs");
    if directory_module
        .try_exists()
        .map_err(|error| file_error(&directory_module, error))?
    {
        return Err(ConsoleError::new(format!(
            "{} already has a directory module.",
            directory.display()
        )));
    }
    let registry = root.join("bootstrap/providers.rs");
    let previous = read(&registry)?;
    let qualified = format!("crate::app::providers::{}::{class}", identifiers.join("::"));
    let registration = format!("Box::new({qualified}),");
    let compact: String = previous
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    let registered = compact.contains(&format!("Box::new({qualified})"))
        || compact.contains(&format!("Box::new({qualified},)"));
    if !registered
        && (compact.contains(&format!("Box::new({class})"))
            || compact.contains(&format!("Box::new({class},)")))
    {
        return Err(ConsoleError::new(format!(
            "{} already references {class}. Check its provider registration manually.",
            registry.display()
        )));
    }
    let updated = if registered {
        None
    } else {
        Some(append_registration(&previous, &registry, &registration)?)
    };

    let code = Stub::Provider
        .load(root)
        .map_err(|error| ConsoleError::new(error.to_string()))?
        .replace("{{class}}", class);
    // A retry can finish missing declarations when the generated source has not been edited.
    let repaired = !force
        && (!registered || !changes.is_empty())
        && fs::read(&source).ok().as_deref() == Some(code.as_bytes());
    if !repaired {
        write_generated(&source, code.as_bytes(), force)
            .map_err(|error| file_error(&source, error))?;
    }
    for change in changes.into_iter().rev() {
        change.apply()?;
    }
    if let Some(updated) = updated {
        replace(&registry, &previous, &updated)?;
    }
    Ok(relative)
}

pub(super) fn append_registration(
    source: &str,
    path: &Path,
    registration: &str,
) -> Result<String, ConsoleError> {
    let manual = || {
        ConsoleError::new(format!(
            "无法确定 {} 的唯一 Provider 列表，请手动注册 {registration}",
            path.display()
        ))
    };
    let file = syn::parse_file(source).map_err(|_| manual())?;
    let functions: Vec<_> = file
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "providers" => Some(function),
            _ => None,
        })
        .collect();
    if functions.len() != 1 {
        return Err(manual());
    }
    // 同时支持直接返回vec与安装器生成的let providers = vec；保留外围cfg和自定义逻辑。
    let vectors: Vec<_> = functions[0]
        .block
        .stmts
        .iter()
        .filter_map(|statement| {
            let candidate = match statement {
                syn::Stmt::Local(local) => {
                    local
                        .init
                        .as_ref()
                        .and_then(|init| match init.expr.as_ref() {
                            syn::Expr::Macro(value) => Some(&value.mac),
                            _ => None,
                        })
                }
                syn::Stmt::Expr(syn::Expr::Macro(value), _) => Some(&value.mac),
                syn::Stmt::Macro(value) => Some(&value.mac),
                _ => None,
            };
            candidate.filter(|value| value.path.is_ident("vec"))
        })
        .collect();
    if vectors.len() != 1 {
        return Err(manual());
    }
    let vector = vectors[0];
    let syn::MacroDelimiter::Bracket(bracket) = &vector.delimiter else {
        return Err(manual());
    };
    let trailing = vector.tokens.clone().into_iter().last().is_none_or(
        |token| matches!(token,proc_macro2::TokenTree::Punct(punct) if punct.as_char()==','),
    );
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let registration = registration.replace("\r\n", "\n").replace('\n', newline);
    let addition = format!(
        "{}{newline}        {registration}{newline}    ",
        if trailing { "" } else { "," }
    );
    let mut updated = source.to_owned();
    updated.insert_str(bracket.span.close().byte_range().start, &addition);
    Ok(updated)
}

struct ModuleEdit {
    path: PathBuf,
    previous: Option<String>,
    updated: String,
}

impl ModuleEdit {
    fn apply(self) -> Result<(), ConsoleError> {
        match self.previous {
            Some(previous) => replace(&self.path, &previous, &self.updated),
            None => write_generated(&self.path, self.updated.as_bytes(), false)
                .map_err(|error| file_error(&self.path, error)),
        }
    }
}

fn module_declaration(
    path: &Path,
    module: &str,
    required: bool,
) -> Result<Option<ModuleEdit>, ConsoleError> {
    let previous = match fs::read_to_string(path) {
        Ok(source) => Some(source),
        Err(error) if !required && error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(file_error(path, error)),
    };
    let private = format!("mod {module};");
    let statement = format!("pub(crate) {private}");
    let public = format!("pub {private}");
    let contents = previous.as_deref().unwrap_or_default();
    if contents
        .lines()
        .any(|line| matches!(line.trim(), text if text == statement || text == public))
    {
        return Ok(None);
    }
    let custom = [format!("mod {module} "), format!("mod {module}{{")];
    if contents.lines().any(|line| {
        line.trim().ends_with(&private)
            || custom.iter().any(|pattern| line.trim().contains(pattern))
    }) {
        return Err(ConsoleError::new(format!(
            "{} has a custom declaration for {module}. Make the module visible to bootstrap/providers.rs.",
            path.display()
        )));
    }
    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let separator = if contents.is_empty() || contents.ends_with('\n') {
        ""
    } else {
        newline
    };
    let updated = format!("{contents}{separator}{statement}{newline}");
    Ok(Some(ModuleEdit {
        path: path.to_owned(),
        previous,
        updated,
    }))
}

fn module_identifier(module: &str) -> String {
    match module {
        "abstract" | "as" | "async" | "await" | "become" | "box" | "break" | "const"
        | "continue" | "do" | "dyn" | "else" | "enum" | "extern" | "false" | "final" | "fn"
        | "for" | "gen" | "if" | "impl" | "in" | "let" | "loop" | "macro" | "match" | "move"
        | "mut" | "override" | "priv" | "pub" | "ref" | "return" | "static" | "struct"
        | "trait" | "true" | "try" | "type" | "typeof" | "unsafe" | "unsized" | "use"
        | "virtual" | "where" | "while" | "yield" => format!("r#{module}"),
        _ => module.to_owned(),
    }
}
