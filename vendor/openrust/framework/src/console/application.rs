use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    io::{self, IsTerminal as _},
    sync::{Arc, OnceLock},
    time::Duration,
};

use clap::{Arg, ArgAction, ArgMatches, Command as Syntax, error::ErrorKind};

use crate::contracts::foundation::Application as _;
use crate::{cache::Cache, foundation::Application as FoundationApplication};

use super::{
    ClosureCommand, Command, CommandDefinition, ConsoleError, DebugFormatterHelper,
    DefaultQuestionHelper, FormatterHelper, HelperSet, Input, Output, ProcessHelper, descriptor,
    help_command, list_command, localization,
    task_command::{self, TaskCommand},
};

#[derive(Clone)]
enum Entry {
    List(CommandDefinition),
    Help(CommandDefinition),
    Tasks(TaskCommand, CommandDefinition),
    Planned(CommandDefinition),
    Closure(ClosureCommand),
    Command(Arc<dyn Command>, HelperSet),
}

impl Entry {
    fn helper_set(&self) -> Option<&HelperSet> {
        match self {
            Self::Closure(command) => command.get_helper_set(),
            Self::Command(_, helpers) => Some(helpers),
            _ => None,
        }
    }

    fn definition(&self) -> &CommandDefinition {
        match self {
            Self::List(definition)
            | Self::Help(definition)
            | Self::Tasks(_, definition)
            | Self::Planned(definition) => definition,
            Self::Closure(command) => command.definition(),
            Self::Command(command, _) => command.definition(),
        }
    }
}

/// Owns the command registry, name resolution, and dispatch.
#[derive(Clone)]
pub struct Application {
    commands: BTreeMap<String, Entry>,
    helper_set: Arc<OnceLock<HelperSet>>,
}

