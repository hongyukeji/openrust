//! `openrust`：OpenRust 框架的全局命令行工具。

#![forbid(unsafe_code)]

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clap::{Parser, Subcommand, ValueEnum};
use serde_json::Value;

static MIGRATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        /// 应用名称；生成到 `apps/<name>/`。
        name: String,
        /// Add a PostgreSQL or MySQL connection and migration crate.
        #[arg(long, value_enum)]
        db: Option<DatabaseBackend>,
        /// Add database-backed authentication and an authenticated example route (requires `--db`).
        #[arg(long, requires = "db")]
        auth: bool,
        /// Enable the optional outbound HTTP client.
        #[arg(long)]
        http_client: bool,
        /// Enable the optional Twilio SMS provider and outbound HTTP client.
        #[arg(long)]
        sms_twilio: bool,
        /// Enable application data encryption and key rotation.
        #[arg(long)]
        encryption: bool,
    },
    /// Run the application in development mode.
    Dev,
    /// Run the application's server command.
    Serve,
    /// Generate application models and database migrations.
    #[command(alias = "g")]
    Generate {
        #[arg(value_enum)]
        kind: GeneratorKind,
        /// Model or table name followed by `field:type` declarations.
        #[arg(required = true)]
        values: Vec<String>,
        /// Generate a `UUIDv7` primary key (use the same option for model and migration).
        #[arg(long)]
        uuid_id: bool,
        /// Add a nullable UTC `deleted_at` column and explicit query scope.
        #[arg(long)]
        soft_delete: bool,
    },
    /// Manage local application encryption keys.
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// Any other command is forwarded to the application binary in the current project.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// Generate a 32-byte encryption key and print its Base64 value.
    Generate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum GeneratorKind {
    Model,
    Migration,
    Mailer,
    Policy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum DatabaseBackend {
    #[value(name = "postgres")]
    Postgres,
    #[value(name = "mysql")]
    Mysql,
}

#[derive(Debug)]
struct WorkspacePackage {
    name: String,
    manifest_path: PathBuf,
}

#[derive(Debug)]
struct FrameworkWorkspace {
    root: PathBuf,
    packages: Vec<WorkspacePackage>,
}

fn framework_workspace(start: &Path) -> anyhow::Result<FrameworkWorkspace> {
    let output = Process::new("cargo")
        .current_dir(start)
        .args(["metadata", "--format-version=1", "--no-deps"])
        .output()
        .context("failed to run `cargo metadata`")?;
    if !output.status.success() {
        bail!("current directory is not a Cargo workspace");
    }
    let metadata: Value =
        serde_json::from_slice(&output.stdout).context("`cargo metadata` returned invalid JSON")?;
    let root = PathBuf::from(
        metadata["workspace_root"]
            .as_str()
            .context("Cargo metadata omitted workspace_root")?,
    )
    .canonicalize()?;
    let packages = metadata["packages"]
        .as_array()
        .context("Cargo metadata omitted packages")?
        .iter()
        .map(|package| {
            Ok(WorkspacePackage {
                name: package["name"]
                    .as_str()
                    .context("Cargo metadata package omitted name")?
                    .to_owned(),
                manifest_path: PathBuf::from(
                    package["manifest_path"]
                        .as_str()
                        .context("Cargo metadata package omitted manifest_path")?,
                )
                .canonicalize()?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let has_member = |name: &str, relative_manifest: &str| {
        packages.iter().any(|package| {
            package.name == name && package.manifest_path == root.join(relative_manifest)
        })
    };
    if !has_member("openrust", "Cargo.toml")
        || !has_member("cli", "cli/Cargo.toml")
        || !has_member("macros", "macros/Cargo.toml")
        || !has_member("testing", "testing/Cargo.toml")
        || !has_member("server", "apps/server/Cargo.toml")
    {
        bail!(
            "OpenRust source workspace was not found; run this command from its checkout or an app inside it"
        );
    }
    Ok(FrameworkWorkspace { root, packages })
}

fn application_directory(
    workspace: &FrameworkWorkspace,
    current_dir: &Path,
) -> anyhow::Result<(PathBuf, String)> {
    let current_dir = current_dir.canonicalize()?;
    let app_root = workspace.root.join("apps").canonicalize()?;
    if app_root.parent() != Some(workspace.root.as_path())
        || app_root.file_name().and_then(|name| name.to_str()) != Some("apps")
    {
        bail!("`apps/` must resolve to a direct directory inside the framework workspace");
    }
    let app = workspace
        .packages
        .iter()
        .filter_map(|package| {
            let directory = package.manifest_path.parent()?;
            if !directory.starts_with(&app_root)
                || directory
                    .components()
                    .any(|component| component.as_os_str() == "migration")
                || !current_dir.starts_with(directory)
            {
                return None;
            }
            Some((directory, package.name.as_str()))
        })
        .max_by_key(|(directory, _)| directory.components().count())
        .context("no OpenRust application package found; run this command from `apps/<name>`")?;
    if app.0.parent() != Some(app_root.as_path())
        || app.0.file_name().and_then(|name| name.to_str()) != Some(app.1)
    {
        bail!("application package name must match its `apps/<name>` directory");
    }
    Ok((app.0.to_path_buf(), app.1.to_owned()))
}

fn manifest_with_members(root: &Path, new_members: &[String]) -> anyhow::Result<String> {
    let manifest_path = root.join("Cargo.toml");
    let source = fs::read_to_string(&manifest_path)?;
    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .context("framework Cargo.toml is invalid TOML")?;
    let members = document
        .get_mut("workspace")
        .and_then(toml_edit::Item::as_table_mut)
        .and_then(|workspace| workspace.get_mut("members"))
        .and_then(toml_edit::Item::as_array_mut)
        .context("framework Cargo.toml must declare workspace.members as an array")?;
    for member in new_members {
        if !members
            .iter()
            .any(|existing| existing.as_str() == Some(member))
        {
            members.push(member.as_str());
        }
    }
    Ok(document.to_string())
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
        Some(Builtin::New {
            name,
            db,
            auth,
            http_client,
            sms_twilio,
            encryption,
        }) => {
            create_project(
                &name,
                db,
                auth,
                http_client,
                sms_twilio,
                encryption,
                &std::env::current_dir()?,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Dev) => run_project(&["serve"], true),
        Some(Builtin::Serve) => run_project(&["serve"], false),
        Some(Builtin::Generate {
            kind,
            values,
            uuid_id,
            soft_delete,
        }) => {
            generate(
                kind,
                &values,
                uuid_id,
                soft_delete,
                &std::env::current_dir()?,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Key {
            command: KeyCommand::Generate,
        }) => {
            writeln!(std::io::stdout().lock(), "{}", generate_key()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Forward(args)) => forward(&args),
    }
}

fn generate_key() -> anyhow::Result<String> {
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key).map_err(|_| anyhow::anyhow!("OS randomness provider failed"))?;
    Ok(STANDARD.encode(key))
}

/// 在已识别的框架 workspace 中创建应用，并注册其 workspace 成员。
#[allow(
    clippy::fn_params_excessive_bools,
    reason = "these flags mirror the public CLI options"
)]
fn create_project(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
    current_dir: &Path,
) -> anyhow::Result<()> {
    if auth && database.is_none() {
        bail!("authentication requires a database; use `openrust new --db postgres|mysql --auth`");
    }
    let workspace = framework_workspace(current_dir)?;
    let apps_root = application_root(&workspace)?;
    let project_name = application_name(name)?;
    let project_dir = apps_root.join(project_name);
    if project_dir.exists() {
        bail!("destination already exists: {}", project_dir.display());
    }

    let type_name = application_type_name(project_name)?;

    let migration_name = format!("{project_name}-migration");
    if workspace
        .packages
        .iter()
        .any(|package| package.name == project_name || package.name == migration_name)
    {
        bail!("application or migration package name is already in use: `{project_name}`");
    }
    let mut new_members = vec![format!("apps/{project_name}")];
    if database.is_some() {
        new_members.push(format!("apps/{project_name}/migration"));
    }
    let updated_workspace_manifest = manifest_with_members(&workspace.root, &new_members)?;

    let result = populate_project(
        &project_dir,
        project_name,
        &type_name,
        database,
        auth,
        http_client,
        sms_twilio,
        encryption,
    );
    if result.is_err() {
        let _ = fs::remove_dir_all(&project_dir);
    }
    result?;
    if let Err(error) = fs::write(
        workspace.root.join("Cargo.toml"),
        updated_workspace_manifest,
    ) {
        let _ = fs::remove_dir_all(&project_dir);
        return Err(error).context("failed to add the generated app to the framework workspace");
    }
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

fn application_root(workspace: &FrameworkWorkspace) -> anyhow::Result<PathBuf> {
    let apps_root = workspace.root.join("apps").canonicalize()?;
    if apps_root.parent() != Some(workspace.root.as_path())
        || apps_root.file_name().and_then(|name| name.to_str()) != Some("apps")
    {
        bail!("`apps/` must resolve to a direct directory inside the framework workspace");
    }
    Ok(apps_root)
}

fn application_name(name: &str) -> anyhow::Result<&str> {
    let requested = Path::new(name);
    if requested.is_absolute() {
        bail!(
            "external destination paths are not supported; applications belong under `apps/<name>`"
        );
    }
    if requested
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        bail!("application name must not contain `.` or `..` path components");
    }
    let components = requested
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    match components.as_slice() {
        [name] | ["apps", name] => Ok(name),
        _ => bail!("application name must be a single directory name or `apps/<name>`"),
    }
}

fn application_type_name(project_name: &str) -> anyhow::Result<String> {
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
    Ok(type_name)
}

#[allow(
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments,
    reason = "template options map directly to the public generator flags"
)]
fn populate_project(
    project_dir: &Path,
    name: &str,
    crate_name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
) -> anyhow::Result<()> {
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
    if database.is_some() {
        fs::create_dir_all(project_dir.join("migration/src"))?;
    }

    let mut files = vec![
        (
            "Cargo.toml",
            cargo_manifest(name, database, auth, http_client, sms_twilio, encryption),
        ),
        (
            ".gitignore",
            include_str!("../templates/.gitignore").to_owned(),
        ),
        (".env.example", env_example(name, database, auth, encryption)),
        ("config/default.toml", default_config(name, auth, encryption)),
        (
            "config/development.toml",
            include_str!("../templates/config/development.toml").to_owned(),
        ),
        (
            "config/test.toml",
            "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name),
        ),
        (
            "config/production.toml",
            "[app]\nname = \"{{name}}\"\n".replace("{{name}}", name),
        ),
        ("src/main.rs", main_source(crate_name, database)),
        ("src/app.rs", app_source(crate_name, database, auth, encryption)),
        ("src/routes.rs", routes_source(auth)),
        ("src/schedule.rs", "//! Application schedule.\n".to_owned()),
        ("src/seed.rs", seed_source(database)),
        (
            "src/controllers/mod.rs",
            "//! HTTP controllers.\n".to_owned(),
        ),
        ("src/models/mod.rs", "//! Application models.\n".to_owned()),
        ("src/models/_entities/.gitkeep", String::new()),
        (
            "src/services/mod.rs",
            "//! Application services.\n".to_owned(),
        ),
        (
            "src/policies/mod.rs",
            "//! Authorization policies.\n".to_owned(),
        ),
        ("src/jobs/mod.rs", "//! Background jobs.\n".to_owned()),
        ("src/events/mod.rs", "//! Application events.\n".to_owned()),
        ("src/mailers/mod.rs", "//! Email messages.\n".to_owned()),
        (
            "src/middleware/mod.rs",
            "//! Application middleware.\n".to_owned(),
        ),
        (
            "tests/application.rs",
            "fn manifest_declares_package_name(manifest: &str, package_name: &str) -> bool {\n    for line in manifest.lines() {\n        if line == format!(\"name = \\\"{package_name}\\\"\") {\n            return true;\n        }\n    }\n    false\n}\n\n#[test]\nfn application_package_is_generated() {\n    let manifest = include_str!(\"../Cargo.toml\");\n    let package_name = env!(\"CARGO_PKG_NAME\");\n    assert!(manifest_declares_package_name(manifest, package_name));\n}\n".to_string(),
        ),
        (
            "AGENTS.md",
            include_str!("../templates/AGENTS.md").to_owned(),
        ),
    ];
    if auth {
        files.push((
            "tests/auth_flows.rs",
            auth_test_template(crate_name, encryption),
        ));
    }
    if let Some(database) = database {
        files.extend([
            (
                "migration/Cargo.toml",
                migration_manifest(name, database, auth),
            ),
            ("migration/src/lib.rs", migration_lib(auth)),
        ]);
    }
    for (relative_path, contents) in files {
        fs::write(project_dir.join(relative_path), contents)
            .with_context(|| format!("failed to write {relative_path}"))?;
    }
    Ok(())
}

fn auth_test_template(crate_name: &str, encryption: bool) -> String {
    let encryption_config = if encryption {
        "\\n[security.encryption]\\nactive_key_id = \\\"test\\\"\\n\\n[security.encryption.keys]\\ntest = \\\"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=\\\""
    } else {
        ""
    };
    include_str!("../templates/tests/auth_flows.rs")
        .replace("OpenRustGeneratedApplication", crate_name)
        .replace("{{encryption_config}}", encryption_config)
}

#[allow(
    clippy::fn_params_excessive_bools,
    reason = "feature flags map directly to the public generator options"
)]
fn cargo_manifest(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
) -> String {
    let mut manifest = include_str!("../templates/Cargo.toml").replace("{{name}}", name);
    if database.is_some() || auth || http_client || sms_twilio || encryption {
        let mut features = Vec::new();
        if let Some(database) = database {
            features.push(database.feature());
        }
        if auth {
            features.push("auth");
        }
        if http_client {
            features.push("http-client");
        }
        if sms_twilio {
            features.push("sms-twilio");
        }
        if encryption {
            features.push("encryption");
        }
        let dependency = if database.is_some() {
            format!(
                "openrust = {{ workspace = true, features = {features:?} }}\nopenrust-migration = {{ package = \"{name}-migration\", path = \"migration\" }}"
            )
        } else {
            format!("openrust = {{ workspace = true, features = {features:?} }}")
        };
        manifest = manifest.replace("openrust = { workspace = true }", &dependency);
    }
    if auth {
        manifest.push_str("serde = { version = \"1\", features = [\"derive\"] }\n\n[dev-dependencies]\nbase64 = \"0.22\"\nfigment = { version = \"0.10\", features = [\"toml\"] }\nserde_json = \"1\"\ntokio = { version = \"1\", features = [\"io-util\", \"macros\", \"net\", \"rt-multi-thread\"] }\n");
        if let Some(database) = database {
            let _ = FmtWrite::write_fmt(
                &mut manifest,
                format_args!(
                    "testing = {{ workspace = true, features = [\"{}\"] }}\n",
                    database.feature()
                ),
            );
        }
    }
    manifest
}

