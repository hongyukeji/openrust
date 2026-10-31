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
        /// 添加 PostgreSQL 或 MySQL 连接和迁移包。
        #[arg(long, value_enum)]
        db: Option<DatabaseBackend>,
        /// 添加数据库认证能力和受保护的示例路由（需要 `--db`）。
        #[arg(long, requires = "db")]
        auth: bool,
        /// 为认证启用 TOTP 多因素登录（需要 `--auth`、`--db` 与 `--encryption`）。
        #[arg(long, requires_all = ["auth", "db", "encryption"])]
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
    Dev,
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
            auth,
            auth_mfa,
            http_client,
            sms_twilio,
            encryption,
            grpc,
        }) => {
            create_project_with_mfa(
                &name,
                db,
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
        Some(Builtin::Dev) => run_project(&["serve".to_owned()], true),
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
        Some(Builtin::Key {
            command: KeyCommand::Generate,
        }) => {
            writeln!(std::io::stdout().lock(), "{}", generate_key()?)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(Builtin::Forward(args)) => run_project(&args, false),
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
        bail!("authentication requires a database; use `openrust new --db postgres|mysql --auth`");
    }
    if auth_mfa && (!auth || !encryption || database.is_none()) {
        bail!("MFA requires `--auth`, `--db postgres|mysql` and `--encryption`");
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
            "bootstrap/app.rs",
            app_source(crate_name, database, auth, encryption, auth_mfa, grpc),
        ),
        (
            "app/mod.rs",
            "//! 通用应用装配；业务模块位于 modules/。\n".to_owned(),
        ),
        (
            "modules/mod.rs",
            include_str!("../templates/modules/mod.rs").to_owned(),
        ),
        (
            "modules_statuses.json",
            include_str!("../templates/modules_statuses.json").to_owned(),
        ),
        ("routes/web.rs", routes_source(auth, auth_mfa)),
        ("routes/console.rs", "//! 应用调度。\n".to_owned()),
        ("database/seeders.rs", seed_source(database)),
        (
            "app/http/controllers/mod.rs",
            "//! HTTP 控制器。\n".to_owned(),
        ),
        ("app/models/mod.rs", "//! 应用模型。\n".to_owned()),
        ("app/models/_entities/.gitkeep", String::new()),
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

    let mut runtime_config = default_config(name, auth, encryption);
    if grpc {
        runtime_config.push_str("\n[grpc]\nlisten_addr = \"127.0.0.1:50051\"\n");
    }
    let domain_files = domain_config_files(&runtime_config)?;
    if database.is_none() && !encryption && !grpc {
        files.push((
            "tests/test_app.rs",
            include_str!("../templates/tests/test_app.rs")
                .replace("OpenRustGeneratedApplication", crate_name),
        ));
    }
    if (database.is_none() && !encryption && !grpc) || auth {
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
            (
                "database/migrations/src/lib.rs",
                migration_lib(auth, auth_mfa),
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
    for (relative_path, contents) in domain_files {
        fs::write(project_dir.join(&relative_path), contents)
            .with_context(|| format!("failed to write {relative_path}"))?;
    }
    Ok(())
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
                "    assert!(registered.session_cookie_is_protected);"
            } else {
                "    assert!(!registered.session_cookie_is_protected);"
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
    manifest.push_str("serde = { version = \"1\", features = [\"derive\"] }\n");
    manifest.push_str("\n[dev-dependencies]\nfigment = { version = \"0.10\", features = [\"toml\"] }\nserde_json = \"1\"\ntime = \"0.3\"\n");
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

#[allow(
    clippy::fn_params_excessive_bools,
    reason = "feature flags map directly to the public generator options"
)]
fn app_source(
    crate_name: &str,
    database: Option<DatabaseBackend>,
    auth: bool,
    encryption: bool,
    auth_mfa: bool,
    grpc: bool,
) -> String {
    let providers = if database.is_some() {
        let mut providers = String::from(
            "\n        providers.push(Box::new(openrust::db::DatabaseServiceProvider));",
        );
        if auth {
            providers.push_str("\n        providers.push(Box::new(openrust::auth::JwtServiceProvider));\n        providers.push(Box::new(openrust::auth::AuthServiceProvider::new()));\n        providers.push(Box::new(openrust::audit::AuditServiceProvider::database()));\n        providers.push(Box::new(\n            openrust::signing::SignedLinksServiceProvider::new()\n                .with_database_revocations_from_app(),\n        ));");
        }
        if encryption {
            providers.push_str("\n        providers.push(Box::new(openrust::security::EncryptionServiceProvider));");
        }
        if auth_mfa {
            providers.push_str(
                "\n        providers.push(Box::new(openrust::auth::MfaServiceProvider));",
            );
        }
        if auth {
            providers.push_str("\n        providers.push(Box::new(openrust::auth::AuthFlowsServiceProvider::new()));");
        }
        providers.push_str("\n        providers.push(Box::new(openrust::db::MigrationServiceProvider::<\n            openrust_migration::Migrator,\n        >::new()));\n        providers.push(Box::new(openrust::db::SeedServiceProvider::new(\n            crate::seed::run,\n        )));");
        providers
    } else {
        String::new()
    };
    let encryption_provider = if encryption && database.is_none() {
        "\n        providers.push(Box::new(openrust::security::EncryptionServiceProvider));"
    } else {
        ""
    };
    let grpc_providers = if grpc {
        "\n        providers.push(Box::new(openrust::grpc::GrpcServiceProvider::new()));\n        providers.push(Box::new(crate::grpc::EchoServiceProvider));\n        providers.push(Box::new(openrust::runtime::HttpGrpcServiceProvider));"
    } else {
        ""
    };
    include_str!("../templates/src/app.rs")
        .replace("{{type_name}}", crate_name)
        .replace("{{database_providers}}", &providers)
        .replace("{{encryption_provider}}", encryption_provider)
        .replace("{{cookie_provider}}", if encryption {
            "\n        providers.push(Box::new(openrust::http::CookieServiceProvider::new()));"
        } else {
            "\n        // 当前 API 宿主显式使用未加密 Cookie；这不是 Laravel web 默认组。\n        providers.push(Box::new(openrust::http::CookieServiceProvider::plain()));"
        })
        .replace("{{grpc_providers}}", grpc_providers)
}

fn routes_source(auth: bool, auth_mfa: bool) -> String {
    let mut source = include_str!("../templates/src/routes.rs").to_owned();
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
            "    routes\n        .get(\"/api/me\", current_user)\n        .layer(openrust::auth::AuthLayer::required(\n            openrust::auth::AuthMode::Jwt,\n        ))\n        .post(\"/api/auth/register\", auth_register)\n        .post(\n            \"/api/auth/request-email-verification\",\n            auth_request_email_verification,\n        )\n        .post(\"/api/auth/verify-email\", auth_verify_email)\n        .post(\"/api/auth/login\", auth_login)\n        .post(\"/api/auth/refresh\", auth_refresh)\n        .post(\"/api/auth/logout\", auth_logout)\n        .post(\"/api/auth/password-reset\", auth_request_password_reset)\n        .post(\"/api/auth/password-reset/confirm\", auth_reset_password)\n        .get(\"/api\", root)\n        .merge(Routes::new().get(\"/api/me/pat\", current_user).layer(\n            openrust::auth::AuthLayer::required(openrust::auth::AuthMode::PersonalAccessToken),\n        ))".to_owned(),
            include_str!("../templates/src/auth_routes.rs")
                .replace("{{login_handler}}", login_handler.trim_end())
                .replace("{{mfa_handler}}", mfa_handler.trim_end()),
        )
    } else {
        ("    routes.get(\"/api\", root)".to_owned(), String::new())
    };
    let registration = if auth_mfa {
        registration.replace(
            ".post(\"/api/auth/login\", auth_login)",
            ".post(\"/api/auth/login\", auth_login)\n        .post(\"/api/auth/mfa/login\", auth_mfa_login)",
        )
        .replace(
            ".get(\"/api\", root)",
            ".get(\"/api\", root)\n        .merge(\n            Routes::new()\n                .post(\"/api/auth/mfa/enrollment\", auth_mfa_enrollment)\n                .post(\n                    \"/api/auth/mfa/enrollment/confirm\",\n                    auth_mfa_enrollment_confirm,\n                )\n                .post(\"/api/auth/mfa/disable\", auth_mfa_disable)\n                .post(\"/api/auth/mfa/recovery-codes\", auth_mfa_recovery_codes)\n                .layer(openrust::auth::AuthLayer::required(\n                    openrust::auth::AuthMode::Jwt,\n                )),\n        )",
        )
    } else {
        registration
    };
    source = source.replace("{{routes}}", &registration);
    if !auth {
        source = source.replace(
            "routes.get(\"/api\", root)\n        .fallback(not_found)",
            "routes.get(\"/api\", root).fallback(not_found)",
        );
    }
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
    let mut contents = "# 复制为 .env 供本地或部署环境使用。不要提交填入实际值的 .env 文件。\n# 生产环境可使用权限受控的 .env 或部署平台注入的变量；进程环境变量优先。\n# 常用环境值和密钥放在这里；结构化默认值见 config/*.toml。\nAPP_ENV=development\nAPP_NAME=".to_owned();
    contents.push_str(name);
    contents.push_str("\nSERVER_PORT=3000\nAPP_URL=\"http://localhost:${SERVER_PORT}\"\n\n# 服务\n# APP_SERVER_HOST=127.0.0.1\n# APP_SERVER_PUBLIC_DIR=public\n# APP_SERVER_ROOT_TARGET=file:/index.html\n\n# 仅本地开发需要时取消注释；生产环境保持默认值。\n# APP_SESSION_SECURE=false\n# APP_VIEW_AUTO_RELOAD=true\n\n# 数据库\n");
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
        contents.push_str("DB_TABLE_PREFIX=tb_\n");
    } else {
        contents.push_str(
            "# 启用 --db postgres 或 --db mysql 后配置。\n# DB_CONNECTION=postgres\n# DB_HOST=127.0.0.1\n# DB_PORT=5432\n# DB_DATABASE=应用数据库名\n# DB_USERNAME=数据库用户名\n# DB_PASSWORD=数据库密码\n",
        );
    }
    contents.push_str(
        "\n# 缓存：默认具名 redis store 使用 Redis。\nREDIS_HOST=127.0.0.1\nREDIS_PASSWORD=null\nREDIS_PORT=6379\nREDIS_DB=0\nCACHE_STORE=redis\n\n# 日志、邮件、短信与国际化\nAPP_LOG_LEVEL=info\nMAIL_DRIVER=log\nSMS_DRIVER=log\nI18N_DEFAULT_LOCALE=en\n\n# Redis 队列需启用 queue-redis feature 后再配置；与缓存共用 REDIS_* 连接信息。\n# QUEUE_DRIVER=redis\n",
    );
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

