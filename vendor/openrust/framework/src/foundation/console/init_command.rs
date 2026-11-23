//! Application initialization through the existing installation and migration services.

use super::{
    install_routes_command::{build_application, initialize_routes},
    source_edit::{lock_manifest, read, replace},
};
use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::Application,
};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, IsTerminal as _},
    path::{Path, PathBuf},
    process::Command as Process,
};

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let mut syntax =
        clap::Command::new("init").about("Configure, install and initialize the application");
    for (name, help) in [
        (
            "force",
            "Overwrite installable scaffolding and allow production migrations",
        ),
        (
            "dry-run",
            "Inspect initialization without writing, connecting or building",
        ),
        (
            "without-migration",
            "Prepare the application but leave database migrations pending",
        ),
    ] {
        syntax = syntax.arg(
            clap::Arg::new(name)
                .long(name)
                .action(clap::ArgAction::SetTrue)
                .help(help),
        );
    }
    syntax = syntax.arg(
        clap::Arg::new("extensions")
            .long("extensions")
            .value_name("LIST")
            .help("Comma-separated extensions: api,broadcasting,reverb,grpc,passport,fortify,passkeys,socialite,precognition,oidc,sso; all installs the complete profile"),
    );
    artisan.add_command(Box::new(InitCommand(CommandDefinition::new(syntax))))?;
    artisan.add_command(Box::new(ResourcesCommand(CommandDefinition::new(
        clap::Command::new("init:resources")
            .hide(true)
            .arg(
                clap::Arg::new("extensions")
                    .long("extensions")
                    .required(true),
            )
            .arg(
                clap::Arg::new("force")
                    .long("force")
                    .action(clap::ArgAction::SetTrue),
            ),
    ))))
}
struct InitCommand(CommandDefinition);
impl Command for InitCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let root = app.base_path().to_owned();
            let matches = input.matches();
            let force = matches.get_flag("force");
            let dry = matches.get_flag("dry-run");
            let without_migration = matches.get_flag("without-migration");
            let mut extensions =
                Extensions::parse(matches.get_one::<String>("extensions").map(String::as_str))?;
            let interactive =
                !matches.get_flag("no-interaction") && io::stdin().is_terminal() && !dry;
            for file in [
                "Cargo.toml",
                "bootstrap/app.rs",
                "routes/mod.rs",
                "config/mod.rs",
            ] {
                read(&root.join(file))?;
            }
            read(&root.join("Cargo.toml"))?
                .parse::<toml_edit::DocumentMut>()
                .map_err(|_| ConsoleError::new("Invalid Cargo.toml."))?;
            let environment = matches
                .get_many::<String>("env")
                .and_then(|mut values| values.next())
                .cloned();
            if environment.as_ref().is_some_and(|name| {
                name.is_empty()
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            }) {
                return Err(ConsoleError::new("Invalid environment file suffix."));
            }
            let env_path = root.join(
                environment
                    .as_ref()
                    .map(|name| format!(".env.{name}"))
                    .unwrap_or_else(|| ".env".to_owned()),
            );
            if env_path.is_symlink() {
                return Err(ConsoleError::new(
                    "Refusing to replace a symlinked .env file.",
                ));
            }
            let source = if env_path.exists() {
                read(&env_path)?
            } else {
                read(&root.join(".env.example"))?
            };
            let mut values = parse_environment(&source)?;
            if interactive && matches.get_one::<String>("extensions").is_none() {
                let mut choice = BTreeMap::new();
                prompt(&mut choice, "EXTENSIONS", "all", false)?;
                extensions = Extensions::parse(choice.get("EXTENSIONS").map(String::as_str))?;
            }
            loop {
                if interactive {
                    configure(&mut values, extensions.has("reverb"))?;
                }
                match validate_choices(&values)
                    .and_then(|()| extensions.validate_application(&root, force, &values))
                {
                    Ok(()) => break,
                    Err(error) if interactive => {
                        eprintln!("{error}");
                        if !confirm("Correct configuration and retry?", true)? {
                            return Err(error);
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            extensions.validate(&values)?;
            output.info(&format!(
                "Selected extensions: {}.\n",
                extensions.names.join(",")
            ));
            output.info(&format!(
                "Initialization: database={}, cache={}, session={}, queue={}.\n",
                value(&values, "DB_CONNECTION", "sqlite"),
                value(&values, "CACHE_STORE", "database"),
                value(&values, "SESSION_DRIVER", "database"),
                value(&values, "QUEUE_CONNECTION", "database")
            ));
            if extensions.has("api") {
                output.info("API / Sanctum: install API routes and authentication.\n");
            }
            if extensions.has("broadcasting") {
                output.info("broadcasting channels: install channel registration.\n");
            }
            if extensions.has("grpc") {
                output.info("gRPC: install service registration.\n");
            }
            if extensions.has("reverb") {
                output.info("Reverb: install package; listener follows REVERB_ENABLED.\n");
            }
            extensions.report(output);
            let manifest = read(&root.join("Cargo.toml"))?;
            for feature in extensions.features() {
                super::install_routes_command::enable_feature(&manifest, feature)?;
            }
            let mut paths = vec![
                env_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(".env"),
                "Cargo.toml",
                "bootstrap/app.rs",
                "routes/mod.rs",
            ];
            if extensions.has("api") {
                paths.extend([
                    "routes/api.rs",
                    "config/api.rs",
                    "app/models/user.rs",
                    "app/providers/app_service_provider.rs",
                ]);
            }
            if extensions.has("broadcasting") {
                paths.push("routes/channels.rs");
            }
            if extensions.has("grpc") {
                paths.push("routes/grpc.rs");
            }
            for name in ["reverb", "oidc"] {
                if extensions.has(name) {
                    paths.push(if name == "reverb" {
                        "config/reverb.rs"
                    } else {
                        "config/oidc.rs"
                    });
                    paths.push("config/mod.rs");
                }
            }
            if extensions.has("passport") {
                paths.extend([
                    "config/passport.rs",
                    "config/auth.rs",
                    "storage/oauth-public.key",
                    "storage/oauth-private.key",
                ]);
            }
            if extensions.has("fortify") {
                paths.extend([
                    "config/fortify.rs",
                    "bootstrap/providers.rs",
                    "app/actions/fortify",
                    "app/providers/fortify_service_provider.rs",
                    "app/models/user.rs",
                ]);
            }
            if extensions.has("passport") || extensions.has("fortify") {
                output.info("Package migrations: publish missing migration identities; preserve every historical migration. Existing APP_KEY and Passport key pairs are always preserved.\n");
            }
            for path in paths {
                let action = if root.join(path).exists() {
                    if force
                        && (path == "routes/api.rs"
                            || path == "config/passport.rs"
                            || path == "config/fortify.rs"
                            || path == "config/reverb.rs"
                            || path == "config/oidc.rs"
                            || path.starts_with("app/actions/fortify")
                            || path == "app/providers/fortify_service_provider.rs")
                    {
                        "replace scaffolding"
                    } else {
                        "preserve existing values / integrate"
                    }
                } else {
                    "create / register"
                };
                output.info(&format!("  {path}: {action}\n"));
            }
            if dry {
                output.info("Dry run: no files, keys, database connections, builds or migrations were changed.\n");
                return Ok(0);
            }
            let production = value(&values, "APP_ENV", "production") == "production";
            if !without_migration && !crate::console::confirm_production(production, matches)? {
                return Err(ConsoleError::new(
                    "Initialization cancelled: production migrations require confirmation or --force. No changes were made.",
                ));
            }
            let migration_force = force || production;
            if interactive
                && !confirm(
                    "Apply this configuration and initialize the application?",
                    true,
                )?
            {
                return Err(ConsoleError::new(
                    "Initialization cancelled; no changes were made.",
                ));
            }
            let mut source_lock = lock_manifest(&root)?;
            loop {
                match verify_connections(&root, &values).await {
                    Ok(()) => break,
                    Err(error) if interactive => {
                        eprintln!("{error}");
                        if !confirm("Edit connection settings and retry?", true)? {
                            return Err(error);
                        }
                        configure(&mut values, extensions.has("reverb"))?;
                        validate_choices(&values)?;
                        extensions.validate(&values)?;
                        extensions.validate_application(&root, force, &values)?;
                    }
                    Err(error) => return Err(error),
                }
            }
            if extensions.has("reverb") {
                prepare_reverb(&mut values)?;
            }
            if extensions.has("passkeys") && !values.contains_key("APP_URL") {
                let origin = format!("http://localhost:{}", value(&values, "APP_PORT", "8000"));
                values.insert("APP_URL".to_owned(), origin);
            }
            if key_missing(&values) {
                values.insert("APP_KEY".to_owned(), String::new());
            }
            let updated = update_environment(&source, &values)?;
            if env_path.exists() {
                let mut editor = crate::config::EnvironmentFile::open(&env_path)
                    .and_then(|file| file.edit())
                    .map_err(|e| ConsoleError::new(e.to_string()))?;
                if editor.original() != source {
                    return Err(ConsoleError::new("环境文件已在初始化期间变化，请重新执行"));
                }
                editor
                    .replace_content(updated)
                    .map_err(|e| ConsoleError::new(e.to_string()))?;
                editor
                    .save()
                    .map_err(|e| ConsoleError::new(e.to_string()))?;
            } else {
                create_environment(&env_path, &updated)?;
            }
            let manifest_path = root.join("Cargo.toml");
            for feature in extensions.features() {
                let previous = read(&manifest_path)?;
                if let Some(updated) =
                    super::install_routes_command::enable_feature(&previous, feature)?
                {
                    replace(&manifest_path, &previous, &updated)?;
                }
            }
            let timestamp = super::vendor_publish_command::migration_publication_timestamp(app)?;
            for event in initialize_routes(&root, force, timestamp, &extensions.names)? {
                crate::support::facades::Event::dispatch(event)
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
            }
            // Clear cached configuration and generate a missing APP_KEY before package boot.
            let current =
                std::env::current_exe().map_err(|error| ConsoleError::new(error.to_string()))?;
            run(
                &current,
                &root,
                &["config:clear", "--no-interaction"],
                environment.as_deref(),
            )?;
            if key_missing(&values) {
                run(
                    &current,
                    &root,
                    &["key:generate", "--no-interaction"],
                    environment.as_deref(),
                )?;
            }
            let registry = app
                .make::<super::installation::ExtensionInstallers>()
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))?;
            let services = extensions.services();
            if !services.is_empty() {
                if services.iter().all(|name| registry.get(name).is_some()) {
                    let installation =
                        super::installation::SourceInstallation::from_lock(app, source_lock);
                    install_resources(&installation, &extensions, force, output).await?;
                    source_lock = installation.into_lock();
                } else {
                    output.info("Loading selected package installers in one bootstrap build.\n");
                    let build_root = root.clone();
                    let bootstrap =
                        tokio::task::spawn_blocking(move || build_application(&build_root))
                            .await
                            .map_err(|error| ConsoleError::new(error.to_string()))??;
                    // The new process acquires the same fail-fast lock; no nested installer locks.
                    drop(source_lock);
                    let selection = extensions.names.join(",");
                    let mut args = vec![
                        "init:resources",
                        "--extensions",
                        &selection,
                        "--no-interaction",
                    ];
                    if force {
                        args.push("--force");
                    }
                    run(&bootstrap, &root, &args, environment.as_deref())?;
                    source_lock = lock_manifest(&root)?;
                }
            }
            let _source_lock = source_lock;
            // One final rebuild for all generated modules before the single migration pass.
            let build_root = root.clone();
            let executable = tokio::task::spawn_blocking(move || build_application(&build_root))
                .await
                .map_err(|e| ConsoleError::new(e.to_string()))??;
            let command_root = root.clone();
            tokio::task::spawn_blocking(move || {
                // Cached configuration must not hide settings just confirmed by the developer.
                run(
                    &executable,
                    &command_root,
                    &["config:clear", "--no-interaction"],
                    environment.as_deref(),
                )?;
                if !without_migration {
                    let mut args = vec!["migrate"];
                    if migration_force {
                        args.push("--force");
                    }
                    if !interactive {
                        args.push("--no-interaction");
                    }
                    run(&executable, &command_root, &args, environment.as_deref())?;
                }
                Ok::<_, ConsoleError>(())
            })
            .await
            .map_err(|e| ConsoleError::new(e.to_string()))??;
            if without_migration {
                output.comment("Configuration and installation prepared; migrations are pending. Initialization is not yet complete.\n");
            } else {
                output.info(
                    "Application initialized for the selected profile. Start it with cargo run.\n",
                );
            }
            if value(&values, "SMS_DRIVER", "log") != "log" {
                output.comment("SMS credentials are configured; no SMS was sent. Provider account, sender identity, templates and handset delivery still require verification.\n");
            }
            Ok(0)
        })
    }
}

