use std::{collections::BTreeMap, ffi::OsString, sync::Arc};

use clap::{Arg, ArgAction, Command as Syntax};

use crate::foundation::Application as FoundationApplication;

use super::{
    ClosureCommand, Command, CommandDefinition, ConsoleError, Input, Output, descriptor,
    help_command, list_command, localization,
};

#[derive(Clone)]
enum Entry {
    List(CommandDefinition),
    Help(CommandDefinition),
    Planned(CommandDefinition),
    Closure(ClosureCommand),
    Command(Arc<dyn Command>),
}

impl Entry {
    fn definition(&self) -> &CommandDefinition {
        match self {
            Self::List(definition) | Self::Help(definition) | Self::Planned(definition) => {
                definition
            }
            Self::Closure(command) => command.definition(),
            Self::Command(command) => command.definition(),
        }
    }
}

/// Owns the command registry, name resolution, and dispatch.
#[derive(Clone)]
pub struct Application {
    commands: BTreeMap<String, Entry>,
}

impl Default for Application {
    fn default() -> Self {
        let mut console = Self {
            commands: BTreeMap::new(),
        };
        for entry in [
            Entry::List(CommandDefinition::new(list_command::command())),
            Entry::Help(CommandDefinition::new(help_command::command())),
        ] {
            console
                .commands
                .insert(entry.definition().name().to_owned(), entry);
        }
        console
    }
}

impl Application {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_command(&mut self, command: Box<dyn Command>) -> Result<(), ConsoleError> {
        self.add(Entry::Command(Arc::from(command))).map(|_| ())
    }

    /// Registers a closure as an Artisan command.
    pub fn command(
        &mut self,
        name: &'static str,
        callback: impl Fn(&mut Output) + Send + Sync + 'static,
    ) -> Result<&mut ClosureCommand, ConsoleError> {
        let entry = self.add(Entry::Closure(ClosureCommand {
            definition: CommandDefinition::new(Syntax::new(name)),
            callback: Arc::new(callback),
        }))?;
        let Entry::Closure(command) = entry else {
            unreachable!("registered closure command");
        };
        Ok(command)
    }

    pub fn add_planned(&mut self, syntax: Syntax) -> Result<(), ConsoleError> {
        self.add(Entry::Planned(CommandDefinition::new(syntax.after_help(
            "This command is planned and is not implemented yet.",
        ))))
        .map(|_| ())
    }

    pub(crate) fn set_closure_purpose(
        &mut self,
        name: &str,
        definition: &std::sync::Weak<Syntax>,
        description: &'static str,
    ) -> Result<(), ConsoleError> {
        match self.commands.get_mut(name) {
            Some(Entry::Closure(command)) if command.registration_key().ptr_eq(definition) => {
                command.purpose(description);
                Ok(())
            }
            _ => Err(ConsoleError::new(format!(
                "Command '{name}' is no longer active in this registration scope."
            ))),
        }
    }

    fn add(&mut self, entry: Entry) -> Result<&mut Entry, ConsoleError> {
        let definition = entry.definition();
        let name = definition.name();
        for candidate in std::iter::once(name).chain(definition.syntax.get_all_aliases()) {
            if self.commands.values().any(|registered| {
                let syntax = &registered.definition().syntax;
                syntax.get_name() != name
                    && (syntax.get_name() == candidate
                        || syntax.get_all_aliases().any(|alias| alias == candidate))
            }) {
                return Err(ConsoleError::new(format!(
                    "Command name or alias '{candidate}' is already in use."
                )));
            }
        }
        Ok(self
            .commands
            .entry(name.to_owned())
            .insert_entry(entry)
            .into_mut())
    }

    pub fn all(&self) -> Vec<&CommandDefinition> {
        self.commands.values().map(Entry::definition).collect()
    }

    pub(super) fn is_implemented(&self, name: &str) -> bool {
        !matches!(self.commands.get(name), Some(Entry::Planned(_)))
    }

    pub(super) fn resolve(&self, name: &str) -> Result<&str, ConsoleError> {
        let matches = |candidate: &str| {
            candidate.starts_with(name)
                || (candidate.split(':').count() == name.split(':').count()
                    && candidate
                        .split(':')
                        .zip(name.split(':'))
                        .all(|(segment, prefix)| !prefix.is_empty() && segment.starts_with(prefix)))
        };
        let mut candidates = Vec::new();
        for (canonical, entry) in &self.commands {
            let syntax = &entry.definition().syntax;
            if canonical == name || syntax.get_all_aliases().any(|alias| alias == name) {
                return Ok(canonical);
            }
            if !syntax.is_hide_set()
                && (matches(canonical) || syntax.get_visible_aliases().any(matches))
            {
                candidates.push(canonical.as_str());
            }
        }
        match candidates.as_slice() {
            [canonical] => Ok(canonical),
            [] => Err(ConsoleError::new(
                localization::text(
                    "command_missing",
                    "Command ':name' is not defined.",
                    "命令「:name」不存在。",
                    "命令「:name」不存在。",
                )
                .replace(":name", name),
            )),
            _ => Err(ConsoleError::new(
                localization::text(
                    "command_ambiguous",
                    "Command ':name' is ambiguous: :choices",
                    "命令「:name」有歧义：:choices",
                    "命令「:name」有歧義：:choices",
                )
                .replace(":choices", &candidates.join(", "))
                .replace(":name", name),
            )),
        }
    }

