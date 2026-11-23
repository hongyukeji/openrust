use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Arg, ArgAction, Command as Syntax, value_parser};
use serde::Serialize;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, queue::QueueMetrics},
    foundation::Application,
    queue::{QueueManager, events::QueueBusy, queue_name},
};

use super::work_command::error;

struct MonitorCommand(CommandDefinition);

#[derive(Serialize)]
struct QueueStatus {
    connection: String,
    queue: String,
    #[serde(flatten)]
    metrics: QueueMetrics,
    status: &'static str,
}

impl Command for MonitorCommand {
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
            let args = input.matches();
            let maximum = *args.get_one::<u64>("max").expect("default maximum");
            let targets = args.get_one::<String>("queues").expect("required queues");
            let manager = app
                .container_handle()
                .make::<QueueManager>()
                .map_err(error)?;
            let mut rows = Vec::new();
            for target in targets.split(',').map(str::trim) {
                let (connection, name) = target
                    .split_once(':')
                    .map_or((None, target), |(connection, name)| {
                        (Some(connection), name)
                    });
                let queue = manager.connection(connection).map_err(error)?;
                queue_name(Some(name), queue.default_queue()).map_err(error)?;
                let metrics = queue.metrics(Some(name)).await.map_err(error)?;
                rows.push(QueueStatus {
                    connection: queue.get_connection_name().into(),
                    queue: name.into(),
                    status: if metrics.size >= maximum {
                        "ALERT"
                    } else {
                        "OK"
                    },
                    metrics,
                });
            }

            if args.get_flag("json") {
                output
                    .stdout
                    .push_str(&serde_json::to_string(&rows).map_err(error)?);
                output.stdout.push('\n');
            } else {
                render_sizes(output, &rows);
            }

            for row in rows.into_iter().filter(|row| row.metrics.size >= maximum) {
                manager
                    .emit_control_event(QueueBusy {
                        connection_name: row.connection,
                        queue: row.queue,
                        size: row.metrics.size,
                    })
                    .await
                    .map_err(error)?;
            }
            Ok(0)
        })
    }
}

fn render_sizes(output: &mut Output, rows: &[QueueStatus]) {
    let count = |value: Option<u64>| value.map_or_else(|| "N/A".into(), |value| value.to_string());
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    output.stdout.push_str("\nQueue name   Size / Status\n");
    for row in rows {
        let age = row.metrics.oldest_pending.map_or_else(
            || "N/A".into(),
            |timestamp| format!("{}s ago", now.saturating_sub(timestamp)),
        );
        output.stdout.push_str(&format!(
            "[{}] {}   [{}] {}\nPending jobs: {}\nDelayed jobs: {}\nReserved jobs: {}\nOldest pending job: {age}\n\n",
            row.connection, row.queue, row.metrics.size, row.status, count(row.metrics.pending),
            count(row.metrics.delayed), count(row.metrics.reserved),
        ));
    }
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let syntax = Syntax::new("queue:monitor")
        .about("Monitor the size of the specified queues")
        .arg(
            Arg::new("queues")
                .required(true)
                .help("The names of the queues to monitor"),
        )
        .arg(
            Arg::new("max")
                .long("max")
                .default_value("1000")
                .value_parser(value_parser!(u64))
                .help("The maximum number of jobs before an event is dispatched"),
        )
        .arg(
            Arg::new("json")
                .long("json")
                .action(ArgAction::SetTrue)
                .help("Output the queue size as JSON"),
        );
    artisan.add_command(Box::new(MonitorCommand(CommandDefinition::new(syntax))))
}
