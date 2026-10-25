//! `openrust`: global command line of the OpenRust framework.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command as Process, ExitCode};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};

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
        /// Add a PostgreSQL or MySQL connection and migration crate.
        #[arg(long, value_enum)]
        db: Option<DatabaseBackend>,
    },
    /// Run the application in development mode.
    Dev,
    /// Run the application's server command.
    Serve,
    /// Any other command is forwarded to the application binary in the current project.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DatabaseBackend {
    #[value(name = "postgres")]
    Postgres,
    #[value(name = "mysql")]
    Mysql,
}

impl DatabaseBackend {
    fn feature(self) -> &'static str {
        match self {
            Self::Postgres => "db-postgres",
            Self::Mysql => "db-mysql",
        }
    }

    fn url(self, database: &str) -> String {
        match self {
            Self::Postgres => format!("postgres://openrust:openrust@localhost:5432/{database}"),
            Self::Mysql => format!("mysql://openrust:openrust@localhost:3306/{database}"),
        }
    }
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
        Some(Builtin::New { name, db }) => {
            create_project(&name, db, &std::env::current_dir()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Dev) => run_project(&["serve"], true),
        Some(Builtin::Serve) => run_project(&["serve"], false),
        Some(Builtin::Forward(args)) => forward(&args),
    }
}