const DEFAULT_EXTENSIONS: &str = "api,broadcasting,reverb,grpc";
const AUTH_EXTENSIONS: &str = "passport,fortify,passkeys,socialite,precognition,oidc,sso";

struct Extensions {
    names: Vec<String>,
}

impl Extensions {
    fn parse(selection: Option<&str>) -> Result<Self, ConsoleError> {
        let selection = selection.unwrap_or("all");
        let mut names = Vec::new();
        for name in selection.split(',').map(str::trim) {
            if name == "all" {
                if selection.trim() != "all" {
                    return Err(ConsoleError::new("Use --extensions=all on its own."));
                }
                names.extend(DEFAULT_EXTENSIONS.split(',').map(str::to_owned));
                names.extend(AUTH_EXTENSIONS.split(',').map(str::to_owned));
                break;
            }
            if !DEFAULT_EXTENSIONS.split(',').any(|known| known == name)
                && !AUTH_EXTENSIONS.split(',').any(|known| known == name)
            {
                return Err(ConsoleError::new(format!(
                    "Unknown initialization extension `{name}`. Supported: {DEFAULT_EXTENSIONS},{AUTH_EXTENSIONS}. No changes were made."
                )));
            }
            if !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        }
        if names.iter().any(|name| name == "reverb")
            && !names.iter().any(|name| name == "broadcasting")
        {
            names.push("broadcasting".to_owned());
        }
        for (extension, dependencies) in [
            ("passport", &["api"][..]),
            ("passkeys", &["fortify"][..]),
            ("sso", &["oidc", "fortify"][..]),
        ] {
            if names.iter().any(|name| name == extension) {
                for dependency in dependencies {
                    if !names.iter().any(|name| name == dependency) {
                        names.push((*dependency).to_owned());
                    }
                }
            }
        }
        Ok(Self { names })
    }

