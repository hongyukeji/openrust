use std::{
    borrow::Cow,
    fs, io,
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
    support::Str,
};

use super::{
    source_edit::{file_error, lock_source},
    test_make_command,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(MailMakeCommand(CommandDefinition::new(
        Syntax::new("make:mail")
            .about("Create a new email type")
            .arg(
                Arg::new("name")
                    .required(true)
                    .help("The name of the mailable"),
            )
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("Replace an existing mailable source file"),
            )
            .arg(
                Arg::new("markdown")
                    .short('m')
                    .long("markdown")
                    .num_args(0..=1)
                    .default_missing_value("")
                    .value_name("view")
                    .help("Generate a Markdown template"),
            )
            .arg(
                Arg::new("view")
                    .long("view")
                    .num_args(0..=1)
                    .default_missing_value("")
                    .value_name("view")
                    .help("Generate a Blade template"),
            )
            .arg(
                Arg::new("test")
                    .long("test")
                    .action(ArgAction::SetTrue)
                    .help("Generate a matching feature test"),
            ),
    ))))
}

struct MailMakeCommand(CommandDefinition);

impl Command for MailMakeCommand {
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
            let target = Target::from_input(input)?;
            let root = app.base_path().to_owned();
            let source = target.source_path();
            let view = target.view.as_deref().map(view_path);
            let view_stub = if target.markdown {
                "<x-mail::message>\n# Your message\n\nThank you for using our application.\n</x-mail::message>\n"
            } else {
                "<p>Your message</p>\n"
            };
            let code = self.get_stub(app, input)?;
            let force = input.matches().get_flag("force");
            let with_test = input.matches().get_flag("test");
            let test = with_test.then(|| format!("{}Test", target.parts.join("/")));
            let test_path = test.as_deref().map(|name| {
                let mut path = PathBuf::from("tests/feature");
                path.extend(name.split('/').map(snake_case));
                path.with_extension("rs")
            });
            let source_output = source.clone();
            let view_output = view.clone();
            let generated = tokio::task::spawn_blocking(move || {
                write_mail(
                    &root,
                    &source,
                    code.as_bytes(),
                    view.as_deref(),
                    view_stub.as_bytes(),
                    test_path.as_deref(),
                    force,
                )
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))??;
            let Some(created_view) = generated else {
                output.stderr.push_str("Mailable already exists.\n");
                return Ok(0);
            };
            output.info(&format!(
                "Mailable [{}] created successfully.",
                source_output.display()
            ));
            if let Some(view) = view_output {
                if created_view {
                    output.info(&format!("View [{}] created successfully.", view.display()));
                } else {
                    output
                        .stderr
                        .push_str(&format!("View [{}] already exists.\n", view.display()));
                }
            }
            output
                .stdout
                .push_str("Declare the mailable module in its parent mod.rs and rebuild.\n");
            if let Some(test) = test {
                test_make_command::generate_matching(app, &test, force, output).await
                    .map_err(|error| ConsoleError::new(format!(
                        "Mailable created. Could not generate its test: {error}. Retry with make:test {test}."
                    )))?;
            }
            Ok(0)
        })
    }
}

impl GeneratorCommand for MailMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        Ok(app.base_path().join(source_path(&mail_parts(name)?)))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let target = Target::from_input(input)?;
        let class = target.parts.last().expect("validated name");
        let stub = match (target.markdown, target.view.is_some()) {
            (true, _) => Stub::MarkdownMail,
            (false, true) => Stub::ViewMail,
            (false, false) => Stub::Mail,
        };
        let template = stub
            .load(app.base_path())
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        Ok(Cow::Owned(
            template
                .replace("{{class}}", class)
                .replace("{{subject}}", &format!("{:?}", Str::headline(class)))
                .replace(
                    "{{view}}",
                    &format!("{:?}", target.view.as_deref().unwrap_or_default()),
                ),
        ))
    }
}

struct Target<'a> {
    parts: Vec<&'a str>,
    view: Option<String>,
    markdown: bool,
}

