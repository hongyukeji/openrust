use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    support::ProviderResult,
};
use openrust_horizon::{
    AddSupervisor, CommandFuture, Error, MasterSupervisor, MasterSupervisorCommandHandler, Result,
    Supervisor, SupervisorCommandHandler, supervisor_commands,
};
use serde_json::{Value, json};
use std::{
    io::Write as _,
    path::{Path, PathBuf},
    sync::Arc,
};

const DEPLOY: &str = r"Probe\Deploy";
const STOP: &str = r"Probe\Stop";

struct Deploy(PathBuf);

impl MasterSupervisorCommandHandler for Deploy {
    fn process<'a>(
        &'a self,
        master: &'a mut MasterSupervisor,
        options: Value,
    ) -> CommandFuture<'a, ()> {
        Box::pin(async move {
            record(&self.0, DEPLOY, master.name(), &options)?;
            AddSupervisor.process(master, options)
        })
    }
}

struct SupervisorProbe {
    path: PathBuf,
    command: &'static str,
    status: Option<u8>,
}

impl SupervisorCommandHandler for SupervisorProbe {
    fn process<'a>(
        &'a self,
        supervisor: &'a mut Supervisor,
        options: Value,
    ) -> CommandFuture<'a, Option<u8>> {
        Box::pin(async move {
            record(&self.path, self.command, supervisor.name(), &options)?;
            Ok(self.status)
        })
    }
}

fn record(path: &Path, command: &str, name: &str, options: &Value) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| Error::Process)?;
    writeln!(
        file,
        "{}",
        json!({"command": command, "name": name, "options": options})
    )
    .map_err(|_| Error::Process)
}

pub fn register(app: &Application) -> ProviderResult {
    let path = app.base_path().join("command-handlers.jsonl");
    let container = app.container();
    container.instance_named::<dyn MasterSupervisorCommandHandler>(
        DEPLOY,
        Arc::new(Deploy(path.clone())),
    )?;
    container.instance_named::<dyn SupervisorCommandHandler>(
        supervisor_commands::PAUSE,
        Arc::new(SupervisorProbe {
            path: path.clone(),
            command: supervisor_commands::PAUSE,
            status: None,
        }),
    )?;
    container.instance_named::<dyn SupervisorCommandHandler>(
        STOP,
        Arc::new(SupervisorProbe {
            path,
            command: STOP,
            status: Some(23),
        }),
    )?;
    // 同名异类型服务不能作为命令处理器运行。
    container.instance_named("Probe\\Wrong", Arc::new(false))?;
    Ok(())
}