fn app_source(
    crate_name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    encryption: bool,
) -> String {
    let plugins = if database.is_some() {
        format!(
            "\n            .plugin(DatabasePlugin){}\n            .plugin(MigrationPlugin::<openrust_migration::Migrator>::new())\n            .plugin(SeedPlugin::new(crate::seed::run))",
            if auth {
                "\n            .plugin(openrust::auth::JwtPlugin::new())\n            .plugin(openrust::auth::AuthPlugin::new())\n            .plugin(openrust::auth::AuthFlowsPlugin::new())"
                    .to_owned()
                    + "\n            .plugin(openrust::audit::AuditPlugin::database())"
                    + "\n            .plugin(\n                openrust::signing::SignedLinksPlugin::new().with_database_revocations_from_app(),\n            )"
            } else {
                String::new()
            }
        )
    } else {
        String::new()
    };
    let encryption_plugin = if encryption {
        "\n            .plugin(openrust::security::EncryptionPlugin)"
    } else {
        ""
    };
    include_str!("../templates/src/app.rs")
        .replace("{{type_name}}", crate_name)
        .replace("{{database_plugins}}", &plugins)
        .replace("{{migration_plugin}}", "")
        .replace("{{encryption_plugin}}", encryption_plugin)
}

fn routes_source(auth: bool) -> String {
    let mut source = include_str!("../templates/src/routes.rs").to_owned();
    let (registration, handler) = if auth {
        (
            "    routes\n        .get(\"/api/me\", current_user)\n        .layer(openrust::auth::AuthLayer::required(\n            openrust::auth::AuthMode::Jwt,\n        ))\n        .post(\"/auth/register\", auth_register)\n        .post(\n            \"/auth/request-email-verification\",\n            auth_request_email_verification,\n        )\n        .post(\"/auth/verify-email\", auth_verify_email)\n        .post(\"/auth/login\", auth_login)\n        .post(\"/auth/refresh\", auth_refresh)\n        .post(\"/auth/logout\", auth_logout)\n        .post(\"/auth/password-reset\", auth_request_password_reset)\n        .post(\"/auth/password-reset/confirm\", auth_reset_password)\n        .get(\"/\", root)\n        .merge(Routes::new().get(\"/api/me/pat\", current_user).layer(\n            openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),\n        ))".to_owned(),
            include_str!("../templates/src/auth_routes.rs").to_owned(),
        )
    } else {
        ("    routes.get(\"/\", root)".to_owned(), String::new())
    };
    source = source.replace("{{routes}}", &registration);
    source.replace("{{auth_handler}}", &handler)
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

fn env_example(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    encryption: bool,
) -> String {
    let mut contents = "# Copy to .env for local development. Never commit the populated .env file.\n# Production reads configuration from process environment, not this file.\nAPP_ENV=development\n\n# Optional overrides for values in config/*.toml:\n# APP_SERVER__PORT=3000\n# APP_SERVER__HOST=127.0.0.1\n# APP_API__HTTP_STATUS=always_ok\n".to_owned();
    if let Some(database) = database {
        use std::fmt::Write as _;
        contents.push('\n');
        let _ = writeln!(contents, "DATABASE_URL={}", database.url(name));
    }
    if auth {
        contents.push_str(
            "\n# Generate independent keys with: openrust key generate\nAPP_AUTH__JWT__KEYS__LOCAL=\nAPP_SECURITY__SIGNED_LINKS__KEYS__LOCAL=\n",
        );
    }
    if encryption {
        contents.push_str(
            "\n# Generate with `openrust key generate`; keep this value in a secret store.\nAPP_SECURITY__ENCRYPTION__ACTIVE_KEY_ID=local\nAPP_SECURITY__ENCRYPTION__KEYS__LOCAL=\n",
        );
    }
    contents.push_str(
        "\n# Optional SMTP delivery overrides:\n# APP_MAIL__DRIVER=smtp\n# APP_MAIL__SMTP_HOST=mail.example.com\n# APP_MAIL__SMTP_PORT=587\n# APP_MAIL__SMTP_STARTTLS=true\n# For local Mailpit on 127.0.0.1:1025 only, set SMTP_STARTTLS=false\n# APP_MAIL__SMTP_USERNAME=\n# APP_MAIL__SMTP_PASSWORD=\n# Optional Twilio SMS provider (sms-twilio feature):\n# APP_SMS__DRIVER=twilio\n# APP_SMS__ACCOUNT_SID=\n# APP_SMS__API_KEY_SID=\n# APP_SMS__API_KEY_SECRET=\n# Set exactly one of APP_SMS__FROM or APP_SMS__MESSAGING_SERVICE_SID\n",
    );
    contents
}

fn default_config(name: &str, auth: bool, encryption: bool) -> String {
    let mut config = include_str!("../templates/config/default.toml").replace("{{name}}", name);
    if auth {
        config.push_str("\n[auth.jwt]\nissuer = \"http://localhost:3000\"\naudience = \"");
        config.push_str(name);
        config.push_str("\"\nactive_key_id = \"local\"\nkeys = {}\n");
        config.push_str("\n[security.signed_links]\nissuer = \"http://localhost:3000\"\nactive_key_id = \"local\"\nkeys = {}\n");
    }
    if encryption {
        config.push_str("\n[security.encryption]\nactive_key_id = \"local\"\nkeys = {}\n");
    }
    config
}

fn seed_source(database: Option<DatabaseBackend>) -> String {
    if database.is_some() {
        "//! Application seed entry point.\n\nuse openrust::{Db, Result};\n\npub async fn run(_database: Db) -> Result<()> {\n    Ok(())\n}\n".to_owned()
    } else {
        "//! Application seed entry point.\n".to_owned()
    }
}

fn migration_manifest(name: &str, database: DatabaseBackend, auth: bool) -> String {
    let mut features = vec![database.feature()];
    if auth {
        features.push("auth");
    }
    format!(
        "[package]\nname = \"{name}-migration\"\npublish = false\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nopenrust = {{ workspace = true, features = {features:?} }}\n"
    )
}

fn migration_lib(auth: bool) -> String {
    let migrations = if auth {
        "vec![\n            Box::new(openrust::auth::AuthMigration),\n            Box::new(openrust::auth::AuthRefreshTokenMigration),\n            Box::new(openrust::auth::AuthVerificationCodeMigration),\n            Box::new(openrust::auth::AuthPolicyMigration),\n            Box::new(openrust::database::DatabaseRateLimitMigration),\n            Box::new(openrust::audit::DatabaseAuditMigration),\n            Box::new(openrust::signing::SignedLinkRevocationMigration),\n        ]"
    } else {
        "vec![]"
    };
    format!(
        "//! Application database migrations.\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\npub struct Migrator;\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigratorTrait for Migrator {{\n    fn migrations() -> Vec<Box<dyn MigrationTrait>> {{\n        {migrations}\n    }}\n}}\n"
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldType {
    String,
    Text,
    Integer,
    BigInteger,
    Boolean,
    DateTime,
    Json,
    References,
}

impl FieldType {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "string" => Some(Self::String),
            "text" => Some(Self::Text),
            "integer" => Some(Self::Integer),
            "big_integer" => Some(Self::BigInteger),
            "boolean" => Some(Self::Boolean),
            "date_time" => Some(Self::DateTime),
            "json" => Some(Self::Json),
            "references" => Some(Self::References),
            _ => None,
        }
    }

    fn rust_type(self) -> &'static str {
        match self {
            Self::String | Self::Text => "String",
            Self::Integer => "i32",
            Self::BigInteger | Self::References => "i64",
            Self::Boolean => "bool",
            Self::DateTime => "DateTimeUtc",
            Self::Json => "Json",
        }
    }

    fn migration_builder(self) -> Option<&'static str> {
        match self {
            Self::String => Some("string"),
            Self::Text => Some("text"),
            Self::Integer => Some("integer"),
            Self::BigInteger | Self::References => Some("big_integer"),
            Self::Boolean => Some("boolean"),
            Self::DateTime | Self::Json => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Field {
    column: String,
    variant: String,
    kind: FieldType,
    nullable: bool,
}

fn generate(
    kind: GeneratorKind,
    values: &[String],
    uuid_id: bool,
    soft_delete: bool,
    current_dir: &Path,
) -> anyhow::Result<()> {
    let workspace = framework_workspace(current_dir)?;
    let (project_dir, _) = application_directory(&workspace, current_dir)?;
    match kind {
        GeneratorKind::Model => generate_model(values, uuid_id, soft_delete, &project_dir),
        GeneratorKind::Migration => generate_migration(values, uuid_id, soft_delete, &project_dir),
        GeneratorKind::Mailer | GeneratorKind::Policy if uuid_id || soft_delete => {
            bail!("--uuid-id and --soft-delete apply only to model and migration generators")
        }
        GeneratorKind::Mailer => generate_mailer(values, &project_dir),
        GeneratorKind::Policy => generate_policy(values, &project_dir),
    }
}

fn enable_uuid_feature(project_dir: &Path) -> anyhow::Result<()> {
    for path in [
        project_dir.join("Cargo.toml"),
        project_dir.join("migration/Cargo.toml"),
    ] {
        let source = fs::read_to_string(&path)?;
        let mut document = source
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("{} is invalid TOML", path.display()))?;
        let dependency = document
            .get_mut("dependencies")
            .and_then(toml_edit::Item::as_table_mut)
            .and_then(|dependencies| dependencies.get_mut("openrust"))
            .and_then(toml_edit::Item::as_value_mut)
            .and_then(toml_edit::Value::as_inline_table_mut)
            .with_context(|| format!("{} has no inline OpenRust dependency", path.display()))?;
        if dependency.get("features").is_none() {
            dependency.insert("features", toml_edit::Value::Array(toml_edit::Array::new()));
        }
        let features = dependency
            .get_mut("features")
            .and_then(toml_edit::Value::as_array_mut)
            .with_context(|| format!("{} has an invalid OpenRust feature list", path.display()))?;
        if !features
            .iter()
            .any(|feature| feature.as_str() == Some("db-uuid"))
        {
            features.push("db-uuid");
            fs::write(path, document.to_string())?;
        }
    }
    Ok(())
}

fn generate_mailer(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let [name] = values else {
        bail!("usage: openrust generate mailer <name>");
    };
    let module = normalize_identifier(name)?;
    let type_name = pascal_case(&module);
    let mailer_file = project_dir.join(format!("src/mailers/{module}.rs"));
    if mailer_file.exists() {
        bail!("mailer already exists: {}", mailer_file.display());
    }
    let module_file = project_dir.join("src/mailers/mod.rs");
    if module_file.exists()
        && fs::read_to_string(&module_file)?
            .lines()
            .any(|line| line.trim() == format!("pub mod {module};"))
    {
        bail!("mailer module is already registered: {module}");
    }
    let main_file = project_dir.join("src/main.rs");
    fs::write(mailer_file, render_mailer(&type_name))?;
    append_sorted_module(
        &module_file,
        &format!("pub mod {module};"),
        "//! Application email types.\n",
    )?;
    append_module(
        &main_file,
        "pub mod mailers;",
        "//! Application entry point.\n",
    )?;
    print_generated(&format!("Generated mailer `{module}`."))?;
    Ok(())
}

fn generate_policy(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let [name] = values else {
        bail!("usage: openrust generate policy <resource>");
    };
    let manifest = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    if !manifest.contains("\"auth\"") {
        bail!("policy generation requires an application with the `auth` feature");
    }
    let module = normalize_identifier(name)?;
    let type_name = format!("{}Policy", pascal_case(&module));
    let resource = pluralize(&module);
    let policy_file = project_dir.join(format!("src/policies/{module}.rs"));
    if policy_file.exists() {
        bail!("policy already exists: {}", policy_file.display());
    }
    let module_file = project_dir.join("src/policies/mod.rs");
    if module_file.exists()
        && fs::read_to_string(&module_file)?
            .lines()
            .any(|line| line.trim() == format!("pub mod {module};"))
    {
        bail!("policy module is already registered: {module}");
    }
    let main_file = project_dir.join("src/main.rs");
    fs::write(policy_file, render_policy(&type_name, &resource))?;
    append_sorted_module(
        &module_file,
        &format!("pub mod {module};"),
        "//! Application authorization policies.\n",
    )?;
    append_module(
        &main_file,
        "pub mod policies;",
        "//! Application entry point.\n",
    )?;
    print_generated(&format!("Generated policy `{module}` for `{resource}`."))?;
    Ok(())
}

fn render_mailer(type_name: &str) -> String {
    format!(
        "//! Application email type generated by OpenRust.\n\nuse openrust::mail::{{\n    Email,\n    lettre::{{Message, message::Mailbox}},\n}};\n\n/// A customizable application email.\n#[derive(Clone, Debug)]\npub struct {type_name} {{\n    /// Sender address.\n    pub from: String,\n    /// Recipient address.\n    pub to: String,\n    /// Subject line.\n    pub subject: String,\n    /// Plain-text body.\n    pub body: String,\n}}\n\nimpl Email for {type_name} {{\n    fn message(&self) -> openrust::Result<Message> {{\n        let from: Mailbox = self.from.parse().map_err(openrust::Error::internal)?;\n        let to: Mailbox = self.to.parse().map_err(openrust::Error::internal)?;\n        Message::builder()\n            .from(from)\n            .to(to)\n            .subject(&self.subject)\n            .body(self.body.clone())\n            .map_err(openrust::Error::internal)\n    }}\n}}\n"
    )
}

fn render_policy(type_name: &str, resource: &str) -> String {
    let permissions = ["read", "create", "update", "delete"];
    let mut source = format!(
        "//! Application authorization policy generated by OpenRust.\n\nuse openrust::auth::{{CurrentUser, Policy}};\n\n/// Authorization helpers for `{resource}`.\n#[derive(Clone, Debug)]\npub struct {type_name} {{\n    policy: Policy,\n}}\n\nimpl {type_name} {{\n    /// Creates the resource policy from the application's shared policy service.\n    pub fn new(policy: Policy) -> Self {{\n        Self {{ policy }}\n    }}\n\n"
    );
    for permission in permissions {
        let constant = permission.to_ascii_uppercase();
        let _ = writeln!(
            source,
            "    /// Permission required to {permission} {resource}.\n    pub const {constant}_PERMISSION: &'static str = \"{resource}:{permission}\";\n\n    /// Authorizes the `{resource}:{permission}` permission.\n    pub async fn authorize_{permission}(&self, user: &CurrentUser) -> openrust::Result<()> {{\n        self.policy.authorize(user, Self::{constant}_PERMISSION).await\n    }}\n"
        );
    }
    if source.ends_with("\n\n") {
        source.pop();
    }
    source.push_str("}\n");
    source
}

fn generate_model(
    values: &[String],
    uuid_id: bool,
    soft_delete: bool,
    project_dir: &Path,
) -> anyhow::Result<()> {
    let (name, fields) = parse_model_arguments(values, soft_delete)?;
    let manifest = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    if !manifest.contains("db-postgres") && !manifest.contains("db-mysql") {
        bail!("model generation requires an application created with `openrust new --db`");
    }
    let module = normalize_identifier(name)?;
    let table = pluralize(&module);
    let model_file = project_dir.join(format!("src/models/_entities/{module}.rs"));
    if model_file.exists() {
        bail!("model already exists: {}", model_file.display());
    }
    let entity_module = project_dir.join("src/models/_entities/mod.rs");
    let models_module = project_dir.join("src/models/mod.rs");
    let main_file = project_dir.join("src/main.rs");
    let generated = render_model(&table, &fields, uuid_id, soft_delete);
    fs::write(&model_file, generated)?;
    append_module(
        &entity_module,
        &format!("pub mod {module};"),
        "//! Generated model entities.\n",
    )?;
    append_module(
        &models_module,
        "pub mod _entities;",
        "//! Application models.\n",
    )?;
    append_module(
        &main_file,
        "pub mod models;",
        "//! Application entry point.\n",
    )?;
    if uuid_id {
        enable_uuid_feature(project_dir)?;
    }
    print_generated(&format!("Generated model `{module}` for table `{table}`."))?;
    Ok(())
}

fn generate_migration(
    values: &[String],
    uuid_id: bool,
    soft_delete: bool,
    project_dir: &Path,
) -> anyhow::Result<()> {
    let (name, fields) = parse_model_arguments(values, soft_delete)?;
    let table = normalize_identifier(name)?;
    let migration_dir = project_dir.join("migration/src");
    if !migration_dir.join("lib.rs").is_file() {
        bail!("migration generation requires a project created with `openrust new --db`");
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| anyhow::anyhow!("system clock is before Unix epoch: {error}"))?
        .as_micros();
    let sequence = MIGRATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let module = format!(
        "m{timestamp}_{}_{}_create_{table}",
        std::process::id(),
        sequence
    );
    let migration_file = migration_dir.join(format!("{module}.rs"));
    let lib_file = migration_dir.join("lib.rs");
    let registry = fs::read_to_string(&lib_file)?;
    if registry.contains(&format!("mod {module};")) || migration_file.exists() {
        bail!("migration already exists: {module}");
    }
    let updated_registry = register_migration(&registry, &module)?;
    fs::write(
        &migration_file,
        render_migration(&module, &table, &fields, uuid_id, soft_delete),
    )?;
    fs::write(lib_file, updated_registry)?;
    if uuid_id {
        enable_uuid_feature(project_dir)?;
    }
    print_generated(&format!(
        "Generated migration `{module}` for table `{table}`."
    ))?;
    Ok(())
}

fn parse_model_arguments(
    values: &[String],
    soft_delete: bool,
) -> anyhow::Result<(&str, Vec<Field>)> {
    let Some((name, fields)) = values.split_first() else {
        bail!("usage: openrust generate model <name> <field:type>...");
    };
    let mut parsed = Vec::with_capacity(fields.len());
    let mut columns = std::collections::HashSet::from(["id".to_owned()]);
    for declaration in fields {
        let Some((field_name, field_type)) = declaration.split_once(':') else {
            bail!("invalid field `{declaration}`; expected `field:type`");
        };
        let name = normalize_identifier(field_name)?;
        let nullable = field_type.ends_with('?');
        let field_type = field_type.strip_suffix('?').unwrap_or(field_type);
        let kind = FieldType::parse(field_type)
            .with_context(|| format!("unsupported field type `{field_type}`"))?;
        let column = if kind == FieldType::References {
            format!("{name}_id")
        } else {
            name.clone()
        };
        if !columns.insert(column.clone()) {
            bail!("duplicate or reserved database column `{column}`");
        }
        parsed.push(Field {
            variant: pascal_case(&column),
            column,
            kind,
            nullable,
        });
    }
    if soft_delete && !columns.insert("deleted_at".to_owned()) {
        bail!("`deleted_at` is reserved by `--soft-delete`");
    }
    if parsed.is_empty() {
        bail!("at least one field is required");
    }
    Ok((name, parsed))
}

fn normalize_identifier(value: &str) -> anyhow::Result<String> {
    let normalized = value.replace('-', "_");
    let keywords = [
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "self", "static", "struct", "super", "trait", "true",
        "type", "unsafe", "use", "where", "while",
    ];
    if !normalized
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_lowercase())
        || !normalized.chars().all(|character| {
            character == '_' || character.is_ascii_lowercase() || character.is_ascii_digit()
        })
        || keywords.contains(&normalized.as_str())
        || normalized.len() > 63
    {
        bail!("`{value}` must be a snake_case ASCII identifier");
    }
    Ok(normalized)
}

