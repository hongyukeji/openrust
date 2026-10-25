//! `openrust`: global command line of the OpenRust framework.

#![forbid(unsafe_code)]

use std::path::Path;
use std::process::{Command as Process, ExitCode};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};

/// OpenRust command line.
#[derive(Debug, Parser)]
#[command(name = "openrust", version, about, long_about = None, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Builtin>,
}

#[derive(Debug, Subcommand)]
enum Builtin {
    /// Any other command is forwarded to the application binary in the current project.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        None => {
            Cli::parse_from(["openrust", "--help"]);
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Forward(args)) => forward(&args),
    }
}

/// Runs `cargo run --quiet -- <args>` in the current project and mirrors its exit status.
fn forward(args: &[String]) -> anyhow::Result<ExitCode> {
    if !Path::new("Cargo.toml").exists() {
        bail!("no Cargo.toml in the current directory; run this inside an OpenRust project");
    }
    let status = Process::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .args(args)
        .status()
        .context("failed to start `cargo run`")?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from))
}