    fn validate_application(
        &self,
        root: &Path,
        force: bool,
        env: &BTreeMap<String, String>,
    ) -> Result<(), ConsoleError> {
        if self.has("passkeys") && (force || !root.join("config/fortify.rs").exists()) {
            let fallback = format!("http://localhost:{}", value(env, "APP_PORT", "8000"));
            let origin = value(env, "APP_URL", &fallback);
            let valid = url::Url::parse(origin).is_ok_and(|url| {
                matches!(url.host(), Some(url::Host::Domain(host)) if !host.is_empty())
                    && (url.scheme() == "https"
                        || (url.scheme() == "http" && url.host_str() == Some("localhost")))
                    && url.origin().ascii_serialization() == origin
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none()
            });
            if !valid {
                return Err(ConsoleError::new(
                    "Passkeys requires an exact HTTPS APP_URL origin with a domain name (no path); HTTP localhost is allowed for development, for example APP_URL=http://localhost:8000. Correct APP_URL or omit passkeys from --extensions. No changes were made.",
                ));
            }
        }
        Ok(())
    }

    fn features(&self) -> Vec<&str> {
        [
            ("api", "sanctum"),
            ("reverb", "reverb"),
            ("passport", "passport"),
            ("fortify", "fortify"),
            ("socialite", "socialite"),
            ("oidc", "oidc"),
        ]
        .into_iter()
        .filter(|(extension, _)| self.has(extension))
        .map(|(_, feature)| feature)
        .collect()
    }
    fn services(&self) -> Vec<&str> {
        ["passport", "fortify", "oidc", "reverb"]
            .into_iter()
            .filter(|name| self.has(name))
            .collect()
    }
    fn report(&self, output: &mut Output) {
        if self.has("passport") {
            output.info("Passport: install keys, migrations, User token support and the independent api guard; /user remains auth:sanctum. Register OAuth clients for your consumers.\n");
        }
        if self.has("fortify") {
            output.info("Fortify / Passkeys: install authentication actions, provider, configuration and migrations. New configuration uses views=false and email-verification; Passkeys is enabled when selected. Existing custom configuration is preserved. Configure the frontend password.reset route or ResetPassword::create_url_using. Verify mail delivery and HTTPS / exact WebAuthn origins before production use.\n");
        }
        if self.has("socialite") {
            output.comment("Socialite: enable package; external configuration pending: provider credentials, callbacks and explicit local account binding.\n");
        }
        if self.has("precognition") {
            output.info("Precognition middleware is available; apply precognitive to the routes you choose to validate.\n");
        }
        if self.has("oidc") || self.has("sso") {
            output.comment("OIDC / SSO: enable package; external configuration pending: issuer, client credentials, registered callbacks, local account binding and application routes. This does not configure a production identity provider or global logout.\n");
        }
    }

