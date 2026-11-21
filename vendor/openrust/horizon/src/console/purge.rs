use super::error;
use crate::Repository;
use openrust::{
    console::{ConsoleError, Input, Output},
    process::Factory,
};

pub(super) async fn purge(
    repository: &Repository,
    input: &Input,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    #[cfg(unix)]
    {
        let signal = input
            .matches()
            .get_one::<String>("signal")
            .expect("default");
        let signal = match signal.as_str() {
            "SIGTERM" | "TERM" => "TERM".to_owned(),
            "SIGKILL" | "KILL" => "KILL".into(),
            "SIGINT" | "INT" => "INT".into(),
            value
                if value
                    .parse::<u8>()
                    .is_ok_and(|value| value > 0 && value < 128) =>
            {
                value.into()
            }
            _ => return Err(ConsoleError::new("Unknown process signal.")),
        };
        let result = Factory::new()
            .command(["ps", "-axo", "pid=,ppid=,command="])
            .run()
            .await
            .map_err(error)?
            .throw()
            .map_err(error)?;
        let executable = std::env::current_exe()
            .map_err(error)?
            .to_string_lossy()
            .to_string();
        for master in repository
            .masterSupervisors()
            .names()
            .await
            .map_err(error)?
        {
            let mut orphans = Vec::new();
            let mut owned = std::collections::BTreeSet::new();
            for line in String::from_utf8_lossy(result.output()).lines() {
                let mut fields = line.split_whitespace();
                let pid = fields.next().and_then(|value| value.parse::<u32>().ok());
                let parent = fields.next().and_then(|value| value.parse::<u32>().ok());
                let command = fields.collect::<Vec<_>>().join(" ");
                if command.starts_with(&format!("{executable} artisan horizon:"))
                    && (command.contains(&format!("--supervisor={master}:"))
                        || command.contains(&format!("horizon:supervisor {master}:")))
                    && let Some(pid) = pid
                {
                    owned.insert(pid);
                    if parent == Some(1) {
                        orphans.push(pid);
                    }
                }
            }
            repository
                .processes()
                .orphaned(&master, &orphans)
                .await
                .map_err(error)?;
            let expired = repository
                .processes()
                .orphanedFor(
                    &master,
                    repository
                        .supervisorRepository()
                        .longestActiveTimeout()
                        .await
                        .map_err(error)?,
                )
                .await
                .map_err(error)?;
            for pid in orphans
                .iter()
                .chain(&expired)
                .copied()
                .collect::<std::collections::BTreeSet<_>>()
            {
                // A persisted PID may have been reused after the original process exited.
                if !owned.contains(&pid) {
                    repository
                        .processes()
                        .forgetOrphans(&master, &[pid])
                        .await
                        .map_err(error)?;
                    continue;
                }
                output.info(&format!("Sending {signal} signal to process: {pid}"));
                let result = Factory::new()
                    .command([
                        "kill".to_owned(),
                        "-s".into(),
                        signal.clone(),
                        pid.to_string(),
                    ])
                    .run()
                    .await
                    .map_err(error)?;
                if result.successful() {
                    repository
                        .processes()
                        .forgetOrphans(&master, &[pid])
                        .await
                        .map_err(error)?;
                } else {
                    output.error(&format!("Failed to send {signal} signal to process: {pid}"));
                }
            }
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (repository, input, output);
        Err(ConsoleError::new(
            "Horizon process inspection requires Unix.",
        ))
    }
}