fn pascal_case(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut characters = part.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_ascii_uppercase().to_string() + characters.as_str()
            })
        })
        .collect()
}

fn pluralize(value: &str) -> String {
    if let Some(stem) = value.strip_suffix('y')
        && !stem.is_empty()
        && stem
            .chars()
            .last()
            .is_some_and(|character| !"aeiou".contains(character))
    {
        format!("{stem}ies")
    } else if value.ends_with('s')
        || value.ends_with('x')
        || value.ends_with("ch")
        || value.ends_with("sh")
    {
        format!("{value}es")
    } else {
        format!("{value}s")
    }
}

fn render_model(table: &str, fields: &[Field], uuid_id: bool, soft_delete: bool) -> String {
    use std::fmt::Write as _;
    let id_type = if uuid_id { "Uuid" } else { "i64" };
    let id_attribute = if uuid_id {
        "#[sea_orm(primary_key, auto_increment = false)]"
    } else {
        "#[sea_orm(primary_key)]"
    };
    let mut source = format!(
        "//! SeaORM entity generated for table `{table}`.\n\nuse openrust::database::sea_orm_migration::sea_orm;\nuse sea_orm::entity::prelude::*;\n\n#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]\n#[sea_orm(table_name = \"{table}\")]\npub struct Model {{\n    {id_attribute}\n    pub id: {id_type},\n"
    );
    for field in fields {
        let type_name = field.kind.rust_type();
        let field_type = if field.nullable {
            format!("Option<{type_name}>")
        } else {
            type_name.to_owned()
        };
        let _ = writeln!(source, "    pub {}: {field_type},", field.column);
    }
    if soft_delete {
        source.push_str("    pub deleted_at: Option<DateTimeUtc>,\n");
    }
    source.push_str(
        "}\n\n#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]\npub enum Relation {}\n",
    );
    if soft_delete {
        source.push_str("\nimpl Entity {\n    /// Returns rows that have not been soft-deleted. Compose this scope with explicit authorization and filtering conditions.\n    pub fn not_deleted() -> Select<Self> {\n        Self::find().filter(Column::DeletedAt.is_null())\n    }\n}\n");
    }
    if uuid_id {
        source.push_str("\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl ActiveModelBehavior for ActiveModel {\n    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>\n    where\n        C: ConnectionTrait,\n    {\n        if insert && self.id.is_not_set() {\n            self.id = sea_orm::ActiveValue::Set(Uuid::now_v7());\n        }\n        Ok(self)\n    }\n}\n");
    } else {
        source.push_str("\nimpl ActiveModelBehavior for ActiveModel {}\n");
    }
    source
}

