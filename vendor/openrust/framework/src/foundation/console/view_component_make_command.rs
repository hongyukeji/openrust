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
    support::Str,
};

use super::source_edit::{file_error, lock_manifest};

const VIEW_STUB: &str = "<div>{{ $slot }}</div>\n";

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(ComponentMakeCommand(CommandDefinition::new(
        Syntax::new("make:component")
            .about("Create a new view component")
            .arg(
                Arg::new("name")
                    .required(true)
                    .help("The name of the component"),
            )
            .arg(
                Arg::new("inline")
                    .long("inline")
                    .conflicts_with("view")
                    .action(ArgAction::SetTrue)
                    .help("Create a component with an inline Blade template"),
            )
            .arg(
                Arg::new("view")
                    .long("view")
                    .action(ArgAction::SetTrue)
                    .help("Create an anonymous Blade component"),
            )
            .arg(
                Arg::new("path")
                    .long("path")
                    .conflicts_with("inline")
                    .help("The target directory relative to resources/views"),
            )
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Replace existing component files"),
            ),
    ))))
}

struct ComponentMakeCommand(CommandDefinition);

impl Command for ComponentMakeCommand {
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
            let path = args.get_one::<String>("path").cloned();
            let inline = args.get_flag("inline");
            let anonymous = args.get_flag("view");
            let force = args.get_flag("force");
            let files = tokio::task::spawn_blocking(move || {
                generate(&root, &name, path.as_deref(), inline, anonymous, force)
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))??;
            for file in files {
                output.info(&format!(
                    "Component file [{}] created successfully.",
                    file.display()
                ));
            }
            Ok(0)
        })
    }
}

fn generate(
    root: &Path,
    name: &str,
    path: Option<&str>,
    inline: bool,
    anonymous: bool,
    force: bool,
) -> Result<Vec<PathBuf>, ConsoleError> {
    let parts = source_name_parts(
        name,
        "Use a PascalCase component name, such as Forms/Input.",
    )?
    .collect::<Vec<_>>();
    let (component, parents) = parts.split_last().expect("validated name");
    let mut view_parts = match path {
        Some(path) => parse_view_path(path)?,
        None => std::iter::once("components".to_owned())
            .chain(parents.iter().map(|part| Str::kebab(part)))
            .collect(),
    };
    view_parts.push(Str::kebab(component));

    let mut files = Vec::new();
    if !anonymous {
        let mut target = PathBuf::from("app/view/components");
        target.extend(parents.iter().map(|part| snake_case(part)));
        target.push(format!("{}.rs", snake_case(component)));
        let (render, imports) = if inline {
            (
                format!(
                    "ComponentOutput::InlineTemplate({:?}.into())",
                    VIEW_STUB.trim_end()
                ),
                "",
            )
        } else {
            (
                format!("ComponentOutput::View(view({:?}))", view_parts.join(".")),
                ", view",
            )
        };
        let source = Stub::ViewComponent
            .load(root)
            .map_err(|error| ConsoleError::new(error.to_string()))?
            .replace("{{type}}", component)
            .replace("{{imports}}", imports)
            .replace("{{render}}", &render);
        files.push((target, source));
    }
    if !inline {
        let mut target = PathBuf::from("resources/views");
        target.extend(view_parts);
        target.set_extension("blade.html");
        files.push((target, VIEW_STUB.to_owned()));
    }

    let _lock = lock_manifest(root)?;
    let mut written = Vec::new();
    for (relative, source) in files {
        let path = root.join(&relative);
        let write = (|| {
            let previous = match fs::symlink_metadata(&path) {
                Ok(metadata) if force && metadata.is_file() => Some(fs::read(&path)?),
                Ok(_) => return Err(io::ErrorKind::AlreadyExists.into()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            write_generated(&path, source.as_bytes(), force)?;
            Ok(previous)
        })();
        match write {
            Ok(previous) => written.push((relative, previous)),
            Err(error) => {
                for (relative, previous) in written.into_iter().rev() {
                    let path = root.join(relative);
                    let restore = match previous {
                        Some(previous) => write_generated(&path, &previous, true),
                        None => fs::remove_file(&path),
                    };
                    if let Err(restore_error) = restore {
                        return Err(ConsoleError::new(format!(
                            "{error}; could not restore {}: {restore_error}",
                            path.display()
                        )));
                    }
                }
                return Err(file_error(&path, error));
            }
        }
    }
    Ok(written.into_iter().map(|(path, _)| path).collect())
}

fn parse_view_path(path: &str) -> Result<Vec<String>, ConsoleError> {
    path.split(['/', '\\'])
        .map(|segment| {
            if segment.is_empty()
                || !segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(ConsoleError::new(
                    "Use a relative view directory such as components/forms.",
                ));
            }
            Ok(Str::kebab(segment))
        })
        .collect()
}