impl Default for Application {
    fn default() -> Self {
        let mut console = Self {
            commands: BTreeMap::new(),
            helper_set: Arc::default(),
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

    /// 按首次查询或注册的需要建立当前应用自己的默认助手集合。
    pub fn get_helper_set(&self) -> HelperSet {
        self.helper_set
            .get_or_init(|| {
                let helpers = HelperSet::new();
                helpers.set(Arc::new(FormatterHelper::new()), None);
                helpers.set(Arc::new(DebugFormatterHelper::new()), None);
                helpers.set(Arc::new(ProcessHelper::new()), None);
                helpers.set(Arc::new(DefaultQuestionHelper::new()), None);
                helpers
            })
            .clone()
    }

    /// 新注册命令继承此集合。已登记命令保留登记时取得的同一集合。
    pub fn set_helper_set(&mut self, helpers: HelperSet) {
        self.helper_set = Arc::new(OnceLock::from(helpers));
    }

    pub fn add_command(&mut self, command: Box<dyn Command>) -> Result<(), ConsoleError> {
        let helpers = self.get_helper_set();
        self.add(Entry::Command(Arc::from(command), helpers))
            .map(|_| ())
    }

    pub fn get_command_helper_set(&self, name: &str) -> Result<Option<HelperSet>, ConsoleError> {
        let name = self.resolve(name)?;
        Ok(self.commands[name].helper_set().cloned())
    }

    /// 显式覆盖登记命令的集合，别名解析到同一登记项。
    pub fn set_command_helper_set(
        &mut self,
        name: &str,
        helpers: HelperSet,
    ) -> Result<(), ConsoleError> {
        let name = self.resolve(name)?.to_owned();
        match self.commands.get_mut(&name) {
            Some(Entry::Command(_, set)) => *set = helpers,
            Some(Entry::Closure(command)) => command.set_helper_set(helpers),
            _ => {
                return Err(ConsoleError::new(format!(
                    "命令不提供可覆盖的控制台助手集合：{name}"
                )));
            }
        }
        Ok(())
    }

    /// 注册Isolatable类型，自动装配可选隔离开关。
    pub fn add_isolatable_command(
        &mut self,
        command: Box<dyn crate::contracts::console::Isolatable>,
    ) -> Result<(), ConsoleError> {
        self.add_command(Box::new(super::IsolatedCommand::new(command)))
    }

    pub(crate) fn register_task_commands(&mut self) -> Result<(), ConsoleError> {
        for operation in [
            TaskCommand::Reload,
            TaskCommand::Optimize,
            TaskCommand::OptimizeClear,
        ] {
            self.add(Entry::Tasks(
                operation,
                CommandDefinition::new(operation.command()),
            ))?;
        }
        Ok(())
    }

    /// Registers a closure as an Artisan command.
    pub fn command(
        &mut self,
        signature: &'static str,
        callback: impl Fn(&mut Output) + Send + Sync + 'static,
    ) -> Result<&mut ClosureCommand, ConsoleError> {
        self.command_with(signature, callback)
    }

    /// 注册可读取输入或异步解析应用依赖的命令闭包。
    pub fn command_with<Arguments>(
        &mut self,
        signature: &'static str,
        callback: impl super::IntoCommandCallback<Arguments>,
    ) -> Result<&mut ClosureCommand, ConsoleError> {
        let entry = self.add(Entry::Closure(ClosureCommand {
            definition: CommandDefinition::from_signature(signature)?,
            callback: callback.into_command_callback(),
            helper_set: Some(self.get_helper_set()),
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
                localization::text("command_missing", "Command ':name' is not defined.")
                    .replace(":name", name),
            )),
            _ => Err(ConsoleError::new(
                localization::text(
                    "command_ambiguous",
                    "Command ':name' is ambiguous: :choices",
                )
                .replace(":choices", &candidates.join(", "))
                .replace(":name", name),
            )),
        }
    }

    fn syntax(&self, version: &'static str) -> Syntax {
        Syntax::new("artisan")
            .about("应用命令行接口")
            .disable_help_subcommand(true)
            .version(version)
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

    fn parsing_syntax(&self, version: &'static str) -> Syntax {
        let mut syntax = self.syntax(version);
        for (name, entry) in &self.commands {
            if let Entry::Command(command, _) = entry
                && command.prompting().is_some()
            {
                syntax = syntax.mut_subcommand(name, super::prompting::relaxed);
            }
        }
        syntax
    }

    pub(crate) async fn handle(
        &self,
        application: &FoundationApplication,
        arguments: &[OsString],
        output: &mut Output,
    ) -> i32 {
        self.handle_with_shutdown(application, arguments, output, None)
            .await
    }

    pub(crate) async fn handle_with_shutdown(
        &self,
        application: &FoundationApplication,
        arguments: &[OsString],
        output: &mut Output,
        shutdown: Option<tokio_util::sync::CancellationToken>,
    ) -> i32 {
        let flags = || arguments.iter().take_while(|argument| *argument != "--");
        let silent = flags().any(|argument| argument == "--silent");
        let quiet = flags().any(|argument| argument == "--quiet" || argument == "-q");
        let stdout_start = output.stdout.len();
        let stderr_start = output.stderr.len();
        let status = self
            .handle_arguments(application, arguments, output, shutdown)
            .await;
        if quiet || silent {
            output.suppress_stdout_since(stdout_start);
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
        shutdown: Option<tokio_util::sync::CancellationToken>,
    ) -> i32 {
        let mut syntax = self.syntax(application.version());
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
                    application.render_console_exception(output, error).await;
                    return 1;
                }
            }
        }
        let parse = |syntax: &mut Syntax, arguments: &[OsString]| {
            syntax.try_get_matches_from_mut(
                std::iter::once(OsString::from("artisan")).chain(arguments.iter().cloned()),
            )
        };
        let mut parsing = self.parsing_syntax(application.version());
        let mut result = parse(&mut parsing, &normalized);
        if matches!(&result, Err(error) if error.kind() == ErrorKind::MissingRequiredArgument)
            && output.streams_to_terminal()
            && io::stdin().is_terminal()
            && let (Some(name), Some(position)) = (command, position)
        {
            match self.prompt_for_missing_argument(application, name, &normalized) {
                Ok(Some(arguments)) => {
                    normalized.splice(position + 1..position + 1, arguments);
                    result = parse(&mut parsing, &normalized);
                }
                Ok(None) => {}
                Err(error) => {
                    application.render_console_exception(output, error).await;
                    return 1;
                }
            }
        }
        let mut matches = match result {
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
                let flags = || arguments.iter().take_while(|argument| *argument != "--");
                let decorated = flags().any(|argument| argument == "--ansi")
                    || (output.is_decorated() && !flags().any(|argument| argument == "--no-ansi"));
                return observe_shortcut(application, "help", || {
                    output
                        .stdout
                        .push_str(&descriptor::help(&target, decorated));
                    0
                })
                .await;
            }
            Err(version) => {
                return observe_shortcut(application, "version", || {
                    output.stdout.push_str(&version.to_string());
                    0
                })
                .await;
            }
        };

        if matches.get_flag("ansi") {
            output.set_decorated(true);
        } else if matches.get_flag("no-ansi") {
            output.set_decorated(false);
        }

        let parsed_quiet = matches.get_flag("quiet");
        let parsed_silent = matches.get_flag("silent");
        let stdout_start = output.stdout.len();
        let stderr_start = output.stderr.len();
        let result = if let Some((name, matches)) = matches.remove_subcommand() {
            let entry = &self.commands[&name];
            self.dispatch(
                entry,
                application,
                (
                    position
                        .and_then(|p| arguments[p].to_str())
                        .unwrap_or(&name),
                    &matches,
                    None,
                ),
                &syntax,
                output,
                shutdown,
            )
            .await
        } else {
            Ok(observe_shortcut(application, "list", || {
                list_command::run(self, &syntax, None, output)
            })
            .await)
        };
        let status = match result {
            Ok(status) => status,
            Err(error) => {
                let status = error.command_exit_code();
                application.render_console_exception(output, error).await;
                status
            }
        };
        if parsed_quiet || parsed_silent {
            output.suppress_stdout_since(stdout_start);
        }
        if parsed_silent {
            output
                .stderr
                .truncate(stderr_start.min(output.stderr.len()));
        }
        status
    }

