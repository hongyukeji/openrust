//! `openrust`：OpenRust 框架的全局命令行工具。

#![forbid(unsafe_code)]

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, bail};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use clap::{
    Arg, ArgAction, Command, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum,
};
use serde_json::Value;

static MIGRATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// OpenRust 框架的命令行工具。
#[derive(Debug, Parser)]
#[command(name = "openrust", version, about, long_about = None, disable_help_subcommand = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Builtin>,
}

#[derive(Debug, Subcommand)]
enum Builtin {
    /// 使用内置模板创建 OpenRust 应用。
    New {
        /// 应用目标目录；支持相对或绝对路径。
        name: String,
        /// 选择数据库驱动；标准应用默认 MySQL，也可显式选择 PostgreSQL。
        #[arg(long, value_enum)]
        db: Option<DatabaseBackend>,
        /// 生成无数据库驱动、迁移包和队列依赖的精简应用。
        #[arg(long, conflicts_with_all = ["db", "auth", "auth_mfa"])]
        minimal: bool,
        /// 添加数据库认证能力和受保护的示例路由。
        #[arg(long)]
        auth: bool,
        /// 为认证启用 TOTP 多因素登录（需要 `--auth` 与 `--encryption`）。
        #[arg(long, requires_all = ["auth", "encryption"])]
        auth_mfa: bool,
        /// 启用可选的出站 HTTP 客户端。
        #[arg(long)]
        http_client: bool,
        /// 启用可选的 Twilio 短信驱动和出站 HTTP 客户端。
        #[arg(long)]
        sms_twilio: bool,
        /// 启用应用数据加密和密钥轮换。
        #[arg(long)]
        encryption: bool,
        /// 启用 gRPC feature 并生成 Protobuf 编译入口。
        #[arg(long)]
        grpc: bool,
    },
    /// 以开发模式运行应用。
    Dev {
        /// 应用的配置环境变量命名空间；须与 `Application::config_namespace` 一致。
        #[arg(long, default_value = "APP_")]
        config_namespace: String,
    },
    /// 运行应用的 server 命令。
    Serve,
    /// 生成模型、迁移、邮件、授权策略或后台任务。
    #[command(alias = "g")]
    Generate {
        #[arg(value_enum)]
        kind: GeneratorKind,
        /// 模型或表名，后接 `field:type` 字段声明。
        #[arg(required = true)]
        values: Vec<String>,
        /// 生成 `UUIDv7` 主键（模型与迁移需使用相同选项）。
        #[arg(long)]
        uuid_id: bool,
        /// 添加可空的 UTC `deleted_at` 列和显式查询范围。
        #[arg(long)]
        soft_delete: bool,
    },
    /// 在 app/ 中创建应用控制器。
    #[command(name = "make:controller")]
    MakeController { name: String },
    /// 在 app/ 中创建数据库模型。
    #[command(name = "make:model")]
    MakeModel {
        name: String,
        /// 模型字段，格式为 field:type。
        fields: Vec<String>,
    },
    /// 创建 `ServiceProvider` 并登记到 `bootstrap/providers.rs`。
    #[command(name = "make:provider")]
    MakeProvider { name: String },
    /// 管理应用本地加密密钥。
    Key {
        #[command(subcommand)]
        command: KeyCommand,
    },
    /// 其他命令将转发给当前项目的应用二进制程序。
    #[command(external_subcommand)]
    Forward(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum KeyCommand {
    /// 生成 32 字节加密密钥并输出其 Base64 值。
    Generate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum GeneratorKind {
    Model,
    Migration,
    Mailer,
    Policy,
    Job,
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
    default_member: bool,
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
    let default_members = metadata["workspace_default_members"]
        .as_array()
        .context("Cargo metadata 缺少 workspace_default_members")?;
    let packages = metadata["packages"]
        .as_array()
        .context("Cargo metadata omitted packages")?
        .iter()
        .map(|package| {
            Ok(WorkspacePackage {
                default_member: default_members.contains(&package["id"]),
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
    Ok(FrameworkWorkspace { root, packages })
}

fn application_directory(
    workspace: &FrameworkWorkspace,
    current_dir: &Path,
) -> anyhow::Result<(PathBuf, String)> {
    let current_dir = current_dir.canonicalize()?;
    let unique_default = workspace
        .packages
        .iter()
        .filter(|package| package.default_member)
        .count()
        == 1;
    let app = workspace
        .packages
        .iter()
        .filter_map(|package| {
            let directory = package.manifest_path.parent()?;
            (directory.join("src/main.rs").is_file()
                && directory.join("bootstrap/app.rs").is_file()
                && (current_dir.starts_with(directory)
                    || (current_dir == workspace.root && unique_default && package.default_member)))
                .then_some((directory, package.name.as_str()))
        })
        .max_by_key(|(directory, _)| directory.components().count())
        .context("未找到应用，请从含 bootstrap/app.rs 的应用目录执行")?;
    Ok((app.0.to_path_buf(), app.1.to_owned()))
}

fn standalone_manifest(source: &Path, project: &Path, database: bool) -> anyhow::Result<()> {
    let source_manifest =
        fs::read_to_string(source.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
    let mut app =
        fs::read_to_string(project.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
    app["workspace"] = source_manifest["workspace"].clone();
    if let Some(workspace) = app["workspace"].as_table_mut() {
        workspace.remove("exclude");
    }
    app["workspace"]["members"] = toml_edit::value(if database {
        ["database/migrations"]
            .into_iter()
            .collect::<toml_edit::Array>()
    } else {
        toml_edit::Array::new()
    });
    app["workspace"]["default-members"] =
        toml_edit::value(["."].into_iter().collect::<toml_edit::Array>());
    let mut required = std::collections::BTreeSet::new();
    for manifest in [
        Some(project.join("Cargo.toml")),
        database.then(|| project.join("database/migrations/Cargo.toml")),
    ]
    .into_iter()
    .flatten()
    {
        let manifest = fs::read_to_string(manifest)?.parse::<toml_edit::DocumentMut>()?;
        for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(dependencies) = manifest.get(section).and_then(toml_edit::Item::as_table) {
                required.extend(dependencies.iter().map(|(name, _)| name.to_owned()));
            }
        }
    }
    if let Some(dependencies) = app["workspace"]["dependencies"].as_table_mut() {
        dependencies.retain(|name, _| required.contains(name));
        for (_, dependency) in dependencies.iter_mut() {
            if let Some(path) = dependency.get("path").and_then(toml_edit::Item::as_str) {
                let path = source.join(path).canonicalize()?;
                dependency["path"] = toml_edit::value(path.to_string_lossy().as_ref());
            }
        }
    }
    fs::write(project.join("Cargo.toml"), app.to_string())?;
    Ok(())
}

impl DatabaseBackend {
    fn feature(self) -> &'static str {
        match self {
            Self::Postgres => "db-postgres",
            Self::Mysql => "db-mysql",
        }
    }

    fn connection_name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::Mysql => "mysql",
        }
    }

    fn port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::Mysql => 3306,
        }
    }
}

fn main() -> ExitCode {
    let matches = localized_command().get_matches();
    let cli = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn localized_command() -> Command {
    fn localize(mut command: Command) -> Command {
        let has_version = command.get_version().is_some();
        let mut template = "{before-help}{about-with-newline}用法：{usage}\n".to_owned();
        if command
            .get_subcommands()
            .any(|subcommand| !subcommand.is_hide_set())
        {
            template.push_str("\n命令：\n{subcommands}\n");
        }
        if command.get_positionals().next().is_some() {
            template.push_str("\n参数：\n{positionals}\n");
        }
        template.push_str("\n选项：\n{options}\n");
        template.push_str("{after-help}");
        command = command
            .help_template(template)
            .disable_help_flag(true)
            .disable_version_flag(true)
            .arg(
                Arg::new("help")
                    .short('h')
                    .long("help")
                    .action(ArgAction::Help)
                    .help("显示帮助信息并退出"),
            );
        if has_version {
            command = command.arg(
                Arg::new("version")
                    .short('V')
                    .long("version")
                    .action(ArgAction::Version)
                    .help("显示版本信息并退出"),
            );
        }
        command.mut_subcommands(localize)
    }

    localize(Cli::command())
}

fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        None => {
            localized_command().print_help()?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::New {
            name,
            db,
            minimal,
            auth,
            auth_mfa,
            http_client,
            sms_twilio,
            encryption,
            grpc,
        }) => {
            create_project_with_mfa(
                &name,
                project_database(db, minimal),
                auth,
                auth_mfa,
                http_client,
                sms_twilio,
                encryption,
                grpc,
                &std::env::current_dir()?,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Dev { config_namespace }) => {
            run_application(&["serve".to_owned()], true, &config_namespace)
        }
        Some(Builtin::Serve) => run_project(&["serve".to_owned()], false),
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
        Some(Builtin::MakeController { name }) => {
            let workspace = framework_workspace(&std::env::current_dir()?)?;
            let (project, _) = application_directory(&workspace, &std::env::current_dir()?)?;
            generate_controller(&name, &project)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::MakeModel { name, fields }) => {
            let workspace = framework_workspace(&std::env::current_dir()?)?;
            let (project, _) = application_directory(&workspace, &std::env::current_dir()?)?;
            let mut values = vec![name];
            values.extend(fields);
            generate_application_model(&values, &project)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::MakeProvider { name }) => {
            let workspace = framework_workspace(&std::env::current_dir()?)?;
            let (project, _) = application_directory(&workspace, &std::env::current_dir()?)?;
            generate_provider(&name, &project)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Key {
            command: KeyCommand::Generate,
        }) => {
            writeln!(std::io::stdout().lock(), "{}", generate_key()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Forward(args)) => run_project(&args, false),
    }
}

fn project_database(database: Option<DatabaseBackend>, minimal: bool) -> Option<DatabaseBackend> {
    if minimal {
        None
    } else {
        Some(database.unwrap_or(DatabaseBackend::Mysql))
    }
}

fn generate_key() -> anyhow::Result<String> {
    let mut key = [0_u8; 32];
    getrandom::fill(&mut key).map_err(|_| anyhow::anyhow!("OS randomness provider failed"))?;
    Ok(STANDARD.encode(key))
}

/// 从源码 workspace 创建使用本地依赖的独立应用。
#[allow(
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments,
    reason = "these flags mirror the public CLI options"
)]
#[cfg(test)]
fn create_project(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
    current_dir: &Path,
) -> anyhow::Result<()> {
    create_project_with_mfa(
        name,
        database,
        auth,
        false,
        http_client,
        sms_twilio,
        encryption,
        false,
        current_dir,
    )
}

#[allow(
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments,
    reason = "these flags mirror the public CLI options"
)]
fn create_project_with_mfa(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    auth_mfa: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
    grpc: bool,
    current_dir: &Path,
) -> anyhow::Result<()> {
    if auth && database.is_none() {
        bail!(
            "authentication requires a standard database profile; omit `--minimal` or select `--db postgres|mysql`"
        );
    }
    if auth_mfa && (!auth || !encryption || database.is_none()) {
        bail!("MFA requires `--auth`, `--encryption` and a standard database profile");
    }
    let workspace = framework_workspace(current_dir)?;
    if !workspace.packages.iter().any(|package| {
        package.name == "openrust" && package.manifest_path == workspace.root.join("Cargo.toml")
    }) {
        bail!("请从包含根 openrust library 的 OpenRust 源码 workspace 创建应用");
    }
    let destination = Path::new(name);
    if destination
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        bail!("应用目标不能包含 .. 路径");
    }
    let project_dir = current_dir.join(destination);
    let project_name = project_dir
        .file_name()
        .and_then(|name| name.to_str())
        .context("应用目标须有有效目录名")?;
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

    let result = populate_project_with_mfa(
        &project_dir,
        project_name,
        &type_name,
        database,
        auth,
        auth_mfa,
        http_client,
        sms_twilio,
        encryption,
        grpc,
    );
    if result.is_err() {
        let _ = fs::remove_dir_all(&project_dir);
    }
    result?;
    if let Err(error) = standalone_manifest(&workspace.root, &project_dir, database.is_some()) {
        let _ = fs::remove_dir_all(&project_dir);
        return Err(error).context("无法配置独立应用的本地依赖");
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
    clippy::too_many_lines,
    reason = "template options map directly to the public generator flags"
)]
#[cfg(test)]
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
    populate_project_with_mfa(
        project_dir,
        name,
        crate_name,
        database,
        auth,
        false,
        http_client,
        sms_twilio,
        encryption,
        false,
    )
}

#[allow(
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "template options map directly to the public generator flags"
)]
fn populate_project_with_mfa(
    project_dir: &Path,
    name: &str,
    crate_name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    auth_mfa: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
    grpc: bool,
) -> anyhow::Result<()> {
    if auth_mfa && (!auth || database.is_none() || !encryption) {
        bail!("MFA requires authentication, a database and encryption");
    }
    fs::create_dir(project_dir)
        .with_context(|| format!("failed to create {}", project_dir.display()))?;
    for directory in [
        "config",
        "src",
        "public",
        "app/http/controllers",
        "app/models/_entities",
        "app/services",
        "app/providers",
        "app/policies",
        "app/jobs",
        "app/events",
        "app/mail",
        "app/http/middleware",
        "resources/mail",
        "resources/views",
        "bootstrap",
        "modules",
        "routes",
        "database",
        "storage/app",
        "storage/logs",
        "storage/framework",
        "tests",
        "tests/support",
    ] {
        fs::create_dir_all(project_dir.join(directory))?;
    }
    if database.is_some() {
        fs::create_dir_all(project_dir.join("database/migrations/src"))?;
    }
    if grpc {
        fs::create_dir_all(project_dir.join("proto"))?;
    }

    let mut files = vec![
        (
            "Cargo.toml",
            cargo_manifest(
                name,
                database,
                auth,
                http_client,
                sms_twilio,
                encryption,
                auth_mfa,
                grpc,
            ),
        ),
        (
            ".gitignore",
            include_str!("../templates/.gitignore").to_owned(),
        ),
        (
            ".env.example",
            env_example(name, database, auth, encryption, grpc),
        ),
        (
            "public/index.html",
            include_str!("../templates/public/index.html").replace("{{name}}", name),
        ),
        (
            "resources/views/index.html",
            include_str!("../templates/views/index.html").replace("{{name}}", name),
        ),
        ("src/main.rs", main_source(crate_name, database, grpc)),
        (
            "config/mod.rs",
            code_config_manifest(grpc, database.is_some()),
        ),
        (
            "config/api.rs",
            include_str!("../templates/config/api.rs").to_owned(),
        ),
        (
            "config/app.rs",
            include_str!("../templates/config/app.rs").replace("{{name}}", name),
        ),
        (
            "config/auth.rs",
            include_str!("../templates/config/auth.rs").replace("{{name}}", name),
        ),
        (
            "config/cache.rs",
            include_str!("../templates/config/cache.rs").replace("{{name}}", name),
        ),
        (
            "config/database.rs",
            include_str!("../templates/config/database.rs").to_owned(),
        ),
        (
            "config/i18n.rs",
            include_str!("../templates/config/i18n.rs").to_owned(),
        ),
        (
            "config/log.rs",
            include_str!("../templates/config/log.rs").to_owned(),
        ),
        (
            "config/mail.rs",
            include_str!("../templates/config/mail.rs").to_owned(),
        ),
        (
            "config/modules.rs",
            include_str!("../templates/config/modules.rs").to_owned(),
        ),
        (
            "config/queue.rs",
            include_str!("../templates/config/queue.rs").to_owned(),
        ),
        (
            "config/realtime.rs",
            include_str!("../templates/config/realtime.rs").to_owned(),
        ),
        (
            "config/security.rs",
            include_str!("../templates/config/security.rs").to_owned(),
        ),
        (
            "config/server.rs",
            include_str!("../templates/config/server.rs").to_owned(),
        ),
        (
            "config/session.rs",
            include_str!("../templates/config/session.rs").to_owned(),
        ),
        (
            "config/sms.rs",
            include_str!("../templates/config/sms.rs").to_owned(),
        ),
        (
            "config/storage.rs",
            include_str!("../templates/config/storage.rs").to_owned(),
        ),
        (
            "config/view.rs",
            include_str!("../templates/config/view.rs").to_owned(),
        ),
        ("bootstrap/app.rs", app_source(crate_name)),
        (
            "app/mod.rs",
            include_str!("../templates/app/mod.rs").to_owned(),
        ),
        (
            "modules/mod.rs",
            include_str!("../templates/modules/mod.rs").to_owned(),
        ),
        (
            "routes/mod.rs",
            include_str!("../templates/src/routes.rs").to_owned(),
        ),
        ("routes/api.rs", api_routes_source(auth, auth_mfa)),
        (
            "routes/web.rs",
            include_str!("../templates/routes/web.rs").to_owned(),
        ),
        ("bootstrap/providers.rs", providers_source(database, grpc)),
        (
            "app/http/mod.rs",
            include_str!("../templates/app/http/mod.rs").to_owned(),
        ),
        (
            "app/http/controllers/home.rs",
            include_str!("../templates/app/http/controllers/home.rs").to_owned(),
        ),
        ("routes/console.rs", "//! 应用调度。\n".to_owned()),
        ("database/seeders.rs", seed_source(database)),
        (
            "app/http/controllers/mod.rs",
            include_str!("../templates/app/http/controllers/mod.rs").to_owned(),
        ),
        ("app/models/mod.rs", "//! 应用模型。\n".to_owned()),
        ("app/models/_entities/.gitkeep", String::new()),
        (
            "app/providers/mod.rs",
            include_str!("../templates/app/providers/mod.rs").to_owned(),
        ),
        (
            "app/providers/app.rs",
            include_str!("../templates/app/providers/app.rs").to_owned(),
        ),
        ("app/services/mod.rs", "//! 应用服务。\n".to_owned()),
        ("app/policies/mod.rs", "//! 授权策略。\n".to_owned()),
        ("app/jobs/mod.rs", "//! 后台任务。\n".to_owned()),
        ("app/events/mod.rs", "//! 应用事件。\n".to_owned()),
        ("app/mail/mod.rs", "//! 邮件类型。\n".to_owned()),
        (
            "app/http/middleware/mod.rs",
            "//! 应用中间件。\n".to_owned(),
        ),
        (
            "AGENTS.md",
            include_str!("../templates/AGENTS.md").to_owned(),
        ),
    ];

    if grpc {
        files.push((
            "config/grpc.rs",
            include_str!("../templates/config/grpc.rs").to_owned(),
        ));
    }
    if !auth && !encryption && !grpc {
        files.push((
            "tests/test_app.rs",
            include_str!("../templates/tests/test_app.rs")
                .replace("OpenRustGeneratedApplication", crate_name)
                .replace(
                    "{{seed_module}}",
                    if database.is_some() {
                        "#[path = \"../database/seeders.rs\"]\nmod seed;\n"
                    } else {
                        ""
                    },
                ),
        ));
    }
    if auth || (!encryption && !grpc) {
        files.push((
            "tests/support/request_input.rs",
            include_str!("../templates/tests/support/request_input.rs").to_owned(),
        ));
        files.push((
            "tests/support/message_bags.rs",
            include_str!("../templates/tests/support/message_bags.rs").to_owned(),
        ));
        files.push((
            "tests/support/request_cookie.rs",
            include_str!("../templates/tests/support/request_cookie.rs").to_owned(),
        ));
        files.push((
            "tests/support/request_session.rs",
            include_str!("../templates/tests/support/request_session.rs").to_owned(),
        ));
        files.push((
            "tests/support/request_redirect.rs",
            include_str!("../templates/tests/support/request_redirect.rs").to_owned(),
        ));
    }
    if auth {
        files.push((
            "tests/auth_flows.rs",
            auth_test_template(crate_name, encryption, auth_mfa, grpc),
        ));
    }
    if let Some(database) = database {
        files.extend([
            (
                "database/migrations/Cargo.toml",
                migration_manifest(name, database, auth, auth_mfa),
            ),
            ("database/migrations/src/lib.rs", migration_lib()),
            (
                "tests/migration_catalog.rs",
                migration_catalog_test_source(auth, auth_mfa),
            ),
        ]);
    }
    if grpc {
        let proto_package = name.replace('-', "_");
        files.extend([
            (
                "build.rs",
                "fn main() -> Result<(), Box<dyn std::error::Error>> {\n    println!(\"cargo:rerun-if-changed=proto/echo.proto\");\n    let mut config = tonic_prost_build::Config::new();\n    config.protoc_executable(protoc_bin_vendored::protoc_bin_path()?);\n    tonic_prost_build::configure().compile_with_config(\n        config,\n        &[\"proto/echo.proto\"],\n        &[\"proto\"],\n    )?;\n    Ok(())\n}\n"
                    .to_owned(),
            ),
            (
                "proto/echo.proto",
                "syntax = \"proto3\";\n\npackage {{package}}.v1;\n\nmessage EchoRequest {\n  string value = 1;\n}\n\nmessage EchoResponse {\n  string value = 1;\n}\n\nservice Echo {\n  rpc Unary(EchoRequest) returns (EchoResponse);\n}\n"
                    .replace("{{package}}", &proto_package),
            ),
            (
                "app/grpc.rs",
                include_str!("../templates/app/grpc.rs")
                    .replace("{{package}}", &proto_package),
            ),
            (
                "tests/grpc_flows.rs",
                grpc_test_template(name, database, auth, encryption),
            ),
        ]);
    }
    for (relative_path, contents) in files {
        fs::write(project_dir.join(relative_path), contents)
            .with_context(|| format!("failed to write {relative_path}"))?;
    }
    Ok(())
}

