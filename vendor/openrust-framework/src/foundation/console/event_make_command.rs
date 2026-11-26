use std::path::PathBuf;

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

use super::test_make_command;

#[derive(Clone, Copy)]
enum Kind {
    Event,
    Listener,
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for (kind, name, about) in [
        (Kind::Event, "make:event", "Create a new event class"),
        (
            Kind::Listener,
            "make:listener",
            "Create a new event listener class",
        ),
    ] {
        let syntax = Syntax::new(name)
            .about(about)
            .arg(Arg::new("name").required(true))
            .arg(
                Arg::new("force")
                    .long("force")
                    .short('f')
                    .action(ArgAction::SetTrue),
            );
        let syntax = if matches!(kind, Kind::Listener) {
            syntax
                .arg(
                    Arg::new("event")
                        .long("event")
                        .short('e')
                        .help("The event type"),
                )
                .arg(Arg::new("queued").long("queued").action(ArgAction::SetTrue))
                .arg(Arg::new("test").long("test").action(ArgAction::SetTrue))
        } else {
            syntax
        };
        artisan.add_command(Box::new(EventMakeCommand {
            definition: CommandDefinition::new(syntax),
            kind,
        }))?;
    }
    Ok(())
}

struct EventMakeCommand {
    definition: CommandDefinition,
    kind: Kind,
}

impl Command for EventMakeCommand {
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
            let (namespace, name) = app_name(name, self.kind.namespace());
            let parts: Vec<_> =
                source_name_parts(name, "Use a PascalCase event or listener name.")?.collect();
            let class = parts.last().expect("validated name");
            let mut relative = PathBuf::from("app");
            relative.extend(namespace.map(snake_case));
            relative.extend(parts.iter().map(|part| snake_case(part)));
            let relative = relative.with_extension("rs");
            let path = app.base_path().join(&relative);
            let directory_module = path.with_extension("").join("mod.rs");
            if directory_module
                .try_exists()
                .map_err(|error| ConsoleError::new(error.to_string()))?
            {
                return Err(ConsoleError::new(format!(
                    "{} already defines the module.",
                    directory_module.display()
                )));
            }
            let (stub, event) = self.kind.template(args)?;
            let template = stub
                .load(app.base_path())
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let code = template
                .replace("{{class}}", class)
                .replace("{{event}}", &event);
            let force = args.get_flag("force");
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
                .push_str("Declare the module before using the generated type.\n");

            if matches!(self.kind, Kind::Listener) && args.get_flag("test") {
                let test = namespace
                    .into_iter()
                    .chain(parts.iter().copied())
                    .collect::<Vec<_>>()
                    .join("/");
                let test = format!("{test}Test");
                test_make_command::generate_matching(app, &test, force, output)
                    .await
                    .map_err(|error| ConsoleError::new(format!(
                        "Listener created. Could not generate its test: {error} Retry with make:test {test}."
                    )))?;
            }
            Ok(0)
        })
    }
}

impl Kind {
    fn namespace(self) -> &'static str {
        match self {
            Self::Event => "Events",
            Self::Listener => "Listeners",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Event => "Event",
            Self::Listener => "Listener",
        }
    }

    fn template(self, args: &ArgMatches) -> Result<(Stub, String), ConsoleError> {
        if matches!(self, Self::Event) {
            return Ok((Stub::Event, String::new()));
        }
        let event = args
            .get_one::<String>("event")
            .filter(|name| !name.is_empty());
        let stub = match (event.is_some(), args.get_flag("queued")) {
            (false, false) => Stub::Listener,
            (true, false) => Stub::TypedListener,
            (false, true) => Stub::QueuedListener,
            (true, true) => Stub::TypedQueuedListener,
        };
        Ok((
            stub,
            event
                .map(|name| event_path(name))
                .transpose()?
                .unwrap_or_default(),
        ))
    }
}

fn app_name<'a>(name: &'a str, namespace: &'static str) -> (Option<&'static str>, &'a str) {
    match name
        .strip_prefix("App/")
        .or_else(|| name.strip_prefix("App\\"))
    {
        Some(name) => (None, name),
        None => (Some(namespace), name),
    }
}

fn event_path(name: &str) -> Result<String, ConsoleError> {
    if name.contains("::") {
        let valid = name.split("::").all(|part| {
            let part = part.strip_prefix("r#").unwrap_or(part);
            part.as_bytes()
                .first()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte == b'_')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        });
        if valid {
            return Ok(name.to_owned());
        }
        return Err(ConsoleError::new("Use a Rust type path for the event."));
    }

    let name = name.strip_prefix('\\').unwrap_or(name);
    let (namespace, name) = app_name(name, "Events");
    let mut parts = source_name_parts(name, "Use a PascalCase event name or a Rust type path.")?
        .collect::<Vec<_>>();
    let class = parts.pop().expect("validated name");
    let modules = namespace
        .into_iter()
        .chain(parts)
        .chain([class])
        .map(|name| format!("r#{}", snake_case(name)))
        .collect::<Vec<_>>()
        .join("::");
    Ok(format!("crate::app::{modules}::{class}"))
}