fn render_migration(
    module: &str,
    table: &str,
    fields: &[Field],
    uuid_id: bool,
    soft_delete: bool,
) -> String {
    use std::fmt::Write as _;
    let table_type = pascal_case(table);
    let mut source = format!(
        "//! Creates the `{table}` table.\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\n#[derive(DeriveIden)]\nenum {table_type} {{\n    Table,\n    Id,\n"
    );
    for field in fields {
        let _ = writeln!(source, "    {},", field.variant);
    }
    if soft_delete {
        source.push_str("    DeletedAt,\n");
    }
    let id_column = if uuid_id {
        format!(".col(ColumnDef::new({table_type}::Id).uuid().not_null().primary_key())")
    } else {
        format!(
            ".col(ColumnDef::new({table_type}::Id).big_integer().not_null().auto_increment().primary_key())"
        )
    };
    let _ = write!(
        source,
        "}}\n\npub struct Migration;\n\nimpl MigrationName for Migration {{\n    fn name(&self) -> &'static str {{\n        \"{module}\"\n    }}\n}}\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigrationTrait for Migration {{\n    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {{\n        manager\n            .create_table(\n                Table::create()\n                    .table({table_type}::Table)\n                    .if_not_exists()\n                    {id_column}\n"
    );
    for field in fields {
        let nullable = if field.nullable { "null" } else { "not_null" };
        match field.kind {
            FieldType::DateTime => {
                let _ = writeln!(
                    source,
                    "                    .col(openrust::database::dialect::timestamp_utc({table_type}::{}, manager.get_database_backend())?{} )",
                    field.variant,
                    if field.nullable { ".null()" } else { "" }
                );
            }
            FieldType::Json => {
                let _ = writeln!(
                    source,
                    "                    .col(openrust::database::dialect::json({table_type}::{}, manager.get_database_backend())?{} )",
                    field.variant,
                    if field.nullable { ".null()" } else { "" }
                );
            }
            kind => {
                let _ = writeln!(
                    source,
                    "                    .col(ColumnDef::new({table_type}::{}).{}().{}())",
                    field.variant,
                    kind.migration_builder().unwrap_or("big_integer"),
                    nullable
                );
            }
        }
    }
    if soft_delete {
        let _ = writeln!(
            source,
            "                    .col(openrust::database::dialect::timestamp_utc({table_type}::DeletedAt, manager.get_database_backend())?.null())"
        );
    }
    for field in fields
        .iter()
        .filter(|field| field.kind == FieldType::References)
    {
        let referenced_table = pluralize(field.column.trim_end_matches("_id"));
        let _ = writeln!(
            source,
            "                    .foreign_key(ForeignKey::create().name(\"{table}_{0}_fkey\").from_tbl({table_type}::Table).from_col({table_type}::{0}).to_tbl(\"{referenced_table}\").to_col(\"id\"))",
            field.variant
        );
    }
    let _ = write!(
        source,
        "                    .to_owned(),\n            )\n            .await?;\n        Ok(())\n    }}\n\n    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {{\n        manager\n            .drop_table(Table::drop().table({table_type}::Table).if_exists().to_owned())\n            .await\n    }}\n}}\n"
    );
    source
}