/// Creates a starter application under the current directory.
fn create_project(
    name: &str,
    database: Option<DatabaseBackend>,
    current_dir: &Path,
) -> anyhow::Result<()> {
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

    let result = populate_project(&project_dir, project_name, &type_name, database);
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

fn populate_project(
    project_dir: &Path,
    name: &str,
    crate_name: &str,
    database: Option<DatabaseBackend>,
) -> anyhow::Result<()> {
    fs::create_dir(project_dir)
        .with_context(|| format!("failed to create {}", project_dir.display()))?;
    let module_name = name.replace('-', "_");
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
    if database.is_some() {
        fs::create_dir_all(project_dir.join("migration/src"))?;
    }

    let mut files = vec![
        ("Cargo.toml", cargo_manifest(name, database)),
        (".env.example", env_example(name, database)),
        ("config/default.toml", include_str!("../templates/config/default.toml").replace("{{name}}", name)),
        ("config/development.toml", include_str!("../templates/config/development.toml").to_owned()),
        ("config/test.toml", "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name)),
        ("config/production.toml", "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name)),
        ("src/main.rs", main_source(crate_name, database)),
        ("src/app.rs", app_source(crate_name, &module_name, database)),
        ("src/routes.rs", include_str!("../templates/src/routes.rs").to_owned()),
        ("src/schedule.rs", "//! Application schedule.\n".to_owned()),
        ("src/seed.rs", seed_source(database)),
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
    if let Some(database) = database {
        files.extend([
            ("migration/Cargo.toml", migration_manifest(name, database)),
            ("migration/src/lib.rs", migration_lib()),
            (
                "migration/src/m20261025_000001_create_posts.rs",
                initial_migration(),
            ),
        ]);
    }
    for (relative_path, contents) in files {
        fs::write(project_dir.join(relative_path), contents)
            .with_context(|| format!("failed to write {relative_path}"))?;
    }
    Ok(())
}

fn cargo_manifest(name: &str, database: Option<DatabaseBackend>) -> String {
    let mut manifest = include_str!("../templates/Cargo.toml").replace("{{name}}", name);
    if let Some(database) = database {
        manifest = manifest.replace(
            "openrust = \"0.0.0\"",
            &format!("openrust = {{ version = \"0.0.0\", features = [\"{}\"] }}\n{name}-migration = {{ path = \"migration\" }}", database.feature()),
        );
    }
    manifest
}

fn app_source(crate_name: &str, module_name: &str, database: Option<DatabaseBackend>) -> String {
    let plugins = if database.is_some() {
        format!(
            ".plugin(DatabasePlugin)\n            .plugin(MigrationPlugin::<{module_name}_migration::Migrator>::new())\n            .plugin(SeedPlugin::new(crate::seed::run))"
        )
    } else {
        String::new()
    };
    include_str!("../templates/src/app.rs")
        .replace("{{type_name}}", crate_name)
        .replace("{{database_plugins}}", &plugins)
        .replace("{{migration_plugin}}", "")
}

fn main_source(crate_name: &str, database: Option<DatabaseBackend>) -> String {
    let database_module = if database.is_some() { "mod seed;" } else { "" };
    let source = include_str!("../templates/src/main.rs").replace("{{type_name}}", crate_name);
    if database_module.is_empty() {
        source.replace("{{database_module}}\n", "")
    } else {
        source.replace("{{database_module}}", database_module)
    }
}

fn env_example(name: &str, database: Option<DatabaseBackend>) -> String {
    let mut contents = "OPENRUST_ENV=development\n".to_owned();
    if let Some(database) = database {
        use std::fmt::Write as _;
        let _ = writeln!(contents, "DATABASE_URL={}", database.url(name));
    }
    contents
}

fn seed_source(database: Option<DatabaseBackend>) -> String {
    if database.is_some() {
        "//! Application seed entry point.\n\nuse openrust::{Db, Result};\n\npub async fn run(_database: Db) -> Result<()> {\n    Ok(())\n}\n".to_owned()
    } else {
        "//! Application seed entry point.\n".to_owned()
    }
}

fn migration_manifest(name: &str, database: DatabaseBackend) -> String {
    format!(
        "[package]\nname = \"{name}-migration\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nopenrust = {{ version = \"0.0.0\", features = [\"{}\"] }}\n",
        database.feature()
    )
}

fn migration_lib() -> String {
    "//! Application database migrations.\n\nmod m20261025_000001_create_posts;\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\npub struct Migrator;\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigratorTrait for Migrator {\n    fn migrations() -> Vec<Box<dyn MigrationTrait>> {\n        vec![Box::new(m20261025_000001_create_posts::Migration)]\n    }\n}\n".to_owned()
}

fn initial_migration() -> String {
    "//! Creates the starter posts table.\n\nuse openrust::database::{dialect, sea_orm_migration::prelude::*};\n\n#[derive(DeriveIden)]\nenum Posts {\n    Table,\n    Id,\n    Title,\n    CreatedAt,\n}\n\npub struct Migration;\n\nimpl MigrationName for Migration {\n    fn name(&self) -> &'static str {\n        \"m20261025_000001_create_posts\"\n    }\n}\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigrationTrait for Migration {\n    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {\n        manager\n            .create_table(\n                Table::create()\n                    .table(Posts::Table)\n                    .if_not_exists()\n                    .col(ColumnDef::new(Posts::Id).big_integer().not_null().primary_key())\n                    .col(ColumnDef::new(Posts::Title).string().not_null())\n                    .col(dialect::timestamp_utc(Posts::CreatedAt, manager.get_database_backend()))\n                    .to_owned(),\n            )\n            .await\n    }\n\n    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {\n        manager\n            .drop_table(Table::drop().table(Posts::Table).if_exists().to_owned())\n            .await\n    }\n}\n".to_owned()
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
        insta::assert_snapshot!(cargo_manifest("my-app", None));
    }

    #[test]
    fn project_template_has_expected_layout_and_compilable_test() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None).expect("generate project");
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
    fn database_templates_include_backend_migration_and_seed_support() {
        let root = std::env::temp_dir().join(format!("openrust-cli-db-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        for (backend, feature) in [
            (DatabaseBackend::Postgres, "db-postgres"),
            (DatabaseBackend::Mysql, "db-mysql"),
        ] {
            let project = root.join(feature);
            populate_project(&project, "blog", "Blog", Some(backend))
                .expect("generate database project");
            let manifest =
                fs::read_to_string(project.join("Cargo.toml")).expect("read generated manifest");
            assert!(manifest.contains(feature));
            assert!(project.join("migration/Cargo.toml").is_file());
            assert!(project.join("migration/src/lib.rs").is_file());
            assert!(
                project
                    .join("migration/src/m20261025_000001_create_posts.rs")
                    .is_file()
            );
            let app = fs::read_to_string(project.join("src/app.rs")).expect("read generated app");
            assert!(app.contains("DatabasePlugin"));
            assert!(app.contains("blog_migration::Migrator"));
            assert!(app.contains("SeedPlugin"));
            assert!(project.join(".env.example").is_file());
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_parent_paths_and_existing_destinations() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        assert!(create_project("../outside", None, &root).is_err());
        assert!(create_project("--", None, &root).is_err());
        fs::create_dir(root.join("existing")).expect("create existing destination");
        assert!(create_project("existing", None, &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