fn default_config(name: &str, auth: bool, encryption: bool) -> String {
    let mut config = include_str!("../templates/config/framework.toml").replace("{{name}}", name);
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

fn domain_config_files(contents: &str) -> anyhow::Result<Vec<(String, String)>> {
    let mut document = contents
        .parse::<toml_edit::DocumentMut>()
        .context("embedded configuration template is invalid TOML")?;
    let domains = document
        .iter()
        .map(|(name, _)| name.to_owned())
        .collect::<Vec<_>>();
    domains
        .into_iter()
        .map(|domain| {
            let mut file = toml_edit::DocumentMut::new();
            if let Some(value) = document.remove(&domain) {
                file.insert(&domain, value);
            }
            Ok((format!("config/{domain}.toml"), file.to_string()))
        })
        .collect()
}

fn seed_source(database: Option<DatabaseBackend>) -> String {
    if database.is_some() {
        "//! 应用数据填充入口。\n\nuse openrust::{Db, Result};\n\npub async fn run(_database: Db) -> Result<()> {\n    Ok(())\n}\n".to_owned()
    } else {
        "//! 应用数据填充入口。\n".to_owned()
    }
}

fn migration_manifest(name: &str, database: DatabaseBackend, auth: bool, auth_mfa: bool) -> String {
    let mut features = vec![database.feature()];
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

fn migration_lib(auth: bool, auth_mfa: bool) -> String {
    let migrations = if auth {
        if auth_mfa {
            "vec![\n            Box::new(openrust::auth::AuthMigration),\n            Box::new(openrust::auth::AuthRefreshTokenMigration),\n            Box::new(openrust::auth::AuthVerificationCodeMigration),\n            Box::new(openrust::auth::AuthPolicyMigration),\n            Box::new(openrust::database::DatabaseRateLimitMigration),\n            Box::new(openrust::audit::DatabaseAuditMigration),\n            Box::new(openrust::audit::DatabaseAuditTenantMigration),\n            Box::new(openrust::signing::SignedLinkRevocationMigration),\n            Box::new(openrust::auth::AuthMfaMigration),\n        ]"
        } else {
            "vec![\n            Box::new(openrust::auth::AuthMigration),\n            Box::new(openrust::auth::AuthRefreshTokenMigration),\n            Box::new(openrust::auth::AuthVerificationCodeMigration),\n            Box::new(openrust::auth::AuthPolicyMigration),\n            Box::new(openrust::database::DatabaseRateLimitMigration),\n            Box::new(openrust::audit::DatabaseAuditMigration),\n            Box::new(openrust::audit::DatabaseAuditTenantMigration),\n            Box::new(openrust::signing::SignedLinkRevocationMigration),\n        ]"
        }
    } else {
        "vec![]"
    };
    format!(
        "//! 应用数据库迁移。\n\nuse openrust::database::sea_orm_migration::prelude::*;\n\npub struct Migrator;\n\n#[openrust::database::sea_orm_migration::async_trait::async_trait]\nimpl MigratorTrait for Migrator {{\n    fn migrations() -> Vec<Box<dyn MigrationTrait>> {{\n        {migrations}\n    }}\n}}\n"
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
    for path in [
        project_dir.join("Cargo.toml"),
        project_dir.join("database/migrations/Cargo.toml"),
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

fn generate_job(values: &[String], project_dir: &Path) -> anyhow::Result<()> {
    let [name] = values else {
        bail!("usage: openrust generate job <name>");
    };
    let module = normalize_identifier(name)?;
    let type_name = format!("{}Job", pascal_case(&module));
    let app_manifest_path = project_dir.join("Cargo.toml");
    let app_manifest = fs::read_to_string(&app_manifest_path)?;
    if !app_manifest.contains("db-postgres") && !app_manifest.contains("db-mysql") {
        bail!("job generation requires an application created with `openrust new --db`");
    }
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
    let app_file = project_dir.join("bootstrap/app.rs");
    let app_source = fs::read_to_string(&app_file)?;
    let auth_test_file = project_dir.join("tests/auth_flows.rs");
    let updated_auth_test = if auth_test_file.exists() {
        let test_source = fs::read_to_string(&auth_test_file)?;
        if test_source.contains("mod jobs;") {
            test_source
        } else {
            test_source.replace(
                "#[path = \"../routes/web.rs\"]\nmod routes;",
                "#[path = \"../routes/web.rs\"]\nmod routes;\n#[path = \"../app/jobs/mod.rs\"]\nmod jobs;",
            )
        }
    } else {
        String::new()
    };
    let updated_migration = register_queue_migration(&migration_registry)?;
    let updated_app_manifest = enable_feature_source(&app_manifest, "queue", &app_manifest_path)?;
    let updated_migration_manifest =
        enable_feature_source(&migration_source, "queue", &migration_manifest_path)?;
    let updated_app_manifest = add_serde_workspace_dependency(&updated_app_manifest)?;
    let updated_app = if app_source.contains("crate::jobs::JobsServiceProvider") {
        app_source
    } else {
        let updated = app_source.replace(
            "        providers.push(Box::new(RoutesProvider));",
            "        let queue = openrust::queue::QueueServiceProvider::new();\n        let contributions = queue.contributions();\n        providers.push(Box::new(crate::jobs::JobsServiceProvider::new(contributions)));\n        providers.push(Box::new(queue));\n        providers.push(Box::new(RoutesProvider));",
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
        let registration = format!("self.0.register_job::<crate::jobs::{module}::{type_name}>()?;");
        if modules.contains(&registration) {
            bail!("job handler is already registered: {module}");
        }
        append_sorted_module_content(&modules, &format!("pub mod {module};"))?.replace(
            "        Ok(())",
            &format!("        {registration}\n        Ok(())"),
        )
    } else {
        format!(
            "{}\npub mod {module};\n\nuse std::sync::Arc;\nuse openrust::core::{{RegisterContext, Result, ServiceProvider}};\nuse openrust::queue::QueueContributions;\n\n/// 注册应用的后台任务。\npub struct JobsServiceProvider(Arc<QueueContributions>);\n\nimpl JobsServiceProvider {{\n    /// 使用队列 Provider 的共享贡献目录。\n    pub fn new(contributions: Arc<QueueContributions>) -> Self {{ Self(contributions) }}\n}}\n\nimpl ServiceProvider for JobsServiceProvider {{\n    fn register(&self, _app: &mut RegisterContext<'_>) -> Result<()> {{\n        self.0.register_job::<crate::jobs::{module}::{type_name}>()?;\n        Ok(())\n    }}\n}}\n",
            modules.trim_end(),
        )
    };
    fs::write(&job_file, render_job(&type_name, &module))?;
    fs::write(&module_file, updated_modules)?;
    fs::write(&main_file, updated_main)?;
    fs::write(&app_file, updated_app)?;
    if auth_test_file.exists() {
        fs::write(&auth_test_file, updated_auth_test)?;
    }
    fs::write(&app_manifest_path, updated_app_manifest)?;
    fs::write(&migration_manifest_path, updated_migration_manifest)?;
    fs::write(&migration_lib_path, updated_migration)?;
    print_generated(&format!("Generated job `{module}`."))?;
    Ok(())
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
    if registry.contains("openrust::queue::QueueMigration") {
        return Ok(registry.to_owned());
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
    let manifest = fs::read_to_string(project_dir.join("Cargo.toml"))?;
    if !manifest.contains("db-postgres") && !manifest.contains("db-mysql") {
        bail!("model generation requires an application created with `openrust new --db`");
    }
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
    let migration_dir = project_dir.join("database/migrations/src");
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
    run_application(args, development)
}

fn run_application(args: &[String], development: bool) -> anyhow::Result<ExitCode> {
    let current_dir = std::env::current_dir()?;
    let workspace = framework_workspace(&current_dir)?;
    let (app_dir, _package_name) = application_directory(&workspace, &current_dir)?;
    let mut command = Process::new("cargo");
    command
        .current_dir(&app_dir)
        .args(["run", "--quiet", "--"])
        .args(args);
    if development {
        command.env("APP_ENV", "development");
        prepare_development_redis(&workspace.root, &app_dir)?;
    }
    command
        .status()
        .context("failed to start `cargo run`")
        .map(exit_code)
}

#[allow(
    clippy::too_many_lines,
    reason = "development Redis discovery, startup, and readiness form one bounded lifecycle"
)]
fn prepare_development_redis(workspace_root: &Path, app_dir: &Path) -> anyhow::Result<()> {
    let mut dotenv = std::collections::HashMap::new();
    if let Ok(entries) = dotenvy::from_path_iter(app_dir.join(".env")) {
        for (key, value) in entries.flatten() {
            dotenv.insert(key, value);
        }
    }
    let config_path = app_dir.join("config/cache.toml");
    if !config_path.is_file() {
        return Ok(());
    }
    let source = fs::read_to_string(&config_path)?;
    let document = source
        .parse::<toml_edit::DocumentMut>()
        .with_context(|| format!("{} is invalid TOML", config_path.display()))?;
    let Some(cache) = document.get("cache").and_then(toml_edit::Item::as_table) else {
        return Ok(());
    };
    let process_store = std::env::var("CACHE_STORE").ok();
    let store_name = selected_cache_store(
        cache,
        process_store.as_deref(),
        dotenv.get("CACHE_STORE").map(String::as_str),
    );
    let Some(store) = cache
        .get("stores")
        .and_then(toml_edit::Item::as_table)
        .and_then(|stores| stores.get(&store_name))
        .and_then(toml_edit::Item::as_table)
    else {
        return Ok(());
    };
    let driver_env = format!(
        "APP_CACHE_STORES_{}_DRIVER",
        store_name.to_ascii_uppercase()
    );
    let process_driver = std::env::var(&driver_env).ok();
    if development_redis_driver(
        store,
        process_driver.as_deref(),
        dotenv.get(&driver_env).map(String::as_str),
    ) != "redis"
    {
        return Ok(());
    }

    let nested_env = format!("APP_CACHE_STORES_{}_DSN", store_name.to_ascii_uppercase());
    let environment_keys = [nested_env.as_str()];
    let process_dsn = environment_keys
        .iter()
        .find_map(|key| std::env::var(key).ok());
    let mut redis_environment = dotenv.clone();
    let redis_keys = [
        "REDIS_HOST",
        "REDIS_USERNAME",
        "REDIS_PASSWORD",
        "REDIS_PORT",
        "REDIS_DB",
        "REDIS_SCHEME",
    ];
    for key in redis_keys {
        if let Ok(value) = std::env::var(key) {
            redis_environment.insert(key.to_owned(), value);
        }
    }
    let dotenv_dsn = environment_keys
        .iter()
        .find_map(|key| dotenv.get(*key).cloned());
    let dsn = development_redis_dsn(
        &store_name,
        store,
        process_dsn.or(dotenv_dsn),
        &redis_environment,
    );

    let parsed =
        url::Url::parse(&dsn).context("configured Redis connection settings are invalid")?;
    if parsed.scheme() != "redis" {
        return Ok(());
    }
    let Some(host) = parsed.host_str() else {
        return Ok(());
    };
    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if !is_loopback {
        return Ok(());
    }
    let port = parsed.port().unwrap_or(6379);
    if redis_ping(host, port) {
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
        if redis_ping(host, port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Redis development service did not become ready within 10 seconds")
}

fn selected_cache_store(
    cache: &toml_edit::Table,
    process_store: Option<&str>,
    dotenv_store: Option<&str>,
) -> String {
    process_store
        .or(dotenv_store)
        .or_else(|| cache.get("default").and_then(toml_edit::Item::as_str))
        .unwrap_or("redis")
        .to_owned()
}

fn development_redis_driver<'a>(
    store: &'a toml_edit::Table,
    process_driver: Option<&'a str>,
    dotenv_driver: Option<&'a str>,
) -> &'a str {
    process_driver
        .or(dotenv_driver)
        .or_else(|| store.get("driver").and_then(toml_edit::Item::as_str))
        .unwrap_or("")
}

fn development_redis_dsn(
    store_name: &str,
    store: &toml_edit::Table,
    explicit_dsn: Option<String>,
    redis_environment: &std::collections::HashMap<String, String>,
) -> String {
    explicit_dsn
        .or_else(|| {
            (store_name == "redis")
                .then(|| redis_dsn_from_environment(redis_environment))
                .flatten()
        })
        .or_else(|| {
            store
                .get("dsn")
                .and_then(toml_edit::Item::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "redis://127.0.0.1:6379/".to_owned())
}

fn redis_dsn_from_environment(
    environment: &std::collections::HashMap<String, String>,
) -> Option<String> {
    const KEYS: [&str; 6] = [
        "REDIS_HOST",
        "REDIS_USERNAME",
        "REDIS_PASSWORD",
        "REDIS_PORT",
        "REDIS_DB",
        "REDIS_SCHEME",
    ];
    if !KEYS.iter().any(|key| environment.contains_key(*key)) {
        return None;
    }
    let scheme = environment
        .get("REDIS_SCHEME")
        .map_or("redis", String::as_str);
    if !matches!(scheme, "redis" | "rediss") {
        return None;
    }
    let mut url = url::Url::parse(&format!("{scheme}://localhost")).ok()?;
    url.set_host(Some(
        environment
            .get("REDIS_HOST")
            .map(String::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("127.0.0.1"),
    ))
    .ok()?;
    let port = environment
        .get("REDIS_PORT")
        .map_or("6379", String::as_str)
        .parse::<u16>()
        .ok()?;
    url.set_port(Some(port)).ok()?;
    if let Some(username) = environment
        .get("REDIS_USERNAME")
        .filter(|value| !value.is_empty())
    {
        url.set_username(username).ok()?;
    }
    if let Some(password) = environment
        .get("REDIS_PASSWORD")
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("null"))
    {
        url.set_password(Some(password)).ok()?;
    }
    let database = environment.get("REDIS_DB").map_or("0", String::as_str);
    database.parse::<u32>().ok()?;
    url.path_segments_mut().ok()?.pop_if_empty().push(database);
    Some(url.into())
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

    #[test]
    fn laravel_style_redis_variables_form_a_connection_url() {
        let environment = std::collections::HashMap::from([
            ("REDIS_HOST".to_owned(), "cache.example.test".to_owned()),
            ("REDIS_PASSWORD".to_owned(), "secret:word".to_owned()),
            ("REDIS_PORT".to_owned(), "6380".to_owned()),
            ("REDIS_DB".to_owned(), "2".to_owned()),
        ]);
        let dsn = redis_dsn_from_environment(&environment)
            .expect("Redis connection variables should produce a URL");
        let url = url::Url::parse(&dsn).expect("generated Redis URL should be valid");
        assert_eq!(url.host_str(), Some("cache.example.test"));
        assert_eq!(url.port(), Some(6380));
        assert_eq!(url.path(), "/2");
        assert_eq!(url.password(), Some("secret%3Aword"));
    }

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
        let app = fs::read_to_string(project.join("bootstrap/app.rs"))
            .expect("read generated application providers");
        assert!(app.contains("GrpcServiceProvider::new()"));
        assert!(app.contains("EchoServiceProvider"));
        assert!(app.contains("HttpGrpcServiceProvider"));
        let proto =
            fs::read_to_string(project.join("proto/echo.proto")).expect("read generated Protobuf");
        assert!(proto.contains("package grpc_demo.v1;"));
        let config = fs::read_to_string(project.join("config/grpc.toml"))
            .expect("read generated gRPC configuration");
        assert!(config.contains("[grpc]"));
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
        let config = fs::read_to_string(project.join("config/security.toml"))
            .expect("read generated encryption configuration");
        let env = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(app.contains("EncryptionServiceProvider"));
        assert!(config.contains("[security.encryption]"));
        assert!(env.contains("APP_SECURITY_ENCRYPTION_KEYS_LOCAL="));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn key_generation_print_value_is_a_32_byte_base64_secret() {
        let encoded = generate_key().expect("generate key");
        assert_eq!(STANDARD.decode(encoded).expect("decode key").len(), 32);
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
            "[cache]\ndefault = 'default'\n[cache.stores.default]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n",
        )
        .expect("write app config");
        prepare_development_redis(&root, &app).expect("reuse existing Redis");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn development_redis_selection_uses_cache_store_and_official_default_field() {
        let source = "[cache]\ndefault = 'memory'\ndefault_store = 'legacy'\n[cache.stores.memory]\ndriver = 'memory'\n[cache.stores.redis]\ndriver = 'redis'\n";
        let document = source
            .parse::<toml_edit::DocumentMut>()
            .expect("parse cache configuration");
        let cache = document["cache"].as_table().expect("cache table");
        assert_eq!(selected_cache_store(cache, None, None), "memory");
        assert_eq!(selected_cache_store(cache, None, Some("redis")), "redis");
        assert_eq!(
            selected_cache_store(cache, Some("memory"), Some("redis")),
            "memory"
        );
        let default = "[cache]\ndefault_store = 'legacy'\n[cache.stores.redis]\ndriver = 'redis'\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("parse default cache configuration");
        assert_eq!(
            selected_cache_store(
                default["cache"].as_table().expect("cache table"),
                None,
                None
            ),
            "redis"
        );
    }

    #[test]
    fn development_redis_dsn_only_applies_global_variables_to_named_redis_store() {
        let document = "[cache.stores.default]\ndriver = 'redis'\ndsn = 'redis://remote.example.test:6380/'\n[cache.stores.local]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n[cache.stores.redis]\ndriver = 'redis'\ndsn = 'redis://127.0.0.1:6379/'\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("parse named Redis stores");
        let stores = document["cache"]["stores"]
            .as_table()
            .expect("stores table");
        let default_store = stores["default"].as_table().expect("custom store");
        let local_store = stores["local"].as_table().expect("local store");
        let redis_store = stores["redis"].as_table().expect("named redis store");
        assert_eq!(development_redis_driver(redis_store, None, None), "redis");
        assert_eq!(
            development_redis_driver(redis_store, None, Some("memory")),
            "memory"
        );
        assert_eq!(
            development_redis_driver(redis_store, Some("redis"), Some("memory")),
            "redis"
        );
        let memory_document = "[store]\ndriver = 'memory'\n"
            .parse::<toml_edit::DocumentMut>()
            .expect("parse memory store");
        let memory_store = memory_document["store"].as_table().expect("memory store");
        assert_eq!(
            development_redis_driver(memory_store, None, Some("redis")),
            "redis"
        );
        let global_local =
            std::collections::HashMap::from([("REDIS_HOST".to_owned(), "127.0.0.1".to_owned())]);
        assert_eq!(
            development_redis_dsn("default", default_store, None, &global_local),
            "redis://remote.example.test:6380/"
        );
        assert_eq!(
            development_redis_dsn("redis", redis_store, None, &global_local),
            "redis://127.0.0.1:6379/0"
        );

        let global_remote = std::collections::HashMap::from([(
            "REDIS_HOST".to_owned(),
            "remote.example.test".to_owned(),
        )]);
        assert_eq!(
            development_redis_dsn("local", local_store, None, &global_remote),
            "redis://127.0.0.1:6379/"
        );
        assert_eq!(
            development_redis_dsn("redis", redis_store, None, &global_remote),
            "redis://remote.example.test:6379/0"
        );
        assert_eq!(
            development_redis_dsn(
                "redis",
                redis_store,
                Some("redis://explicit.example.test:6381/".to_owned()),
                &global_local,
            ),
            "redis://explicit.example.test:6381/"
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
        assert!(
            module_manifest.contains("pub fn manifest(root: &std::path::Path) -> ModuleManifest")
        );
        assert!(module_manifest.contains("ModuleManifest::from_file("));
        assert!(!module_manifest.contains("CARGO_MANIFEST_DIR"));
        assert!(module_manifest.contains("root.join(\"modules_statuses.json\")"));
        assert!(module_manifest.contains("modules_statuses.json"));
        assert!(!module_manifest.contains(".enable("));
        let statuses: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(project.join("modules_statuses.json"))
                .expect("read generated module statuses"),
        )
        .expect("valid module statuses JSON");
        assert_eq!(statuses, serde_json::json!({}));
        assert!(!project.join("modules/example").exists());
        let application = fs::read_to_string(project.join("bootstrap/app.rs"))
            .expect("read generated application");
        assert!(application.contains("fn modules("));
        assert!(application.contains("crate::modules::manifest("));
        assert!(application.contains("config"));
        assert!(application.contains(".modules_root"));
        assert!(application.contains(".as_deref()"));
        let main = fs::read_to_string(project.join("src/main.rs"))
            .expect("read generated application entry");
        assert!(main.contains("../modules/mod.rs"));
        assert!(!project.join("config/default.toml").exists());
        assert!(!project.join("config/development.toml").exists());
    }

    #[test]
    fn project_template_has_expected_layout() {
        let root = std::env::temp_dir().join(format!("openrust-cli-{}", std::process::id()));
        let project = root.join("blog");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create temporary test directory");
        populate_project(&project, "blog", "Blog", None, false, false, false, false)
            .expect("generate project");
        for path in [
            "Cargo.toml",
            ".gitignore",
            "public/index.html",
            "resources/views/index.html",
            "src/main.rs",
            "bootstrap/app.rs",
            "modules/mod.rs",
            "modules_statuses.json",
            "routes/web.rs",
            "config/app.toml",
            "config/cache.toml",
            "config/server.toml",
            "config/session.toml",
            "config/i18n.toml",
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
        assert_module_layout(&project);
        let snapshot = [
            "Cargo.toml",
            ".gitignore",
            "public/index.html",
            "resources/views/index.html",
            "config/app.toml",
            "config/cache.toml",
            "config/server.toml",
            "config/session.toml",
            "config/i18n.toml",
            "src/main.rs",
            "bootstrap/app.rs",
            "modules/mod.rs",
            "modules_statuses.json",
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
        assert!(gitignore.lines().any(|line| line == ".env"));
        assert!(gitignore.lines().any(|line| line == ".env.*"));
        assert!(gitignore.lines().any(|line| line == "/storage/"));
        assert!(gitignore.lines().any(|line| line == "!.env.example"));
        assert!(gitignore.lines().any(|line| line == "!.env.*.example"));
        let env_example = fs::read_to_string(project.join(".env.example"))
            .expect("read generated environment example");
        assert!(env_example.contains("APP_NAME=blog"));
        assert!(
            env_example.contains("SERVER_PORT=3000\nAPP_URL=\"http://localhost:${SERVER_PORT}\"")
        );
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
        assert!(env_example.contains("# QUEUE_DRIVER=redis"));
        assert!(env_example.contains("生产环境可使用权限受控的 .env"));
        assert!(!env_example.contains("Production reads configuration"));
        let guide = fs::read_to_string(project.join("AGENTS.md")).expect("read generated guide");
        assert!(guide.contains("应用开发指南"));
        assert!(!guide.contains("Application guide"));
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
            assert!(project.join("database/migrations/Cargo.toml").is_file());
            assert!(project.join("database/migrations/src/lib.rs").is_file());
            assert!(
                fs::read_to_string(project.join("config/database.toml"))
                    .expect("read database domain config")
                    .contains("table_prefix = \"tb_\"")
            );
            assert!(
                fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                    .expect("read empty migration registry")
                    .contains("vec![]")
            );
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.contains("DatabaseServiceProvider"));
            assert!(app.contains("openrust_migration::Migrator"));
            assert!(app.contains("SeedServiceProvider"));
            assert!(project.join(".env.example").is_file());
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated database environment example");
            assert!(env.contains(&format!("DB_CONNECTION={}", backend.connection_name())));
            assert!(env.contains(&format!("DB_PORT={}", backend.port())));
            assert!(env.contains("DB_DATABASE=blog"));
            assert!(env.contains("DB_TABLE_PREFIX=tb_"));
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
            assert_eq!(
                fs::read_to_string(project.join("modules_statuses.json"))
                    .expect("read generated auth module statuses"),
                "{}\n"
            );
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
            assert!(migration.contains("openrust::auth::AuthMigration"));
            assert!(migration.contains("openrust::auth::AuthRefreshTokenMigration"));
            assert!(migration.contains("openrust::auth::AuthVerificationCodeMigration"));
            assert!(migration.contains("openrust::auth::AuthPolicyMigration"));
            assert!(migration.contains("openrust::database::DatabaseRateLimitMigration"));
            assert!(migration.contains("openrust::audit::DatabaseAuditMigration"));
            assert!(migration.contains("openrust::audit::DatabaseAuditTenantMigration"));
            assert!(migration.contains("openrust::signing::SignedLinkRevocationMigration"));
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.contains("AuthServiceProvider"));
            assert!(app.contains("JwtServiceProvider"));
            assert!(app.contains("AuthFlowsServiceProvider"));
            assert!(app.contains("AuditServiceProvider::database()"));
            assert!(app.contains("SignedLinksServiceProvider"));
            let config = fs::read_to_string(project.join("config/security.toml"))
                .expect("read generated signing config");
            assert!(config.contains("[security.signed_links]"));
            let env = fs::read_to_string(project.join(".env.example"))
                .expect("read generated signing environment template");
            assert!(env.contains("APP_SECURITY_SIGNED_LINKS_KEYS_LOCAL="));
            assert!(!app.contains("SessionPlugin::new()"));
            let routes =
                fs::read_to_string(project.join("routes/web.rs")).expect("read generated routes");
            assert!(routes.contains("AuthLayer::required("));
            assert!(routes.contains("AuthMode::Jwt"));
            assert!(routes.contains("/api/me/pat"));
            assert!(routes.contains("AuthMode::PersonalAccessToken"));
            assert!(routes.contains("CurrentUser"));
            assert!(routes.contains("/api/auth/register"));
            assert!(routes.contains("/api/auth/verify-email"));
            assert!(routes.contains("/api/auth/refresh"));
            assert!(routes.contains("/api/auth/password-reset/confirm"));
            assert!(routes.contains("ClientIp"));
            let auth_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
                .expect("read generated auth HTTP integration test");
            assert!(auth_test.contains("trusted_proxy_ranges"));
            let config = fs::read_to_string(project.join("config/auth.toml"))
                .expect("read generated auth config");
            assert!(config.contains("[auth.jwt]"));
            assert!(config.contains("keys = {}"));
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
        assert!(Cli::try_parse_from(["openrust", "new", "blog", "--auth"]).is_err());
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
            let app =
                fs::read_to_string(project.join("bootstrap/app.rs")).expect("read generated app");
            assert!(app.find("EncryptionServiceProvider") < app.find("MfaServiceProvider"));
            assert!(app.find("MfaServiceProvider") < app.find("AuthFlowsServiceProvider"));
            assert!(
                fs::read_to_string(project.join("database/migrations/src/lib.rs"))
                    .expect("migration")
                    .contains("AuthMfaMigration")
            );
            assert!(
                fs::read_to_string(project.join("routes/web.rs"))
                    .expect("routes")
                    .contains("/api/auth/mfa/login")
            );
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
        generate_job(&["welcome_email".to_owned()], &project).expect("generate job");
        let source = fs::read_to_string(project.join("app/jobs/welcome_email.rs"))
            .expect("read generated job");
        assert!(source.contains("WelcomeEmailJob"));
        assert!(source.contains("job_not_implemented"));
        assert!(source.contains("请先实现此任务的业务逻辑"));
        let app = fs::read_to_string(project.join("bootstrap/app.rs")).expect("read app");
        assert!(app.contains("crate::jobs::JobsServiceProvider"));
        let main = fs::read_to_string(project.join("src/main.rs")).expect("read main");
        assert!(main.contains("#[path = \"../app/jobs/mod.rs\"]\nmod jobs;"));
        let auth_test = fs::read_to_string(project.join("tests/auth_flows.rs"))
            .expect("read generated auth test");
        assert!(auth_test.contains("#[path = \"../app/jobs/mod.rs\"]\nmod jobs;"));
        let migration = fs::read_to_string(project.join("database/migrations/src/lib.rs"))
            .expect("read migration registry");
        assert!(migration.contains("openrust::queue::QueueMigration"));
        assert!(!migration.contains(",,"));
        assert!(migration.contains(
            "SignedLinkRevocationMigration),\n            Box::new(openrust::queue::QueueMigration),"
        ));
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
        let modules =
            fs::read_to_string(project.join("app/jobs/mod.rs")).expect("read jobs module");
        assert_eq!(modules.matches("pub struct JobsServiceProvider").count(), 1);
        assert_eq!(
            modules
                .matches("self.0.register_job::<crate::jobs::")
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