impl<'a> Target<'a> {
    fn from_input(input: &'a Input) -> Result<Self, ConsoleError> {
        let matches = input.matches();
        let name = matches
            .get_one::<String>("name")
            .expect("name required by Clap");
        let parts = mail_parts(name)?;
        let markdown = matches.get_one::<String>("markdown");
        let blade = matches.get_one::<String>("view");
        for view in [markdown, blade]
            .into_iter()
            .flatten()
            .filter(|view| !view.is_empty())
        {
            if !view.split('.').all(|part| {
                !part.is_empty()
                    && part
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            }) {
                return Err(ConsoleError::new(
                    "Use a dotted view name such as mail.orders.shipped.",
                ));
            }
        }
        let view = markdown
            .filter(|name| !name.is_empty())
            .or(blade)
            .or(markdown)
            .map(|name| {
                if name.is_empty() {
                    let mail = parts.strip_prefix(&["Mail"]).unwrap_or(&parts);
                    format!(
                        "mail.{}",
                        mail.iter()
                            .map(|name| Str::kebab(name))
                            .collect::<Vec<_>>()
                            .join(".")
                    )
                } else {
                    name.clone()
                }
            });
        Ok(Self {
            parts,
            view,
            markdown: markdown.is_some(),
        })
    }

    fn source_path(&self) -> PathBuf {
        source_path(&self.parts)
    }
}

fn mail_parts(name: &str) -> Result<Vec<&str>, ConsoleError> {
    let qualified = name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"));
    let parts = source_name_parts(
        qualified.unwrap_or(name),
        "Use a PascalCase mail name, such as Orders/Shipped.",
    )?;
    Ok(qualified
        .is_none()
        .then_some("Mail")
        .into_iter()
        .chain(parts)
        .collect())
}

fn source_path(parts: &[&str]) -> PathBuf {
    let mut path = PathBuf::from("app");
    path.extend(parts.iter().map(|name| snake_case(name)));
    path.with_extension("rs")
}

fn view_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from("resources/views");
    path.extend(name.split('.'));
    path.with_extension("blade.html")
}

fn write_mail(
    root: &Path,
    source: &Path,
    code: &[u8],
    view: Option<&Path>,
    view_stub: &[u8],
    test: Option<&Path>,
    force: bool,
) -> Result<Option<bool>, ConsoleError> {
    output_file(root, &root.join("tmp/artisan-source.lock"))?;
    let _lock = lock_source(root)?;
    let path = root.join(source);
    let exists = output_file(root, &path)?;
    if exists && !force {
        return Ok(None);
    }
    let module = path.with_extension("").join("mod.rs");
    if output_file(root, &module)? {
        return Err(ConsoleError::new(format!(
            "{} already defines the module.",
            module.display()
        )));
    }
    let pending_view = match view {
        Some(view) => {
            let path = root.join(view);
            (!output_file(root, &path)?).then_some(path)
        }
        None => None,
    };
    if let Some(test) = test {
        output_file(root, &root.join(test))?;
    }
    let previous = exists
        .then(|| fs::read(&path))
        .transpose()
        .map_err(|error| file_error(&path, error))?;
    write_generated(&path, code, force).map_err(|error| file_error(&path, error))?;
    let Some(view) = pending_view else {
        return Ok(Some(false));
    };
    match write_generated(&view, view_stub, false) {
        Ok(()) => Ok(Some(true)),
        Err(error)
            if error.kind() == io::ErrorKind::AlreadyExists
                && output_file(root, &view).is_ok_and(|exists| exists) =>
        {
            Ok(Some(false))
        }
        Err(error) => {
            let rollback = match previous {
                Some(previous) => write_generated(&path, &previous, true),
                None => fs::remove_file(&path),
            };
            if let Err(rollback) = rollback {
                return Err(ConsoleError::new(format!(
                    "Could not write {}: {error}. Could not restore {}: {rollback}.",
                    view.display(),
                    path.display()
                )));
            }
            Err(file_error(&view, error))
        }
    }
}

fn output_file(root: &Path, path: &Path) -> Result<bool, ConsoleError> {
    let mut exists = false;
    for entry in path.ancestors().take_while(|entry| *entry != root) {
        match fs::symlink_metadata(entry) {
            Ok(metadata) if entry == path && metadata.is_file() => exists = true,
            Ok(metadata) if entry != path && metadata.is_dir() => {}
            Ok(_) => {
                return Err(ConsoleError::new(format!(
                    "Unsupported generator path: {}",
                    entry.display()
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(file_error(entry, error)),
        }
    }
    Ok(exists)
}