fn append_module(path: &Path, module: &str, header: &str) -> anyhow::Result<()> {
    use std::fmt::Write as _;
    let mut contents = if path.exists() {
        fs::read_to_string(path)?
    } else {
        header.to_owned()
    };
    if contents.lines().any(|line| line.trim() == module) {
        return Ok(());
    }
    let _ = writeln!(contents, "{module}");
    fs::write(path, contents)?;
    Ok(())
}

fn append_sorted_module(path: &Path, module: &str, header: &str) -> anyhow::Result<()> {
    let contents = if path.exists() {
        fs::read_to_string(path)?
    } else {
        header.to_owned()
    };
    if contents.lines().any(|line| line.trim() == module) {
        return Ok(());
    }
    let module_name = module
        .strip_prefix("pub mod ")
        .and_then(|name| name.strip_suffix(';'))
        .context("generated module declaration is invalid")?;
    let mut lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
    let insertion = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            let existing_name = line.trim().strip_prefix("pub mod ")?.strip_suffix(';')?;
            if existing_name <= module_name {
                return None;
            }
            let mut insertion = index;
            while insertion > 0 {
                let previous = lines[insertion - 1].trim();
                if previous.starts_with("///")
                    || previous.starts_with("// ")
                    || previous.starts_with("/*")
                {
                    insertion -= 1;
                } else {
                    break;
                }
            }
            Some(insertion)
        })
        .unwrap_or(lines.len());
    lines.insert(insertion, module.to_owned());
    let mut updated = lines.join("\n");
    updated.push('\n');
    fs::write(path, updated)?;
    Ok(())
}

