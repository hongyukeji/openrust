//! Provider 顺序诊断共用正常启动的准备和实际注册表。

use std::io::{self, Write as _};
use std::process::ExitCode;

use serde_json::{Value, json};

use super::{Command, print_help};
use crate::core::{Error, Result};
use crate::foundation::{Application, ApplicationDefinition, lifecycle};
use crate::support::BoxFuture;

pub(crate) const NAME: &str = "package:doctor";

pub(crate) struct PackageDoctorCommand;

impl Command for PackageDoctorCommand {
    fn name(&self) -> &'static str {
        NAME
    }

    fn about(&self) -> &'static str {
        "检查 Provider 注册顺序及包冲突"
    }

    fn usage(&self) -> Option<&'static str> {
        Some("[--boot]")
    }

    fn accepts_args(&self) -> bool {
        true
    }

    fn run<'a>(&'a self, app: &'a Application, args: Vec<String>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if args.as_slice() != ["--boot"] {
                return Err(invalid_arguments());
            }
            let registered: Vec<_> = app
                .diagnostic_provider_order()
                .into_iter()
                .map(|(provider, package)| json!({"provider": provider, "package": package}))
                .collect();
            write_json(&json!({"registered": registered}))
        })
    }
}

pub(crate) fn before_boot<A: ApplicationDefinition>(
    args: &[String],
    environment: Option<&str>,
) -> Option<ExitCode> {
    if args == ["--boot"] {
        return None;
    }
    if args == ["--help"] || args == ["-h"] {
        print_help(&PackageDoctorCommand);
        return Some(ExitCode::SUCCESS);
    }
    let result = if args.is_empty() {
        lifecycle::diagnose_provider_order::<A>(environment)
    } else {
        Err(invalid_arguments())
    };
    Some(match result {
        Ok(planned) => output(&json!({"planned": planned}), ExitCode::SUCCESS),
        Err(error) => error_exit(&error),
    })
}

pub(crate) fn error_exit(error: &Error) -> ExitCode {
    output(
        &json!({"error": {"code": error.code(), "message": error.to_string()}}),
        ExitCode::FAILURE,
    )
}

fn output(report: &Value, code: ExitCode) -> ExitCode {
    match write_json(report) {
        Ok(()) => code,
        Err(error) => crate::fail(&format!("{NAME}: {error}")),
    }
}

fn invalid_arguments() -> Error {
    Error::custom("invalid_arguments", "用法：package:doctor [--boot]")
}

fn write_json(report: &Value) -> Result<()> {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, report).map_err(Error::internal)?;
    writeln!(stdout)?;
    Ok(())
}
