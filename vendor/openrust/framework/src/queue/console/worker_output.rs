use std::{fmt::Write as _, io::Write as _};

use chrono::{DateTime, SecondsFormat};
use chrono_tz::Tz;
use clap::ArgMatches;
use serde_json::{Value, json};

use crate::{
    console::Output,
    contracts::queue::Job,
    queue::{
        WorkResult,
        worker::{JobOutcome, WorkerStatus},
    },
};

pub(super) struct WorkerOutput<'a> {
    output: &'a mut Output,
    json: bool,
    suppressed: bool,
    once: bool,
    timezone: Tz,
}

impl<'a> WorkerOutput<'a> {
    pub(super) fn new(output: &'a mut Output, args: &ArgMatches, timezone: Tz) -> Self {
        Self {
            output,
            json: args.get_flag("json"),
            suppressed: args.get_flag("quiet") || args.get_flag("silent"),
            once: args.get_flag("once"),
            timezone,
        }
    }

    pub(super) fn write(&mut self, event: WorkerStatus<'_>) {
        if self.suppressed || (self.once && matches!(event, WorkerStatus::Stopped(_))) {
            return;
        }
        let now = chrono::Utc::now().with_timezone(&self.timezone);
        let line = if self.json {
            record(event, now).to_string()
        } else {
            format!("  {} {}", now.format("%Y-%m-%d %H:%M:%S"), summary(event))
        };
        if self.output.streams_to_terminal() {
            // A closed output pipe must not interrupt an already reserved job.
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
        } else {
            writeln!(self.output.stdout, "{line}").expect("writing to a String");
        }
    }
}

fn record(event: WorkerStatus<'_>, now: DateTime<Tz>) -> Value {
    let timestamp = now.to_rfc3339_opts(SecondsFormat::Micros, false);
    match event {
        WorkerStatus::Paused(event) => {
            queue_status(&event.connection, &event.queue, "paused", timestamp)
        }
        WorkerStatus::Resumed(event) => {
            queue_status(&event.connection, &event.queue, "resumed", timestamp)
        }
        WorkerStatus::Stopped(event) => json!({
            "level": "info", "connection": event.connection_name, "queue": event.queue,
            "status": "stopped", "reason": event.reason, "exit_code": event.status,
            "jobs_processed": event.jobs_processed, "memory": null, "timestamp": timestamp,
        }),
        WorkerStatus::Job(job, outcome) => job_status(job, outcome, timestamp),
    }
}

fn queue_status(connection: &str, queue: &str, status: &str, timestamp: String) -> Value {
    json!({"level": "warning", "connection": connection, "queue": queue, "status": status, "timestamp": timestamp})
}

fn job_status(job: &dyn Job, outcome: Option<JobOutcome<'_>>, timestamp: String) -> Value {
    let status = match &outcome {
        None => "starting",
        Some(JobOutcome {
            result: WorkResult::Released,
            cause: Some(_),
            ..
        }) => "released_after_exception",
        Some(JobOutcome {
            result: WorkResult::Failed,
            ..
        }) => "failed",
        Some(_) => "success",
    };
    let mut entry = json!({
        "level": if matches!(status, "starting" | "success") {"info"} else {"warning"},
        "id": job.get_job_id(), "uuid": job.uuid(), "connection": job.get_connection_name(),
        "queue": job.get_queue(), "job": job.payload().name(), "status": status,
        "attempts": job.attempts(), "timestamp": timestamp,
    });
    if let Some(outcome) = outcome {
        entry["duration"] = json!(outcome.elapsed.as_micros() as f64 / 1_000_000.0);
        if let Some(error) = outcome.cause {
            entry["exception"] = json!("QueueError");
            entry["message"] = json!(error.to_string());
        }
        let result = if job.is_deleted() {
            Some("deleted")
        } else if job.is_released() {
            Some("released")
        } else if job.has_failed() {
            Some("failed")
        } else {
            None
        };
        if let Some(result) = result {
            entry["result"] = json!(result);
        }
    }
    entry
}

fn summary(event: WorkerStatus<'_>) -> String {
    match event {
        WorkerStatus::Paused(event) => format!("Queue {} PAUSED", event.queue),
        WorkerStatus::Resumed(event) => format!("Queue {} RESUMED", event.queue),
        WorkerStatus::Stopped(event) => format!("Worker STOPPED {}", event.reason.description()),
        WorkerStatus::Job(job, outcome) => match outcome {
            None => format!("{} RUNNING", job.payload().name()),
            Some(outcome) => {
                let status = match outcome.result {
                    WorkResult::Failed => "FAIL",
                    WorkResult::Released if outcome.cause.is_some() => "RELEASED",
                    _ => "DONE",
                };
                format!(
                    "{} {}ms {status}",
                    job.payload().name(),
                    outcome.elapsed.as_millis()
                )
            }
        },
    }
}