fn code_config_manifest(grpc: bool, queue: bool) -> String {
    include_str!("../templates/config/mod.rs")
        .replace(
            "{{queue_module}}",
            if queue { "pub mod queue;\n" } else { "" },
        )
        .replace(
            "{{queue_registration}}",
            if queue {
                "    let manifest = manifest.section(\"queue\", queue::config);\n"
            } else {
                ""
            },
        )
        .replace("{{grpc_module}}", if grpc { "pub mod grpc;\n" } else { "" })
        .replace(
            "{{grpc_registration}}",
            if grpc {
                "    let manifest = manifest.section(\"grpc\", grpc::config);\n"
            } else {
                ""
            },
        )
}

fn auth_test_template(crate_name: &str, encryption: bool, auth_mfa: bool, grpc: bool) -> String {
    let encryption_config = if encryption {
        "\\n[security.encryption]\\nactive_key_id = \\\"test\\\"\\n\\n[security.encryption.keys]\\ntest = \\\"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=\\\""
    } else {
        ""
    };
    let mut template = include_str!("../templates/tests/auth_flows.rs")
        .replace("OpenRustGeneratedApplication", crate_name)
        .replace("{{encryption_config}}", encryption_config)
        .replace(
            "{{session_protection_assertion}}",
            if encryption {
                "    assert!(session_response.session_cookie_is_protected);"
            } else {
                "    assert!(!session_response.session_cookie_is_protected);"
            },
        )
        .replace(
            "{{grpc_module}}",
            if grpc {
                "#[path = \"../app/grpc.rs\"]\nmod grpc;\n"
            } else {
                ""
            },
        );
    if auth_mfa {
        template = template
            .replace("{{login_credentials_suffix}}", "[\"credentials\"]")
            .replace(
                "{{mfa_flow}}",
                "    run_bounded_mfa_flow(address, mail).await?;\n",
            );
        template.push('\n');
        template.push_str(include_str!("../templates/tests/auth_mfa_flow.rs"));
    } else {
        template = template
            .replace("{{login_credentials_suffix}}", "")
            .replace("{{mfa_flow}}", "");
    }
    template
}

fn grpc_test_template(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    encryption: bool,
) -> String {
    let database_setup = if database.is_some() {
        include_str!("../templates/tests/grpc_database_setup.txt")
    } else {
        ""
    };
    let database_cleanup = if database.is_some() {
        "    let database_result: TestResult = database.close().await.map_err(Into::into);\n"
    } else {
        "    let database_result: TestResult = Ok(());\n"
    };
    let auth_config = if auth {
        include_str!("../templates/tests/grpc_auth_config.txt")
    } else {
        ""
    };
    let encryption_config = if encryption {
        include_str!("../templates/tests/grpc_encryption_config.txt")
    } else {
        ""
    };
    include_str!("../templates/tests/grpc_flows.rs")
        .replace("{{binary_name}}", name)
        .replace("{{database_setup}}", database_setup)
        .replace("{{database_cleanup}}", database_cleanup)
        .replace("{{auth_config}}", auth_config)
        .replace("{{encryption_config}}", encryption_config)
}