    fn has(&self, name: &str) -> bool {
        self.names.iter().any(|selected| selected == name)
    }

    fn validate(&self, env: &BTreeMap<String, String>) -> Result<(), ConsoleError> {
        if !self.has("reverb")
            && (reverb_enabled(env) || value(env, "BROADCAST_CONNECTION", "log") == "reverb")
        {
            return Err(ConsoleError::new(
                "Reverb is configured but was not selected. Include reverb in --extensions or disable its listener and broadcast connection. No changes were made.",
            ));
        }
        Ok(())
    }
}

fn value<'a>(env: &'a BTreeMap<String, String>, key: &str, default: &'a str) -> &'a str {
    env.get(key).map(String::as_str).unwrap_or(default)
}
fn parse_environment(source: &str) -> Result<BTreeMap<String, String>, ConsoleError> {
    dotenvy::from_read_iter(source.trim_start_matches('\u{feff}').as_bytes())
        .map(|entry| {
            entry.map_err(|_| {
                ConsoleError::new("Invalid .env syntax; correct the configuration and retry.")
            })
        })
        .collect()
}
fn update_environment(
    source: &str,
    values: &BTreeMap<String, String>,
) -> Result<String, ConsoleError> {
    let original = parse_environment(source)?;
    crate::config::render_environment(
        source,
        values
            .iter()
            .filter(|(key, value)| {
                // 不改写未变更项，保留原文件中的变量引用与引号。
                original.get(*key) != Some(*value)
                    || (key.as_str() == "APP_KEY" && value.is_empty())
            })
            .map(|(key, value)| {
                (
                    key.as_str(),
                    value.as_str(),
                    key == "APP_KEY" && value.is_empty(),
                )
            }),
    )
    .map_err(|e| ConsoleError::new(e.to_string()))
}
fn create_environment(path: &Path, content: &str) -> Result<(), ConsoleError> {
    crate::config::EnvironmentFile::create(path, content)
        .map(|_| ())
        .map_err(|e| ConsoleError::new(e.to_string()))
}
fn prompt(
    env: &mut BTreeMap<String, String>,
    key: &str,
    default: &str,
    secret: bool,
) -> Result<(), ConsoleError> {
    let current = value(env, key, default).to_owned();
    let term = ::console::Term::stderr();
    let label = if secret {
        format!("{key} [Enter keeps existing value]: ")
    } else {
        format!("{key} [{current}]: ")
    };
    term.write_str(&label)
        .map_err(|e| ConsoleError::new(e.to_string()))?;
    let entered = if secret {
        term.read_secure_line()
    } else {
        term.read_line()
    }
    .map_err(|e| ConsoleError::new(e.to_string()))?;
    if entered.is_empty() && current.is_empty() && !env.contains_key(key) {
        return Ok(());
    }
    env.insert(
        key.to_owned(),
        if entered.is_empty() { current } else { entered },
    );
    Ok(())
}
fn confirm(question: &str, default: bool) -> Result<bool, ConsoleError> {
    let term = ::console::Term::stderr();
    term.write_str(&format!(
        "{question} {} ",
        if default { "[Y/n]" } else { "[y/N]" }
    ))
    .map_err(|e| ConsoleError::new(e.to_string()))?;
    let answer = term
        .read_line()
        .map_err(|e| ConsoleError::new(e.to_string()))?;
    Ok(if answer.trim().is_empty() {
        default
    } else {
        matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
    })
}
fn configure(env: &mut BTreeMap<String, String>, reverb: bool) -> Result<(), ConsoleError> {
    for (key, default) in [
        ("APP_NAME", "App"),
        ("APP_URL", "http://127.0.0.1:8000"),
        ("APP_TIMEZONE", "Asia/Shanghai"),
        ("APP_LOCALE", "zh-Hans"),
        ("DB_CONNECTION", "mysql"),
    ] {
        prompt(env, key, default, false)?;
    }
    if value(env, "DB_CONNECTION", "sqlite") != "sqlite" {
        let port = if value(env, "DB_CONNECTION", "") == "pgsql" {
            "5432"
        } else {
            "3306"
        };
        for (key, default, secret) in [
            ("DB_HOST", "127.0.0.1", false),
            ("DB_PORT", port, false),
            ("DB_DATABASE", "openrust", false),
            ("DB_USERNAME", "root", false),
            ("DB_PASSWORD", "", true),
        ] {
            prompt(env, key, default, secret)?;
        }
    }
    for (key, default) in [
        ("CACHE_STORE", "database"),
        ("SESSION_DRIVER", "database"),
        ("QUEUE_CONNECTION", "database"),
        ("MAIL_MAILER", "log"),
        ("SMS_DRIVER", "log"),
    ] {
        prompt(env, key, default, false)?;
    }
    if value(env, "MAIL_MAILER", "log") == "smtp" {
        for (key, default, secret) in [
            ("MAIL_HOST", "127.0.0.1", false),
            ("MAIL_PORT", "2525", false),
            ("MAIL_SCHEME", "smtp", false),
            ("MAIL_USERNAME", "null", false),
            ("MAIL_PASSWORD", "null", true),
            ("MAIL_FROM_ADDRESS", "hello@example.com", false),
        ] {
            prompt(env, key, default, secret)?;
        }
    }
    for key in sms_credential_keys(value(env, "SMS_DRIVER", "log")) {
        let secret = key.contains("SECRET") || key.ends_with("KEY") || key.ends_with("PASSWORD");
        prompt(env, key, "", secret)?;
    }
    if reverb {
        prompt(env, "REVERB_ENABLED", "false", false)?;
    }
    if reverb && reverb_enabled(env) {
        for (key, default) in [
            ("REVERB_SERVER_HOST", "127.0.0.1"),
            ("REVERB_SERVER_PORT", "8080"),
            ("REVERB_HOST", "127.0.0.1"),
            ("REVERB_PORT", "8080"),
            ("REVERB_SCHEME", "http"),
            ("BROADCAST_CONNECTION", "reverb"),
        ] {
            prompt(env, key, default, false)?;
        }
    }
    if uses_redis(env) {
        for (key, default, secret) in [
            ("REDIS_HOST", "127.0.0.1", false),
            ("REDIS_PORT", "6379", false),
            ("REDIS_USERNAME", "", false),
            ("REDIS_PASSWORD", "", true),
            ("REDIS_DB", "0", false),
        ] {
            prompt(env, key, default, secret)?;
        }
    }
    Ok(())
}
fn validate_choices(env: &BTreeMap<String, String>) -> Result<(), ConsoleError> {
    let cipher = value(env, "APP_CIPHER", "AES-256-CBC");
    cipher
        .parse::<crate::encryption::Cipher>()
        .map_err(|_| ConsoleError::new("Unsupported APP_CIPHER."))?;
    if !key_missing(env) {
        let previous = value(env, "APP_PREVIOUS_KEYS", "")
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(str::trim)
            .collect::<Vec<_>>();
        let repository = crate::config::Repository::new(BTreeMap::from([(
            "app".to_owned(),
            serde_json::json!({"key":value(env,"APP_KEY",""),"cipher":cipher,"previous_keys":previous}),
        )]));
        crate::encryption::resolve_encrypter(&repository).map_err(|_| ConsoleError::new("APP_KEY or APP_PREVIOUS_KEYS is invalid for APP_CIPHER. Existing keys were preserved; correct the configuration before initializing."))?;
    }
    for (key, default, allowed) in [
        (
            "DB_CONNECTION",
            "sqlite",
            &["sqlite", "mysql", "mariadb", "pgsql"][..],
        ),
        (
            "CACHE_STORE",
            "database",
            &["database", "file", "redis", "array"][..],
        ),
        (
            "SESSION_DRIVER",
            "database",
            &["database", "file", "redis", "cookie", "array"][..],
        ),
        (
            "QUEUE_CONNECTION",
            "database",
            &["database", "redis", "sync"][..],
        ),
        ("MAIL_MAILER", "log", &["log", "smtp"][..]),
        (
            "SMS_DRIVER",
            "log",
            &[
                "log", "aliyun", "tencent", "twilio", "vonage", "smsbao", "yunpian",
            ][..],
        ),
    ] {
        if !allowed.contains(&value(env, key, default)) {
            return Err(ConsoleError::new(format!(
                "{key}: this initialization profile does not support the configured driver. Existing configuration was preserved; configure it separately or choose a supported initialization driver."
            )));
        }
    }
    for key in sms_credential_keys(value(env, "SMS_DRIVER", "log")) {
        if value(env, key, "").is_empty()
            || matches!(
                value(env, key, "").to_ascii_lowercase().as_str(),
                "null" | "(null)"
            )
        {
            return Err(ConsoleError::new(format!(
                "Missing {key} for the selected SMS driver; configure credentials or choose log."
            )));
        }
    }
    if !matches!(
        value(env, "REVERB_ENABLED", "false")
            .to_ascii_lowercase()
            .as_str(),
        "true" | "false" | "(true)" | "(false)" | "1" | "0"
    ) {
        return Err(ConsoleError::new("REVERB_ENABLED must be a boolean."));
    }
    Ok(())
}