    fn syntax(&self) -> Syntax {
        Syntax::new("artisan")
            .about("OpenRust command-line interface")
            .disable_help_subcommand(true)
            .version(env!("CARGO_PKG_VERSION"))
            .propagate_version(true)
            .arg(
                Arg::new("quiet")
                    .long("quiet")
                    .short('q')
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Suppress normal output"),
            )
            .arg(
                Arg::new("silent")
                    .long("silent")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Suppress output and errors"),
            )
            .arg(
                Arg::new("verbose")
                    .long("verbose")
                    .short('v')
                    .global(true)
                    .action(ArgAction::Count)
                    .help("Increase verbosity"),
            )
            .arg(
                Arg::new("no-interaction")
                    .long("no-interaction")
                    .short('n')
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Do not ask interactive questions"),
            )
            .arg(
                Arg::new("ansi")
                    .long("ansi")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Force ANSI output"),
            )
            .arg(
                Arg::new("no-ansi")
                    .long("no-ansi")
                    .global(true)
                    .action(ArgAction::SetTrue)
                    .help("Disable ANSI output"),
            )
            .arg(
                Arg::new("env")
                    .long("env")
                    .global(true)
                    .action(ArgAction::Append)
                    .value_name("ENV")
                    .help("The environment the command should run under"),
            )
            .subcommands(
                self.commands
                    .values()
                    .map(|entry| entry.definition().syntax.as_ref().clone()),
            )
    }

    pub(crate) async fn handle(
        &self,
        application: &FoundationApplication,
        arguments: &[OsString],
        output: &mut Output,
    ) -> i32 {
        let flags = || arguments.iter().take_while(|argument| *argument != "--");
        let silent = flags().any(|argument| argument == "--silent");
        let quiet = flags().any(|argument| argument == "--quiet" || argument == "-q");
        let stdout_start = output.stdout.len();
        let stderr_start = output.stderr.len();
        let status = self.handle_arguments(application, arguments, output).await;
        if quiet || silent {
            output.stdout.truncate(stdout_start);
        }
        if silent {
            output.stderr.truncate(stderr_start);
        }
        status
    }

    async fn handle_arguments(
        &self,
        application: &FoundationApplication,
        arguments: &[OsString],
        output: &mut Output,
    ) -> i32 {
        let mut syntax = self.syntax();
        syntax.build();
        let position = command_position(arguments);
        let mut normalized = arguments.to_vec();
        let mut command = None;
        if let Some(position) = position
            && let Some(name) = arguments[position].to_str()
        {
            match self.resolve(name) {
                Ok(name) => {
                    normalized[position] = name.into();
                    command = Some(name);
                }
                Err(error) => {
                    application.render_console_exception(output, &error).await;
                    return 1;
                }
            }
        }
        let mut matches = match syntax
            .try_get_matches_from_mut(std::iter::once(OsString::from("artisan")).chain(normalized))
        {
            Ok(matches) => matches,
            Err(error) if error.use_stderr() => {
                output
                    .stderr
                    .push_str(&localization::argument_error(&error));
                return 1;
            }
            Err(help) if help.kind() == clap::error::ErrorKind::DisplayHelp => {
                let target = command
                    .and_then(|name| syntax.find_subcommand(name))
                    .unwrap_or(&syntax);
                let mut target = target.clone();
                if let Some(name) = command {
                    target = target.bin_name(format!("artisan {name}"));
                }
                output.stdout.push_str(&descriptor::help(&target, true));
                return 0;
            }
            Err(version) => {
                output.stdout.push_str(&version.to_string());
                return 0;
            }
        };

        if matches.get_flag("ansi") {
            output.decorated = true;
        } else if matches.get_flag("no-ansi") {
            output.decorated = false;
        }

        let result = if let Some((name, matches)) = matches.remove_subcommand() {
            let entry = &self.commands[&name];
            self.dispatch(entry, application, &matches, &syntax, output)
                .await
        } else {
            Ok(list_command::run(self, &syntax, None, output))
        };
        match result {
            Ok(status) => status,
            Err(error) => {
                application.render_console_exception(output, &error).await;
                1
            }
        }
    }

    pub(crate) async fn call(
        &self,
        application: &FoundationApplication,
        command: &str,
        input: &Input,
        output: &mut Output,
    ) -> Result<i32, ConsoleError> {
        let mut syntax = self.syntax();
        syntax.build();
        let definition = syntax
            .find_subcommand(self.resolve(command)?)
            .expect("resolved command is registered");
        let entry = &self.commands[definition.get_name()];
        if !std::sync::Arc::ptr_eq(&entry.definition().syntax, &input.syntax) {
            return Err(ConsoleError::new(format!(
                "Input was parsed for a different definition of '{}'.",
                input.syntax.get_name()
            )));
        }
        self.dispatch(entry, application, input.matches(), &syntax, output)
            .await
    }

    async fn dispatch(
        &self,
        entry: &Entry,
        application: &FoundationApplication,
        matches: &clap::ArgMatches,
        syntax: &Syntax,
        output: &mut Output,
    ) -> Result<i32, ConsoleError> {
        let command: &dyn Command = match entry {
            Entry::List(_) => return Ok(list_command::run(self, syntax, Some(matches), output)),
            Entry::Help(_) => return help_command::run(self, syntax, matches, output),
            Entry::Planned(definition) => {
                return Err(ConsoleError::new(format!(
                    "Command '{}' is not implemented yet.",
                    definition.name(),
                )));
            }
            Entry::Closure(command) => command,
            Entry::Command(command) => command.as_ref(),
        };
        let input = Input {
            syntax: command.definition().syntax.clone(),
            matches: matches.clone(),
        };
        command.handle(application, &input, output).await
    }
}

fn command_position(arguments: &[OsString]) -> Option<usize> {
    let mut arguments = arguments.iter().enumerate();
    while let Some((index, argument)) = arguments.next() {
        if argument == "--" {
            return None;
        }
        if argument == "--env" {
            arguments.next();
            continue;
        }
        if !argument.as_encoded_bytes().starts_with(b"-") {
            return Some(index);
        }
    }
    None
}