#[allow(
    clippy::fn_params_excessive_bools,
    clippy::too_many_arguments,
    reason = "feature flags map directly to the public generator options"
)]
fn cargo_manifest(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    http_client: bool,
    sms_twilio: bool,
    encryption: bool,
    auth_mfa: bool,
    grpc: bool,
) -> String {
    let mut manifest = include_str!("../templates/Cargo.toml").replace("{{name}}", name);
    {
        let mut features = vec!["cache-redis", "view", "session"];
        if let Some(database) = database {
            features.push(database.feature());
            features.extend(["queue", "queue-redis"]);
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
        if auth_mfa {
            features.push("auth-mfa");
        }
        if grpc {
            features.push("grpc");
        }
        let mut dependency = if database.is_some() {
            format!(
                "openrust = {{ workspace = true, features = {features:?} }}\nopenrust-migration = {{ package = \"{name}-migration\", path = \"database/migrations\" }}"
            )
        } else {
            format!("openrust = {{ workspace = true, features = {features:?} }}")
        };
        if grpc {
            dependency.push_str(
                "\ntonic = { workspace = true }\ntonic-prost = { workspace = true }\nprost = { workspace = true }",
            );
        }
        manifest = manifest.replace("openrust = { workspace = true }", &dependency);
    }
    manifest.push_str("serde = { version = \"1\", features = [\"derive\"] }\nserde_json = \"1\"\n");
    manifest.push_str("\n[dev-dependencies]\nfigment = { version = \"0.10\", features = [\"toml\"] }\ntime = \"0.3\"\n");
    if auth {
        manifest.push_str("base64 = \"0.22\"\n");
    }
    if grpc {
        manifest.push_str("tokio = { version = \"1\", features = [\"io-util\", \"macros\", \"net\", \"process\", \"rt-multi-thread\", \"time\"] }\n");
    } else if auth {
        manifest.push_str("tokio = { version = \"1\", features = [\"io-util\", \"macros\", \"net\", \"rt-multi-thread\"] }\n");
    } else {
        manifest.push_str(
            "tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }\n",
        );
    }
    if auth_mfa {
        manifest.push_str("hmac = \"0.13\"\nsha1 = \"0.11\"\nurl = \"2.5\"\n");
    }
    if grpc {
        manifest.push_str("rcgen = \"0.14.10\"\ntempfile = \"3.27\"\n");
    }
    if (auth || grpc)
        && let Some(database) = database
    {
        let _ = FmtWrite::write_fmt(
            &mut manifest,
            format_args!(
                "testing = {{ workspace = true, features = [\"{}\"] }}\n",
                database.feature()
            ),
        );
    }
    if grpc {
        manifest.push_str(
            "\n[build-dependencies]\ntonic-prost-build = { workspace = true }\nprotoc-bin-vendored = { workspace = true }\n",
        );
    }
    manifest
}

fn app_source(crate_name: &str) -> String {
    include_str!("../templates/src/app.rs").replace("{{type_name}}", crate_name)
}

fn providers_source(database: Option<DatabaseBackend>, grpc: bool) -> String {
    let mut additions = String::new();
    if database.is_some() {
        additions.push_str("\n    providers.push(Box::new(openrust::db::MigrationServiceProvider::<\n        openrust_migration::Migrator,\n    >::new()));\n    providers.push(Box::new(openrust::db::SeedServiceProvider::new(crate::seed::run)));");
    } else {
        additions.push_str("\n    providers.push(Box::new(openrust::db::MigrationServiceProvider::<\n        openrust::db::FrameworkMigrator,\n    >::new()));");
    }
    if grpc {
        additions.push_str("\n    providers.push(Box::new(openrust::grpc::GrpcServiceProvider::new()));\n    providers.push(Box::new(crate::grpc::EchoServiceProvider));\n    providers.push(Box::new(openrust::runtime::HttpGrpcServiceProvider));");
    }
    include_str!("../templates/src/providers.rs").replace("{{application_providers}}", &additions)
}

fn api_routes_source(auth: bool, auth_mfa: bool) -> String {
    let mut source = include_str!("../templates/routes/api.rs").to_owned();
    let (registration, handler) = if auth {
        let login_handler = if auth_mfa {
            mfa_login_handler()
        } else {
            standard_login_handler()
        };
        let mfa_handler = if auth_mfa {
            mfa_management_handlers()
        } else {
            String::new()
        };
        (
            "    routes\n        .get(\"/me\", current_user)\n        .layer(openrust::auth::AuthLayer::required(\n            openrust::auth::AuthMode::Jwt,\n        ))\n        .post(\"/auth/register\", auth_register)\n        .post(\n            \"/auth/request-email-verification\",\n            auth_request_email_verification,\n        )\n        .post(\"/auth/verify-email\", auth_verify_email)\n        .post(\"/auth/login\", auth_login)\n        .post(\"/auth/refresh\", auth_refresh)\n        .post(\"/auth/logout\", auth_logout)\n        .post(\"/auth/password-reset\", auth_request_password_reset)\n        .post(\"/auth/password-reset/confirm\", auth_reset_password)\n        .get(\"/\", root)\n        .merge(Routes::new().get(\"/me/pat\", current_user).layer(\n            openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),\n        ))".to_owned(),
            include_str!("../templates/src/auth_routes.rs")
                .replace("{{login_handler}}", login_handler.trim_end())
                .replace("{{mfa_handler}}", mfa_handler.trim_end()),
        )
    } else {
        ("    routes.get(\"/\", root)".to_owned(), String::new())
    };
    let registration = if auth_mfa {
        registration.replace(
            ".post(\"/auth/login\", auth_login)",
            ".post(\"/auth/login\", auth_login)\n        .post(\"/auth/mfa/login\", auth_mfa_login)",
        )
        .replace(
            ".get(\"/\", root)",
            ".get(\"/\", root)\n        .merge(\n            Routes::new()\n                .post(\"/auth/mfa/enrollment\", auth_mfa_enrollment)\n                .post(\n                    \"/auth/mfa/enrollment/confirm\",\n                    auth_mfa_enrollment_confirm,\n                )\n                .post(\"/auth/mfa/disable\", auth_mfa_disable)\n                .post(\"/auth/mfa/recovery-codes\", auth_mfa_recovery_codes)\n                .layer(openrust::auth::AuthLayer::required(\n                    openrust::auth::AuthMode::Jwt,\n                )),\n        )",
        )
    } else {
        registration
    };
    source = source.replace("{{routes}}", &registration);
    source.replace("{{auth_handler}}", handler.trim_end())
}

fn standard_login_handler() -> String {
    "async fn auth_login(\n    Inject(flows): Inject<AuthFlows>,\n    client_ip: ClientIp,\n    Json(input): Json<EmailPasswordInput>,\n) -> openrust::Result<Json<CredentialsResponse>> {\n    Ok(credentials_response(\n        flows\n            .login(&input.email, &input.password, &client_ip.ip().to_string())\n            .await?,\n    ))\n}"
        .to_owned()
}

fn mfa_login_handler() -> String {
    r#"#[derive(serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum AuthLoginResponse {
    Authenticated {
        credentials: CredentialsResponse,
    },
    MfaRequired {
        challenge: String,
        expires_at: String,
    },
}

fn auth_no_store(
    response: impl openrust::http::axum::response::IntoResponse,
) -> openrust::http::axum::response::Response {
    let mut response = response.into_response();
    response.headers_mut().insert(
        openrust::http::axum::http::header::CACHE_CONTROL,
        openrust::http::axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

async fn auth_login(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<EmailPasswordInput>,
) -> openrust::Result<openrust::http::axum::response::Response> {
    match flows
        .begin_login(&input.email, &input.password, &client_ip.ip().to_string())
        .await?
    {
        openrust::auth::LoginAttempt::Authenticated(credentials) => {
            Ok(auth_no_store(Json(AuthLoginResponse::Authenticated {
                credentials: credentials_response(credentials).0,
            })))
        }
        openrust::auth::LoginAttempt::MfaRequired(challenge) => {
            Ok(auth_no_store(Json(AuthLoginResponse::MfaRequired {
                challenge: challenge.expose().to_owned(),
                expires_at: challenge.expires_at().to_rfc3339(),
            })))
        }
    }
}

#[derive(serde::Deserialize)]
struct MfaChallengeInput {
    challenge: String,
    code: String,
}

async fn auth_mfa_login(
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<MfaChallengeInput>,
) -> openrust::Result<openrust::http::axum::response::Response> {
    Ok(auth_no_store(credentials_response(
        flows
            .complete_mfa_login(&input.challenge, &input.code, &client_ip.ip().to_string())
            .await?,
    )))
}"#
    .to_owned()
}

fn mfa_management_handlers() -> String {
    r#"#[derive(serde::Deserialize)]
struct MfaEnrollmentInput {
    email: String,
    password: String,
}

#[derive(serde::Deserialize)]
struct MfaCodeInput {
    code: String,
}

#[derive(serde::Deserialize)]
struct MfaFactorInput {
    email: String,
    password: String,
    code: String,
}

#[derive(serde::Serialize)]
struct MfaEnrollmentResponse {
    provisioning_uri: String,
    expires_at: String,
}

#[derive(serde::Serialize)]
struct MfaRecoveryCodesResponse {
    recovery_codes: Vec<String>,
}

async fn auth_mfa_enrollment(
    user: openrust::auth::CurrentUser,
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<MfaEnrollmentInput>,
) -> openrust::Result<openrust::http::axum::response::Response> {
    let enrollment = flows
        .begin_totp_enrollment(
            user.id(),
            &input.email,
            &input.password,
            env!("CARGO_PKG_NAME"),
            &client_ip.ip().to_string(),
        )
        .await?;
    Ok(auth_no_store(Json(MfaEnrollmentResponse {
        provisioning_uri: enrollment.provisioning_uri().to_owned(),
        expires_at: enrollment.expires_at().to_rfc3339(),
    })))
}

async fn auth_mfa_enrollment_confirm(
    user: openrust::auth::CurrentUser,
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<MfaCodeInput>,
) -> openrust::Result<openrust::http::axum::response::Response> {
    let codes = flows
        .confirm_totp_enrollment(user.id(), &input.code, &client_ip.ip().to_string())
        .await?;
    Ok(auth_no_store(Json(MfaRecoveryCodesResponse {
        recovery_codes: codes.expose().to_vec(),
    })))
}

async fn auth_mfa_disable(
    user: openrust::auth::CurrentUser,
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<MfaFactorInput>,
) -> openrust::Result<Json<std::collections::BTreeMap<&'static str, &'static str>>> {
    flows
        .disable_mfa(
            user.id(),
            &input.email,
            &input.password,
            &input.code,
            &client_ip.ip().to_string(),
        )
        .await?;
    Ok(Json(std::collections::BTreeMap::from([(
        "status", "disabled",
    )])))
}

async fn auth_mfa_recovery_codes(
    user: openrust::auth::CurrentUser,
    Inject(flows): Inject<AuthFlows>,
    client_ip: ClientIp,
    Json(input): Json<MfaFactorInput>,
) -> openrust::Result<openrust::http::axum::response::Response> {
    let codes = flows
        .regenerate_mfa_recovery_codes(
            user.id(),
            &input.email,
            &input.password,
            &input.code,
            &client_ip.ip().to_string(),
        )
        .await?;
    Ok(auth_no_store(Json(MfaRecoveryCodesResponse {
        recovery_codes: codes.expose().to_vec(),
    })))
}"#
    .to_owned()
}

fn main_source(crate_name: &str, database: Option<DatabaseBackend>, grpc: bool) -> String {
    let database_module = if database.is_some() {
        "#[path = \"../database/seeders.rs\"]\nmod seed;"
    } else {
        ""
    };
    let grpc_module = if grpc {
        "#[path = \"../app/grpc.rs\"]\nmod grpc;\n"
    } else {
        ""
    };
    let source = include_str!("../templates/src/main.rs")
        .replace("{{type_name}}", crate_name)
        .replace("{{grpc_module}}", grpc_module);
    if database_module.is_empty() {
        source.replace("{{database_module}}", "")
    } else {
        source.replace("{{database_module}}", &format!("{database_module}\n"))
    }
}

fn env_example(
    name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    encryption: bool,
    grpc: bool,
) -> String {
    let mut contents = "# 复制为 .env 供本地或部署环境使用。不要提交填入实际值的 .env 文件。\n# 生产环境可使用权限受控的 .env 或部署平台注入的变量；进程环境变量优先。\n# 常用环境值和密钥放在这里；结构化默认值见 config/*.rs。\nAPP_ENV=development\nAPP_NAME=".to_owned();
    contents.push_str(name);
    contents.push_str("\n# 服务端错误详情开关；部署环境保持 false。\nAPP_DEBUG=false\nSERVER_PORT=3000\nAPP_URL=\"http://localhost:${SERVER_PORT}\"\n# 签名 URL 使用原始 APP_KEY 字节；生产环境设置不可预测的密钥。\n# APP_KEY=<随机密钥>\n# 轮换时保留旧值用于验签，多个旧值以逗号分隔。\n# APP_PREVIOUS_KEYS=old-one,old-two\n\n# 服务\n# APP_SERVER_HOST=127.0.0.1\n# APP_SERVER_PUBLIC_DIR=public\n# APP_SERVER_ROOT_TARGET=file:/index.html\n\n# 仅本地开发需要时取消注释；生产环境保持默认值。\n# APP_SESSION_SECURE=false\n# APP_VIEW_AUTO_RELOAD=true\n\n# 数据库\n");
    if grpc {
        contents.push_str(
            "\n# 仅本地开发可按需取消注释；生产环境请保持关闭。\n# APP_GRPC_ALLOW_INSECURE_SERVER=true\n",
        );
    }
    contents.push('\n');
    if let Some(database) = database {
        use std::fmt::Write as _;
        let _ = writeln!(contents, "DB_CONNECTION={}", database.connection_name());
        contents.push_str("DB_HOST=127.0.0.1\n");
        let _ = writeln!(contents, "DB_PORT={}", database.port());
        let _ = writeln!(contents, "DB_DATABASE={name}");
        contents.push_str("DB_USERNAME=openrust\nDB_PASSWORD=openrust\n");
        contents.push_str("DB_TABLE_PREFIX=\n");
    } else {
        contents.push_str(
            "# 此精简应用未编译数据库驱动或队列；标准 new 默认 MySQL，--db postgres 可选择 PostgreSQL。\n# DB_CONNECTION=postgres\n# DB_HOST=127.0.0.1\n# DB_PORT=5432\n# DB_DATABASE=应用数据库名\n# DB_USERNAME=数据库用户名\n# DB_PASSWORD=数据库密码\n",
        );
    }
    contents.push_str(
        "\n# 缓存：默认具名 redis store 使用 Redis。\nREDIS_HOST=127.0.0.1\nREDIS_PASSWORD=null\nREDIS_PORT=6379\nREDIS_DB=0\nCACHE_STORE=redis\n\n# 日志、邮件、短信与国际化\nAPP_LOG_LEVEL=info\nMAIL_DRIVER=log\nSMS_DRIVER=log\nI18N_DEFAULT_LOCALE=en\n",
    );
    if database.is_some() {
        contents.push_str(
            "\n# 默认数据库队列；可选择已预置的 Redis 连接，URL 默认继承有效缓存 dsn。\nQUEUE_CONNECTION=database\n# QUEUE_CONNECTION=redis\n# 独立 Redis 队列连接和命名空间可按需覆盖：\n# QUEUE_REDIS_URL=redis://127.0.0.1:6379/\n# QUEUE_REDIS_NAMESPACE=app_queue\n# 先执行 openrust migrate，再显式运行 openrust queue work database --queue default。\n# Redis worker：openrust queue work redis --queue default；serve 不会自动启动 worker。\n",
        );
    } else {
        contents.push_str(
            "\n# --minimal 不编译数据库驱动和队列；标准 new 已预置 MySQL 与可选 Redis 队列。\n",
        );
    }
    if auth {
        contents.push_str(
            "\n# 使用 openrust key generate 生成独立密钥。\nAPP_AUTH_JWT_KEYS_LOCAL=\nAPP_SECURITY_SIGNED_LINKS_KEYS_LOCAL=\n",
        );
    }
    if encryption {
        contents.push_str(
            "\n# 使用 `openrust key generate` 生成密钥，并将其保存在密钥存储中。\nAPP_SECURITY_ENCRYPTION_ACTIVE_KEY_ID=local\nAPP_SECURITY_ENCRYPTION_KEYS_LOCAL=\n",
        );
    }
    contents.push_str(
        "\n# 可选：SMTP 邮件投递配置。\n# MAIL_DRIVER=smtp\n# MAIL_SMTP_HOST=mail.example.com\n# MAIL_SMTP_PORT=587\n# MAIL_SMTP_STARTTLS=true\n# 本地 Mailpit 地址为 127.0.0.1:1025 时，将 MAIL_SMTP_STARTTLS 设为 false。\n# MAIL_SMTP_USERNAME=\n# MAIL_SMTP_PASSWORD=\n# 可选：Twilio 短信驱动（sms-twilio feature）。\n# SMS_DRIVER=twilio\n# SMS_ACCOUNT_SID=\n# SMS_API_KEY_SID=\n# SMS_API_KEY_SECRET=\n# SMS_FROM 和 SMS_MESSAGING_SERVICE_SID 必须且只能设置一项。\n",
    );
    contents
}

fn seed_source(database: Option<DatabaseBackend>) -> String {
    if database.is_some() {
        "//! 应用数据填充入口。\n\nuse openrust::{Db, Result};\n\npub async fn run(_database: Db) -> Result<()> {\n    Ok(())\n}\n".to_owned()
    } else {
        "//! 应用数据填充入口。\n".to_owned()
    }
}

fn migration_manifest(name: &str, database: DatabaseBackend, auth: bool, auth_mfa: bool) -> String {
    let mut features = vec![database.feature(), "session", "queue"];
    if auth {
        features.push("auth");
    }
    if auth_mfa {
        features.push("auth-mfa");
    }
    format!(
        "[package]\nname = \"{name}-migration\"\npublish = false\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nopenrust = {{ workspace = true, features = {features:?} }}\n"
    )
}

fn migration_lib() -> String {
    "//! 应用数据库迁移；框架基础设施仅从统一清单贡献一次。\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\npub struct Migrator;\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigratorTrait for Migrator {\n    fn migrations() -> Vec<Box<dyn MigrationTrait>> {\n        let mut migrations = openrust::db::FrameworkMigrator::migrations();\n        let application_migrations: Vec<Box<dyn MigrationTrait>> = vec![];\n        migrations.extend(application_migrations);\n        migrations\n    }\n}\n".to_owned()
}

fn migration_catalog_test_source(auth: bool, auth_mfa: bool) -> String {
    let auth = if auth {
        "assert!(names.contains(&openrust::auth::AuthMigration.name().to_owned()));\n    assert!(names.contains(&openrust::auth::AuthRefreshTokenMigration.name().to_owned()));\n    assert!(names.contains(&openrust::auth::AuthVerificationCodeMigration.name().to_owned()));\n    assert!(names.contains(&openrust::auth::AuthPolicyMigration.name().to_owned()));"
    } else {
        "assert!(names.contains(&openrust::session::SessionMigration.name().to_owned()));"
    };
    include_str!("../templates/tests/migration_catalog.rs")
        .replace("// {{auth_migration_assertions}}", auth)
        .replace(
            "// {{mfa_migration_assertions}}",
            if auth_mfa {
                "assert!(names.contains(&openrust::auth::AuthMfaMigration.name().to_owned()));"
            } else {
                ""
            },
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
        GeneratorKind::Mailer | GeneratorKind::Policy | GeneratorKind::Job
            if uuid_id || soft_delete =>
        {
            bail!("--uuid-id and --soft-delete apply only to model and migration generators")
        }
        GeneratorKind::Mailer => generate_mailer(values, &project_dir),
        GeneratorKind::Policy => generate_policy(values, &project_dir),
        GeneratorKind::Job => generate_job(values, &project_dir),
    }
}

fn enable_uuid_feature(project_dir: &Path) -> anyhow::Result<()> {
    let mut manifests = vec![project_dir.join("Cargo.toml")];
    let migration_package = project_dir.join("database/migrations");
    match fs::symlink_metadata(&migration_package) {
        Ok(_) => manifests.push(migration_package.join("Cargo.toml")),
        // 仅整个 package 不存在时可省略；目录存在而清单缺失或不可读必须报错。
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut updates = Vec::new();
    for path in manifests {
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
            updates.push((path, document.to_string()));
        }
    }
    // 两份清单全部读取、解析和准备成功后才开始写，不能留下预检失败的半更新。
    for (path, source) in updates {
        fs::write(path, source)?;
    }
    Ok(())
}

fn generate_mailer(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let [name] = values else {
        bail!("usage: openrust generate mailer <name>");
    };
    let module = normalize_identifier(name)?;
    let type_name = pascal_case(&module);
    let mailer_file = project_dir.join(format!("app/mail/{module}.rs"));
    let template_dir = project_dir.join(format!("resources/mail/{module}"));
    if mailer_file.exists() {
        bail!("mailer already exists: {}", mailer_file.display());
    }
    if template_dir.exists() {
        bail!("mailer templates already exist: {}", template_dir.display());
    }
    let module_file = project_dir.join("app/mail/mod.rs");
    if module_file.exists()
        && fs::read_to_string(&module_file)?
            .lines()
            .any(|line| line.trim() == format!("pub mod {module};"))
    {
        bail!("mailer module is already registered: {module}");
    }
    let main_file = project_dir.join("src/main.rs");
    enable_openrust_feature(project_dir, "mail-templates")?;
    for (locale, text, html) in [
        ("en", "Hello, {{ user }}", "<p>Hello, {{ user }}</p>"),
        ("zh-CN", "您好，{{ user }}", "<p>您好，{{ user }}</p>"),
    ] {
        let locale_dir = template_dir.join(locale);
        fs::create_dir_all(&locale_dir)?;
        fs::write(locale_dir.join("text.txt"), format!("{text}\n"))?;
        fs::write(locale_dir.join("html.html"), format!("{html}\n"))?;
    }
    fs::write(mailer_file, render_mailer(&type_name, &module))?;
    append_sorted_module(
        &module_file,
        &format!("pub mod {module};"),
        "//! 应用邮件类型。\n",
    )?;
    append_module(
        &main_file,
        "#[path = \"../app/mail/mod.rs\"]\npub mod mailers;",
        "//! 应用入口。\n",
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
    let policy_file = project_dir.join(format!("app/policies/{module}.rs"));
    if policy_file.exists() {
        bail!("policy already exists: {}", policy_file.display());
    }
    let module_file = project_dir.join("app/policies/mod.rs");
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
        "//! 应用授权策略。\n",
    )?;
    append_module(
        &main_file,
        "#[path = \"../app/policies/mod.rs\"]\npub mod policies;",
        "//! 应用入口。\n",
    )?;
    print_generated(&format!("Generated policy `{module}` for `{resource}`."))?;
    Ok(())
}

fn register_job_module_in_test(test_source: String) -> anyhow::Result<String> {
    let registered = test_source
        .lines()
        .filter(|line| line.trim() == "mod jobs;")
        .count();
    if registered > 1 {
        bail!("application test has ambiguous job module registration");
    }
    if registered == 1 {
        Ok(test_source)
    } else {
        let anchor = "#[path = \"../routes/mod.rs\"]\nmod routes;";
        if test_source.matches(anchor).count() != 1 {
            bail!("application test source does not match the generated template");
        }
        Ok(test_source.replace(
        "#[path = \"../routes/mod.rs\"]\nmod routes;",
        "#[path = \"../routes/mod.rs\"]\nmod routes;\n#[path = \"../app/jobs/mod.rs\"]\nmod jobs;",
    ))
    }
}

fn generate_job(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let [name] = values else {
        bail!("usage: openrust generate job <name>");
    };
    let module = normalize_identifier(name)?;
    let type_name = format!("{}Job", pascal_case(&module));
    let app_manifest_path = project_dir.join("Cargo.toml");
    let app_manifest = fs::read_to_string(&app_manifest_path)?;
    let migration_manifest_path = project_dir.join("database/migrations/Cargo.toml");
    let migration_source = fs::read_to_string(&migration_manifest_path)
        .context("job generation requires a database migration package")?;
    let migration_lib_path = project_dir.join("database/migrations/src/lib.rs");
    let migration_registry = fs::read_to_string(&migration_lib_path)?;
    let job_file = project_dir.join(format!("app/jobs/{module}.rs"));
    if job_file.exists() {
        bail!("job already exists: {}", job_file.display());
    }
    let module_file = project_dir.join("app/jobs/mod.rs");
    let modules = fs::read_to_string(&module_file)?;
    if modules
        .lines()
        .any(|line| line.trim() == format!("pub mod {module};"))
    {
        bail!("job module is already registered: {module}");
    }
    let main_file = project_dir.join("src/main.rs");
    let main_source = fs::read_to_string(&main_file)?;
    let app_file = project_dir.join("bootstrap/providers.rs");
    let app_source = fs::read_to_string(&app_file)?;
    let mut updated_tests = Vec::new();
    for relative in ["tests/auth_flows.rs", "tests/test_app.rs"] {
        let path = project_dir.join(relative);
        if path.exists() {
            updated_tests.push((
                path.clone(),
                register_job_module_in_test(fs::read_to_string(path)?)?,
            ));
        }
    }
    let (updated_config_manifest, queue_config_missing) = prepare_queue_configuration(project_dir)?;
    let updated_migration = register_queue_migration(&migration_registry)?;
    let updated_app_manifest = enable_feature_source(&app_manifest, "queue", &app_manifest_path)?;
    let updated_migration_manifest =
        enable_feature_source(&migration_source, "queue", &migration_manifest_path)?;
    let updated_app_manifest = add_serde_workspace_dependency(&updated_app_manifest)?;
    let updated_app = if app_source.contains("crate::jobs::JobsServiceProvider") {
        app_source
    } else {
        if app_source.matches("    providers\n}").count() != 1 {
            bail!("application provider registry has a missing or ambiguous insertion point");
        }
        let updated = app_source.replace(
            "    providers\n}",
            "    providers.push(Box::new(crate::jobs::JobsServiceProvider));\n    providers\n}",
        );
        if updated == app_source {
            bail!("application source does not match the generated app template");
        }
        updated
    };
    let updated_main = if main_source.contains("mod jobs;") {
        main_source
    } else {
        format!("{main_source}\n#[path = \"../app/jobs/mod.rs\"]\nmod jobs;\n")
    };
    let updated_modules = if modules.contains("pub struct JobsServiceProvider") {
        let registration =
            format!("contributions.register_job::<crate::jobs::{module}::{type_name}>()?;");
        if modules.contains(&registration) {
            bail!("job handler is already registered: {module}");
        }
        if modules.matches("            Ok(())").count() != 1 {
            bail!("job provider source has a missing or ambiguous registration point");
        }
        append_sorted_module_content(&modules, &format!("pub mod {module};"))?.replace(
            "            Ok(())",
            &format!("            {registration}\n            Ok(())"),
        )
    } else {
        format!(
            "{}\npub mod {module};\n\nuse openrust::core::{{BootContext, Result, ServiceProvider}};\nuse openrust::queue::QueueContributions;\n\n/// 注册应用的后台任务。\npub struct JobsServiceProvider;\n\nimpl ServiceProvider for JobsServiceProvider {{\n    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> openrust::core::BoxFuture<'a, Result<()>> {{\n        Box::pin(async move {{\n            let contributions = app.resolve::<QueueContributions>().await?;\n            contributions.register_job::<crate::jobs::{module}::{type_name}>()?;\n            Ok(())\n        }})\n    }}\n}}\n",
            modules.trim_end(),
        )
    };
    write_queue_configuration(project_dir, &updated_config_manifest, queue_config_missing)?;
    fs::write(&job_file, render_job(&type_name, &module))?;
    fs::write(&module_file, updated_modules)?;
    fs::write(&main_file, updated_main)?;
    fs::write(&app_file, updated_app)?;
    for (path, source) in updated_tests {
        fs::write(path, source)?;
    }
    fs::write(&app_manifest_path, updated_app_manifest)?;
    fs::write(&migration_manifest_path, updated_migration_manifest)?;
    fs::write(&migration_lib_path, updated_migration)?;
    print_generated(&format!("Generated job `{module}`."))?;
    Ok(())
}

// 在任何写入前准备配置清单；用户已有 queue.rs 保留，冲突清单明确拒绝。
fn prepare_queue_configuration(project_dir: &Path) -> anyhow::Result<(String, bool)> {
    let config_manifest = fs::read_to_string(project_dir.join("config/mod.rs"))
        .context("job generation requires the application code configuration manifest")?;
    let updated_manifest = register_queue_config(&config_manifest)?;
    let queue_config_missing = match fs::symlink_metadata(project_dir.join("config/queue.rs")) {
        Ok(metadata) if metadata.is_file() => false,
        Ok(_) => bail!("queue configuration path is not a file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => return Err(error.into()),
    };
    Ok((updated_manifest, queue_config_missing))
}

// 与生成器其他产物一起在所有预检完成后写入；已有可编辑 queue.rs 不覆盖。
fn write_queue_configuration(
    project_dir: &Path,
    manifest: &str,
    create_queue: bool,
) -> anyhow::Result<()> {
    if create_queue {
        fs::write(
            project_dir.join("config/queue.rs"),
            include_str!("../templates/config/queue.rs"),
        )?;
    }
    fs::write(project_dir.join("config/mod.rs"), manifest)?;
    Ok(())
}

// 只接受生成清单的唯一公开插入点，不猜测任意 Rust 源码或覆写用户配置。
fn register_queue_config(source: &str) -> anyhow::Result<String> {
    let declaration = "pub mod queue;";
    let registration = ".section(\"queue\", queue::config)";
    let declarations = source
        .lines()
        .filter(|line| line.trim() == declaration)
        .count();
    let registrations = source.matches(registration).count();
    if declarations > 1 || registrations > 1 {
        bail!("queue configuration has ambiguous declarations or registrations");
    }
    let compact: String = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    if compact.matches(".section(\"queue\",").count() != registrations
        || compact.matches("modqueue;").count() != declarations
    {
        bail!(
            "queue configuration uses a custom declaration or registration; register it explicitly"
        );
    }
    let mut updated = source.to_owned();
    if declarations == 0 {
        let anchor = "pub mod mail;";
        if source.lines().filter(|line| line.trim() == anchor).count() != 1
            || source.matches(anchor).count() != 1
        {
            bail!("configuration manifest has a missing or ambiguous module insertion point");
        }
        updated = updated.replace(anchor, &format!("{anchor}\n{declaration}"));
    }
    if registrations == 0 {
        let anchor = ".section(\"storage\", storage::config)";
        if source.matches(anchor).count() != 1 {
            bail!("configuration manifest has a missing or ambiguous section insertion point");
        }
        updated = updated.replace(anchor, &format!("{anchor}\n        {registration}"));
    }
    Ok(updated)
}

fn render_job(type_name: &str, module: &str) -> String {
    format!(
        "//! 可由数据库队列执行的应用任务。\n\nuse openrust::core::{{BoxFuture, Error, Result}};\nuse openrust::queue::{{Job, JobContext}};\nuse serde::{{Deserialize, Serialize}};\n\n/// `{module}` 后台任务。\n#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {type_name};\n\nimpl Job for {type_name} {{\n    const NAME: &'static str = \"{module}.v1\";\n\n    fn handle(&self, _context: JobContext) -> BoxFuture<'_, Result<()>> {{\n        Box::pin(async {{\n            Err(Error::custom(\n                \"job_not_implemented\",\n                \"请先实现此任务的业务逻辑，再启动 worker\",\n            ))\n        }})\n    }}\n}}\n"
    )
}

fn enable_feature_source(source: &str, feature: &str, path: &Path) -> anyhow::Result<String> {
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
        .context("OpenRust feature list is invalid")?;
    if !features.iter().any(|item| item.as_str() == Some(feature)) {
        features.push(feature);
    }
    Ok(document.to_string())
}

fn add_serde_workspace_dependency(source: &str) -> anyhow::Result<String> {
    let mut document = source
        .parse::<toml_edit::DocumentMut>()
        .context("application Cargo.toml is invalid TOML")?;
    let dependencies = document
        .get_mut("dependencies")
        .and_then(toml_edit::Item::as_table_mut)
        .context("application manifest has no dependencies table")?;
    if !dependencies.contains_key("serde") {
        let mut dependency = toml_edit::InlineTable::new();
        dependency.insert("workspace", toml_edit::Value::from(true));
        dependencies.insert(
            "serde",
            toml_edit::Item::Value(toml_edit::Value::InlineTable(dependency)),
        );
    }
    Ok(document.to_string())
}

fn register_queue_migration(registry: &str) -> anyhow::Result<String> {
    let framework = "let mut migrations = openrust::db::FrameworkMigrator::migrations();";
    if registry.contains("FrameworkMigrator::migrations()") {
        if registry.matches("FrameworkMigrator::migrations()").count() != 1
            || registry
                .lines()
                .filter(|line| line.trim() == framework)
                .count()
                != 1
            || registry
                .matches("migrations.extend(application_migrations);")
                .count()
                != 1
            || registry.contains("openrust::queue::QueueMigration")
        {
            bail!("framework migration registry is ambiguous or duplicates queue migrations");
        }
        // 应用和迁移依赖均启用 queue 后，唯一框架清单已贡献 QueueMigration。
        return Ok(registry.to_owned());
    }
    if registry.contains("openrust::queue::QueueMigration") {
        if registry.matches("openrust::queue::QueueMigration").count() == 1 {
            return Ok(registry.to_owned());
        }
        bail!("queue migration is registered more than once");
    }
    let start = registry
        .find("vec![")
        .context("migration registry has no migrations vector")?;
    let relative_end = registry[start..]
        .find(']')
        .context("migration list is unterminated")?;
    let end = start + relative_end;
    let list_start = start + "vec![".len();
    let list = &registry[list_start..end];
    let trimmed_list = list.trim_end();
    let insertion_at = list_start + trimmed_list.len();
    let suffix = if trimmed_list.is_empty() || trimmed_list.ends_with(',') {
        ""
    } else {
        ","
    };
    let insertion = format!("{suffix}\n            Box::new(openrust::queue::QueueMigration),");
    let mut updated = registry.to_owned();
    updated.insert_str(insertion_at, &insertion);
    Ok(updated)
}

fn enable_openrust_feature(project_dir: &Path, feature: &str) -> anyhow::Result<()> {
    let path = project_dir.join("Cargo.toml");
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
        .any(|existing| existing.as_str() == Some(feature))
    {
        features.push(feature);
        fs::write(path, document.to_string())?;
    }
    Ok(())
}

fn render_mailer(type_name: &str, module: &str) -> String {
    r#"//! OpenRust 生成的应用邮件类型。
//! 请在 `i18n` catalog 中提供 `mailers.__MODULE__.subject`，并按需编辑 `resources/mail/__MODULE__/`。

use std::collections::BTreeMap;

use openrust::{
    i18n::{I18n, Locale, MessageArgs},
    mail::{Email, EmailTemplateBody, EmailTemplateCatalog, EmailTemplateError, lettre::Message},
};

/// 使用受信模板构建的邮件。
pub struct __TYPE__(Message);

impl __TYPE__ {
    /// 注册嵌入式简体中文和英文模板。
    pub fn register(catalog: &mut EmailTemplateCatalog) -> Result<(), EmailTemplateError> {
        catalog.insert(
            "en",
            "__MODULE__/text.txt",
            include_str!("../../resources/mail/__MODULE__/en/text.txt"),
        )?;
        catalog.insert(
            "en",
            "__MODULE__/html.html",
            include_str!("../../resources/mail/__MODULE__/en/html.html"),
        )?;
        catalog.insert(
            "zh-CN",
            "__MODULE__/text.txt",
            include_str!("../../resources/mail/__MODULE__/zh-CN/text.txt"),
        )?;
        catalog.insert(
            "zh-CN",
            "__MODULE__/html.html",
            include_str!("../../resources/mail/__MODULE__/zh-CN/html.html"),
        )?;
        Ok(())
    }

    /// 按显式 locale 渲染邮件并构建 MIME alternative。
    pub fn new(
        catalog: &EmailTemplateCatalog,
        i18n: &I18n,
        locale: &Locale,
        subject_args: &MessageArgs,
        context: &BTreeMap<String, String>,
        from: &str,
        to: &str,
    ) -> Result<Self, EmailTemplateError> {
        let rendered = catalog.render_email(
            i18n,
            locale,
            "mailers.__MODULE__.subject",
            subject_args,
            EmailTemplateBody {
                text: "__MODULE__/text.txt",
                html: Some("__MODULE__/html.html"),
            },
            context,
        )?;
        rendered.message(from, to).map(Self)
    }
}

impl Email for __TYPE__ {
    fn message(&self) -> openrust::Result<Message> {
        Ok(self.0.clone())
    }
}
"#
    .replace("__TYPE__", type_name)
    .replace("__MODULE__", module)
}

fn render_policy(type_name: &str, resource: &str) -> String {
    let permissions = ["read", "create", "update", "delete"];
    let mut source = format!(
        "//! OpenRust 生成的应用授权策略。\n\nuse openrust::auth::{{CurrentUser, Policy}};\n\n/// `{resource}` 的授权辅助类型。\n#[derive(Clone, Debug)]\npub struct {type_name} {{\n    policy: Policy,\n}}\n\nimpl {type_name} {{\n    /// 使用应用共享的策略服务创建资源策略。\n    pub fn new(policy: Policy) -> Self {{\n        Self {{ policy }}\n    }}\n\n"
    );
    for permission in permissions {
        let constant = permission.to_ascii_uppercase();
        let _ = writeln!(
            source,
            "    /// 执行 {permission} {resource} 操作所需的权限。\n    pub const {constant}_PERMISSION: &'static str = \"{resource}:{permission}\";\n\n    /// 检查是否具有 `{resource}:{permission}` 权限。\n    pub async fn authorize_{permission}(&self, user: &CurrentUser) -> openrust::Result<()> {{\n        self.policy.authorize(user, Self::{constant}_PERMISSION).await\n    }}\n"
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
    // 通用 ORM 实体不要求连接驱动；仍在任何写入前确认目标应用清单可读。
    let _manifest = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    let module = normalize_identifier(name)?;
    let table = pluralize(&module);
    let model_file = project_dir.join(format!("app/models/_entities/{module}.rs"));
    if model_file.exists() {
        bail!("model already exists: {}", model_file.display());
    }
    let entity_module = project_dir.join("app/models/_entities/mod.rs");
    let models_module = project_dir.join("app/models/mod.rs");
    let main_file = project_dir.join("src/main.rs");
    let generated = render_model(&table, &fields, uuid_id, soft_delete);
    if uuid_id {
        enable_uuid_feature(project_dir)?;
    }
    fs::write(&model_file, generated)?;
    append_module(
        &entity_module,
        &format!("pub mod {module};"),
        "//! 生成的模型实体。\n",
    )?;
    append_module(&models_module, "pub mod _entities;", "//! 应用模型。\n")?;
    append_module(
        &main_file,
        "#[path = \"../app/models/mod.rs\"]\npub mod models;",
        "//! 应用入口。\n",
    )?;
    print_generated(&format!("Generated model `{module}` for table `{table}`."))?;
    Ok(())
}

fn generate_controller(name: &str, project_dir: &Path) -> anyhow::Result<()> {
    let module = normalize_generator_type(name, "Controller")?;
    let type_name = format!("{}Controller", pascal_case(&module));
    let file = project_dir.join(format!("app/http/controllers/{module}.rs"));
    if file.exists() {
        bail!("controller already exists: {}", file.display());
    }
    let registry = project_dir.join("app/http/controllers/mod.rs");
    let source = format!(
        "//! 应用控制器。\n\n/// `{type_name}` 控制器；在 `routes/web.rs` 或 `routes/api.rs` 中注册其处理方法。\npub struct {type_name};\n"
    );
    fs::write(&file, source)?;
    append_sorted_module(
        &registry,
        &format!("pub mod {module};"),
        "//! 应用控制器。\n",
    )?;
    print_generated(&format!("Generated controller `{type_name}`."))?;
    Ok(())
}

fn generate_application_model(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let Some(name) = values.first() else {
        bail!("usage: openrust make:model <name> [field:type ...]");
    };
    let _manifest = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    let module = normalize_generator_type(name, "")?;
    let fields = if values.len() == 1 {
        Vec::new()
    } else {
        parse_model_arguments(values, false)?.1
    };
    let file = project_dir.join(format!("app/models/{module}.rs"));
    if file.exists() {
        bail!("model already exists: {}", file.display());
    }
    let table = pluralize(&module);
    fs::create_dir_all(project_dir.join("app/models"))?;
    fs::write(&file, render_model(&table, &fields, false, false))?;
    append_sorted_module(
        &project_dir.join("app/models/mod.rs"),
        &format!("pub mod {module};"),
        "//! 应用模型。\n",
    )?;
    append_sorted_module(
        &project_dir.join("app/mod.rs"),
        "pub mod models;",
        "//! 应用代码编译入口。\n",
    )?;
    print_generated(&format!("Generated model `{module}` for table `{table}`."))?;
    Ok(())
}

fn generate_provider(name: &str, project_dir: &Path) -> anyhow::Result<()> {
    let module = normalize_generator_type(name, "ServiceProvider")?;
    let type_name = format!("{}ServiceProvider", pascal_case(&module));
    let file = project_dir.join(format!("app/providers/{module}.rs"));
    if file.exists() {
        bail!("provider already exists: {}", file.display());
    }
    let modules = project_dir.join("app/providers/mod.rs");
    let bootstrap = project_dir.join("bootstrap/providers.rs");
    let bootstrap_source = fs::read_to_string(&bootstrap)?;
    let registration =
        format!("    providers.push(Box::new(crate::app::providers::{module}::{type_name}));");
    if bootstrap_source.contains(&registration) {
        bail!("provider is already registered: {type_name}");
    }
    let updated_bootstrap = bootstrap_source.replace(
        "    Vec::new()",
        &format!("    let mut providers: Vec<Box<dyn ServiceProvider>> = Vec::new();\n{registration}\n    providers"),
    );
    let updated_bootstrap = if updated_bootstrap == bootstrap_source {
        bootstrap_source.replace(
            "    providers\n}",
            &format!("{registration}\n    providers\n}}"),
        )
    } else {
        updated_bootstrap
    };
    if updated_bootstrap == bootstrap_source {
        bail!("bootstrap/providers.rs has an unsupported provider registry layout");
    }
    let source = format!(
        "//! 应用服务 Provider。\n\nuse openrust::core::{{BootContext, BoxFuture, RegisterContext, ServiceProvider}};\nuse openrust::prelude::*;\n\n/// `{type_name}` 的注册与启动钩子。\npub struct {type_name};\n\nimpl ServiceProvider for {type_name} {{\n    fn register<'a>(&'a self, _app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {{\n        Box::pin(async {{ Ok(()) }})\n    }}\n\n    fn boot<'a>(&'a self, _app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {{\n        Box::pin(async {{ Ok(()) }})\n    }}\n}}\n"
    );
    fs::write(&file, source)?;
    append_sorted_module(
        &modules,
        &format!("pub mod {module};"),
        "//! 应用 Provider。\n",
    )?;
    fs::write(&bootstrap, updated_bootstrap)?;
    print_generated(&format!("Generated and registered provider `{type_name}`."))?;
    Ok(())
}

fn normalize_generator_type(name: &str, suffix: &str) -> anyhow::Result<String> {
    let bare = name.strip_suffix(suffix).unwrap_or(name);
    if bare.is_empty() || !bare.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("`{name}` must be an ASCII Rust type or snake_case identifier");
    }
    let mut module = String::new();
    for (index, character) in bare.chars().enumerate() {
        if character.is_ascii_uppercase() {
            if index > 0 && !module.ends_with('_') {
                module.push('_');
            }
            module.push(character.to_ascii_lowercase());
        } else {
            module.push(character);
        }
    }
    normalize_identifier(&module)
}

fn generate_migration(
    values: &[String],
    uuid_id: bool,
    soft_delete: bool,
    project_dir: &Path,
) -> anyhow::Result<()> {
    let (name, fields) = parse_model_arguments(values, soft_delete)?;
    let table = normalize_identifier(name)?;
    let migration_dir = project_dir.join("database/migrations/src");
    if !migration_dir.join("lib.rs").is_file() {
        bail!(
            "migration generation requires a standard project with a database migration package; `--minimal` omits it"
        );
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
    if uuid_id {
        enable_uuid_feature(project_dir)?;
    }
    fs::write(
        &migration_file,
        render_migration(&module, &table, &fields, uuid_id, soft_delete),
    )?;
    fs::write(lib_file, updated_registry)?;
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
        "//! 为 `{table}` 表生成的 SeaORM 实体。\n\nuse openrust::database::sea_orm_migration::sea_orm;\nuse sea_orm::entity::prelude::*;\n\n#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]\n#[sea_orm(table_name = \"{table}\")]\npub struct Model {{\n    {id_attribute}\n    pub id: {id_type},\n"
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
        source.push_str("\nimpl Entity {\n    /// 返回尚未软删除的记录。请将此范围与显式授权及筛选条件组合使用。\n    pub fn not_deleted() -> Select<Self> {\n        Self::find().filter(Column::DeletedAt.is_null())\n    }\n}\n");
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
        "//! 创建 `{table}` 表。\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\n#[derive(DeriveIden)]\nenum {table_type} {{\n    Table,\n    Id,\n"
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
    if contents.contains(module) {
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
    let updated = append_sorted_module_content(&contents, module)?;
    fs::write(path, updated)?;
    Ok(())
}

fn append_sorted_module_content(contents: &str, module: &str) -> anyhow::Result<String> {
    let module_name = module
        .strip_prefix("pub mod ")
        .and_then(|name| name.strip_suffix(';'))
        .context("generated module declaration is invalid")?;
    let mut lines = contents.lines().map(str::to_owned).collect::<Vec<_>>();
    let declarations = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            let name = line.trim().strip_prefix("pub mod ")?.strip_suffix(';')?;
            Some((index, name))
        })
        .collect::<Vec<_>>();
    let mut insertion = declarations
        .iter()
        .find_map(|(index, name)| (*name > module_name).then_some(*index))
        .or_else(|| declarations.last().map(|(index, _)| index + 1))
        .unwrap_or_else(|| {
            lines
                .iter()
                .take_while(|line| line.trim().starts_with("//!") || line.trim().is_empty())
                .count()
        });
    if declarations
        .iter()
        .any(|(index, name)| *index == insertion && *name > module_name)
    {
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
    }
    lines.insert(insertion, module.to_owned());
    let mut updated = lines.join("\n");
    updated.push('\n');
    Ok(updated)
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
fn run_project(args: &[String], development: bool) -> anyhow::Result<ExitCode> {
    run_application(args, development, openrust::config::ENV_PREFIX)
}

fn run_application(
    args: &[String],
    development: bool,
    config_namespace: &str,
) -> anyhow::Result<ExitCode> {
    let current_dir = std::env::current_dir()?;
    let workspace = framework_workspace(&current_dir)?;
    let (app_dir, _package_name) = application_directory(&workspace, &current_dir)?;
    let mut command = Process::new("cargo");
    command
        .current_dir(&app_dir)
        .args(["run", "--quiet", "--"])
        .args(args);
    if development {
        prepare_development_redis(&workspace.root, &app_dir, config_namespace)?;
    }
    command
        .status()
        .context("failed to start `cargo run`")
        .map(exit_code)
}

fn prepare_development_redis(
    workspace_root: &Path,
    app_dir: &Path,
    config_namespace: &str,
) -> anyhow::Result<()> {
    let Some((host, port)) = development_redis_endpoint(app_dir, config_namespace)? else {
        return Ok(());
    };
    if redis_ping(&host, port) {
        return Ok(());
    }

    let compose_file = workspace_root.join("compose.ci.yaml");
    if !compose_file.is_file() {
        bail!(
            "Redis is configured for local development but no Redis service is reachable and compose.ci.yaml is missing"
        );
    }
    let project = workspace_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("openrust")
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    let status = Process::new("docker")
        .args(["compose", "--project-name", &format!("openrust-dev-{project}"), "-f"])
        .arg(compose_file)
        .args(["up", "-d", "redis"])
        .env("OPENRUST_TEST_REDIS_PORT", port.to_string())
        .status()
        .context("failed to start Redis with Docker Compose; start a Redis service or install Docker Compose")?;
    if !status.success() {
        bail!("Docker Compose could not start the configured Redis development service");
    }
    for _ in 0..100 {
        if redis_ping(&host, port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Redis development service did not become ready within 10 seconds")
}

fn development_redis_endpoint(
    app_dir: &Path,
    config_namespace: &str,
) -> anyhow::Result<Option<(String, u16)>> {
    let sources = openrust::config::ConfigLoader::application_sources(app_dir, config_namespace)?;
    let repository = openrust::config::repository::ConfigRepository::new(sources);
    if repository.get("cache")?.is_none() {
        return Ok(None);
    }
    let cache: openrust::config::CacheConfig = repository.section("cache")?;
    let Some(store) = cache.stores.get(&cache.default) else {
        return Ok(None);
    };
    if store.driver != "redis" {
        return Ok(None);
    }
    let dsn = match store.options.get("dsn") {
        Some(Value::String(value)) => value.as_str(),
        Some(_) => bail!("configured Redis connection settings are invalid"),
        None => "redis://127.0.0.1:6379/",
    };
    let parsed =
        url::Url::parse(dsn).context("configured Redis connection settings are invalid")?;
    if parsed.scheme() != "redis" {
        return Ok(None);
    }
    let Some(host) = parsed.host_str() else {
        return Ok(None);
    };
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    Ok(is_loopback.then(|| (host.to_owned(), parsed.port().unwrap_or(6379))))
}

fn redis_ping(host: &str, port: u16) -> bool {
    let Ok(addresses) = (host, port).to_socket_addrs() else {
        return false;
    };
    for address in addresses {
        let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(200))
        else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        if stream.write_all(b"*1\r\n$4\r\nPING\r\n").is_err() {
            continue;
        }
        let mut response = [0_u8; 128];
        if let Ok(length) = stream.read(&mut response) {
            let response = &response[..length];
            if response.starts_with(b"+PONG") || response.starts_with(b"-NOAUTH") {
                return true;
            }
        }
    }
    false
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
        fs::create_dir_all(root.join("src")).expect("创建框架目录");
        fs::write(root.join("src/lib.rs"), "").expect("创建框架库");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"openrust\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\nresolver = \"3\"\n# 保留工作区专有配置与成员说明。\ncustom-marker = \"keep-me\"\nexclude = [\"source-only\"]\nmembers = [\"tools/cli\", \"tools/macros\", \"tools/testing\", \"tools/performance\"]\ndefault-members = [\".\"]\n\n[workspace.package]\nversion = \"0.0.0\"\nedition = \"2024\"\nlicense = \"MIT OR Apache-2.0\"\n\n[workspace.dependencies]\nopenrust = { path = \".\" }\ncli = { path = \"tools/cli\" }\nmacros = { path = \"tools/macros\" }\ntesting = { path = \"tools/testing\" }\n",
        )
        .expect("create framework workspace manifest");
        fs::write(root.join("src/main.rs"), "fn main() {} ").expect("默认应用入口");
        fs::create_dir_all(root.join("bootstrap")).expect("创建应用装配目录");
        fs::write(root.join("bootstrap/app.rs"), "").expect("应用标识文件");
        for (name, path, target) in [
            ("cli", "tools/cli", "src/main.rs"),
            ("macros", "tools/macros", "src/lib.rs"),
            ("testing", "tools/testing", "src/lib.rs"),
            ("performance", "tools/performance", "src/lib.rs"),
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
        fs::create_dir_all(root.join("apps")).expect("create apps directory");
    }

    #[test]
    fn command_help_uses_simplified_chinese_descriptions() {
        let mut command = localized_command();
        let help = command.render_help().to_string();
        assert!(help.contains("用法："));
        assert!(help.contains("命令："));
        assert!(help.contains("选项："));
        assert!(help.contains("使用内置模板创建 OpenRust 应用"));
        assert!(help.contains("以开发模式运行应用"));
        assert!(!help.contains("Create a new OpenRust application"));
        assert!(!help.contains("Usage:"));
        assert!(!help.contains("Print help"));
        assert!(!help.contains("Print version"));

        let new_help = command
            .find_subcommand_mut("new")
            .expect("new subcommand exists")
            .render_help()
            .to_string();
        assert!(new_help.contains("添加数据库认证能力"));
        assert!(new_help.contains("Twilio 短信驱动"));
        assert!(new_help.contains("Protobuf"));

        let generate_help = command
            .find_subcommand_mut("generate")
            .expect("generate subcommand exists")
            .render_help()
            .to_string();
        assert!(generate_help.contains("模型或表名"));
        assert!(generate_help.contains("policy, job"));
        assert!(generate_help.contains("UUIDv7"));
    }

    #[test]
    fn generated_manifest_enables_requested_http_client_feature() {
        let manifest = cargo_manifest("my-app", None, false, true, false, false, false, false);
        assert!(manifest.contains("\"cache-redis\""));
        assert!(manifest.contains("\"http-client\""));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--http-client"]).is_ok());
    }

    #[test]
    fn grpc_project_template_generates_proto_build_without_global_protoc() {
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--grpc"]).is_ok());
        let root = std::env::temp_dir().join(format!("openrust-cli-grpc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project_with_mfa(
            &project,
            "grpc-demo",
            "GrpcDemo",
            None,
            false,
            false,
            false,
            false,
            false,
            true,
        )
        .expect("generate gRPC project");

        let manifest =
            fs::read_to_string(project.join("Cargo.toml")).expect("read generated manifest");
        assert!(manifest.contains("\"grpc\""));
        assert!(manifest.contains("tonic-prost-build = { workspace = true }"));
        assert!(manifest.contains("protoc-bin-vendored = { workspace = true }"));
        assert!(project.join("build.rs").is_file());
        assert!(project.join("proto/echo.proto").is_file());
        assert!(project.join("app/grpc.rs").is_file());
        assert!(project.join("tests/grpc_flows.rs").is_file());
        let app = fs::read_to_string(project.join("bootstrap/providers.rs"))
            .expect("read generated application providers");
        assert!(app.contains("GrpcServiceProvider::new()"));
        assert!(app.contains("EchoServiceProvider"));
        assert!(app.contains("HttpGrpcServiceProvider"));
        let proto =
            fs::read_to_string(project.join("proto/echo.proto")).expect("read generated Protobuf");
        assert!(proto.contains("package grpc_demo.v1;"));
        let config = fs::read_to_string(project.join("config/grpc.rs"))
            .expect("read generated gRPC configuration");
        assert!(config.contains("max_concurrent_rpcs: 1024"));
        assert!(config.contains("allow_insecure_server: false"));
        assert!(config.contains("GrpcServerTlsConfig {"));
        assert!(config.contains("GrpcClientTlsConfig {"));
        let env = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment configuration");
        assert!(env.contains("# APP_GRPC_ALLOW_INSECURE_SERVER=true"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generated_manifest_enables_twilio_sms_feature() {
        let manifest = cargo_manifest("my-app", None, false, false, true, false, false, false);
        assert!(manifest.contains("\"cache-redis\""));
        assert!(manifest.contains("\"sms-twilio\""));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--sms-twilio"]).is_ok());
    }

    #[test]
    fn generated_manifest_and_project_enable_application_encryption() {
        let manifest = cargo_manifest("my-app", None, false, false, false, true, false, false);
        assert!(manifest.contains("\"cache-redis\""));
        assert!(manifest.contains("\"encryption\""));
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--encryption"]).is_ok());

        let root =
            std::env::temp_dir().join(format!("openrust-cli-encryption-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false, false, true)
            .expect("generate encryption project");
        let app = fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
        let config = fs::read_to_string(project.join("config/security.rs"))
            .expect("read generated encryption configuration");
        let env = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(app.contains("providers::application_providers(config)"));
        assert!(!app.contains("EncryptionServiceProvider"));
        assert!(config.contains("encryption: KeyRing"));
        assert!(env.contains("APP_SECURITY_ENCRYPTION_KEYS_LOCAL="));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn key_generation_print_value_is_a_32_byte_base64_secret() {
        let encoded = generate_key().expect("generate key");
        assert_eq!(STANDARD.decode(encoded).expect("decode key").len(), 32);
    }

    #[test]
    fn generated_relative_cache_file_exposes_default_driver_and_dsn_to_development() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-cache-discovery-{}",
            std::process::id()
        ));
        let app = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&app, "blog", "Blog", None, false, false, false, false)
            .expect("generate project");
        let source = fs::read_to_string(app.join("config/cache.rs"))
            .expect("read generated cache configuration");
        assert!(source.contains("ctx.env_or(\"CACHE_STORE\", \"redis\")"));
        assert!(source.contains("driver: \"redis\""));
        assert!(source.contains("redis://127.0.0.1:6379/"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn development_reuses_a_live_local_redis_without_compose() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-redis-reuse-{}", std::process::id()));
        let app = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(app.join("config")).expect("create app config directory");
        fs::write(
            app.join("config/cache.toml"),
            "default = 'default'\n[stores.default]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n",
        )
        .expect("write app config");
        prepare_development_redis(&root, &app, "APP_").expect("reuse existing Redis");
        let _ = fs::remove_dir_all(root);
    }

    type RedisCase = (
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        &'static str,
        Option<(&'static str, &'static str)>,
    );

    const CASES: [RedisCase; 10] = [
        (
            "nested-memory",
            "default = 'redis'\n[stores.redis]\ndriver = 'redis'\n",
            "cache/stores/redis.toml",
            "driver = 'memory'\n",
            "",
            None,
        ),
        (
            "nested-redis",
            "default = 'redis'\n[stores.redis]\ndriver = 'memory'\n",
            "cache/stores/redis.toml",
            "driver = 'redis'\n",
            "",
            None,
        ),
        (
            "nested-remote",
            "default = 'redis'\n[stores.redis]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n",
            "cache/stores/redis.toml",
            "dsn = 'redis://remote.example.test:6380/'\n",
            "",
            None,
        ),
        (
            "nested-store",
            "default = 'custom'\n[stores.redis]\ndriver = 'memory'\n",
            "cache/stores/custom.toml",
            "driver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n",
            "",
            None,
        ),
        (
            "dotenv-redis",
            "default = 'memory'\n[stores.memory]\ndriver = 'memory'\n[stores.redis]\ndriver = 'redis'\n",
            "cache/stores/redis.toml",
            "dsn = 'redis://127.0.0.1:6379/'\n",
            "CACHE_STORE=redis\n",
            None,
        ),
        (
            "env-memory",
            "default = 'memory'\n[stores.memory]\ndriver = 'memory'\n[stores.redis]\ndriver = 'redis'\n",
            "cache/stores/redis.toml",
            "dsn = 'redis://127.0.0.1:6379/'\n",
            "CACHE_STORE=redis\n",
            Some(("CACHE_STORE", "memory")),
        ),
        (
            "env-driver",
            "default = 'redis'\n[stores.redis]\ndriver = 'memory'\n",
            "cache/stores/redis.toml",
            "driver = 'redis'\n",
            "",
            Some(("APP_CACHE_STORES_REDIS_DRIVER", "memory")),
        ),
        (
            "env-dsn",
            "default = 'redis'\n[stores.redis]\ndriver = 'redis'\n",
            "cache/stores/redis.toml",
            "dsn = 'redis://127.0.0.1:6379/'\n",
            "",
            Some((
                "APP_CACHE_STORES_REDIS_DSN",
                "redis://remote.example.test:6380/",
            )),
        ),
        (
            "missing-config-env-store",
            "",
            "",
            "",
            "CACHE_STORE=redis\nAPP_CACHE_STORES_REDIS_DRIVER=redis\nAPP_CACHE_STORES_REDIS_DSN=redis://127.0.0.1:6379/\n",
            None,
        ),
        (
            "shop-namespace",
            "default = 'custom'\n[stores.custom]\ndriver = 'memory'\n",
            "cache/stores/custom.toml",
            "namespace = 'business'\n",
            "SHOP_CONFIG_DIR=shop-config\nSHOP_CACHE_STORES_CUSTOM_DRIVER=redis\nSHOP_CACHE_STORES_CUSTOM_DSN=redis://127.0.0.1:6379/\n",
            Some(("APP_CACHE_STORES_CUSTOM_DRIVER", "memory")),
        ),
    ];

    #[test]
    fn dev_accepts_explicit_application_config_namespace() -> Result<(), Box<dyn std::error::Error>>
    {
        for (args, expected) in [
            (vec!["openrust", "dev"], "APP_"),
            (
                vec!["openrust", "dev", "--config-namespace", "SHOP_"],
                "SHOP_",
            ),
        ] {
            let cli = Cli::try_parse_from(args).expect("解析开发命令");
            let Some(Builtin::Dev { config_namespace }) = cli.command else {
                return Err("预期开发命令".into());
            };
            assert_eq!(config_namespace, expected);
        }
        Ok(())
    }

    #[test]
    fn development_redis_uses_recursive_config_and_environment_priority()
    -> Result<(), Box<dyn std::error::Error>> {
        if let Some(case) = std::env::var_os("OPENRUST_CLI_REDIS_TEST_CASE") {
            let app = PathBuf::from(
                std::env::var_os("OPENRUST_CLI_REDIS_TEST_APP").expect("子进程应用目录"),
            );
            let namespace = if case == "shop-namespace" {
                "SHOP_"
            } else {
                "APP_"
            };
            let actual =
                development_redis_endpoint(&app, namespace).expect("加载应用的有效缓存配置");
            let expected = match case.to_str().expect("用例标识") {
                "nested-memory" | "nested-remote" | "env-memory" | "env-driver" | "env-dsn" => None,
                "nested-redis"
                | "nested-store"
                | "dotenv-redis"
                | "missing-config-env-store"
                | "shop-namespace" => Some(("127.0.0.1".to_owned(), 6379)),
                other => return Err(format!("未知隔离用例: {other}").into()),
            };
            assert_eq!(actual, expected);
            return Ok(());
        }

        let root = std::env::temp_dir().join(format!(
            "openrust-cli-recursive-redis-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        for (case, root_cache, nested_path, nested, dotenv, process_env) in CASES {
            assert_development_redis_case(
                &root,
                case,
                root_cache,
                nested_path,
                nested,
                dotenv,
                process_env,
            );
        }
        fs::remove_dir_all(root).expect("清理测试配置");
        Ok(())
    }

    #[test]
    fn development_redis_uses_bootstrap_cache_before_live_sources()
    -> Result<(), Box<dyn std::error::Error>> {
        if let Some(case) = std::env::var_os("OPENRUST_CLI_CACHE_TEST_CASE") {
            let app = PathBuf::from(
                std::env::var_os("OPENRUST_CLI_CACHE_TEST_APP").expect("子进程应用目录"),
            );
            let environment_before = std::env::var_os("APP_ENV");
            let cache_path_before = std::env::var_os("APP_CONFIG_CACHE");
            let actual = development_redis_endpoint(&app, "APP_").expect("读取启动缓存");
            assert_eq!(std::env::var_os("APP_ENV"), environment_before);
            assert_eq!(std::env::var_os("APP_CONFIG_CACHE"), cache_path_before);
            let expected = match case.to_str().expect("用例名") {
                "cached-memory-live-redis"
                | "cached-memory-bad-live"
                | "interpolated-production" => None,
                "cached-redis-live-memory" | "dotenv-selected-cache" | "external-cache-wins" => {
                    Some(("127.0.0.1".to_owned(), 6379))
                }
                other => return Err(format!("未知缓存用例: {other}").into()),
            };
            assert_eq!(actual, expected);
            return Ok(());
        }

        let root = std::env::temp_dir().join(format!(
            "openrust-cli-bootstrap-cache-{}",
            std::process::id()
        ));
        let parent_cache_selection = std::env::var_os("APP_CONFIG_CACHE");
        let parent_environment = std::env::var_os("APP_ENV");
        let _ = fs::remove_dir_all(&root);
        for (case, cache_redis, live, dotenv, external_cache) in [
            (
                "cached-memory-live-redis",
                false,
                "default = 'redis'\n[stores.redis]\ndriver = 'redis'\n",
                "",
                None,
            ),
            (
                "cached-redis-live-memory",
                true,
                "default = 'memory'\n[stores.memory]\ndriver = 'memory'\n",
                "",
                None,
            ),
            (
                "cached-memory-bad-live",
                false,
                "invalid = [",
                "invalid dotenv line",
                None,
            ),
            (
                "dotenv-selected-cache",
                true,
                "invalid = [",
                "APP_CONFIG_CACHE=alternate/config.json\n",
                None,
            ),
            (
                "external-cache-wins",
                true,
                "invalid = [",
                "APP_CONFIG_CACHE=wrong/config.json\n",
                Some("external/config.json"),
            ),
            (
                "interpolated-production",
                false,
                "invalid = [",
                "APP_CONFIG_CACHE=${APP_ENV}/config.json\n",
                None,
            ),
        ] {
            assert_bootstrap_cache_case(&root, case, cache_redis, live, dotenv, external_cache);
        }
        assert_eq!(std::env::var_os("APP_CONFIG_CACHE"), parent_cache_selection);
        assert_eq!(std::env::var_os("APP_ENV"), parent_environment);
        fs::remove_dir_all(root).expect("清理配置缓存用例");
        Ok(())
    }

    fn assert_bootstrap_cache_case(
        root: &Path,
        case: &str,
        cache_redis: bool,
        live: &str,
        dotenv: &str,
        external_cache: Option<&str>,
    ) {
        const TEST: &str = "tests::development_redis_uses_bootstrap_cache_before_live_sources";
        let app = root.join(case);
        fs::create_dir_all(app.join("config")).expect("创建配置目录");
        fs::write(app.join("config/cache.toml"), live).expect("写实时配置");
        if !dotenv.is_empty() {
            fs::write(app.join(".env"), dotenv).expect("写环境文件");
        }
        let selected = if let Some(path) = external_cache {
            path
        } else if case == "interpolated-production" {
            "production/config.json"
        } else if case == "dotenv-selected-cache" {
            "alternate/config.json"
        } else {
            "bootstrap/cache/config.json"
        };
        let cache_path = app.join(selected);
        fs::create_dir_all(cache_path.parent().expect("缓存父目录")).expect("创建缓存目录");
        let mut raw = serde_json::to_value(openrust::config::FrameworkConfig::default())
            .expect("序列化框架默认值");
        raw["cache"] = if cache_redis {
            serde_json::json!({"default":"redis","stores":{"redis":{"driver":"redis","dsn":"redis://127.0.0.1:6379/"}}})
        } else {
            serde_json::json!({"default":"memory","stores":{"memory":{"driver":"memory"}}})
        };
        fs::write(&cache_path, serde_json::to_vec(&raw).expect("序列化缓存")).expect("写配置缓存");
        if case == "interpolated-production" {
            raw["cache"] = serde_json::json!({"default":"redis","stores":{"redis":{"driver":"redis","dsn":"redis://127.0.0.1:6379/"}}});
            let alternate = app.join("development/config.json");
            fs::create_dir_all(alternate.parent().expect("另一个缓存父目录"))
                .expect("创建另一个缓存目录");
            fs::write(
                alternate,
                serde_json::to_vec(&raw).expect("序列化另一个缓存"),
            )
            .expect("写另一个缓存");
        }
        let mut child = Process::new(std::env::current_exe().expect("测试可执行文件"));
        child
            .env_clear()
            .env("OPENRUST_CLI_CACHE_TEST_CASE", case)
            .env("OPENRUST_CLI_CACHE_TEST_APP", &app)
            .args(["--exact", TEST]);
        if let Some(path) = external_cache {
            child.env("APP_CONFIG_CACHE", path);
        }
        if case == "interpolated-production" {
            child.env("APP_ENV", "production");
        }
        assert!(
            child.status().expect("运行隔离缓存用例").success(),
            "{case}"
        );
    }

    fn assert_development_redis_case(
        root: &Path,
        case: &str,
        root_cache: &str,
        nested_path: &str,
        nested: &str,
        dotenv: &str,
        process_env: Option<(&str, &str)>,
    ) {
        let app = root.join(case);
        let config_dir = if case == "shop-namespace" {
            "shop-config"
        } else {
            "config"
        };
        fs::create_dir_all(&app).expect("创建应用目录");
        if !root_cache.is_empty() {
            let nested_file = app.join(config_dir).join(nested_path);
            fs::create_dir_all(nested_file.parent().expect("配置父目录"))
                .expect("创建递归配置目录");
            fs::write(app.join(config_dir).join("cache.toml"), root_cache).expect("写根缓存配置");
            fs::write(nested_file, nested).expect("写子缓存配置");
        }
        if case == "shop-namespace" {
            fs::create_dir_all(app.join("config")).expect("创建非目标配置目录");
            fs::write(
                app.join("config/cache.toml"),
                "default = 'redis'\n[stores.redis]\ndriver = 'redis'\ndsn = 'redis://remote.example.test:6380/'\n",
            )
            .expect("写非目标配置");
        }
        if !dotenv.is_empty() {
            fs::write(app.join(".env"), dotenv).expect("写环境配置");
        }
        let mut child = Process::new(std::env::current_exe().expect("测试可执行文件"));
        child
            .env_clear()
            .env("OPENRUST_CLI_REDIS_TEST_CASE", case)
            .env("OPENRUST_CLI_REDIS_TEST_APP", &app)
            .args([
                "--exact",
                "tests::development_redis_uses_recursive_config_and_environment_priority",
            ]);
        if let Some((key, value)) = process_env {
            child.env(key, value);
        }
        assert!(
            child.status().expect("运行隔离配置用例").success(),
            "{case}"
        );
    }

    fn assert_module_layout(project: &Path) {
        let app_test = fs::read_to_string(project.join("tests/test_app.rs"))
            .expect("read generated TestApp test");
        assert!(app_test.contains("use bootstrap::Blog as GeneratedApplication;"));
        assert!(app_test.contains("TestApp::<GeneratedApplication>"));
        assert!(app_test.contains(r#".uri("/health")"#));
        assert!(app_test.contains("app.shutdown("));
        assert!(!app_test.contains("OpenRustGeneratedApplication"));
        assert!(app_test.contains("../modules/mod.rs"));
        let module_manifest = fs::read_to_string(project.join("modules/mod.rs"))
            .expect("read generated module manifest");
        assert!(module_manifest.contains("config: &openrust::config::ModulesConfig"));
        assert!(module_manifest.contains("ModuleManifest::discover_from_config(root, config)"));
        assert!(!module_manifest.contains("CARGO_MANIFEST_DIR"));
        assert!(!module_manifest.contains("root.join(\"modules_statuses.json\")"));
        assert!(!module_manifest.contains(".enable("));
        assert!(module_manifest.contains("config.activator == \"registered\""));
        assert!(module_manifest.contains("ModuleManifest::new().with_application_root(root)"));
        assert!(!project.join("modules_statuses.json").exists());
        assert!(!project.join("modules/example").exists());
        let application = fs::read_to_string(project.join("bootstrap/app.rs"))
            .expect("read generated application");
        assert!(application.contains("fn modules("));
        assert!(application.contains("crate::modules::manifest("));
        assert!(application.contains("config"));
        assert!(application.contains(".application_root"));
        assert!(application.contains("&config.modules"));
        assert!(application.contains(".as_deref()"));
        let main = fs::read_to_string(project.join("src/main.rs"))
            .expect("read generated application entry");
        assert!(main.contains("../modules/mod.rs"));
        assert!(!project.join("config/default.toml").exists());
        assert!(!project.join("config/development.toml").exists());
    }

    fn assert_generated_application_files(project: &Path) {
        for path in [
            "Cargo.toml",
            ".gitignore",
            "public/index.html",
            "resources/views/index.html",
            "src/main.rs",
            "bootstrap/app.rs",
            "bootstrap/providers.rs",
            "app/mod.rs",
            "app/http/controllers/home.rs",
            "app/providers/mod.rs",
            "modules/mod.rs",
            "routes/mod.rs",
            "routes/api.rs",
            "routes/web.rs",
            "config/mod.rs",
            "config/app.rs",
            "config/cache.rs",
            "config/server.rs",
            "config/session.rs",
            "config/i18n.rs",
            "config/api.rs",
            "config/auth.rs",
            "config/database.rs",
            "config/log.rs",
            "config/mail.rs",
            "config/modules.rs",
            "config/queue.rs",
            "config/realtime.rs",
            "config/security.rs",
            "config/sms.rs",
            "config/storage.rs",
            "config/view.rs",
            "tests/test_app.rs",
            "tests/support/request_input.rs",
            "tests/support/message_bags.rs",
            "tests/support/request_cookie.rs",
            "tests/support/request_session.rs",
            "tests/support/request_redirect.rs",
            "AGENTS.md",
        ] {
            assert!(project.join(path).is_file(), "missing generated {path}");
        }
    }

    fn assert_generated_route_layout(project: &Path) {
        let entry = fs::read_to_string(project.join("routes/mod.rs")).expect("read route module");
        assert!(entry.contains("pub mod api;"));
        assert!(entry.contains("pub mod web;"));
        assert!(!entry.contains("fn register("));
        assert!(!entry.contains(".get("));
        assert!(!entry.contains(".fallback("));
        let api = fs::read_to_string(project.join("routes/api.rs")).expect("read API routes");
        assert!(!api.contains(".prefix("));
        assert!(!api.contains(".middleware_group("));
        let bootstrap =
            fs::read_to_string(project.join("bootstrap/app.rs")).expect("read routing bootstrap");
        assert!(bootstrap.contains("pub(crate) fn routes(routes: Routes, prefix: &str) -> Routes"));
        assert!(bootstrap.contains(".prefix(prefix).middleware_group(\"api\")"));
        assert!(bootstrap.contains("crate::routes::api::register"));
        assert!(bootstrap.contains("crate::routes::web::register(routes)"));
        assert!(api.contains(".get(\"/\", root)"));
        assert!(!api.contains(".get(\"/api"));
        assert!(!api.contains(".post(\"/api"));
        assert!(!api.contains(".fallback("));
        assert!(!api.contains("middleware_group(\"web\")"));
        assert!(!api.contains("middleware_alias(\"session\")"));
        let web = fs::read_to_string(project.join("routes/web.rs")).expect("read Web routes");
        assert!(web.contains(".fallback(not_found)"));
        assert!(!web.contains("AuthLayer"));
        assert!(!web.contains("middleware_group(\"api\")"));
        let provider = fs::read_to_string(project.join("app/providers/app.rs"))
            .expect("read application Provider");
        assert!(provider.contains("app.section::<openrust::config::ApiConfig>(\"api\")?"));
        assert!(provider.contains("api.normalized_prefix()?"));
        assert!(provider.contains("api.matches_path(request.path())"));
        assert!(
            provider
                .contains("app.routes(move |routes| crate::bootstrap::routes(routes, &prefix))?;")
        );
        for path in ["src/main.rs", "tests/test_app.rs", "tests/auth_flows.rs"] {
            if project.join(path).is_file() {
                let source = fs::read_to_string(project.join(path)).expect("read route caller");
                assert!(
                    source.contains("#[path = \"../routes/mod.rs\"]\nmod routes;"),
                    "{path}"
                );
                assert!(!source.contains("../routes/web.rs"), "{path}");
            }
        }
    }

    fn assert_generated_configuration(project: &Path) {
        let config_manifest =
            fs::read_to_string(project.join("config/mod.rs")).expect("read configuration manifest");
        let server = config_manifest
            .find(".section(\"server\"")
            .expect("server declaration");
        let app = config_manifest
            .find(".section(\"app\"")
            .expect("app declaration");
        let auth = config_manifest
            .find(".section(\"auth\"")
            .expect("auth declaration");
        let security = config_manifest
            .find(".section(\"security\"")
            .expect("security declaration");
        assert!(server < app && app < auth && app < security);
        assert!(!config_manifest.contains("pub mod queue;"));
        assert!(!config_manifest.contains("pub mod realtime;"));
        assert!(config_manifest.contains("if view::ENABLED"));
        let view = fs::read_to_string(project.join("config/view.rs")).expect("read view defaults");
        assert!(view.contains("ENABLED: bool = true"));
        assert!(config_manifest.contains(".section(\"modules\", modules::config)"));
        let modules = fs::read_to_string(project.join("config/modules.rs"))
            .expect("read independent module activation configuration");
        assert!(modules.contains("ctx.env_or(\"MODULES_ACTIVATOR\", \"registered\")"));
        let api =
            fs::read_to_string(project.join("config/api.rs")).expect("read API configuration");
        assert!(api.contains("prefix: ctx.env_or(\"API_PREFIX\", \"/api\")"));
        assert!(modules.contains("MODULES_STATUSES_FILE"));
        assert!(modules.contains("modules_statuses.json"));
    }

    fn assert_framework_migration_registry(source: &str) {
        assert_eq!(
            source
                .matches("openrust::db::FrameworkMigrator::migrations()")
                .count(),
            1
        );
        assert_eq!(
            source
                .matches("let application_migrations: Vec<Box<dyn MigrationTrait>> = vec![")
                .count(),
            1
        );
        assert_eq!(
            source
                .matches("migrations.extend(application_migrations);")
                .count(),
            1
        );
    }

    #[test]
    fn new_defaults_to_mysql_and_minimal_is_an_explicit_conflicting_choice() -> anyhow::Result<()> {
        for (arguments, expected) in [
            (
                vec!["openrust", "new", "blog"],
                Some(DatabaseBackend::Mysql),
            ),
            (
                vec!["openrust", "new", "blog", "--auth"],
                Some(DatabaseBackend::Mysql),
            ),
            (
                vec![
                    "openrust",
                    "new",
                    "blog",
                    "--auth",
                    "--auth-mfa",
                    "--encryption",
                ],
                Some(DatabaseBackend::Mysql),
            ),
            (
                vec!["openrust", "new", "blog", "--db", "postgres"],
                Some(DatabaseBackend::Postgres),
            ),
            (vec!["openrust", "new", "blog", "--minimal"], None),
        ] {
            let cli = Cli::try_parse_from(arguments)?;
            let Some(Builtin::New { db, minimal, .. }) = cli.command else {
                bail!("预期 new 命令");
            };
            assert_eq!(project_database(db, minimal), expected);
        }
        for arguments in [
            vec!["openrust", "new", "blog", "--minimal", "--db", "mysql"],
            vec!["openrust", "new", "blog", "--minimal", "--auth"],
            vec![
                "openrust",
                "new",
                "blog",
                "--minimal",
                "--auth-mfa",
                "--encryption",
            ],
        ] {
            assert!(Cli::try_parse_from(arguments).is_err());
        }
        Ok(())
    }

    #[test]
    fn standard_project_preconfigures_mysql_and_both_queue_drivers() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-standard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        let project = root.join("blog");
        populate_project(
            &project,
            "blog",
            "Blog",
            project_database(None, false),
            false,
            false,
            false,
            false,
        )
        .expect("生成标准应用");
        assert_generated_application_files(&project);
        assert_module_layout(&project);
        assert_generated_route_layout(&project);
        let manifest = fs::read_to_string(project.join("Cargo.toml")).expect("应用依赖");
        for feature in ["db-mysql", "cache-redis", "queue", "queue-redis", "session"] {
            assert!(
                manifest.contains(&format!("\"{feature}\"")),
                "缺少 {feature}"
            );
        }
        assert!(!manifest.contains("db-postgres"));
        let migration_manifest =
            fs::read_to_string(project.join("database/migrations/Cargo.toml")).expect("迁移依赖");
        for feature in ["db-mysql", "session", "queue"] {
            assert!(
                migration_manifest.contains(&format!("\"{feature}\"")),
                "迁移缺少 {feature}"
            );
        }
        let registry_path = project.join("database/migrations/src/lib.rs");
        let registry = fs::read_to_string(&registry_path).expect("迁移清单");
        assert_framework_migration_registry(&registry);
        let config = fs::read_to_string(project.join("config/mod.rs")).expect("配置清单");
        assert_eq!(config.matches("pub mod queue;").count(), 1);
        assert_eq!(
            config.matches(".section(\"queue\", queue::config)").count(),
            1
        );
        let queue = fs::read_to_string(project.join("config/queue.rs")).expect("队列配置");
        assert!(queue.contains("ctx.env_or(\"QUEUE_CONNECTION\", \"database\")"));
        assert!(queue.contains("cache.stores.redis.dsn"));
        assert!(!queue.contains("NAMED_CONNECTIONS_ENABLED"));
        assert!(!queue.contains("QueueDriverKind"));
        let env = fs::read_to_string(project.join(".env.example")).expect("环境配置");
        assert!(env.contains("DB_CONNECTION=mysql"));
        assert!(env.contains("DB_PORT=3306"));
        assert!(env.contains("QUEUE_CONNECTION=database"));
        assert!(env.contains("QUEUE_REDIS_NAMESPACE"));
        assert!(!env.contains("QUEUE_DRIVER"));
        generate_job(&["delivery".to_owned()], &project).expect("标准应用生成任务无需手改依赖");
        generate_job(&["retry_delivery".to_owned()], &project).expect("重复登记共用单个 Provider");
        assert_eq!(
            fs::read_to_string(registry_path).expect("生成后迁移"),
            registry
        );
        let providers =
            fs::read_to_string(project.join("bootstrap/providers.rs")).expect("Provider");
        assert_eq!(
            providers
                .matches("crate::jobs::JobsServiceProvider")
                .count(),
            1
        );
        let test = fs::read_to_string(project.join("tests/test_app.rs")).expect("普通 HTTP 回归");
        assert!(test.contains("#[path = \"../database/seeders.rs\"]\nmod seed;"));
        assert_eq!(test.matches("mod jobs;").count(), 1);
        assert!(!test.contains("{{seed_module}}"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn project_template_has_expected_layout() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("generate project");
        assert_generated_application_files(&project);
        assert_module_layout(&project);
        assert_generated_configuration(&project);
        assert_generated_route_layout(&project);
        let routes = fs::read_to_string(project.join("routes/api.rs")).expect("read API routes");
        assert!(routes.contains("crate::app::http::controllers::home::index"));
        let application = fs::read_to_string(project.join("bootstrap/app.rs"))
            .expect("read application bootstrap");
        assert!(application.contains("providers::application_providers(config)"));
        assert!(project.join("app/providers/app.rs").is_file());
        let registry = fs::read_to_string(project.join("bootstrap/providers.rs"))
            .expect("read application Provider list");
        assert!(registry.contains("AppServiceProvider"));
        assert!(registry.contains("openrust::db::FrameworkMigrator"));
        assert_eq!(registry.matches("MigrationServiceProvider").count(), 1);
        assert!(!registry.contains("SeedServiceProvider"));
        assert!(!project.join("database/migrations").exists());
        let snapshot = [
            "Cargo.toml",
            ".gitignore",
            "public/index.html",
            "resources/views/index.html",
            "config/mod.rs",
            "config/app.rs",
            "config/cache.rs",
            "config/server.rs",
            "config/session.rs",
            "config/i18n.rs",
            "config/modules.rs",
            "src/main.rs",
            "bootstrap/app.rs",
            "bootstrap/providers.rs",
            "modules/mod.rs",
            "routes/mod.rs",
            "routes/api.rs",
            "routes/web.rs",
            "tests/test_app.rs",
            "tests/support/request_input.rs",
            "tests/support/message_bags.rs",
            "tests/support/request_cookie.rs",
            "tests/support/request_session.rs",
            "tests/support/request_redirect.rs",
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
        assert!(
            gitignore
                .lines()
                .any(|line| line == "**/.modules_statuses.json.lock")
        );
        assert!(gitignore.lines().any(|line| line == ".env"));
        assert!(gitignore.lines().any(|line| line == ".env.*"));
        assert!(gitignore.lines().any(|line| line == "/storage/"));
        assert!(
            gitignore
                .lines()
                .any(|line| line == "/bootstrap/cache/*.json")
        );
        assert!(gitignore.lines().any(|line| line == "!.env.example"));
        assert!(gitignore.lines().any(|line| line == "!.env.*.example"));
        let env_example = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(env_example.contains("APP_NAME=blog"));
        assert!(
            env_example.contains("SERVER_PORT=3000\nAPP_URL=\"http://localhost:${SERVER_PORT}\"")
        );
        assert!(env_example.contains("# APP_KEY=<随机密钥>"));
        assert!(env_example.contains("# APP_PREVIOUS_KEYS=old-one,old-two"));
        assert!(env_example.contains("REDIS_HOST=127.0.0.1"));
        assert!(env_example.contains("CACHE_STORE=redis"));
        assert!(!env_example.contains("REDIS_DEFAULT_STORE"));
        assert!(!env_example.contains("CACHE_DEFAULT_STORE"));
        assert!(!env_example.contains("APP_PORT="));
        assert!(env_example.contains("# DB_CONNECTION=postgres"));
        assert!(env_example.contains("SERVER_PORT=3000"));
        assert!(env_example.contains("APP_SERVER_PUBLIC_DIR=public"));
        assert!(env_example.contains("APP_SERVER_ROOT_TARGET=file:/index.html"));
        assert!(env_example.contains("MAIL_SMTP_PASSWORD="));
        assert!(env_example.contains("SMS_API_KEY_SECRET="));
        assert!(env_example.contains("--minimal 不编译数据库驱动和队列"));
        assert!(!env_example.contains("QUEUE_DRIVER"));
        assert!(env_example.contains("生产环境可使用权限受控的 .env"));
        assert!(!env_example.contains("Production reads configuration"));
        let guide = fs::read_to_string(project.join("AGENTS.md")).expect("read generated guide");
        assert!(guide.contains("应用开发指南"));
        assert!(!guide.contains("Application guide"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn application_generators_register_types_without_overwriting_existing_sources() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-application-generators-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("generate project");
        generate_controller("PostController", &project).expect("generate controller");
        generate_provider("CatalogServiceProvider", &project).expect("generate provider");
        assert!(project.join("app/http/controllers/post.rs").is_file());
        assert!(project.join("app/providers/catalog.rs").is_file());
        let providers = fs::read_to_string(project.join("bootstrap/providers.rs"))
            .expect("read provider registry");
        assert!(providers.contains("crate::app::providers::catalog::CatalogServiceProvider"));
        assert!(generate_controller("PostController", &project).is_err());
        assert!(generate_provider("CatalogServiceProvider", &project).is_err());
        assert!(Cli::try_parse_from(["openrust", "make:controller", "PostController"]).is_ok());
        assert!(
            Cli::try_parse_from(["openrust", "make:provider", "CatalogServiceProvider"]).is_ok()
        );
        assert!(Cli::try_parse_from(["openrust", "make:model", "Post"]).is_ok());
        fs::remove_dir_all(root).expect("remove temporary test directory");
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
            assert!(project.join("database/migrations/Cargo.toml").is_file());
            assert!(project.join("database/migrations/src/lib.rs").is_file());
            assert!(
                fs::read_to_string(project.join("config/database.rs"))
                    .expect("read database domain config")
                    .contains("DatabaseConfig {")
            );
            assert!(
                fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                    .expect("read empty migration registry")
                    .contains("vec![]")
            );
            let registry = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                .expect("读取统一迁移清单");
            assert_framework_migration_registry(&registry);
            let catalog = fs::read_to_string(project.join("tests/migration_catalog.rs"))
                .expect("读取可执行迁移目录回归");
            assert!(catalog.contains("QueueMigration.name()"));
            assert!(catalog.contains("SessionGcMigration.name()"));
            assert!(catalog.contains("SessionMigration.name()"));
            assert!(!catalog.contains("{{"));
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.contains("providers::application_providers(config)"));
            assert!(!app.contains("fn default_providers("));
            let defaults = include_str!("../../../src/core/default_providers.rs");
            assert!(defaults.contains("DatabaseServiceProvider"));
            let providers = fs::read_to_string(project.join("bootstrap/providers.rs"))
                .expect("read generated provider registry");
            assert!(providers.contains("openrust_migration::Migrator"));
            assert!(providers.contains("SeedServiceProvider"));
            assert!(!providers.contains("openrust::db::FrameworkMigrator"));
            assert_eq!(providers.matches("MigrationServiceProvider").count(), 1);
            assert!(project.join(".env.example").is_file());
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated database environment example");
            assert!(env.contains(&format!("DB_CONNECTION={}", backend.connection_name())));
            assert!(env.contains(&format!("DB_PORT={}", backend.port())));
            assert!(env.contains("DB_DATABASE=blog"));
            assert!(env.contains("DB_TABLE_PREFIX="));
            assert!(env.contains("REDIS_HOST=127.0.0.1"));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "完整认证模板需同时核对路由、迁移、配置与公开依赖"
    )]
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
            assert_generated_route_layout(&project);
            let manifest = fs::read_to_string(project.join("Cargo.toml"))
                .expect("read generated app manifest");
            assert!(manifest.contains(feature));
            assert!(manifest.contains("testing"));
            assert!(manifest.contains("figment"));
            assert!(manifest.contains("serde_json"));
            let http_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
                .expect("read generated auth HTTP integration test");
            assert!(http_test.contains("ProviderBuilder::for_application::<"));
            assert!(http_test.contains("use bootstrap::Blog as GeneratedApplication;"));
            assert!(http_test.contains("for_application::<GeneratedApplication>"));
            assert!(http_test.contains("../modules/mod.rs"));
            assert!(project.join("tests/support/request_input.rs").is_file());
            assert!(
                http_test.contains("request_input::assert_request_input::<GeneratedApplication>")
            );
            assert!(!project.join("modules_statuses.json").exists());
            assert!(http_test.contains("into_make_service_with_connect_info"));
            assert!(http_test.contains("/api/auth/password-reset/confirm"));
            assert!(!http_test.contains("{{type_name}}"));
            assert!(manifest.contains("\"auth\""));
            let migration_manifest =
                fs::read_to_string(project.join("database/migrations/Cargo.toml"))
                    .expect("read generated migration manifest");
            assert!(migration_manifest.contains("\"auth\""));
            let migration = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                .expect("read generated migration registry");
            assert_framework_migration_registry(&migration);
            let framework = include_str!("../../../src/db/migration.rs");
            for name in [
                "AuthMigration",
                "AuthRefreshTokenMigration",
                "AuthVerificationCodeMigration",
                "AuthPolicyMigration",
                "DatabaseRateLimitMigration",
                "DatabaseAuditMigration",
                "DatabaseAuditTenantMigration",
                "SignedLinkRevocationMigration",
                "SessionGcMigration",
                "QueueMigration",
            ] {
                assert!(framework.contains(name), "框架清单缺少 {name}");
            }
            for feature in ["auth", "session", "queue", backend.feature()] {
                assert!(
                    migration_manifest.contains(&format!("\"{feature}\"")),
                    "迁移包缺少 {feature}"
                );
                assert!(
                    manifest.contains(&format!("\"{feature}\"")),
                    "应用缺少 {feature}"
                );
            }
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.contains("providers::application_providers(config)"));
            assert!(!app.contains("fn default_providers("));
            let defaults = include_str!("../../../src/core/default_providers.rs");
            assert!(defaults.contains("AuthServiceProvider"));
            assert!(defaults.contains("JwtServiceProvider"));
            assert!(defaults.contains("AuthFlowsServiceProvider"));
            assert!(defaults.contains("AuditServiceProvider::database()"));
            assert!(defaults.contains("SignedLinksServiceProvider"));
            let config = fs::read_to_string(project.join("config/security.rs"))
                .expect("read generated signing config");
            assert!(config.contains("signed_links: SignedLinks"));
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated signing environment template");
            assert!(env.contains("APP_SECURITY_SIGNED_LINKS_KEYS_LOCAL="));
            assert!(!app.contains("SessionPlugin::new()"));
            let routes = fs::read_to_string(project.join("routes/api.rs"))
                .expect("read generated API routes");
            assert!(routes.contains("AuthLayer::required("));
            assert!(routes.contains("AuthMode::Jwt"));
            assert!(routes.contains("/me/pat"));
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
            let config = fs::read_to_string(project.join("config/auth.rs"))
                .expect("read generated auth config");
            assert!(config.contains("jwt: Jwt"));
            assert!(config.contains("BTreeMap::new()"));
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated environment example");
            assert!(env.contains("APP_AUTH_JWT_KEYS_LOCAL="));
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
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--auth"]).is_ok());
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--minimal", "--auth"]).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mfa_generator_registers_secure_routes_and_dual_backend_migrations() {
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--auth-mfa"]).is_err());
        assert!(
            Cli::try_parse_from([
                "openrust",
                "new",
                "blog",
                "--db",
                "postgres",
                "--auth",
                "--auth-mfa"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "openrust",
                "new",
                "blog",
                "--db",
                "postgres",
                "--auth",
                "--auth-mfa",
                "--encryption"
            ])
            .is_ok()
        );

        let root = std::env::temp_dir().join(format!("openrust-cli-mfa-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        for (backend, feature) in [
            (DatabaseBackend::Postgres, "db-postgres"),
            (DatabaseBackend::Mysql, "db-mysql"),
        ] {
            let project = root.join(feature);
            populate_project_with_mfa(
                &project,
                "blog",
                "Blog",
                Some(backend),
                true,
                true,
                false,
                false,
                true,
                false,
            )
            .expect("generate MFA project");
            assert_generated_route_layout(&project);
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.contains("providers::application_providers(config)"));
            assert!(!app.contains("MfaServiceProvider"));
            let migration = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                .expect("migration");
            assert_framework_migration_registry(&migration);
            assert!(include_str!("../../../src/db/migration.rs").contains("AuthMfaMigration"));
            for path in ["Cargo.toml", "database/migrations/Cargo.toml"] {
                assert!(
                    fs::read_to_string(project.join(path))
                        .expect("MFA dependency")
                        .contains("\"auth-mfa\"")
                );
            }
            assert!(
                fs::read_to_string(project.join("routes/api.rs"))
                    .expect("API routes")
                    .contains("/auth/mfa/login")
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn minimal_projects_generate_both_model_styles_without_driver_features() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-model-no-driver-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        let project = root.join("blog");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("生成精简应用");
        let manifest = fs::read(project.join("Cargo.toml")).expect("读取应用清单");
        generate_application_model(&["Article", "title:string"].map(str::to_owned), &project)
            .expect("普通应用生成 make:model");
        let model = ["post", "title:string", "metadata:json"].map(str::to_owned);
        generate_model(&model, false, false, &project).expect("普通应用生成旧式实体");
        for relative in ["app/models/article.rs", "app/models/_entities/post.rs"] {
            let source = fs::read_to_string(project.join(relative)).expect("读取实体");
            assert!(source.contains("use openrust::database::sea_orm_migration::sea_orm;"));
            assert!(source.contains("DeriveEntityModel"));
            assert!(source.contains("pub title: String"));
        }
        assert!(
            fs::read_to_string(project.join("app/mod.rs"))
                .expect("读取应用模块")
                .contains("pub mod models;")
        );
        assert!(
            fs::read_to_string(project.join("app/models/_entities/mod.rs"))
                .expect("读取实体模块")
                .contains("pub mod post;")
        );
        assert_eq!(
            fs::read(project.join("Cargo.toml")).expect("读取清单"),
            manifest
        );
        assert!(!project.join("database/migrations").exists());
        let paths = [
            "Cargo.toml",
            "app/mod.rs",
            "app/models/mod.rs",
            "app/models/article.rs",
            "app/models/_entities/mod.rs",
            "app/models/_entities/post.rs",
            "src/main.rs",
        ];
        let before = paths.map(|path| fs::read(project.join(path)).expect("保存生成内容"));
        assert!(generate_application_model(&["Article".to_owned()], &project).is_err());
        assert!(generate_model(&model, false, false, &project).is_err());
        for (path, expected) in paths.into_iter().zip(before) {
            assert_eq!(
                fs::read(project.join(path)).expect("读取冲突后内容"),
                expected
            );
        }
        let missing = root.join("not-an-application");
        assert!(generate_application_model(&["Article".to_owned()], &missing).is_err());
        assert!(generate_model(&model, false, false, &missing).is_err());
        assert!(!missing.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uuid_model_without_migration_package_enables_only_application_extension() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-uuid-no-driver-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        let project = root.join("blog");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("生成精简应用");
        generate_model(
            &["entry", "title:string"].map(str::to_owned),
            true,
            true,
            &project,
        )
        .expect("无迁移包生成 UUID 实体");
        let source = fs::read_to_string(project.join("app/models/_entities/entry.rs"))
            .expect("读取 UUID 实体");
        assert!(source.contains("pub id: Uuid"));
        assert!(source.contains("Uuid::now_v7()"));
        assert!(source.contains("pub deleted_at: Option<DateTimeUtc>"));
        let manifest = fs::read_to_string(project.join("Cargo.toml")).expect("读取应用清单");
        assert!(manifest.contains("\"db-uuid\""));
        assert!(!manifest.contains("db-mysql"));
        assert!(!manifest.contains("db-postgres"));
        assert!(!project.join("database/migrations").exists());
        fs::create_dir_all(project.join("database/migrations/Cargo.toml"))
            .expect("用目录模拟已存在但不可读取的迁移清单");
        assert!(enable_uuid_feature(&project).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn uuid_generators_preflight_both_manifests_before_mutating_any_files() {
        fn file_bytes(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
            let mut result = std::collections::BTreeMap::new();
            let mut directories = vec![root.to_path_buf()];
            while let Some(directory) = directories.pop() {
                for entry in fs::read_dir(directory).expect("读取 fixture 目录") {
                    let entry = entry.expect("读取 fixture 条目");
                    if entry.file_type().expect("读取条目类型").is_dir() {
                        directories.push(entry.path());
                    } else {
                        result.insert(
                            entry.path(),
                            fs::read(entry.path()).expect("保存原文件字节"),
                        );
                    }
                }
            }
            result
        }

        let root = std::env::temp_dir().join(format!(
            "openrust-cli-uuid-preflight-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        for (case, invalid) in [
            ("syntax", Some("[dependencies")),
            ("directory", None),
            ("missing", None),
            (
                "features",
                Some("[dependencies]\nopenrust = { workspace = true, features = 17 }\n"),
            ),
        ] {
            let project = root.join(case);
            populate_project(
                &project,
                "blog",
                "Blog",
                Some(DatabaseBackend::Mysql),
                false,
                false,
                false,
                false,
            )
            .expect("生成带迁移 package 的应用");
            let manifest = project.join("database/migrations/Cargo.toml");
            if let Some(source) = invalid {
                fs::write(&manifest, source).expect("模拟损坏迁移清单");
            } else {
                fs::remove_file(&manifest).expect("移除自有 fixture 迁移清单");
                if case == "directory" {
                    fs::create_dir(&manifest).expect("用目录模拟不可读的迁移清单");
                }
            }
            assert!(project.join("database/migrations").is_dir());
            assert!(
                !fs::read_to_string(project.join("Cargo.toml"))
                    .expect("读取应用清单")
                    .contains("db-uuid")
            );
            let before = file_bytes(&project);
            let model = ["entry", "title:string"].map(str::to_owned);
            assert!(
                generate_model(&model, true, false, &project).is_err(),
                "{case}"
            );
            assert!(!project.join("app/models/_entities/entry.rs").exists());
            assert_eq!(file_bytes(&project), before, "model / {case}");
            assert!(
                generate_migration(&model, true, false, &project).is_err(),
                "{case}"
            );
            assert_eq!(file_bytes(&project), before, "migration / {case}");
            assert!(enable_uuid_feature(&project).is_err(), "{case}");
            assert_eq!(file_bytes(&project), before, "feature / {case}");
        }
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
        generate_application_model(&["Article".to_owned()], &project)
            .expect("generate Laravel-style application model without fields");
        let application_model = fs::read_to_string(project.join("app/models/article.rs"))
            .expect("read application model");
        assert!(application_model.contains("table_name = \"articles\""));
        assert!(generate_application_model(&["Article".to_owned()], &project).is_err());
        let entity = fs::read_to_string(project.join("app/models/_entities/post.rs"))
            .expect("read generated model");
        assert!(entity.contains("pub title: String"));
        assert!(entity.contains("为 `posts` 表生成的 SeaORM 实体"));
        assert!(entity.contains("pub published_at: Option<DateTimeUtc>"));
        assert!(entity.contains("pub author_id: i64"));

        generate_model(
            &["entry", "title:string"].map(str::to_owned),
            true,
            true,
            &project,
        )
        .expect("generate UUID soft-delete model");
        let entry = fs::read_to_string(project.join("app/models/_entities/entry.rs"))
            .expect("read generated UUID model");
        assert!(entry.contains("pub id: Uuid"));
        assert!(entry.contains("Uuid::now_v7()"));
        assert!(entry.contains("pub deleted_at: Option<DateTimeUtc>"));
        assert!(entry.contains("pub fn not_deleted()"));
        let app_manifest = fs::read_to_string(project.join("Cargo.toml")).expect("app manifest");
        let migration_manifest = fs::read_to_string(project.join("database/migrations/Cargo.toml"))
            .expect("migration manifest");
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
        let migration = fs::read_dir(project.join("database/migrations/src"))
            .expect("read generated migrations")
            .filter_map(std::result::Result::ok)
            .map(|entry| fs::read_to_string(entry.path()).unwrap_or_default())
            .find(|source| source.contains("uuid().not_null().primary_key()"))
            .unwrap_or_default();
        assert!(migration.contains(".uuid().not_null().primary_key()"));
        assert!(migration.contains("创建 `entries` 表"));
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
        let registry = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
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
            project.join("app/mail/mod.rs"),
            "//! 应用邮件类型。\n/// 欢迎邮件说明。\npub mod welcome_email;\n",
        )
        .expect("add existing module documentation");
        generate_mailer(&["password_reset".to_owned()], &project).expect("generate second mailer");
        generate_policy(&["post".to_owned()], &project).expect("generate policy");
        generate_policy(&["comment".to_owned()], &project).expect("generate second policy");

        let mailer = fs::read_to_string(project.join("app/mail/welcome_email.rs"))
            .expect("read generated mailer");
        assert!(mailer.contains("impl Email for WelcomeEmail"));
        assert!(mailer.contains("OpenRust 生成的应用邮件类型"));
        assert!(!mailer.contains("Application email type generated"));
        assert!(mailer.contains("EmailTemplateCatalog"));
        assert!(mailer.contains("mailers.welcome_email.subject"));
        assert!(
            mailer.contains("include_str!(\"../../resources/mail/welcome_email/zh-CN/html.html\")")
        );
        let template =
            fs::read_to_string(project.join("resources/mail/welcome_email/zh-CN/html.html"))
                .expect("read generated HTML template");
        assert!(template.contains("您好，{{ user }}"));
        let policy = fs::read_to_string(project.join("app/policies/post.rs"))
            .expect("read generated policy");
        assert!(policy.contains("pub struct PostPolicy"));
        assert!(policy.contains("OpenRust 生成的应用授权策略"));
        assert!(policy.contains("\"posts:read\""));
        assert!(policy.contains("authorize_create"));
        assert!(policy.contains("authorize_update"));
        assert!(policy.contains("authorize_delete"));
        let mailer_modules = fs::read_to_string(project.join("app/mail/mod.rs"))
            .expect("read mailer module registry");
        assert!(mailer_modules.contains("pub mod welcome_email;"));
        assert!(mailer_modules.contains("/// 欢迎邮件说明。\npub mod welcome_email;"));
        assert!(
            mailer_modules.find("pub mod password_reset;")
                < mailer_modules.find("pub mod welcome_email;")
        );
        let policy_modules = fs::read_to_string(project.join("app/policies/mod.rs"))
            .expect("read policy module registry");
        assert!(policy_modules.contains("pub mod post;"));
        assert!(policy_modules.find("pub mod comment;") < policy_modules.find("pub mod post;"));
        let main = fs::read_to_string(project.join("src/main.rs")).expect("read app entry");
        assert_eq!(
            main.matches("#[path = \"../app/mail/mod.rs\"]\npub mod mailers;")
                .count(),
            1
        );
        assert_eq!(
            main.matches("#[path = \"../app/policies/mod.rs\"]\npub mod policies;")
                .count(),
            1
        );
        let manifest =
            fs::read_to_string(project.join("Cargo.toml")).expect("read generated app manifest");
        assert!(manifest.contains("mail-templates"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn job_generation_accepts_inherited_or_custom_driver_with_migration_structure() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-job-no-driver-text-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        for (name, custom_driver) in [("inherited", false), ("custom", true)] {
            let project = root.join(name);
            populate_project(
                &project,
                name,
                "Blog",
                Some(DatabaseBackend::Mysql),
                false,
                false,
                false,
                false,
            )
            .expect("生成带迁移结构的应用");
            for relative in ["Cargo.toml", "database/migrations/Cargo.toml"] {
                let path = project.join(relative);
                let mut manifest = fs::read_to_string(&path)
                    .expect("读取 fixture 清单")
                    .parse::<toml_edit::DocumentMut>()
                    .expect("解析 fixture 清单");
                let dependency = manifest["dependencies"]["openrust"]
                    .as_value_mut()
                    .and_then(toml_edit::Value::as_inline_table_mut)
                    .expect("工作区依赖");
                assert_eq!(
                    dependency
                        .get("workspace")
                        .and_then(toml_edit::Value::as_bool),
                    Some(true)
                );
                dependency.remove("features");
                if custom_driver {
                    let mut driver = toml_edit::InlineTable::new();
                    driver.insert("version", "2.0.4".into());
                    driver.insert("default-features", false.into());
                    let mut features = toml_edit::Array::new();
                    features.push("sqlx-sqlite");
                    driver.insert("features", toml_edit::Value::Array(features));
                    manifest["dependencies"]["sea-orm"] = toml_edit::value(driver);
                }
                let source = manifest.to_string();
                assert!(!source.contains("db-mysql"));
                assert!(!source.contains("db-postgres"));
                fs::write(path, source).expect("保存继承或外部驱动清单");
            }
            generate_job(&["delivery".to_owned()], &project).expect("按迁移结构生成任务");
            assert!(project.join("app/jobs/delivery.rs").is_file());
            for relative in ["Cargo.toml", "database/migrations/Cargo.toml"] {
                let source = fs::read_to_string(project.join(relative)).expect("读取任务依赖");
                assert!(source.contains("\"queue\""));
                assert!(!source.contains("db-mysql"));
                assert!(!source.contains("db-postgres"));
                assert_eq!(source.contains("sqlx-sqlite"), custom_driver);
            }
            let source = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                .expect("读取迁移注册");
            assert_framework_migration_registry(&source);
            assert!(!source.contains("Box::new(openrust::queue::QueueMigration)"));
            let source = fs::read_to_string(project.join("bootstrap/providers.rs"))
                .expect("读取任务 Provider");
            assert!(source.contains("crate::jobs::JobsServiceProvider"));
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn minimal_project_job_without_migration_package_fails_before_any_write() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-job-no-migrations-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        let project = root.join("blog");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("生成精简应用");
        let paths = [
            "Cargo.toml",
            "config/mod.rs",
            "app/jobs/mod.rs",
            "bootstrap/providers.rs",
            "src/main.rs",
        ];
        let before = paths.map(|path| fs::read(project.join(path)).expect("保存生成前内容"));
        let queue_before = fs::read(project.join("config/queue.rs")).ok();
        let result = generate_job(&["delivery".to_owned()], &project);
        assert!(result.is_err());
        if let Err(error) = result {
            assert!(error.to_string().contains("migration package"));
        }
        assert!(!project.join("app/jobs/delivery.rs").exists());
        assert!(!project.join("database/migrations").exists());
        assert_eq!(fs::read(project.join("config/queue.rs")).ok(), queue_before);
        for (path, expected) in paths.into_iter().zip(before) {
            assert_eq!(
                fs::read(project.join(path)).expect("读取拒绝后内容"),
                expected
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn job_generator_enables_queue_registers_migration_and_fails_closed() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-job-generator-{}", std::process::id()));
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
        .expect("generate database project");
        assert!(Cli::try_parse_from(["openrust", "generate", "job", "welcome_email"]).is_ok());
        let queue_path = project.join("config/queue.rs");
        let queue_source = fs::read_to_string(&queue_path)
            .expect("read queue defaults")
            .replace("worker_concurrency: 1", "worker_concurrency: 3");
        fs::write(&queue_path, &queue_source).expect("customize queue defaults");
        generate_job(&["welcome_email".to_owned()], &project).expect("generate job");
        assert_generated_route_layout(&project);
        let source = fs::read_to_string(project.join("app/jobs/welcome_email.rs"))
            .expect("read generated job");
        assert!(source.contains("WelcomeEmailJob"));
        assert!(source.contains("job_not_implemented"));
        assert!(source.contains("请先实现此任务的业务逻辑"));
        let app = fs::read_to_string(project.join("bootstrap/providers.rs"))
            .expect("read application provider registry");
        assert!(app.contains("crate::jobs::JobsServiceProvider"));
        let main = fs::read_to_string(project.join("src/main.rs")).expect("read main");
        assert!(main.contains("#[path = \"../app/jobs/mod.rs\"]\nmod jobs;"));
        let auth_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
            .expect("read generated auth test");
        assert!(auth_test.contains("#[path = \"../app/jobs/mod.rs\"]\nmod jobs;"));
        let migration = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
            .expect("read migration registry");
        assert_framework_migration_registry(&migration);
        assert!(!migration.contains("Box::new(openrust::queue::QueueMigration)"));
        assert!(!migration.contains(",,"));
        let app_manifest =
            fs::read_to_string(project.join("Cargo.toml")).expect("read app manifest");
        assert!(app_manifest.contains("\"queue\""));
        assert!(
            app_manifest.contains("serde = { workspace = true }")
                || app_manifest.contains("serde = { version = \"1\"")
        );
        let migration_manifest = fs::read_to_string(project.join("database/migrations/Cargo.toml"))
            .expect("read migration manifest");
        assert!(migration_manifest.contains("\"queue\""));
        generate_job(&["audit".to_owned()], &project).expect("generate second job");
        assert_eq!(
            fs::read_to_string(&queue_path).expect("read queue after jobs"),
            queue_source
        );
        let config_manifest = fs::read_to_string(project.join("config/mod.rs"))
            .expect("read queue configuration registration");
        assert_eq!(config_manifest.matches("pub mod queue;").count(), 1);
        assert_eq!(
            config_manifest
                .matches(".section(\"queue\", queue::config)")
                .count(),
            1
        );
        let modules =
            fs::read_to_string(project.join("app/jobs/mod.rs")).expect("read jobs module");
        assert_eq!(modules.matches("pub struct JobsServiceProvider").count(), 1);
        assert_eq!(
            modules
                .matches("contributions.register_job::<crate::jobs::")
                .count(),
            2
        );
        assert!(modules.find("pub mod audit;") < modules.find("pub mod welcome_email;"));
        let before = fs::read_dir(project.join("app/jobs"))
            .expect("read jobs directory")
            .count();
        assert!(generate_job(&["welcome_email".to_owned()], &project).is_err());
        assert_eq!(
            fs::read_dir(project.join("app/jobs"))
                .expect("read jobs directory after duplicate")
                .count(),
            before
        );
        fs::remove_file(&queue_path).expect("remove temporary queue config to simulate older app");
        generate_job(&["retry".to_owned()], &project).expect("restore missing queue config");
        assert_eq!(
            fs::read_to_string(&queue_path).expect("read restored queue defaults"),
            include_str!("../templates/config/queue.rs"),
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn queue_config_registration_is_idempotent_and_rejects_custom_or_ambiguous_sources() {
        let original = code_config_manifest(false, false);
        let registered = register_queue_config(&original).expect("register queue");
        assert_eq!(
            register_queue_config(&registered).expect("keep queue"),
            registered
        );
        for source in [
            format!("{registered}\npub mod queue;\n"),
            original.replace("pub mod mail;", "mod queue;\npub mod mail;"),
            original.replace(
                ".section(\"storage\", storage::config)",
                ".section(\"queue\", custom_queue)\n        .section(\"storage\", storage::config)",
            ),
            original.replace("pub mod mail;", "pub mod mailer;"),
            original.replace(".section(\"storage\", storage::config)", ""),
        ] {
            assert!(register_queue_config(&source).is_err());
        }
    }

    #[test]
    fn queue_migrations_reuse_framework_or_extend_a_legacy_application_list_once() {
        let framework = migration_lib();
        assert_eq!(
            register_queue_migration(&framework).expect("框架已经贡献队列"),
            framework
        );
        let legacy = "fn migrations() -> Vec<Box<dyn MigrationTrait>> { vec![] }";
        let registered = register_queue_migration(legacy).expect("旧应用显式贡献队列迁移");
        assert_eq!(
            registered
                .matches("openrust::queue::QueueMigration")
                .count(),
            1
        );
        assert_eq!(
            register_queue_migration(&registered).expect("保持旧登记"),
            registered
        );
        for conflicting in [
            format!("{framework}\nlet duplicate = openrust::db::FrameworkMigrator::migrations();"),
            format!("{framework}\nBox::new(openrust::queue::QueueMigration)"),
            registered.replace(
                "openrust::queue::QueueMigration",
                "openrust::queue::QueueMigration, openrust::queue::QueueMigration",
            ),
        ] {
            assert!(register_queue_migration(&conflicting).is_err());
        }
    }

    #[test]
    fn job_generator_rejects_config_conflict_before_any_mutation() {
        let root = std::env::temp_dir().join(format!(
            "openrust-cli-queue-config-conflict-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        let project = root.join("blog");
        populate_project(
            &project,
            "blog",
            "Blog",
            Some(DatabaseBackend::Mysql),
            false,
            false,
            false,
            false,
        )
        .expect("generate database project");
        let config_path = project.join("config/mod.rs");
        let config = fs::read_to_string(&config_path)
            .expect("read manifest")
            .replace(
                ".section(\"storage\", storage::config)",
                ".section(\"queue\", custom_queue)",
            );
        fs::write(&config_path, config).expect("customize manifest");
        let paths = [
            "Cargo.toml",
            "config/mod.rs",
            "config/queue.rs",
            "app/jobs/mod.rs",
            "bootstrap/providers.rs",
            "src/main.rs",
            "database/migrations/Cargo.toml",
            "database/migrations/src/lib.rs",
        ];
        let before: Vec<_> = paths
            .iter()
            .map(|path| fs::read(project.join(path)).expect("read before"))
            .collect();
        assert!(generate_job(&["delivery".to_owned()], &project).is_err());
        assert!(!project.join("app/jobs/delivery.rs").exists());
        for (path, expected) in paths.iter().zip(before) {
            assert_eq!(
                fs::read(project.join(path)).expect("read after"),
                expected,
                "mutated {path}"
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn job_generator_rejects_changed_registration_before_writing_files() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-job-anchors-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create fixture root");
        for (index, duplicate) in [false, true].into_iter().enumerate() {
            let project = root.join(format!("case-{index}"));
            populate_project(
                &project,
                "blog",
                "Blog",
                Some(DatabaseBackend::Mysql),
                true,
                false,
                false,
                false,
            )
            .expect("generate database fixture");
            generate_job(&["first".to_owned()], &project).expect("generate first job");
            let module = project.join("app/jobs/mod.rs");
            let source = fs::read_to_string(&module).expect("read job provider");
            fs::write(
                &module,
                source.replace(
                    "            Ok(())",
                    if duplicate {
                        "            Ok(())\n            Ok(())"
                    } else {
                        "            std::result::Result::Ok(())"
                    },
                ),
            )
            .expect("change registration anchor");
            let paths = [
                "app/jobs/mod.rs",
                "bootstrap/providers.rs",
                "src/main.rs",
                "Cargo.toml",
                "database/migrations/Cargo.toml",
                "database/migrations/src/lib.rs",
                "tests/auth_flows.rs",
            ];
            let before =
                paths.map(|path| fs::read(project.join(path)).expect("capture fixture input"));
            assert!(generate_job(&["second".to_owned()], &project).is_err());
            assert!(!project.join("app/jobs/second.rs").exists());
            for (path, expected) in paths.into_iter().zip(before) {
                assert_eq!(
                    fs::read(project.join(path)).expect("read rejected fixture"),
                    expected,
                    "{path}"
                );
            }
        }
        let project = root.join("auth-anchor");
        populate_project(
            &project,
            "blog",
            "Blog",
            Some(DatabaseBackend::Mysql),
            true,
            false,
            false,
            false,
        )
        .expect("generate authentication fixture");
        let test = project.join("tests/auth_flows.rs");
        fs::write(&test, "//! 应用已自定义认证测试入口。\n").expect("change auth fixture");
        let registry =
            fs::read(project.join("bootstrap/providers.rs")).expect("capture provider registry");
        assert!(generate_job(&["unregistered".to_owned()], &project).is_err());
        assert!(!project.join("app/jobs/unregistered.rs").exists());
        assert_eq!(
            fs::read(project.join("bootstrap/providers.rs")).expect("read provider registry"),
            registry
        );
        assert_eq!(
            fs::read_to_string(test).expect("read auth fixture"),
            "//! 应用已自定义认证测试入口。\n"
        );
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
        let mailer_path = project.join("app/mail/notice.rs");
        fs::write(&mailer_path, "preserve this file").expect("replace generated file for guard");
        assert!(generate_mailer(&["notice".to_owned()], &project).is_err());
        assert_eq!(
            fs::read_to_string(mailer_path).expect("read protected file"),
            "preserve this file"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_root_commands_select_the_root_application() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-default-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        create_framework_fixture(&root);
        let workspace = framework_workspace(&root).expect("源码 workspace metadata");
        let (directory, package) = application_directory(&workspace, &root).expect("默认应用");
        assert_eq!(directory, root.canonicalize().expect("应用路径"));
        assert_eq!(package, "openrust");
        fs::remove_dir_all(root).expect("清理临时目录");
    }

    #[test]
    fn creates_independent_apps_without_changing_source_workspace() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-independent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        create_framework_fixture(&root);
        let source = framework_workspace(&root).expect("解析源码 workspace");
        assert!(source.packages.iter().any(|package| {
            package.name == "openrust"
                && package.manifest_path
                    == root.join("Cargo.toml").canonicalize().expect("源码路径")
        }));
        let original = fs::read_to_string(root.join("Cargo.toml")).expect("读取源配置");
        for (name, backend) in [
            ("blog", DatabaseBackend::Postgres),
            ("shop", DatabaseBackend::Mysql),
        ] {
            create_project(name, Some(backend), false, false, false, false, &root)
                .expect("生成独立应用");
            let app = root.join(name);
            let manifest = fs::read_to_string(app.join("Cargo.toml")).expect("读取应用配置");
            assert!(manifest.contains("[workspace]"));
            assert!(manifest.contains("database/migrations"));
            assert!(!manifest.contains("modules/example"));
            assert!(!manifest.contains("source-only"));
            let metadata = framework_workspace(&app).expect("独立应用 metadata");
            assert_eq!(
                metadata.root,
                app.canonicalize().expect("独立 workspace 根")
            );
            let (resolved, package) = application_directory(&metadata, &app).expect("解析当前应用");
            assert_eq!(resolved, app.canonicalize().expect("应用路径"));
            assert_eq!(package, name);
        }
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).expect("源配置"),
            original
        );
        fs::remove_dir_all(root).expect("清理本测试目录");
    }

    #[test]
    fn rejects_invalid_existing_and_conflicting_destinations_without_mutation() {
        let root =
            std::env::temp_dir().join(format!("openrust-cli-invalid-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("创建临时目录");
        create_framework_fixture(&root);
        let original = fs::read_to_string(root.join("Cargo.toml")).expect("读取源配置");
        fs::create_dir(root.join("existing")).expect("已有目录");
        for name in ["../outside", "--", "existing", "cli"] {
            assert!(create_project(name, None, false, false, false, false, &root).is_err());
        }
        assert_eq!(
            fs::read_to_string(root.join("Cargo.toml")).expect("源配置"),
            original
        );
        fs::remove_dir_all(root).expect("清理本测试目录");
    }
}