fn sms_credential_keys(driver: &str) -> &'static [&'static str] {
    match driver {
        "aliyun" => &["SMS_ALIYUN_ACCESS_KEY_ID", "SMS_ALIYUN_ACCESS_KEY_SECRET"],
        "tencent" => &[
            "SMS_TENCENT_SECRET_ID",
            "SMS_TENCENT_SECRET_KEY",
            "SMS_TENCENT_SDK_APP_ID",
        ],
        "twilio" => &[
            "SMS_TWILIO_ACCOUNT_SID",
            "SMS_TWILIO_API_KEY_SID",
            "SMS_TWILIO_API_KEY_SECRET",
        ],
        "vonage" => &[
            "SMS_VONAGE_API_KEY",
            "SMS_VONAGE_API_SECRET",
            "SMS_VONAGE_FROM",
        ],
        "smsbao" => &["SMS_SMSBAO_USERNAME", "SMS_SMSBAO_API_KEY"],
        "yunpian" => &["SMS_YUNPIAN_API_KEY"],
        _ => &[],
    }
}
fn reverb_enabled(env: &BTreeMap<String, String>) -> bool {
    matches!(
        value(env, "REVERB_ENABLED", "false")
            .to_ascii_lowercase()
            .as_str(),
        "true" | "(true)" | "1"
    )
}
fn prepare_reverb(env: &mut BTreeMap<String, String>) -> Result<(), ConsoleError> {
    if reverb_enabled(env) {
        for (key, default) in [
            ("REVERB_MODE", "embedded"),
            ("REVERB_SERVER_HOST", "127.0.0.1"),
            ("REVERB_SERVER_PORT", "8080"),
            ("REVERB_HOST", "127.0.0.1"),
            ("REVERB_PORT", "8080"),
            ("REVERB_SCHEME", "http"),
        ] {
            env.entry(key.to_owned())
                .or_insert_with(|| default.to_owned());
        }
        for key in ["REVERB_APP_ID", "REVERB_APP_KEY", "REVERB_APP_SECRET"] {
            if value(env, key, "").is_empty()
                || matches!(
                    value(env, key, "").to_ascii_lowercase().as_str(),
                    "null" | "(null)"
                )
            {
                let bytes = crate::encryption::Encrypter::generate_key(
                    crate::encryption::Cipher::Aes256Cbc,
                )
                .map_err(|_| {
                    ConsoleError::new("Could not generate Reverb application credentials.")
                })?;
                let generated = bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                env.insert(key.to_owned(), generated);
            }
        }
    }
    Ok(())
}
fn key_missing(env: &BTreeMap<String, String>) -> bool {
    let key = value(env, "APP_KEY", "");
    key.is_empty() || matches!(key.to_ascii_lowercase().as_str(), "null" | "(null)")
}
fn uses_redis(env: &BTreeMap<String, String>) -> bool {
    ["CACHE_STORE", "SESSION_DRIVER", "QUEUE_CONNECTION"]
        .iter()
        .any(|k| value(env, k, "") == "redis")
}
async fn verify_connections(
    root: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), ConsoleError> {
    use sqlx::Connection as _;
    let driver = value(env, "DB_CONNECTION", "sqlite");
    let query = async {
        match driver {
            "mysql" | "mariadb" => {
                let port = value(env, "DB_PORT", "3306")
                    .parse::<u16>()
                    .map_err(|_| ())?;
                let fields = sqlx::mysql::MySqlConnectOptions::new()
                    .host(value(env, "DB_HOST", "127.0.0.1"))
                    .port(port)
                    .database(value(env, "DB_DATABASE", "openrust"))
                    .username(value(env, "DB_USERNAME", "root"))
                    .password(value(env, "DB_PASSWORD", ""));
                let opts = match env
                    .get("DB_URL")
                    .filter(|s| !s.is_empty() && s.as_str() != "null")
                {
                    Some(url) => url
                        .parse::<sqlx::mysql::MySqlConnectOptions>()
                        .map_err(|_| ())?,
                    None => fields,
                };
                let mut c = sqlx::MySqlConnection::connect_with(&opts)
                    .await
                    .map_err(|_| ())?;
                sqlx::query("SELECT 1")
                    .execute(&mut c)
                    .await
                    .map_err(|_| ())?;
                c.close().await.map_err(|_| ())?;
            }
            "pgsql" => {
                let port = value(env, "DB_PORT", "5432")
                    .parse::<u16>()
                    .map_err(|_| ())?;
                let fields = sqlx::postgres::PgConnectOptions::new()
                    .host(value(env, "DB_HOST", "127.0.0.1"))
                    .port(port)
                    .database(value(env, "DB_DATABASE", "openrust"))
                    .username(value(env, "DB_USERNAME", "postgres"))
                    .password(value(env, "DB_PASSWORD", ""));
                let opts = match env
                    .get("DB_URL")
                    .filter(|s| !s.is_empty() && s.as_str() != "null")
                {
                    Some(url) => url
                        .parse::<sqlx::postgres::PgConnectOptions>()
                        .map_err(|_| ())?,
                    None => fields,
                };
                let mut c = sqlx::PgConnection::connect_with(&opts)
                    .await
                    .map_err(|_| ())?;
                sqlx::query("SELECT 1")
                    .execute(&mut c)
                    .await
                    .map_err(|_| ())?;
                c.close().await.map_err(|_| ())?;
            }
            _ => {
                let filename = value(env, "DB_DATABASE", "database/database.sqlite");
                let path = if filename == ":memory:" {
                    PathBuf::from(filename)
                } else {
                    root.join(filename)
                };
                if filename != ":memory:"
                    && let Some(parent) = path.parent()
                {
                    fs::create_dir_all(parent).map_err(|_| ())?;
                }
                let opts = sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(true);
                let c = sqlx::SqliteConnection::connect_with(&opts)
                    .await
                    .map_err(|_| ())?;
                c.close().await.map_err(|_| ())?;
            }
        }
        Ok::<(), ()>(())
    };
    tokio::time::timeout(std::time::Duration::from_secs(10),query).await.map_err(|_| ConsoleError::new("Database connection timed out; check DB_* and retry."))?.map_err(|()| ConsoleError::new("Database connection check failed; verify DB_CONNECTION, DB_HOST, DB_PORT, DB_DATABASE, DB_USERNAME, DB_PASSWORD and server TLS requirements."))?;
    if value(env, "MAIL_MAILER", "log") == "smtp" {
        let optional = |name: &str| {
            env.get(name)
                .filter(|s| !matches!(s.to_ascii_lowercase().as_str(), "null" | "(null)"))
                .map(|s| serde_json::json!(s))
                .unwrap_or_default()
        };
        let port = value(env, "MAIL_PORT", "2525")
            .parse::<u16>()
            .map_err(|_| ConsoleError::new("Invalid MAIL_PORT."))?;
        let settings = serde_json::json!({"scheme":optional("MAIL_SCHEME"),"url":optional("MAIL_URL"),"host":value(env,"MAIL_HOST","127.0.0.1"),"port":port,"username":optional("MAIL_USERNAME"),"password":optional("MAIL_PASSWORD"),"timeout":10,"local_domain":optional("MAIL_EHLO_DOMAIN")});
        tokio::time::timeout(std::time::Duration::from_secs(15),crate::mail::transport::SmtpTransport::verify(settings)).await
            .map_err(|_| ConsoleError::new("SMTP verification timed out."))?
            .map_err(|_| ConsoleError::new("SMTP verification failed; check MAIL_HOST, MAIL_PORT, MAIL_SCHEME and credentials. No test email was sent."))?;
    }
    if uses_redis(env) {
        let port = value(env, "REDIS_PORT", "6379")
            .parse::<u16>()
            .map_err(|_| ConsoleError::new("Invalid REDIS_PORT."))?;
        let db = value(env, "REDIS_DB", "0")
            .parse::<i64>()
            .map_err(|_| ConsoleError::new("Invalid REDIS_DB."))?;
        let cache_db = value(env, "REDIS_CACHE_DB", value(env, "REDIS_DB", "0"))
            .parse::<i64>()
            .map_err(|_| ConsoleError::new("Invalid REDIS_CACHE_DB."))?;
        let optional = |name: &str| {
            env.get(name)
                .filter(|s| !matches!(s.to_ascii_lowercase().as_str(), "null" | "(null)"))
                .map(|s| serde_json::json!(s))
                .unwrap_or_default()
        };
        let mut default = serde_json::json!({"host":value(env,"REDIS_HOST","127.0.0.1"),"port":port,"database":db,"url":optional("REDIS_URL"),"username":optional("REDIS_USERNAME"),"password":optional("REDIS_PASSWORD")});
        let mut cache = default.clone();
        cache["database"] = serde_json::json!(cache_db);
        let repository = crate::config::Repository::new(BTreeMap::from([(
            "database".to_owned(),
            serde_json::json!({"redis":{"default":default.take(),"cache":cache}}),
        )]));
        let manager = crate::redis::RedisManager::new(std::sync::Arc::new(repository));
        let mut names = Vec::new();
        if value(env, "CACHE_STORE", "") == "redis" {
            names.push(value(env, "REDIS_CACHE_CONNECTION", "cache"));
            names.push(value(env, "REDIS_CACHE_LOCK_CONNECTION", "default"));
        }
        if value(env, "QUEUE_CONNECTION", "") == "redis" {
            names.push(value(env, "REDIS_QUEUE_CONNECTION", "default"));
        }
        if value(env, "SESSION_DRIVER", "") == "redis" {
            let name = value(env, "SESSION_CONNECTION", "default");
            names.push(
                if matches!(name.to_ascii_lowercase().as_str(), "null" | "(null)") {
                    "default"
                } else {
                    name
                },
            );
        }
        for name in names {
            let connection=manager.connection(Some(name)).map_err(|_| ConsoleError::new("Unsupported Redis connection configuration; check selected connection names and REDIS_* settings."))?;
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                connection.command("PING", &[]),
            )
            .await
            .map_err(|_| ConsoleError::new("Redis connection timed out."))?
            .map_err(|_| {
                ConsoleError::new("Redis connection check failed; verify REDIS_* and retry.")
            })?;
        }
    }
    Ok(())
}
fn run(
    executable: &Path,
    root: &Path,
    args: &[&str],
    environment: Option<&str>,
) -> Result<(), ConsoleError> {
    let mut command = Process::new(executable);
    command.current_dir(root).arg("artisan").args(args);
    if let Some(environment) = environment {
        command.args(["--env", environment]);
    }
    let status = command
        .status()
        .map_err(|e| ConsoleError::new(format!("Could not start {}: {e}", args[0])))?;
    if !status.success() {
        return Err(ConsoleError::new(format!(
            "{} failed ({status}); correct the issue and run artisan init again. Completed files were preserved.",
            args[0]
        )));
    }
    Ok(())
}

struct ResourcesCommand(CommandDefinition);
impl Command for ResourcesCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let extensions = Extensions::parse(
                input
                    .matches()
                    .get_one::<String>("extensions")
                    .map(String::as_str),
            )?;
            let installation = super::installation::SourceInstallation::new(app)?;
            install_resources(
                &installation,
                &extensions,
                input.matches().get_flag("force"),
                output,
            )
            .await?;
            Ok(0)
        })
    }
}
async fn install_resources(
    installation: &super::installation::SourceInstallation<'_>,
    extensions: &Extensions,
    force: bool,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    let registry = installation
        .application()
        .make::<super::installation::ExtensionInstallers>()
        .await
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    // Resolve every required service before publishing any package resources.
    let services = extensions.services().into_iter().map(|name| registry.get(name).ok_or_else(|| ConsoleError::new(format!("The {name} installer is unavailable. Enable the corresponding application feature and package discovery, then retry init.")))).collect::<Result<Vec<_>, _>>()?;
    for service in services {
        service(
            installation,
            super::installation::InstallationOptions {
                force,
                extensions: &extensions.names,
            },
            output,
        )
        .await?;
    }
    Ok(())
}
