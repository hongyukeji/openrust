use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, ArgMatches, Command as Syntax};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
    },
    contracts::foundation::Application as _,
    foundation::Application,
};

#[derive(Clone, Copy)]
enum Kind {
    Class,
    Interface,
    Trait,
    Enum,
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (kind, name, description) in [
        (Kind::Class, "make:class", "Create a new class"),
        (Kind::Interface, "make:interface", "Create a new interface"),
        (Kind::Trait, "make:trait", "Create a new trait"),
        (Kind::Enum, "make:enum", "Create a new enum"),
    ] {
        let syntax = Syntax::new(name)
            .about(description)
            .arg(Arg::new("name").required(true).help("The name of the type"))
            .arg(flag("force", 'f', "Replace an existing file"));
        let syntax = match kind {
            Kind::Class => syntax.arg(flag("invokable", 'i', "Generate an invoke method")),
            Kind::Enum => syntax
                .arg(flag("string", 's', "Generate a string-backed enum"))
                .arg(flag("int", 'i', "Generate an integer-backed enum")),
            _ => syntax,
        };
        artisan.add_command(Box::new(TypeMakeCommand {
            definition: CommandDefinition::new(syntax),
            kind,
        }))?;
    }
    Ok(())
}

fn flag(name: &'static str, short: char, help: &'static str) -> Arg {
    Arg::new(name)
        .long(name)
        .short(short)
        .help(help)
        .action(ArgAction::SetTrue)
}

struct TypeMakeCommand {
    definition: CommandDefinition,
    kind: Kind,
}

impl Command for TypeMakeCommand {
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
            let args = input.matches();
            let name = args
                .get_one::<String>("name")
                .expect("name required by Clap");
            let qualified = name
                .strip_prefix("App/")
                .or_else(|| name.strip_prefix("App\\"));
            let parts: Vec<_> =
                source_name_parts(qualified.unwrap_or(name), "Use a PascalCase type name.")?
                    .collect();
            let class = parts.last().expect("validated name");
            let root = app.base_path();
            let mut relative = PathBuf::from("app");
            if qualified.is_none()
                && let Some(namespace) = self.kind.namespace(root)
            {
                relative.push(namespace);
            }
            relative.extend(parts.iter().map(|name| snake_case(name)));
            let relative = relative.with_extension("rs");
            let path = root.join(&relative);
            let directory_module = path.with_extension("").join("mod.rs");
            if directory_module.try_exists().map_err(|error| {
                ConsoleError::new(format!("{}: {error}", directory_module.display()))
            })? {
                return Err(ConsoleError::new(format!(
                    "{} already defines the module.",
                    directory_module.display()
                )));
            }
            let force = args.get_flag("force");
            let code = self.kind.template(root, args)?.replace("{{class}}", class);
            tokio::task::spawn_blocking(move || write_generated(&path, code.as_bytes(), force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?
                .map_err(|error| ConsoleError::new(format!("{}: {error}", relative.display())))?;
            output.info(&format!(
                "{} [{}] created successfully.",
                self.kind.label(),
                relative.display()
            ));
            output
                .stdout
                .push_str("Declare the module in the application before using the type.\n");
            Ok(0)
        })
    }
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Class => "Class",
            Self::Interface => "Interface",
            Self::Trait => "Trait",
            Self::Enum => "Enum",
        }
    }

    fn namespace(self, root: &Path) -> Option<&'static str> {
        let candidates: &[&str] = match self {
            Self::Class => &[],
            Self::Interface => &["contracts", "interfaces"],
            Self::Trait => &["concerns", "traits"],
            Self::Enum => &["enums", "enumerations"],
        };
        candidates
            .iter()
            .copied()
            .find(|name| root.join("app").join(name).is_dir())
    }

    fn template(
        self,
        root: &Path,
        args: &ArgMatches,
    ) -> Result<std::borrow::Cow<'static, str>, ConsoleError> {
        let stub = match self {
            Self::Class if args.get_flag("invokable") => Stub::InvokableClass,
            Self::Class => Stub::Class,
            Self::Interface => return Ok(include_str!("stubs/interface.rs.stub").into()),
            Self::Trait => Stub::Trait,
            Self::Enum if args.get_flag("string") => Stub::StringEnum,
            Self::Enum if args.get_flag("int") => Stub::IntEnum,
            Self::Enum => Stub::Enum,
        };
        stub.load(root)
            .map_err(|error| ConsoleError::new(error.to_string()))
    }
}