    fn prompt_for_missing_argument(
        &self,
        application: &FoundationApplication,
        command: &str,
        arguments: &[OsString],
    ) -> Result<Option<Vec<OsString>>, ConsoleError> {
        let Some((argument, prompt)) = &self.commands[command].definition().missing_argument else {
            return Ok(None);
        };
        let relaxed = self
            .syntax(application.version())
            .mut_subcommand(command, |syntax| {
                syntax.mut_arg(*argument, |arg| arg.required(false))
            });
        let Ok(matches) = relaxed.try_get_matches_from(
            std::iter::once(OsString::from("artisan")).chain(arguments.iter().cloned()),
        ) else {
            return Ok(None);
        };
        let input: &ArgMatches = matches
            .subcommand_matches(command)
            .expect("resolved command");
        if input.contains_id(argument)
            || ["no-interaction", "quiet", "silent"]
                .into_iter()
                .any(|flag| matches.get_flag(flag))
        {
            return Ok(None);
        }
        prompt(input).map(Some)
    }

    pub(crate) async fn call(
        &self,
        application: &FoundationApplication,
        command: &str,
        input: &Input,
        output: &mut Output,
    ) -> Result<i32, ConsoleError> {
        let mut syntax = self.syntax(application.version());
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
        self.dispatch(
            entry,
            application,
            (command, input.matches(), Some(input)),
            &syntax,
            output,
            None,
        )
        .await
    }

