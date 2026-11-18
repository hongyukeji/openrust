use std::{
    borrow::Cow,
    ffi::OsString,
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
};

use clap::{Arg, ArgAction, ArgMatches, Command as Syntax, parser::ValueSource};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture,
        GeneratorCommand, Input, Output,
        generator_command::{Stub, snake_case, source_name_parts, write_generated},
        localized_text,
    },
    contracts::foundation::Application as _,
    foundation::Application,
    support::Str,
};

use super::{
    source_edit::{file_error, lock_source},
    test_make_command,
};

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Mailable,
    Notification,
}

impl Kind {
    fn namespace(self) -> &'static str {
        match self {
            Self::Mailable => "Mail",
            Self::Notification => "Notifications",
        }
    }
}

pub(super) fn register(artisan: &mut Artisan, kind: Kind) -> Result<(), ConsoleError> {
    let (name, label, description) = match kind {
        Kind::Mailable => ("make:mail", "Mailable", "Create a new email type"),
        Kind::Notification => (
            "make:notification",
            "Notification",
            "Create a new notification type",
        ),
    };
    let mut syntax = Syntax::new(name)
        .about(description)
        .arg(
            Arg::new("name")
                .required(true)
                .help(format!("The name of the {label}")),
        )
        .arg(
            Arg::new("force")
                .short('f')
                .long("force")
                .action(ArgAction::SetTrue)
                .help(format!("Replace an existing {label} source file")),
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
            Arg::new("test")
                .long("test")
                .action(ArgAction::SetTrue)
                .help("Generate a matching feature test"),
        );
    if matches!(kind, Kind::Mailable) {
        syntax = syntax.arg(
            Arg::new("view")
                .long("view")
                .num_args(0..=1)
                .default_missing_value("")
                .value_name("view")
                .help("Generate a Blade template"),
        );
    }
    artisan.add_command(Box::new(MessageMakeCommand {
        definition: CommandDefinition::new(syntax)
            .with_missing_argument_prompt("name", move |input| {
                prompt_for_name_and_view(kind, input)
            }),
        kind,
        label,
    }))
}

fn prompt_for_name_and_view(kind: Kind, input: &ArgMatches) -> Result<Vec<OsString>, ConsoleError> {
    let question = match kind {
        Kind::Mailable => localized_text(
            "make.mail.name",
            "What is the name of the mailable?",
            "邮件类叫什么？",
            "郵件類別叫什麼？",
        ),
        Kind::Notification => localized_text(
            "make.notification.name",
            "What is the name of the notification?",
            "通知类叫什么？",
            "通知類別叫什麼？",
        ),
    };
    let name = loop {
        let name = read_prompt(&question)?;
        match message_parts(&name, kind) {
            Ok(_) => break name,
            Err(error) => eprintln!("{error}"),
        }
    };
    let option = if input
        .ids()
        .any(|id| input.value_source(id.as_str()) == Some(ValueSource::CommandLine))
    {
        None
    } else {
        match kind {
            Kind::Mailable => prompt_mailable_view()?,
            Kind::Notification => prompt_notification_view(&name)?,
        }
    };
    Ok(std::iter::once(name.into())
        .chain(option.map(OsString::from))
        .collect())
}

fn prompt_mailable_view() -> Result<Option<String>, ConsoleError> {
    let question = localized_text(
        "make.mail.view",
        "Would you like to create a view?\n  1. Markdown View\n  2. Empty View\n  3. No View\nChoose [1]",
        "要创建视图吗？\n  1. Markdown 视图\n  2. 空白视图\n  3. 不创建视图\n请选择 [1]",
        "要建立視圖嗎？\n  1. Markdown 視圖\n  2. 空白視圖\n  3. 不建立視圖\n請選擇 [1]",
    );
    loop {
        match read_prompt(&question)?.as_str() {
            "" | "1" => return Ok(Some("--markdown=".into())),
            "2" => return Ok(Some("--view=".into())),
            "3" => return Ok(None),
            _ => {}
        }
    }
}

fn prompt_notification_view(name: &str) -> Result<Option<String>, ConsoleError> {
    let question = localized_text(
        "make.notification.markdown",
        "Would you like to create a markdown view? [Y/n]",
        "要创建 Markdown 视图吗？ [Y/n]",
        "要建立 Markdown 視圖嗎？ [Y/n]",
    );
    loop {
        match read_prompt(&question)?.to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => break,
            "n" | "no" => return Ok(None),
            _ => {}
        }
    }

    let kind = Kind::Notification;
    let parts = message_parts(name, kind)?;
    let default = default_view_name(&parts, kind);
    let question = localized_text(
        "make.notification.view",
        "What should the markdown view be named?",
        "Markdown 视图叫什么？",
        "Markdown 視圖叫什麼？",
    );
    let view = read_prompt(&format!("{question} [{default}]"))?;
    Ok(Some(format!("--markdown={view}")))
}

