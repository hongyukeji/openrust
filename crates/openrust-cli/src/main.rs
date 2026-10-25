//! `openrust`: global command line of the OpenRust framework.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
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
    /// Create a new OpenRust application from the built-in starter template.
    New {
        /// Application directory name or destination path.
        name: String,
    },
    /// Run the application in development mode.
    Dev,
    /// Run the application's server command.
    Serve,
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
        Some(Builtin::New { name }) => {
            create_project(&name, &std::env::current_dir()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Dev) => run_project(&["serve"], true),
        Some(Builtin::Serve) => run_project(&["serve"], false),
        Some(Builtin::Forward(args)) => forward(&args),
    }
}

/// Creates a starter application under the current directory.
fn create_project(name: &str, current_dir: &Path) -> anyhow::Result<()> {
    let name_path = Path::new(name);
    if name_path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("project path must not contain `..`");
    }
    let project_name = name_path
        .file_name()
        .and_then(|value| value.to_str())
        .context("project path must end with a valid directory name")?;
    let project_dir = if name_path.is_absolute() {
        name_path.to_path_buf()
    } else {
        current_dir.join(name_path)
    };
    if project_dir.exists() {
        bail!("destination already exists: {}", project_dir.display());
    }

    let snake_name = project_name.replace('-', "_");
    if snake_name.is_empty()
        || !project_name
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        || !snake_name
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
        || !snake_name
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
    {
        bail!(
            "project name must contain only ASCII letters, digits, `_` or `-`, and start with a letter or `_`"
        );
    }
    let type_name = project_name
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect::<String>();
    if type_name.is_empty() {
        bail!("project name must contain at least one letter or digit");
    }

    let result = populate_project(&project_dir, project_name, &type_name);
    if result.is_err() {
        let _ = fs::remove_dir_all(&project_dir);
    }
    result?;
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "Created OpenRust application `{project_name}` at {}",
        project_dir.display()
    )?;
    writeln!(
        stdout,
        "Next steps: cd {} && openrust dev",
        project_dir.display()
    )?;
    Ok(())
}

fn populate_project(project_dir: &Path, name: &str, crate_name: &str) -> anyhow::Result<()> {
    fs::create_dir(project_dir)
        .with_context(|| format!("failed to create {}", project_dir.display()))?;
    for directory in [
        "config",
        "src/controllers",
        "src/models/_entities",
        "src/services",
        "src/policies",
        "src/jobs",
        "src/events",
        "src/mailers",
        "src/middleware",
        "templates/mail",
        "tests",
    ] {
        fs::create_dir_all(project_dir.join(directory))?;
    }

    let files = [
        ("Cargo.toml", cargo_manifest(name)),
        (".env.example", "OPENRUST_ENV=development\n".to_owned()),
        ("config/default.toml", include_str!("../templates/config/default.toml").replace("{{name}}", name)),
        ("config/development.toml", include_str!("../templates/config/development.toml").to_owned()),
        ("config/test.toml", "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name)),
        ("config/production.toml", "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name)),
        ("src/main.rs", include_str!("../templates/src/main.rs").replace("{{type_name}}", crate_name)),
        ("src/app.rs", include_str!("../templates/src/app.rs").replace("{{type_name}}", crate_name)),
        ("src/routes.rs", include_str!("../templates/src/routes.rs").to_owned()),
        ("src/schedule.rs", "//! Application schedule.\n".to_owned()),
        ("src/seed.rs", "//! Application seed entry point.\n".to_owned()),
        ("src/controllers/mod.rs", "//! HTTP controllers.\n".to_owned()),
        ("src/models/mod.rs", "//! Application models.\n".to_owned()),
        ("src/models/_entities/.gitkeep", String::new()),
        ("src/services/mod.rs", "//! Application services.\n".to_owned()),
        ("src/policies/mod.rs", "//! Authorization policies.\n".to_owned()),
        ("src/jobs/mod.rs", "//! Background jobs.\n".to_owned()),
        ("src/events/mod.rs", "//! Application events.\n".to_owned()),
        ("src/mailers/mod.rs", "//! Email messages.\n".to_owned()),
        ("src/middleware/mod.rs", "//! Application middleware.\n".to_owned()),
        ("tests/application.rs", "#[test]\nfn application_package_is_generated() {\n    assert_eq!(env!(\"CARGO_PKG_NAME\"), \"{{name}}\");\n}\n".replace("{{name}}", name)),
        ("AGENTS.md", include_str!("../templates/AGENTS.md").to_owned()),
    ];
    for (relative_path, contents) in files {
        fs::write(project_dir.join(relative_path), contents)
            .with_context(|| format!("failed to write {relative_path}"))?;
    }
    Ok(())
}

fn cargo_manifest(name: &str) -> String {
    include_str!("../templates/Cargo.toml").replace("{{name}}", name)
}

/// Executes the project binary and returns its exit code.
fn run_project(args: &[&str], development: bool) -> anyhow::Result<ExitCode> {
    if !Path::new("Cargo.toml").exists() {
        bail!("no Cargo.toml in the current directory; run this inside an OpenRust project");
    }
    let mut command = Process::new("cargo");
    command.arg("run").arg("--quiet").arg("--").args(args);
    if development {
        command.env("OPENRUST_ENV", "development");
    }
    command
        .status()
        .context("failed to start `cargo run`")
        .map(exit_code)
}

/// Runs `cargo run --quiet -- <args>` in the current project and mirrors its exit status.
fn forward(args: &[String]) -> anyhow::Result<ExitCode> {
    if !Path::new("Cargo.toml").exists() {
        bail!("no Cargo.toml in the current directory; run this inside an OpenRust project");
    }
    Process::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--")
        .args(args)
        .status()
        .context("failed to start `cargo run`")
        .map(exit_code)
}

fn exit_code(status: std::process::ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or(ExitCode::FAILURE, ExitCode::from)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "test setup failures must fail the test immediately"
)]
mod tests {
    use super::*;

    #[test]
    fn generated_manifest_matches_starter_template() {
        insta::assert_snapshot!(cargo_manifest("my-app"));
    }

    #[test]
    fn project_template_has_expected_layout_and_compilable_test() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog").expect("generate project");
        for path in [
            "Cargo.toml",
            "src/main.rs",
            "src/app.rs",
            "src/routes.rs",
            "config/default.toml",
            "tests/application.rs",
            "AGENTS.md",
        ] {
            assert!(project.join(path).is_file(), "missing generated {path}");
        }
        let snapshot = [
            "Cargo.toml",
            "config/default.toml",
            "src/main.rs",
            "src/app.rs",
            "src/routes.rs",
            "tests/application.rs",
            "AGENTS.md",
        ]
        .into_iter()
        .map(|path| {
            format!(
                "--- {path} ---\n{}",
                fs::read_to_string(project.join(path)).expect("read generated snapshot file")
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
        insta::assert_snapshot!(snapshot);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_parent_paths_and_existing_destinations() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        assert!(create_project("../outside", &root).is_err());
        assert!(create_project("--", &root).is_err());
        fs::create_dir(root.join("existing")).expect("create existing destination");
        assert!(create_project("existing", &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
