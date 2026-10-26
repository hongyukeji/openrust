//! `openrust`: global command line of the OpenRust framework.

#![forbid(unsafe_code)]

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command as Process, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};

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
        /// Application directory name or destination path.
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
    },
    /// Any other command is forwarded to the application binary in the current project.
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum GeneratorKind {
    Model,
    Migration,
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
        Some(Builtin::New {
            name,
            db,
            auth,
            http_client,
        }) => {
            create_project(&name, db, auth, http_client, &std::env::current_dir()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Dev) => run_project(&["serve"], true),
        Some(Builtin::Serve) => run_project(&["serve"], false),
        Some(Builtin::Generate { kind, values }) => {
            generate(kind, &values, &std::env::current_dir()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Forward(args)) => forward(&args),
    }
}

/// Creates a starter application under the current directory.
fn create_project(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    current_dir: &Path,
) -> anyhow::Result<()> {
    if auth && database.is_none() {
        bail!("authentication requires a database; use `openrust new --db postgres|mysql --auth`");
    }
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

    let result = populate_project(
        &project_dir,
        project_name,
        &type_name,
        database,
        auth,
        http_client,
    );
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
    auth: bool,
    http_client: bool,
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
            cargo_manifest(name, database, auth, http_client),
        ),
        (
            ".gitignore",
            include_str!("../templates/.gitignore").to_owned(),
        ),
        (".env.example", env_example(name, database, auth)),
        ("config/default.toml", default_config(name, auth)),
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
        ("src/app.rs", app_source(crate_name, database, auth)),
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
            include_str!("../templates/tests/auth_flows.rs")
                .replace("OpenRustGeneratedApplication", crate_name),
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

fn cargo_manifest(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
) -> String {
    let mut manifest = include_str!("../templates/Cargo.toml").replace("{{name}}", name);
    if database.is_some() || auth || http_client {
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
        let dependency = if database.is_some() {
            format!(
                "openrust = {{ version = \"0.0.0\", features = {features:?} }}\nopenrust-migration = {{ package = \"{name}-migration\", path = \"migration\" }}"
            )
        } else {
            format!("openrust = {{ version = \"0.0.0\", features = {features:?} }}")
        };
        manifest = manifest.replace("openrust = \"0.0.0\"", &dependency);
    }
    if auth {
        manifest.push_str("serde = { version = \"1\", features = [\"derive\"] }\n\n[dev-dependencies]\nbase64 = \"0.22\"\nfigment = { version = \"0.10\", features = [\"toml\"] }\nserde_json = \"1\"\ntokio = { version = \"1\", features = [\"io-util\", \"macros\", \"net\", \"rt-multi-thread\"] }\n");
        if let Some(database) = database {
            let _ = FmtWrite::write_fmt(
                &mut manifest,
                format_args!(
                    "openrust-test = {{ version = \"0.0.0\", features = [\"{}\"] }}\n",
                    database.feature()
                ),
            );
        }
    }
    manifest
}

fn app_source(crate_name: &str, database: Option<DatabaseBackend>, auth: bool) -> String {
    let plugins = if database.is_some() {
        format!(
            "\n            .plugin(DatabasePlugin){}\n            .plugin(MigrationPlugin::<openrust_migration::Migrator>::new())\n            .plugin(SeedPlugin::new(crate::seed::run))",
            if auth {
                "\n            .plugin(openrust::auth::JwtPlugin::new())\n            .plugin(openrust::auth::AuthPlugin::new())\n            .plugin(openrust::auth::AuthFlowsPlugin::new())"
            } else {
                ""
            }
        )
    } else {
        String::new()
    };
    include_str!("../templates/src/app.rs")
        .replace("{{type_name}}", crate_name)
        .replace("{{database_plugins}}", &plugins)
        .replace("{{migration_plugin}}", "")
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

fn env_example(name: &str, database: Option<DatabaseBackend>, auth: bool) -> String {
    let mut contents = "# Copy to .env for local development. Never commit the populated .env file.\n# Production reads configuration from process environment, not this file.\nAPP_ENV=development\n\n# Optional overrides for values in config/*.toml:\n# APP_SERVER__PORT=3000\n# APP_SERVER__HOST=127.0.0.1\n# APP_API__HTTP_STATUS=always_ok\n".to_owned();
    if let Some(database) = database {
        use std::fmt::Write as _;
        contents.push('\n');
        let _ = writeln!(contents, "DATABASE_URL={}", database.url(name));
    }
    if auth {
        contents.push_str(
            "\n# Generate a key with: openssl rand -base64 32\nAPP_AUTH__JWT__KEYS__LOCAL=\n",
        );
    }
    contents.push_str(
        "\n# Optional SMTP delivery overrides:\n# APP_MAIL__DRIVER=smtp\n# APP_MAIL__SMTP_HOST=mail.example.com\n# APP_MAIL__SMTP_PORT=587\n# APP_MAIL__SMTP_USERNAME=\n# APP_MAIL__SMTP_PASSWORD=\n",
    );
    contents
}

fn default_config(name: &str, auth: bool) -> String {
    let mut config = include_str!("../templates/config/default.toml").replace("{{name}}", name);
    if auth {
        config.push_str("\n[auth.jwt]\nissuer = \"http://localhost:3000\"\naudience = \"");
        config.push_str(name);
        config.push_str("\"\nactive_key_id = \"local\"\nkeys = {}\n");
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
        "[package]\nname = \"{name}-migration\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nopenrust = {{ version = \"0.0.0\", features = {features:?} }}\n"
    )
}

fn migration_lib(auth: bool) -> String {
    let migrations = if auth {
        "vec![\n            Box::new(openrust::auth::AuthMigration),\n            Box::new(openrust::auth::AuthRefreshTokenMigration),\n            Box::new(openrust::auth::AuthVerificationCodeMigration),\n            Box::new(openrust::auth::AuthPolicyMigration),\n            Box::new(openrust::database::DatabaseRateLimitMigration),\n        ]"
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

fn generate(kind: GeneratorKind, values: &[String], current_dir: &Path) -> anyhow::Result<()> {
    if !current_dir.join("Cargo.toml").is_file() {
        bail!("no Cargo.toml in the current directory; run this inside an OpenRust project");
    }
    match kind {
        GeneratorKind::Model => generate_model(values, current_dir),
        GeneratorKind::Migration => generate_migration(values, current_dir),
    }
}

fn generate_model(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let (name, fields) = parse_model_arguments(values)?;
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
    let generated = render_model(&table, &fields);
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
    print_generated(&format!("Generated model `{module}` for table `{table}`."))?;
    Ok(())
}

fn generate_migration(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let (name, fields) = parse_model_arguments(values)?;
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
    fs::write(&migration_file, render_migration(&module, &table, &fields))?;
    fs::write(lib_file, updated_registry)?;
    print_generated(&format!(
        "Generated migration `{module}` for table `{table}`."
    ))?;
    Ok(())
}

fn parse_model_arguments(values: &[String]) -> anyhow::Result<(&str, Vec<Field>)> {
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

fn render_model(table: &str, fields: &[Field]) -> String {
    use std::fmt::Write as _;
    let mut source = format!(
        "//! SeaORM entity generated for table `{table}`.\n\nuse openrust::database::sea_orm_migration::sea_orm;\nuse sea_orm::entity::prelude::*;\n\n#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]\n#[sea_orm(table_name = \"{table}\")]\npub struct Model {{\n    #[sea_orm(primary_key)]\n    pub id: i64,\n"
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
    source.push_str("}\n\n#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]\npub enum Relation {}\n\nimpl ActiveModelBehavior for ActiveModel {}\n");
    source
}

fn render_migration(module: &str, table: &str, fields: &[Field]) -> String {
    use std::fmt::Write as _;
    let table_type = pascal_case(table);
    let mut source = format!(
        "//! Creates the `{table}` table.\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\n#[derive(DeriveIden)]\nenum {table_type} {{\n    Table,\n    Id,\n"
    );
    for field in fields {
        let _ = writeln!(source, "    {},", field.variant);
    }
    let _ = write!(
        source,
        "}}\n\npub struct Migration;\n\nimpl MigrationName for Migration {{\n    fn name(&self) -> &'static str {{\n        \"{module}\"\n    }}\n}}\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigrationTrait for Migration {{\n    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {{\n        manager\n            .create_table(\n                Table::create()\n                    .table({table_type}::Table)\n                    .if_not_exists()\n                    .col(ColumnDef::new({table_type}::Id).big_integer().not_null().auto_increment().primary_key())\n"
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

/// Executes the project binary and returns its exit code.
fn run_project(args: &[&str], development: bool) -> anyhow::Result<ExitCode> {
    if !Path::new("Cargo.toml").exists() {
        bail!("no Cargo.toml in the current directory; run this inside an OpenRust project");
    }
    let mut command = Process::new("cargo");
    command.arg("run").arg("--quiet").arg("--").args(args);
    if development {
        command.env("APP_ENV", "development");
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
        insta::assert_snapshot!(cargo_manifest("my-app", None, false, false));
    }

    #[test]
    fn generated_manifest_enables_requested_http_client_feature() {
        let manifest = cargo_manifest("my-app", None, false, true);
        assert!(manifest.contains("features = [\"http-client\"]"));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--http-client"]).is_ok());
    }

    #[test]
    fn project_template_has_expected_layout_and_compilable_test() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false).expect("generate project");
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
            populate_project(&project, "blog", "Blog", Some(backend), false, false)
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
            populate_project(&project, "blog", "Blog", Some(backend), true, false)
                .expect("generate database-backed auth project");
            let manifest = fs::read_to_string(project.join("Cargo.toml"))
                .expect("read generated app manifest");
            assert!(manifest.contains(feature));
            assert!(manifest.contains("openrust-test"));
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
            let app = fs::read_to_string(project.join("src/app.rs")).expect("read generated app");
            assert!(app.contains("AuthPlugin::new()"));
            assert!(app.contains("JwtPlugin::new()"));
            assert!(app.contains("AuthFlowsPlugin::new()"));
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
        generate_model(&model, &project).expect("generate model");
        let entity = fs::read_to_string(project.join("src/models/_entities/post.rs"))
            .expect("read generated model");
        assert!(entity.contains("pub title: String"));
        assert!(entity.contains("pub published_at: Option<DateTimeUtc>"));
        assert!(entity.contains("pub author_id: i64"));

        generate_migration(
            &[
                "posts".to_owned(),
                "title:string".to_owned(),
                "published_at:date_time?".to_owned(),
                "metadata:json".to_owned(),
                "author:references".to_owned(),
            ],
            &project,
        )
        .expect("generate model migration");
        for (table, field) in [("comments", "body:text"), ("labels", "slug:string")] {
            generate_migration(&[table.to_owned(), field.to_owned()], &project)
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
        assert!(parse_model_arguments(&values).is_err());
        let parsed = Cli::try_parse_from(["openrust", "g", "model", "post", "title:string"]);
        assert!(parsed.is_ok());
    }

    #[test]
    fn rejects_parent_paths_and_existing_destinations() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        assert!(create_project("../outside", None, false, false, &root).is_err());
        assert!(create_project("--", None, false, false, &root).is_err());
        fs::create_dir(root.join("existing")).expect("create existing destination");
        assert!(create_project("existing", None, false, false, &root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
