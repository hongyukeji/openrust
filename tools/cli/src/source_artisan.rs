//! 源码 Artisan 先分派构建工具命令，其他命令继续由应用执行。

use std::path::Path;
use std::process::{Command, ExitCode};

use cli::release::BuildCommand;
use openrust::console::ConsoleInvocation;

pub(crate) fn run(project: &Path, arguments: Vec<String>) -> anyhow::Result<ExitCode> {
    let invocation = ConsoleInvocation::parse(arguments.iter().cloned(), "list");
    match invocation.name() {
        BuildCommand::NAME => {
            BuildCommand::run_source(project, invocation.arguments(), invocation.environment())
        }
        "help" if invocation.arguments() == [BuildCommand::NAME] => {
            BuildCommand::run_source(project, &["--help".into()], invocation.environment())
        }
        _ => {
            let mut application = Command::new("cargo");
            application
                .current_dir(project)
                .args(["run", "--quiet", "--features", "source-artisan", "--"])
                .args(arguments)
                .env("OPENRUST_ARTISAN", "1");
            exec_application(application)
        }
    }
}

#[cfg(unix)]
fn exec_application(mut application: Command) -> anyhow::Result<ExitCode> {
    use std::os::unix::process::CommandExt as _;

    Err(application.exec().into())
}

#[cfg(not(unix))]
fn exec_application(mut application: Command) -> anyhow::Result<ExitCode> {
    Ok(crate::exit_code(application.status()?))
}