    pub(crate) async fn call_arguments(
        &self,
        application: &FoundationApplication,
        command: &str,
        arguments: &[OsString],
        output: &mut Output,
    ) -> Result<i32, ConsoleError> {
        let name = self.resolve(command)?;
        let mut syntax = self.syntax(application.version());
        syntax.build();
        let parsed = self
            .parsing_syntax(application.version())
            .try_get_matches_from_mut(
                ["artisan", name]
                    .into_iter()
                    .map(OsStr::new)
                    .chain(arguments.iter().map(OsString::as_os_str)),
            );
        let flags = || arguments.iter().take_while(|argument| *argument != "--");
        let suppress_stdout = || {
            flags().any(|argument| matches!(argument.to_str(), Some("--silent" | "--quiet" | "-q")))
        };
        let mut parsed = match parsed {
            Ok(matches) => matches,
            Err(help) if help.kind() == ErrorKind::DisplayHelp => {
                if !suppress_stdout() {
                    let target = syntax.find_subcommand(name).expect("resolved command");
                    let decorated = flags().any(|argument| argument == "--ansi")
                        || (output.is_decorated()
                            && !flags().any(|argument| argument == "--no-ansi"));
                    output.stdout.push_str(&descriptor::help(
                        &target.clone().bin_name(format!("artisan {name}")),
                        decorated,
                    ));
                }
                return Ok(0);
            }
            Err(version) if version.kind() == ErrorKind::DisplayVersion => {
                if !suppress_stdout() {
                    output.stdout.push_str(&version.to_string());
                }
                return Ok(0);
            }
            Err(error) => return Err(ConsoleError::new(localization::argument_error(&error))),
        };
        let (stdout, stderr) = (output.stdout.len(), output.stderr.len());
        let decorated = (output.is_decorated(), output.progress_is_decorated());
        let generations = (
            output.formatter_generation,
            output.error_formatter_generation,
        );
        let decoration = ScopedDecoration {
            output,
            decorated,
            generations,
        };
        let output = &mut *decoration.output;
        let (_, matches) = parsed.remove_subcommand().expect("resolved command");
        if matches.get_flag("ansi") {
            output.set_decorated(true);
        } else if matches.get_flag("no-ansi") {
            output.set_decorated(false);
        }
        let result = self
            .dispatch(
                &self.commands[name],
                application,
                (command, &matches, None),
                &syntax,
                output,
                None,
            )
            .await;
        if matches.get_flag("quiet") || matches.get_flag("silent") {
            output.suppress_stdout_since(stdout);
        }
        if matches.get_flag("silent") {
            output.stderr.truncate(stderr);
        }
        result
    }

    pub(super) async fn call_task(
        &self,
        application: &FoundationApplication,
        syntax: &Syntax,
        name: &str,
        environment: Option<&String>,
    ) -> Result<i32, ConsoleError> {
        let canonical = self.resolve(name)?;
        let entry = &self.commands[canonical];
        if matches!(entry, Entry::Tasks(..)) {
            return Err(ConsoleError::new(
                "An Artisan task cannot call another task command.",
            ));
        }
        let mut arguments = vec!["artisan", canonical];
        if let Some(environment) = environment {
            arguments.extend(["--env", environment]);
        }
        let mut parsed = self
            .parsing_syntax(application.version())
            .try_get_matches_from(arguments)
            .map_err(|error| ConsoleError::new(error.to_string()))?;
        let (_, matches) = parsed.remove_subcommand().expect("resolved command");
        Box::pin(self.dispatch(
            entry,
            application,
            (name, &matches, None),
            syntax,
            &mut Output::default(),
            None,
        ))
        .await
    }