fn register_migration(registry: &str, module: &str) -> anyhow::Result<String> {
    use std::fmt::Write as _;
    let Some(migrations_start) = registry.find("vec![") else {
        bail!("migration registry does not match the generated project template");
    };
    let Some(migrations_end) = registry[migrations_start..].find(']') else {
        bail!("migration registry has an unterminated migration list");
    };
    if registry.contains(&format!("mod {module};")) {
        bail!("migration already exists: {module}");
    }
    let insertion = migrations_start + migrations_end;
    let migration_expression = if registry[migrations_start + "vec![".len()..insertion]
        .trim()
        .is_empty()
    {
        format!("Box::new({module}::Migration)")
    } else {
        format!(", Box::new({module}::Migration)")
    };
    let mut updated = registry.to_owned();
    updated.insert_str(insertion, &migration_expression);
    let _ = writeln!(updated, "mod {module};");
    Ok(updated)
}

fn print_generated(message: &str) -> anyhow::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{message}")?;
    Ok(())
}

/// 执行项目二进制并返回其退出码。
fn run_project(args: &[&str], development: bool) -> anyhow::Result<ExitCode> {
    run_application(args, development)
}

fn run_application(args: &[&str], development: bool) -> anyhow::Result<ExitCode> {
    let current_dir = std::env::current_dir()?;
    let workspace = framework_workspace(&current_dir)?;
    let (app_dir, package_name) = application_directory(&workspace, &current_dir)?;
    let mut command = Process::new("cargo");
    command
        .current_dir(&app_dir)
        .args([
            "run",
            "--quiet",
            "--package",
            &package_name,
            "--bin",
            &package_name,
            "--",
        ])
        .args(args);
    if development {
        command.env("APP_ENV", "development");
    }
    command
        .status()
        .context("failed to start `cargo run`")
        .map(exit_code)
}

