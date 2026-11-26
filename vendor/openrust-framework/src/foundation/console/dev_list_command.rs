use std::{
    fmt::Write as _,
    io::{self, IsTerminal as _},
};

use clap::{Arg, ArgAction};
use console::{Term, measure_text_width, truncate_str};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    foundation::{Application, DevCommand, DevCommands, DevProcess},
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let command = clap::Command::new("dev:list")
        .about("List the registered dev processes")
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Output the dev process list as JSON"),
        )
        .arg(
            Arg::new("filter")
                .long("filter")
                .help("Filter the dev processes by name or command"),
        )
        .arg(
            Arg::new("except-vendor")
                .long("except-vendor")
                .action(ArgAction::SetTrue)
                .help("Do not display dev processes registered by vendor packages"),
        )
        .arg(
            Arg::new("only-vendor")
                .long("only-vendor")
                .action(ArgAction::SetTrue)
                .help("Only display dev processes registered by vendor packages"),
        );
    artisan.add_command(Box::new(DevListCommand(CommandDefinition::new(command))))
}

struct DevListCommand(CommandDefinition);

impl Command for DevListCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        _app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let options = input.matches();
            let filter = options
                .get_one::<String>("filter")
                .filter(|value| !value.is_empty() && *value != "0");
            let except_vendor = options.get_flag("except-vendor");
            let only_vendor = options.get_flag("only-vendor");
            let filtering = filter.is_some() || except_vendor || only_vendor;
            let mut processes = DevCommands::commands()?;
            processes.retain(|process| {
                let vendor = process.priority == DevCommand::PRIORITY_VENDOR;
                filter.is_none_or(|pattern| {
                    process.name.contains(pattern) || process.command.contains(pattern)
                }) && !(except_vendor && vendor || only_vendor && !vendor)
            });
            let status = i32::from(filtering && processes.is_empty());
            if options.get_flag("json")
                || options.get_flag("no-interaction")
                || !io::stdin().is_terminal()
                || !output.streams_to_terminal()
            {
                let json = serde_json::to_string(&processes)
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
                let _ = writeln!(output.stdout, "{json}");
            } else if processes.is_empty() {
                let message = if filtering {
                    "Your application doesn't have any dev processes matching the given criteria."
                } else {
                    "Your application doesn't have any dev processes."
                };
                let _ = writeln!(output.stdout, "\n  {message}\n");
            } else {
                render_interactive(&processes, output);
            }
            Ok(status)
        })
    }
}

fn render_interactive(processes: &[DevProcess], output: &mut Output) {
    let label_width = processes
        .iter()
        .map(|process| measure_text_width(&process.name))
        .max()
        .unwrap_or_default();
    let columns = usize::from(Term::stdout().size().1);
    let decorated = output.is_decorated();
    output.stdout.push('\n');
    for process in processes {
        let padding = " ".repeat(label_width - measure_text_width(&process.name));
        let color = process
            .color
            .strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok());
        if let Some(rgb) = color.filter(|_| decorated) {
            let red = (rgb >> 16) & 255;
            let green = (rgb >> 8) & 255;
            let blue = rgb & 255;
            let _ = write!(
                output.stdout,
                "  \x1b[38;2;{red};{green};{blue}m{}{padding}\x1b[39m",
                process.name,
            );
        } else {
            let _ = write!(output.stdout, "  {}{padding}", process.name);
        }
        let available =
            columns.saturating_sub(label_width + measure_text_width(&process.command) + 6);
        let dots = ".".repeat(available.saturating_sub(measure_text_width(&process.source)));
        let source = if available == 0 {
            "".into()
        } else {
            truncate_str(&process.source, available, "…")
        };
        let _ = writeln!(output.stdout, " {} {dots} {source}", process.command);
    }
    let suffix = if processes.len() == 1 { "" } else { "s" };
    let total = format!("Showing [{}] dev command{suffix}", processes.len());
    let padding = " ".repeat(columns.saturating_sub(measure_text_width(&total) + 2));
    let _ = writeln!(output.stdout, "\n{padding}{total}\n");
}