    async fn dispatch(
        &self,
        entry: &Entry,
        application: &FoundationApplication,
        invocation: (&str, &clap::ArgMatches, Option<&Input>),
        syntax: &Syntax,
        output: &mut Output,
        shutdown: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<i32, ConsoleError> {
        let (invoked_name, matches, supplied_input) = invocation;
        let (verbosity, suppressed, silent) = (output.verbosity, output.suppressed, output.silent);
        let error_verbosity = output.error_verbosity;
        output.silent |= matches
            .try_get_one::<bool>("silent")
            .ok()
            .flatten()
            .copied()
            .unwrap_or(false);
        output.verbosity = verbosity.max(
            matches
                .try_get_one::<u8>("verbose")
                .ok()
                .flatten()
                .copied()
                .unwrap_or(0),
        );
        output.suppressed = suppressed
            || ["quiet", "silent"].into_iter().any(|name| {
                matches
                    .try_get_one::<bool>(name)
                    .ok()
                    .flatten()
                    .copied()
                    .unwrap_or(false)
            });
        if matches
            .try_get_one::<u8>("verbose")
            .ok()
            .flatten()
            .is_some_and(|level| *level > 0)
            || ["quiet", "silent"].into_iter().any(|name| {
                matches
                    .try_get_one::<bool>(name)
                    .ok()
                    .flatten()
                    .copied()
                    .unwrap_or(false)
            })
        {
            output.error_verbosity = None;
        }
        let stdout_start = output.stdout.len();
        let stderr_start = output.stderr.len();
        let scope = ScopedOutput {
            output,
            stdout_start,
            stderr_start,
            verbosity,
            suppressed,
            silent,
            error_verbosity,
        };
        let output = &mut *scope.output;
        let origin = crate::log::context::Origin::enabled(application.container()).then(|| {
            match crate::log::context::Origin::current() {
                Some(origin @ crate::log::context::Origin::Http(_)) => origin,
                _ => crate::log::context::Origin::Console {
                    command: Some(entry.definition().name().to_owned()),
                },
            }
        });
        use crate::foundation::observability::{self, Operation, Outcome};
        let observation = observability::begin(
            application.container(),
            Operation::Command {
                name: entry.definition().name(),
            },
        );
        let execution = crate::log::context::Origin::scope(
            application.container_handle(),
            origin,
            Box::pin(async {
                let command: &dyn Command = match entry {
                    Entry::List(_) => {
                        return Ok(list_command::run(self, syntax, Some(matches), output));
                    }
                    Entry::Help(_) => return help_command::run(self, syntax, matches, output),
                    Entry::Tasks(operation, _) => {
                        return task_command::run(
                            self,
                            application,
                            syntax,
                            *operation,
                            matches,
                            output,
                        )
                        .await;
                    }
                    Entry::Planned(definition) => {
                        return Err(ConsoleError::new(format!(
                            "Command '{}' is not implemented yet.",
                            definition.name(),
                        )));
                    }
                    Entry::Closure(command) => command,
                    Entry::Command(command, _) => command.as_ref(),
                };
                let mut parsing = self.parsing_syntax(application.version());
                parsing.build();
                let mut input = Input {
                    syntax: command.definition().syntax.clone(),
                    parsing_syntax: Arc::new(
                        parsing
                            .find_subcommand(command.definition().name())
                            .expect("已注册命令存在于解析定义")
                            .clone(),
                    ),
                    invoked_name: Some(invoked_name.to_owned()),
                    matches: matches.clone(),
                    shutdown: shutdown
                        .or_else(|| supplied_input.and_then(|input| input.shutdown.clone())),
                    interaction: supplied_input
                        .map(|input| input.interaction.shared())
                        .unwrap_or_default(),
                    stream: supplied_input.and_then(|input| input.stream.clone()),
                    helper_set: entry.helper_set().cloned(),
                };
                if output.suppressed
                    || input.option("no-interaction") == Some(serde_json::Value::Bool(true))
                {
                    input.interaction.set(false);
                }
                if let Some(prompting) = command.prompting() {
                    super::prompting::prompt(prompting, application, &mut input, output).await?;
                }
                let matches = input.matches();
                let definition = command.definition();
                let Some(code) = definition
                    .isolated
                    .then(|| matches.get_one::<i32>("isolated").copied())
                    .flatten()
                else {
                    return run_command_with_events(command, application, &input, output).await;
                };

                let name = definition.name();
                let mut key = format!("framework/command-{name}");
                let mut lease = Duration::from_secs(3600);
                if let Some(isolation) = command.isolation() {
                    if let Some(id) = isolation.isolatable_id(&input)? {
                        key.push('-');
                        key.push_str(&id);
                    }
                    lease = super::isolation::lease(isolation.isolation_lock_expires_at(&input)?);
                }
                let lock_error =
                    |error| ConsoleError::from_source(super::isolation::LockError(error));
                let lock = Cache::lock(&key, Some(lease)).map_err(lock_error)?;
                let Some(lock) = crate::cache::acquire_scoped_lock(lock, None)
                    .await
                    .map_err(lock_error)?
                else {
                    output.comment(&format!("The [{name}] command is already running."));
                    return Ok(code);
                };
                let result = run_command_with_events(command, application, &input, output).await;
                let released = lock.release().await.map_err(lock_error);
                result.and_then(|status| released.map(|_| status))
            }),
        );
        observation
            .run(execution, |result| match result {
                Ok(code) => Outcome::Command(*code),
                Err(_) => Outcome::Failed,
            })
            .await
    }
}

async fn observe_shortcut(
    application: &FoundationApplication,
    name: &'static str,
    handle: impl FnOnce() -> i32,
) -> i32 {
    use crate::foundation::observability::{self, Operation, Outcome};
    observability::begin(application.container(), Operation::Command { name })
        .run(async { handle() }, |code| Outcome::Command(*code))
        .await
}

async fn run_command_with_events(
    command: &dyn Command,
    application: &FoundationApplication,
    input: &Input,
    output: &mut Output,
) -> Result<i32, ConsoleError> {
    use crate::contracts::events::Dispatcher as _;
    let events = application
        .make::<crate::events::Dispatcher>()
        .await
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let name = command.definition().name().to_owned();
    events
        .dispatch(
            Arc::new(super::events::CommandStarting {
                command: name.clone(),
                input: input.matches().clone(),
                definition: command.definition().syntax.clone(),
            }),
            &mut crate::events::EventContext::new(),
        )
        .await
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    let offset = output.stdout.len();
    let generation = output.capture_generation;
    let started = std::time::Instant::now();
    let status = command.handle(application, input, output).await?;
    let duration = started.elapsed();
    events
        .dispatch(
            Arc::new(super::events::CommandFinished {
                command: name,
                input: input.matches().clone(),
                definition: command.definition().syntax.clone(),
                exit_code: status,
                duration,
                output: if generation == output.capture_generation {
                    output
                        .stdout
                        .get(offset..)
                        .unwrap_or(&output.stdout)
                        .to_owned()
                } else {
                    output.stdout.clone()
                },
            }),
            &mut crate::events::EventContext::new(),
        )
        .await
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    Ok(status)
}

// 长时间运行的命令被取消时也恢复当前调用的输出配置。
struct ScopedOutput<'a> {
    output: &'a mut Output,
    stdout_start: usize,
    stderr_start: usize,
    verbosity: u8,
    suppressed: bool,
    silent: bool,
    error_verbosity: Option<super::OutputVerbosity>,
}
impl Drop for ScopedOutput<'_> {
    fn drop(&mut self) {
        if self.output.suppressed {
            self.output.suppress_stdout_since(self.stdout_start);
        }
        if self.output.silent {
            self.output
                .stderr
                .truncate(self.stderr_start.min(self.output.stderr.len()));
        }
        self.output.verbosity = self.verbosity;
        self.output.suppressed = self.suppressed;
        self.output.silent = self.silent;
        self.output.error_verbosity = self.error_verbosity;
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

/// 嵌套调用完成或取消时一并恢复两条流的装饰设置。
struct ScopedDecoration<'a> {
    output: &'a mut Output,
    decorated: (bool, bool),
    generations: (u64, u64),
}
impl Drop for ScopedDecoration<'_> {
    fn drop(&mut self) {
        if self.output.formatter_generation == self.generations.0 {
            self.output.formatter.set_decorated(self.decorated.0);
        }
        if self.output.terminal && self.output.error_formatter_generation == self.generations.1 {
            self.output.set_progress_decorated(self.decorated.1);
        }
    }
}