/// 在当前项目中运行 `cargo run --quiet -- <args>`，并返回相同的退出状态。
fn forward(args: &[String]) -> anyhow::Result<ExitCode> {
    let current_dir = std::env::current_dir()?;
    let workspace = framework_workspace(&current_dir)?;
    let (app_dir, package_name) = application_directory(&workspace, &current_dir)?;
    Process::new("cargo")
        .current_dir(app_dir)
        .args([
            "run",
            "--quiet",
            "--package",
            &package_name,
            "--bin",
            &package_name,
            "--",
        ])
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

    fn create_framework_fixture(root: &Path) {
        fs::create_dir_all(root.join("src")).expect("create framework source directory");
        fs::write(root.join("src/lib.rs"), "").expect("create framework library target");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"openrust\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\nresolver = \"3\"\n# 保留工作区专有配置与成员说明。\ncustom-marker = \"keep-me\"\nmembers = [\"cli\", \"macros\", \"testing\", \"apps/server\"]\n\n[workspace.package]\nversion = \"0.0.0\"\nedition = \"2024\"\nlicense = \"MIT OR Apache-2.0\"\n\n[workspace.dependencies]\nopenrust = { path = \".\" }\ncli = { path = \"cli\" }\nmacros = { path = \"macros\" }\ntesting = { path = \"testing\" }\nserver = { path = \"apps/server\" }\n",
        )
        .expect("create framework workspace manifest");
        for (name, path, target) in [
            ("cli", "cli", "src/main.rs"),
            ("macros", "macros", "src/lib.rs"),
            ("testing", "testing", "src/lib.rs"),
            ("server", "apps/server", "src/main.rs"),
        ] {
            let package = root.join(path);
            fs::create_dir_all(package.join("src")).expect("create package source directory");
            fs::write(package.join(target), "").expect("create package target");
            let mut manifest = format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n"
            );
            if name == "macros" {
                manifest.push_str("\n[lib]\nproc-macro = true\n");
            }
            fs::write(package.join("Cargo.toml"), manifest)
                .expect("create workspace member manifest");
        }
        fs::create_dir_all(root.join("apps/server")).expect("create apps directory");
    }

    #[test]
    fn generated_manifest_matches_starter_template() {
        insta::assert_snapshot!(cargo_manifest("my-app", None, false, false, false, false));
    }

    #[test]
    fn generated_manifest_enables_requested_http_client_feature() {
        let manifest = cargo_manifest("my-app", None, false, true, false, false);
        assert!(manifest.contains("features = [\"http-client\"]"));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--http-client"]).is_ok());
    }

    #[test]
    fn generated_manifest_enables_twilio_sms_feature() {
        let manifest = cargo_manifest("my-app", None, false, false, true, false);
        assert!(manifest.contains("features = [\"sms-twilio\"]"));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--sms-twilio"]).is_ok());
    }

    #[test]
    fn generated_manifest_and_project_enable_application_encryption() {
        let manifest = cargo_manifest("my-app", None, false, false, false, true);
        assert!(manifest.contains("features = [\"encryption\"]"));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--encryption"]).is_ok());

        let root =
            std::env::temp_dir().join(format!("openrust-cli-encryption-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false, false, true)
            .expect("generate encryption project");
        let app = fs::read_to_string(project.join("src/app.rs")).expect("read generated app");
        let config = fs::read_to_string(project.join("config/default.toml"))
            .expect("read generated encryption configuration");
        let env = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(app.contains("EncryptionPlugin"));
        assert!(config.contains("[security.encryption]"));
        assert!(env.contains("APP_SECURITY__ENCRYPTION__KEYS__LOCAL="));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn key_generation_print_value_is_a_32_byte_base64_secret() {
        let encoded = generate_key().expect("generate key");
        assert_eq!(STANDARD.decode(encoded).expect("decode key").len(), 32);
    }

    #[test]
    fn project_template_has_expected_layout_and_compilable_test() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("generate project");
        for path in [
            "Cargo.toml",
            ".gitignore",
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
            ".gitignore",
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
        let gitignore =
            fs::read_to_string(project.join(".gitignore")).expect("read generated gitignore");
        assert!(gitignore.lines().any(|line| line == ".env"));
        assert!(gitignore.lines().any(|line| line == ".env.*"));
        assert!(gitignore.lines().any(|line| line == "!.env.example"));
        assert!(gitignore.lines().any(|line| line == "!.env.*.example"));
        let env_example = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(env_example.contains("APP_SERVER__PORT=3000"));
        assert!(env_example.contains("APP_MAIL__SMTP_PASSWORD="));
        assert!(env_example.contains("Production reads configuration from process environment"));
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
            populate_project(
                &project,
                "blog",
                "Blog",
                Some(backend),
                false,
                false,
                false,
                false,
            )
            .expect("generate database project");
            let manifest =
                fs::read_to_string(project.join("Cargo.toml")).expect("read generated manifest");
            assert!(manifest.contains(feature));
            assert!(project.join("migration/Cargo.toml").is_file());
            assert!(project.join("migration/src/lib.rs").is_file());
            assert!(
                fs::read_to_string(project.join("migration/src/lib.rs"))
                    .expect("read empty migration registry")
                    .contains("vec![]")
            );
            let app = fs::read_to_string(project.join("src/app.rs")).expect("read generated app");
            assert!(app.contains("DatabasePlugin"));
            assert!(app.contains("openrust_migration::Migrator"));
            assert!(app.contains("SeedPlugin"));
            assert!(project.join(".env.example").is_file());
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn auth_templates_register_auth_plugin_migration_and_protected_route() {
        let root = std::env::temp_dir().join(format!("openrust-cli-auth-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        for (backend, feature) in [
            (DatabaseBackend::Postgres, "db-postgres"),
            (DatabaseBackend::Mysql, "db-mysql"),
        ] {
            let project = root.join(feature);
            populate_project(
                &project,
                "blog",
                "Blog",
                Some(backend),
                true,
                false,
                false,
                false,
            )
            .expect("generate database-backed auth project");
            let manifest = fs::read_to_string(project.join("Cargo.toml"))
                .expect("read generated app manifest");
            assert!(manifest.contains(feature));
            assert!(manifest.contains("testing"));
            assert!(manifest.contains("figment"));
            assert!(manifest.contains("serde_json"));
            let http_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
                .expect("read generated auth HTTP integration test");
            assert!(http_test.contains("app::Blog::build(&mut builder)"));
            assert!(http_test.contains("into_make_service_with_connect_info"));
            assert!(http_test.contains("/auth/password-reset/confirm"));
            assert!(!http_test.contains("{{type_name}}"));
            assert!(manifest.contains("\"auth\""));
            let migration_manifest = fs::read_to_string(project.join("migration/Cargo.toml"))
                .expect("read generated migration manifest");
            assert!(migration_manifest.contains("\"auth\""));
            let migration = fs::read_to_string(project.join("migration/src/lib.rs"))
                .expect("read generated migration registry");
            assert!(migration.contains("openrust::auth::AuthMigration"));
            assert!(migration.contains("openrust::auth::AuthRefreshTokenMigration"));
            assert!(migration.contains("openrust::auth::AuthVerificationCodeMigration"));
            assert!(migration.contains("openrust::auth::AuthPolicyMigration"));
            assert!(migration.contains("openrust::database::DatabaseRateLimitMigration"));
            assert!(migration.contains("openrust::audit::DatabaseAuditMigration"));
            assert!(migration.contains("openrust::signing::SignedLinkRevocationMigration"));
            let app = fs::read_to_string(project.join("src/app.rs")).expect("read generated app");
            assert!(app.contains("AuthPlugin::new()"));
            assert!(app.contains("JwtPlugin::new()"));
            assert!(app.contains("AuthFlowsPlugin::new()"));
            assert!(app.contains("AuditPlugin::database()"));
            assert!(app.contains("SignedLinksPlugin::new().with_database_revocations_from_app()"));
            let config = fs::read_to_string(project.join("config/default.toml"))
                .expect("read generated signing config");
            assert!(config.contains("[security.signed_links]"));
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated signing environment template");
            assert!(env.contains("APP_SECURITY__SIGNED_LINKS__KEYS__LOCAL="));
            assert!(!app.contains("SessionPlugin::new()"));
            let routes =
                fs::read_to_string(project.join("src/routes.rs")).expect("read generated routes");
            assert!(routes.contains("AuthLayer::required("));
            assert!(routes.contains("AuthMode::Jwt"));
            assert!(routes.contains("/api/me/pat"));
            assert!(routes.contains("AuthMode::PersonalAccessToken"));
            assert!(routes.contains("CurrentUser"));
            assert!(routes.contains("/auth/register"));
            assert!(routes.contains("/auth/verify-email"));
            assert!(routes.contains("/auth/refresh"));
            assert!(routes.contains("/auth/password-reset/confirm"));
            assert!(routes.contains("ClientIp"));
            let auth_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
                .expect("read generated auth HTTP integration test");
            assert!(auth_test.contains("trusted_proxy_ranges"));
            let config = fs::read_to_string(project.join("config/default.toml"))
                .expect("read generated auth config");
            assert!(config.contains("[auth.jwt]"));
            assert!(config.contains("keys = {}"));
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated environment example");
            assert!(env.contains("APP_AUTH__JWT__KEYS__LOCAL="));
        }
        let encrypted_project = root.join("encrypted");
        populate_project(
            &encrypted_project,
            "secure_blog",
            "SecureBlog",
            Some(DatabaseBackend::Postgres),
            true,
            false,
            false,
            true,
        )
        .expect("generate encrypted database-backed auth project");
        let auth_test = fs::read_to_string(encrypted_project.join("tests/auth_flows.rs"))
            .expect("read encrypted auth HTTP integration test");
        assert!(auth_test.contains("\\n[security.encryption]\\n"));
        assert!(auth_test.contains("active_key_id = \\\"test\\\""));
        assert!(auth_test.contains("KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--auth"]).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn model_and_migration_generators_render_fields_and_register_repeated_migrations() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-generate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project(
            &project,
            "blog",
            "Blog",
            Some(DatabaseBackend::Postgres),
            false,
            false,
            false,
            false,
        )
        .expect("generate database project");

        let model = [
            "post",
            "title:string",
            "published_at:date_time?",
            "metadata:json",
            "author:references",
        ]
        .map(str::to_owned);
        generate_model(&model, false, false, &project).expect("generate model");
        let entity = fs::read_to_string(project.join("src/models/_entities/post.rs"))
            .expect("read generated model");
        assert!(entity.contains("pub title: String"));
        assert!(entity.contains("pub published_at: Option<DateTimeUtc>"));
        assert!(entity.contains("pub author_id: i64"));

        generate_model(
            &["entry", "title:string"].map(str::to_owned),
            true,
            true,
            &project,
        )
        .expect("generate UUID soft-delete model");
        let entry = fs::read_to_string(project.join("src/models/_entities/entry.rs"))
            .expect("read generated UUID model");
        assert!(entry.contains("pub id: Uuid"));
        assert!(entry.contains("Uuid::now_v7()"));
        assert!(entry.contains("pub deleted_at: Option<DateTimeUtc>"));
        assert!(entry.contains("pub fn not_deleted()"));
        let app_manifest = fs::read_to_string(project.join("Cargo.toml")).expect("app manifest");
        let migration_manifest =
            fs::read_to_string(project.join("migration/Cargo.toml")).expect("migration manifest");
        assert!(app_manifest.contains("\"db-uuid\""));
        assert!(migration_manifest.contains("\"db-uuid\""));

        generate_migration(
            &[
                "posts".to_owned(),
                "title:string".to_owned(),
                "published_at:date_time?".to_owned(),
                "metadata:json".to_owned(),
                "author:references".to_owned(),
            ],
            false,
            false,
            &project,
        )
        .expect("generate model migration");
        generate_migration(
            &["entries".to_owned(), "title:string".to_owned()],
            true,
            true,
            &project,
        )
        .expect("generate UUID soft-delete migration");
        let migration = fs::read_dir(project.join("migration/src"))
            .expect("read generated migrations")
            .filter_map(std::result::Result::ok)
            .map(|entry| fs::read_to_string(entry.path()).unwrap_or_default())
            .find(|source| source.contains("uuid().not_null().primary_key()"))
            .unwrap_or_default();
        assert!(migration.contains(".uuid().not_null().primary_key()"));
        assert!(migration.contains("DeletedAt"));
        for (table, field) in [("comments", "body:text"), ("labels", "slug:string")] {
            generate_migration(
                &[table.to_owned(), field.to_owned()],
                false,
                false,
                &project,
            )
            .expect("generate migration");
        }
        let registry = fs::read_to_string(project.join("migration/src/lib.rs"))
            .expect("read migration registry");
        assert!(registry.contains("create_posts::Migration"));
        assert!(registry.contains("create_comments::Migration"));
        assert!(registry.contains("create_labels::Migration"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generator_rejects_invalid_identifiers_and_unknown_types() {
        let invalid = ["UpperCase", "9items", "type"];
        for name in invalid {
            assert!(normalize_identifier(name).is_err());
        }
        let values = ["post".to_owned(), "title:unknown".to_owned()];
        assert!(parse_model_arguments(&values, false).is_err());
        let duplicate_soft_delete = ["post".to_owned(), "deleted_at:date_time?".to_owned()];
        assert!(parse_model_arguments(&duplicate_soft_delete, true).is_err());
        let parsed = Cli::try_parse_from([
            "openrust",
            "g",
            "model",
            "post",
            "--uuid-id",
            "--soft-delete",
            "title:string",
        ]);
        assert!(parsed.is_ok());
    }

    #[test]
    fn mailer_and_policy_generators_emit_and_register_typed_modules() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-app-generators-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project(
            &project,
            "blog",
            "Blog",
            Some(DatabaseBackend::Postgres),
            true,
            false,
            false,
            false,
        )
        .expect("generate database-backed auth project");

        assert!(Cli::try_parse_from(["openrust", "g", "mailer", "welcome_email"]).is_ok());
        assert!(Cli::try_parse_from(["openrust", "g", "policy", "post"]).is_ok());
        generate_mailer(&["welcome_email".to_owned()], &project).expect("generate mailer");
        fs::write(
            project.join("src/mailers/mod.rs"),
            "//! Application email types.\n/// Welcome email documentation.\npub mod welcome_email;\n",
        )
        .expect("add existing module documentation");
        generate_mailer(&["password_reset".to_owned()], &project).expect("generate second mailer");
        generate_policy(&["post".to_owned()], &project).expect("generate policy");
        generate_policy(&["comment".to_owned()], &project).expect("generate second policy");

        let mailer = fs::read_to_string(project.join("src/mailers/welcome_email.rs"))
            .expect("read generated mailer");
        assert!(mailer.contains("impl Email for WelcomeEmail"));
        assert!(mailer.contains("pub from: String"));
        assert!(mailer.contains(".body(self.body.clone())"));
        let policy = fs::read_to_string(project.join("src/policies/post.rs"))
            .expect("read generated policy");
        assert!(policy.contains("pub struct PostPolicy"));
        assert!(policy.contains("\"posts:read\""));
        assert!(policy.contains("authorize_create"));
        assert!(policy.contains("authorize_update"));
        assert!(policy.contains("authorize_delete"));
        let mailer_modules = fs::read_to_string(project.join("src/mailers/mod.rs"))
            .expect("read mailer module registry");
        assert!(mailer_modules.contains("pub mod welcome_email;"));
        assert!(
            mailer_modules.contains("/// Welcome email documentation.\npub mod welcome_email;")
        );
        assert!(
            mailer_modules.find("pub mod password_reset;")
                < mailer_modules.find("pub mod welcome_email;")
        );
        let policy_modules = fs::read_to_string(project.join("src/policies/mod.rs"))
            .expect("read policy module registry");
        assert!(policy_modules.contains("pub mod post;"));
        assert!(policy_modules.find("pub mod comment;") < policy_modules.find("pub mod post;"));
        let main = fs::read_to_string(project.join("src/main.rs")).expect("read app entry");
        assert_eq!(main.matches("pub mod mailers;").count(), 1);
        assert_eq!(main.matches("pub mod policies;").count(), 1);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn application_generators_reject_invalid_input_missing_auth_and_existing_files() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-app-generator-errors-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project(
            &project,
            "blog",
            "Blog",
            Some(DatabaseBackend::Postgres),
            false,
            false,
            false,
            false,
        )
        .expect("generate database project");

        assert!(generate_mailer(&[], &project).is_err());
        assert!(generate_mailer(&["Invalid".to_owned()], &project).is_err());
        assert!(
            generate_policy(&["post".to_owned()], &project)
                .expect_err("policy requires auth")
                .to_string()
                .contains("auth")
        );

        generate_mailer(&["notice".to_owned()], &project).expect("generate first mailer");
        let mailer_path = project.join("src/mailers/notice.rs");
        fs::write(&mailer_path, "preserve this file").expect("replace generated file for guard");
        assert!(generate_mailer(&["notice".to_owned()], &project).is_err());
        assert_eq!(
            fs::read_to_string(mailer_path).expect("read protected file"),
            "preserve this file"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn creates_multiple_workspace_apps_with_local_dependencies_and_isolated_configs() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-workspace-apps-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary workspace");
        create_framework_fixture(&root);

        create_project(
            "blog",
            Some(DatabaseBackend::Postgres),
            false,
            false,
            false,
            false,
            &root,
        )
        .expect("create first app");
        create_project(
            "apps/shop",
            Some(DatabaseBackend::Mysql),
            false,
            false,
            false,
            false,
            &root,
        )
        .expect("create second app");

        let metadata = framework_workspace(&root).expect("discover generated workspace");
        let names = metadata
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>();
        for name in ["blog", "blog-migration", "shop", "shop-migration"] {
            assert!(names.contains(&name), "missing workspace package {name}");
        }
        for (app, backend) in [("blog", "db-postgres"), ("shop", "db-mysql")] {
            let app_manifest = fs::read_to_string(root.join(format!("apps/{app}/Cargo.toml")))
                .expect("read app manifest");
            let migration_manifest =
                fs::read_to_string(root.join(format!("apps/{app}/migration/Cargo.toml")))
                    .expect("read migration manifest");
            assert!(app_manifest.contains("publish = false"));
            assert!(app_manifest.contains("workspace = true"));
            assert!(app_manifest.contains(backend));
            assert!(migration_manifest.contains("publish = false"));
            assert!(migration_manifest.contains("workspace = true"));
            assert!(migration_manifest.contains(backend));
            assert!(
                fs::read_to_string(root.join(format!("apps/{app}/config/default.toml")))
                    .expect("read app config")
                    .contains(&format!("name = \"{app}\""))
            );
            let app_dir = root.join(format!("apps/{app}"));
            let (resolved, package) =
                application_directory(&metadata, &app_dir).expect("resolve app from its directory");
            assert_eq!(
                resolved,
                app_dir.canonicalize().expect("canonicalize app directory")
            );
            assert_eq!(package, app);
        }
        let workspace_manifest =
            fs::read_to_string(root.join("Cargo.toml")).expect("read updated workspace manifest");
        assert!(workspace_manifest.contains("# 保留工作区专有配置与成员说明。"));
        assert!(workspace_manifest.contains("custom-marker = \"keep-me\""));
        for member in [
            "apps/blog",
            "apps/blog/migration",
            "apps/shop",
            "apps/shop/migration",
        ] {
            assert!(workspace_manifest.contains(member));
        }
        let before_repeat = workspace_manifest.clone();
        let repeated = manifest_with_members(
            &root,
            &["apps/blog".to_owned(), "apps/blog/migration".to_owned()],
        )
        .expect("merge existing workspace members");
        assert_eq!(repeated, before_repeat);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_external_invalid_duplicate_and_existing_destinations_without_mutation() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        create_framework_fixture(&root);
        let original_manifest =
            fs::read_to_string(root.join("Cargo.toml")).expect("read original workspace manifest");
        let external_path = root.join("external-app");
        assert!(
            create_project(
                external_path.to_string_lossy().as_ref(),
                None,
                false,
                false,
                false,
                false,
                &root,
            )
            .is_err()
        );
        assert!(create_project("../outside", None, false, false, false, false, &root).is_err());
        assert!(create_project("outside/path", None, false, false, false, false, &root).is_err());
        assert!(create_project("--", None, false, false, false, false, &root).is_err());
        fs::create_dir(root.join("apps/existing")).expect("create existing destination");
        assert!(create_project("existing", None, false, false, false, false, &root).is_err());
        assert!(create_project("server", None, false, false, false, false, &root).is_err());
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).expect("read workspace manifest"),
            original_manifest
        );
        assert!(!root.join("outside").exists());
        assert!(!root.join("apps/outside").exists());

        fs::create_dir_all(root.join("apps/blog-migration/src"))
            .expect("create conflicting migration package");
        fs::write(
            root.join("apps/blog-migration/Cargo.toml"),
            "[package]\nname = \"blog-migration\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .expect("write conflicting migration manifest");
        let with_conflict = manifest_with_members(&root, &["apps/blog-migration".to_owned()])
            .expect("register conflicting migration package");
        fs::write(root.join("Cargo.toml"), &with_conflict)
            .expect("update fixture workspace manifest");
        assert!(
            create_project(
                "blog",
                Some(DatabaseBackend::Postgres),
                false,
                false,
                false,
                false,
                &root
            )
            .is_err()
        );
        assert!(!root.join("apps/blog").exists());
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).expect("read unchanged workspace"),
            with_conflict
        );
        let _ = fs::remove_dir_all(root);
    }
}