fn read_prompt(question: &str) -> Result<String, ConsoleError> {
    let error = |error: io::Error| ConsoleError::new(error.to_string());
    {
        let mut output = io::stderr().lock();
        write!(output, "{question} ").map_err(error)?;
        output.flush().map_err(error)?;
    }
    let mut answer = String::new();
    if io::stdin().read_line(&mut answer).map_err(error)? == 0 {
        return Err(ConsoleError::new(localized_text(
            "make.message.cancelled",
            "Command cancelled.",
            "命令已取消。",
            "命令已取消。",
        )));
    }
    Ok(answer.trim().to_owned())
}

struct MessageMakeCommand {
    definition: CommandDefinition,
    kind: Kind,
    label: &'static str,
}

impl Command for MessageMakeCommand {
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
            let target = Target::from_input(input, self.kind)?;
            let root = app.base_path().to_owned();
            let source = target.source_path();
            let view = target.view.as_deref().map(view_path);
            let view_stub = if target.markdown {
                "<x-mail::message>\n# Your message\n\nThank you for using our application.\n</x-mail::message>\n"
            } else {
                "<p>Your message</p>\n"
            };
            let replace_view = matches!(self.kind, Kind::Notification);
            let label = self.label;
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
                write_message(
                    &root,
                    &source,
                    code.as_bytes(),
                    view.as_deref()
                        .map(|path| (path, view_stub.as_bytes(), replace_view)),
                    test_path.as_deref(),
                    force,
                )
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))??;
            let Some(created_view) = generated else {
                output
                    .stderr
                    .push_str(&format!("{label} already exists.\n"));
                return Ok(0);
            };
            output.info(&format!(
                "{label} [{}] created successfully.",
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
            output.stdout.push_str(&format!(
                "Declare the {} module in its parent mod.rs and rebuild.\n",
                label.to_ascii_lowercase()
            ));
            if let Some(test) = test {
                test_make_command::generate_matching(app, &test, force, output).await
                    .map_err(|error| ConsoleError::new(format!(
                        "{label} created. Could not generate its test: {error}. Retry with make:test {test}."
                    )))?;
            }
            Ok(0)
        })
    }
}

impl GeneratorCommand for MessageMakeCommand {
    fn get_path(&self, app: &Application, name: &str, _: &Input) -> Result<PathBuf, ConsoleError> {
        Ok(app
            .base_path()
            .join(source_path(&message_parts(name, self.kind)?)))
    }

    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<Cow<'static, str>, ConsoleError> {
        let target = Target::from_input(input, self.kind)?;
        let class = target.parts.last().expect("validated name");
        let stub = match (self.kind, target.markdown, target.view.is_some()) {
            (Kind::Mailable, true, _) => Stub::MarkdownMail,
            (Kind::Mailable, false, true) => Stub::ViewMail,
            (Kind::Mailable, false, false) => Stub::Mail,
            (Kind::Notification, true, _) => Stub::MarkdownNotification,
            (Kind::Notification, false, _) => Stub::Notification,
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
    fn from_input(input: &'a Input, kind: Kind) -> Result<Self, ConsoleError> {
        let matches = input.matches();
        let name = matches
            .get_one::<String>("name")
            .expect("name required by Clap");
        let parts = message_parts(name, kind)?;
        let markdown = matches.get_one::<String>("markdown");
        let blade = match kind {
            Kind::Mailable => matches.get_one::<String>("view"),
            Kind::Notification => None,
        };
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
                    default_view_name(&parts, kind)
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

fn default_view_name(parts: &[&str], kind: Kind) -> String {
    let mail = parts.strip_prefix(&[kind.namespace()]).unwrap_or(parts);
    format!(
        "mail.{}",
        mail.iter()
            .map(|name| Str::kebab(name))
            .collect::<Vec<_>>()
            .join(".")
    )
}

fn message_parts(name: &str, kind: Kind) -> Result<Vec<&str>, ConsoleError> {
    let qualified = name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"));
    let parts = source_name_parts(
        qualified.unwrap_or(name),
        "Use a PascalCase type name, such as Orders/Shipped.",
    )?;
    Ok(qualified
        .is_none()
        .then_some(kind.namespace())
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

fn write_message(
    root: &Path,
    source: &Path,
    code: &[u8],
    view: Option<(&Path, &[u8], bool)>,
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
        Some((view, stub, replace)) => {
            let path = root.join(view);
            let exists = output_file(root, &path)?;
            (!exists || replace).then_some((path, stub, replace))
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
    let Some((view, view_stub, replace)) = pending_view else {
        return Ok(Some(false));
    };
    match write_generated(&view, view_stub, replace) {
        Ok(()) => Ok(Some(true)),
        Err(error)
            if !replace
                && error.kind() == io::ErrorKind::AlreadyExists
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
