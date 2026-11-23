use std::{
    fs::{self, OpenOptions},
    io::{self, IsTerminal as _, Write as _},
    path::Path,
};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::foundation::Application as _,
    foundation::{Application, events::VendorTagPublished},
    support::facades::Event,
};

use super::{
    source_edit::{file_error, insert_before, lock_manifest, read, replace},
    vendor_publish_command::{
        migration_publication_timestamp, publish_package_if_migration_missing,
    },
};

struct InstallSpec {
    command: &'static str,
    description: &'static str,
    module: &'static str,
    module_anchor: &'static str,
    registration: &'static str,
    registration_methods: &'static [&'static str],
    equivalent: Option<&'static str>,
    bootstrap_anchor: &'static str,
    stub: &'static [u8],
    next_steps: &'static str,
}

const INSTALLERS: &[InstallSpec] = &[
    InstallSpec {
        command: "install:api",
        description: "Install the application API route scaffolding",
        module: "api",
        module_anchor: "console",
        registration: ".api(routes::api::register)",
        registration_methods: &[".api("],
        equivalent: None,
        bootstrap_anchor: ".commands(",
        stub: include_bytes!("stubs/api-routes.stub"),
        next_steps: "Sanctum is enabled for the next Cargo build.",
    },
    InstallSpec {
        command: "install:broadcasting",
        description: "Install the broadcasting channel scaffolding",
        module: "channels",
        module_anchor: "console",
        registration: ".channels(routes::channels::register)",
        registration_methods: &[".channels(", ".with_broadcasting("],
        equivalent: Some(".with_broadcasting(routes::channels::register)"),
        bootstrap_anchor: ".health(",
        stub: include_bytes!("stubs/broadcasting-routes.stub"),
        next_steps: "Configure a broadcast driver, add your channel rules, and rebuild the application.",
    },
    InstallSpec {
        command: "install:grpc",
        description: "Install the application gRPC scaffolding",
        module: "grpc",
        module_anchor: "web",
        registration: ".with_grpc(routes::grpc::register)",
        registration_methods: &[".with_grpc("],
        equivalent: None,
        bootstrap_anchor: ".with_middleware(",
        stub: include_bytes!("stubs/grpc-routes.stub"),
        next_steps: "Register your gRPC services, rebuild, and set GRPC_HOST and GRPC_PORT to enable the listener.",
    },
];

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for spec in INSTALLERS {
        let mut syntax = clap::Command::new(spec.command).about(spec.description);
        if spec.command == "install:api" {
            syntax = syntax
                .arg(
                    clap::Arg::new("passport")
                        .long("passport")
                        .action(clap::ArgAction::SetTrue)
                        .help("Install Passport instead of Sanctum"),
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .action(clap::ArgAction::SetTrue)
                        .help("Overwrite an existing API routes file"),
                )
                .arg(
                    clap::Arg::new("without-migration-prompt")
                        .long("without-migration-prompt")
                        .action(clap::ArgAction::SetTrue)
                        .help("Do not prompt to run pending migrations"),
                );
        }
        artisan.add_command(Box::new(InstallRoutesCommand {
            definition: CommandDefinition::new(syntax),
            spec,
        }))?;
    }
    Ok(())
}

struct InstallRoutesCommand {
    definition: CommandDefinition,
    spec: &'static InstallSpec,
}

impl Command for InstallRoutesCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.definition
    }

    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let root = app.base_path().to_owned();
            let spec = self.spec;
            let api = spec.command == "install:api";
            let force = api && input.matches().get_flag("force");
            let passport = api && input.matches().get_flag("passport");
            let timestamp = if api {
                migration_publication_timestamp(app)?
            } else {
                None
            };
            let result = tokio::task::spawn_blocking(move || {
                install(&root, spec, force, timestamp, passport)
            })
            .await
            .map_err(|error| ConsoleError::new(error.to_string()))??;
            if let Some(event) = result.publication {
                Event::dispatch(event)
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
            }
            if api && result.existing_routes && !force {
                output.stderr.push_str("API routes file already exists.\n");
            }
            let manual = !result.warnings.is_empty();
            for warning in result.warnings {
                output.stderr.push_str(&format!("Warning: {warning}\n"));
            }
            if passport {
                if manual {
                    output.error("Passport source integration is incomplete. Resolve the warnings before rebuilding and running passport:install.");
                    return Ok(1);
                }
                let root = app.base_path().to_owned();
                let skip = input.matches().get_flag("without-migration-prompt");
                let noninteractive = input.matches().get_flag("no-interaction");
                let environment = input
                    .matches()
                    .get_many::<String>("env")
                    .and_then(|mut v| v.next())
                    .cloned();
                tokio::task::spawn_blocking(move || {
                    install_passport_after_rebuild(
                        &root,
                        environment.as_deref(),
                        skip,
                        noninteractive,
                    )
                })
                .await
                .map_err(|e| ConsoleError::new(e.to_string()))??;
                output.info("Passport API scaffolding is installed. Rebuilds completed; review the migration status reported above.");
                return Ok(0);
            }
            output.info(&format!(
                "routes/{}.rs is installed. {}\n",
                spec.module, spec.next_steps
            ));
            if api
                && !input.matches().get_flag("without-migration-prompt")
                && !input.matches().get_flag("no-interaction")
                && output.streams_to_terminal()
                && tokio::task::spawn_blocking(confirm_migrations)
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))??
            {
                let root = app.base_path().to_owned();
                let environment = input
                    .matches()
                    .get_many::<String>("env")
                    .and_then(|mut values| values.next())
                    .cloned();
                tokio::task::spawn_blocking(move || {
                    migrate_after_rebuild(&root, environment.as_deref())
                })
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))??;
            }
            Ok(0)
        })
    }
}

fn migrate_after_rebuild(root: &Path, environment: Option<&str>) -> Result<(), ConsoleError> {
    let executable = build_application(root)?;
    let mut command = std::process::Command::new(executable);
    command.current_dir(root).args(["artisan", "migrate"]);
    if let Some(environment) = environment {
        command.args(["--env", environment]);
    }
    let status = command
        .status()
        .map_err(|error| ConsoleError::new(format!("Could not run pending migrations: {error}")))?;
    if !status.success() {
        return Err(ConsoleError::new(format!(
            "Pending migrations failed: {status}"
        )));
    }
    Ok(())
}

pub(super) fn build_application(root: &Path) -> Result<std::path::PathBuf, ConsoleError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command
        .current_dir(root)
        .args([
            "build",
            "--quiet",
            "--locked",
            "--message-format=json-render-diagnostics",
        ])
        .stderr(std::process::Stdio::inherit());
    // Windows locks the installer executable until this command returns.
    #[cfg(windows)]
    let target = {
        let primary = root.join("target/artisan-rebuild");
        let current =
            std::env::current_exe().map_err(|error| ConsoleError::new(error.to_string()))?;
        if current.starts_with(&primary) {
            root.join("target/artisan-rebuild-next")
        } else {
            primary
        }
    };
    #[cfg(windows)]
    command.arg("--target-dir").arg(&target);
    let built = command.output().map_err(|error| {
        ConsoleError::new(format!(
            "Could not rebuild and run pending migrations: {error}"
        ))
    })?;
    if !built.status.success() {
        return Err(ConsoleError::new(format!(
            "Could not rebuild the application: {}",
            built.status
        )));
    }
    let manifest_path = root.join("Cargo.toml");
    let manifest = read(&manifest_path)?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|error| ConsoleError::new(format!("Invalid Cargo.toml: {error}")))?;
    let default_run = manifest
        .get("package")
        .and_then(toml_edit::Item::as_table_like)
        .and_then(|package| package.get("default-run"))
        .and_then(toml_edit::Item::as_str);
    let mut executables = String::from_utf8_lossy(&built.stdout)
        .lines()
        .filter_map(|line| {
            let message: serde_json::Value = serde_json::from_str(line).ok()?;
            let path = message["manifest_path"].as_str()?;
            if Path::new(path) != manifest_path
                && fs::canonicalize(path).ok()? != fs::canonicalize(&manifest_path).ok()?
            {
                return None;
            }
            let target = &message["target"];
            if !target["kind"].as_array()?.iter().any(|kind| kind == "bin")
                || default_run.is_some_and(|name| target["name"] != name)
            {
                return None;
            }
            message["executable"].as_str().map(std::path::PathBuf::from)
        })
        .collect::<Vec<_>>();
    executables.dedup();
    let [executable] = executables.as_slice() else {
        return Err(ConsoleError::new(
            "Could not identify the application binary. Set package.default-run in Cargo.toml.",
        ));
    };
    Ok(executable.clone())
}

fn confirm_migrations() -> Result<bool, ConsoleError> {
    if !io::stdin().is_terminal() {
        return Ok(false);
    }
    let mut stderr = io::stderr().lock();
    write!(
        stderr,
        "Would you like to run all pending database migrations? [Y/n] "
    )
    .and_then(|_| stderr.flush())
    .map_err(|error| ConsoleError::new(error.to_string()))?;
    let mut response = String::new();
    let count = io::stdin()
        .read_line(&mut response)
        .map_err(|error| ConsoleError::new(error.to_string()))?;
    Ok(count != 0
        && matches!(
            response.trim().to_ascii_lowercase().as_str(),
            "" | "y" | "yes"
        ))
}

struct InstallResult {
    existing_routes: bool,
    warnings: Vec<ConsoleError>,
    publication: Option<VendorTagPublished>,
}

fn install(
    root: &Path,
    spec: &InstallSpec,
    force: bool,
    timestamp: Option<jiff::civil::DateTime>,
    passport: bool,
) -> Result<InstallResult, ConsoleError> {
    let _manifest = lock_manifest(root)?;
    install_unlocked(root, spec, force, timestamp, passport)
}

fn install_unlocked(
    root: &Path,
    spec: &InstallSpec,
    force: bool,
    timestamp: Option<jiff::civil::DateTime>,
    passport: bool,
) -> Result<InstallResult, ConsoleError> {
    let routes_module = root.join("routes/mod.rs");
    let bootstrap = root.join("bootstrap/app.rs");
    let routes = root.join("routes").join(format!("{}.rs", spec.module));
    let modules = read(&routes_module)?;
    let startup = read(&bootstrap)?;
    let manifest_path = root.join("Cargo.toml");
    let api = spec.command == "install:api";
    let manifest_update = if api {
        let manifest = read(&manifest_path)?;
        let updated = if passport {
            enable_feature(&manifest, "passport")?
        } else {
            enable_sanctum(&manifest)?
        };
        updated.map(|updated| (manifest, updated))
    } else {
        None
    };
    let module = format!("mod {};", spec.module);
    let module_statement = format!("pub(crate) {module}");
    let anchor = format!("pub(crate) mod {};", spec.module_anchor);
    let mut warnings = Vec::new();
    let modules_update = if modules.lines().any(|line| {
        let line = line.trim();
        line == module
            || line == module_statement
            || line.strip_prefix("pub ") == Some(module.as_str())
    }) {
        None
    } else {
        registration_update(
            insert_before(&modules, &routes_module, &module_statement, |line| {
                line.trim() == anchor
            }),
            spec,
            &mut warnings,
        )?
    };
    let startup_update = if startup.lines().any(|line| {
        let line = line.trim();
        line == spec.registration || spec.equivalent == Some(line)
    }) {
        None
    } else {
        if startup.lines().any(|line| {
            spec.registration_methods
                .iter()
                .any(|method| line.trim_start().starts_with(method))
        }) {
            registration_update(
                Err(ConsoleError::new(format!(
                    "bootstrap/app.rs already has a custom {} registration. Add `{}` manually if needed.",
                    spec.module, spec.registration
                ))),
                spec,
                &mut warnings,
            )?
        } else {
            registration_update(
                insert_before(&startup, &bootstrap, spec.registration, |line| {
                    line.trim_start().starts_with(spec.bootstrap_anchor)
                }),
                spec,
                &mut warnings,
            )?
        }
    };

    let publication = if api && !passport {
        publish_package_if_migration_missing(
            root,
            crate::sanctum_service_provider::publishable_resources(),
            timestamp,
        )
        .map_err(|error| ConsoleError::new(error.to_string()))?
    } else {
        None
    };
    if api {
        install_api_configuration(root, &mut warnings)?;
        if passport {
            wire_passport(root, &mut warnings)?;
        } else {
            wire_sanctum(root, &mut warnings)?;
        }
    }
    let passport_stub = if passport {
        Some(
            std::str::from_utf8(spec.stub)
                .expect("API stub UTF-8")
                .replace("feature = \"sanctum\"", "feature = \"passport\"")
                .replace("auth:sanctum", "auth:api"),
        )
    } else {
        None
    };
    let stub = passport_stub
        .as_deref()
        .map(str::as_bytes)
        .unwrap_or(spec.stub);
    let existing = !create_source(&routes, stub)?;
    if existing && force {
        let previous = read(&routes)?;
        let stub = std::str::from_utf8(stub).expect("API stub is UTF-8");
        replace(&routes, &previous, stub)?;
    }
    if let Some(updated) = modules_update {
        replace(&routes_module, &modules, &updated)?;
    }
    if let Some(updated) = startup_update {
        replace(&bootstrap, &startup, &updated)?;
    }
    if let Some((previous, updated)) = manifest_update {
        replace(&manifest_path, &previous, &updated)?;
    }
    Ok(InstallResult {
        existing_routes: existing,
        warnings,
        publication,
    })
}

// The caller holds the shared source lock across the whole initialization.
pub(super) fn initialize_routes(
    root: &Path,
    force: bool,
    timestamp: Option<jiff::civil::DateTime>,
    extensions: &[String],
) -> Result<Vec<VendorTagPublished>, ConsoleError> {
    let mut publications = Vec::new();
    for spec in INSTALLERS {
        let name = if spec.module == "channels" {
            "broadcasting"
        } else {
            spec.module
        };
        if !extensions.iter().any(|selected| selected == name) {
            continue;
        }
        let result = install_unlocked(
            root,
            spec,
            force && spec.command == "install:api",
            timestamp,
            false,
        )?;
        if let Some(event) = result.publication {
            publications.push(event);
        }
        if !result.warnings.is_empty() {
            return Err(ConsoleError::new(format!(
                "{} requires manual integration; initialization is incomplete: {}",
                spec.command,
                result
                    .warnings
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ")
            )));
        }
    }
    Ok(publications)
}

fn install_api_configuration(
    root: &Path,
    warnings: &mut Vec<ConsoleError>,
) -> Result<(), ConsoleError> {
    const MODULE: &str = "mod api;";
    const SECTION: &str = r#"("api".to_owned(), api::load(env)?),"#;
    let directory = root.join("config");
    let path = directory.join("api.rs");
    fs::create_dir_all(&directory).map_err(|error| file_error(&directory, error))?;
    create_source(&path, include_bytes!("stubs/api-config.stub"))?;

    let loader = directory.join("mod.rs");
    if !loader.is_file() {
        warnings.push(ConsoleError::new(format!(
            "Cannot find {}. Declare `{MODULE}` and add `{SECTION}` to your configuration loader manually.",
            loader.display()
        )));
        return Ok(());
    }
    let source = read(&loader)?;
    let updated = insert_config_hook(&source, &loader, MODULE, |line| {
        line.trim() == "pub(crate) mod app;"
    })
    .and_then(|updated| {
        insert_config_hook(&updated, &loader, SECTION, |line| {
            line.trim_start().starts_with(r#"("auth".to_owned(),"#)
        })
    });
    match updated {
        Ok(updated) => replace(&loader, &source, &updated),
        Err(warning) => {
            warnings.push(warning);
            Ok(())
        }
    }
}

fn insert_config_hook(
    source: &str,
    path: &Path,
    statement: &str,
    anchor: impl Fn(&str) -> bool,
) -> Result<String, ConsoleError> {
    if source.lines().any(|line| line.trim() == statement) {
        Ok(source.to_owned())
    } else {
        insert_before(source, path, statement, anchor)
    }
}

fn wire_sanctum(root: &Path, warnings: &mut Vec<ConsoleError>) -> Result<(), ConsoleError> {
    const IMPL: &str = "impl openrust::sanctum::HasApiTokens for User {}";
    const USER: &str = "openrust::auth::model!(crate::app::models::User)";
    const REGISTER: &str = "openrust::auth::model!(crate::app::models::User, sanctum)";
    const CONFIG: &str = r#"("sanctum".to_owned(), sanctum::load(env)?),"#;

    let model = root.join("app/models/user.rs");
    let auth_config = root.join("config/auth.rs");
    if model.is_file() && auth_config.is_file() {
        let model_source = read(&model)?;
        let config_source = read(&auth_config)?;
        let add_impl = source_hook(&model_source, &model, IMPL, |line| {
            line.trim() == "impl Authenticatable for User {"
        });
        let add_reference = (|| {
            if config_source.contains(REGISTER) {
                return Ok(config_source.clone());
            }
            if config_source.matches(USER).count() != 1 {
                return Err(manual_token_model());
            }
            let reference = format!(
                "{{\n                    #[cfg(feature = \"sanctum\")]\n                    {{ {REGISTER} }}\n                    #[cfg(not(feature = \"sanctum\"))]\n                    {{ {USER} }}\n                }}"
            );
            Ok(config_source.replacen(USER, &reference, 1))
        })();
        match (add_impl, add_reference) {
            (Ok(model_update), Ok(config_update)) => {
                replace(&model, &model_source, &model_update)?;
                replace(&auth_config, &config_source, &config_update)?;
            }
            (model_update, config_update) => {
                warnings.extend(model_update.err());
                warnings.extend(config_update.err());
                warnings.push(manual_token_model());
            }
        }
    } else {
        warnings.push(manual_token_model());
    }

    let config = root.join("config/mod.rs");
    if root.join("config/sanctum.rs").is_file() && config.is_file() {
        let source = read(&config)?;
        let updated = source_hook(&source, &config, "mod sanctum;", |line| {
            line.trim() == "mod services;"
        })
        .and_then(|updated| {
            source_hook(&updated, &config, CONFIG, |line| {
                line.trim_start().starts_with(r#"("services".to_owned(),"#)
            })
        });
        match updated {
            Ok(updated) => replace(&config, &source, &updated)?,
            Err(warning) => warnings.push(warning),
        }
    }
    Ok(())
}

fn source_hook(
    source: &str,
    path: &Path,
    statement: &str,
    anchor: impl Fn(&str) -> bool,
) -> Result<String, ConsoleError> {
    if source.lines().any(|line| line.trim() == statement) {
        return Ok(source.to_owned());
    }
    let mut updated = insert_before(source, path, statement, anchor)?;
    updated = insert_before(&updated, path, r#"#[cfg(feature = "sanctum")]"#, |line| {
        line.trim() == statement
    })?;
    Ok(updated)
}

fn manual_token_model() -> ConsoleError {
    ConsoleError::new(
        "For a custom user model, implement openrust::sanctum::HasApiTokens and reference it with openrust::auth::model!(YourUser, sanctum) in config/auth.rs, or register it manually with AuthManager::register_sanctum_model.",
    )
}

fn registration_update(
    update: Result<String, ConsoleError>,
    spec: &InstallSpec,
    warnings: &mut Vec<ConsoleError>,
) -> Result<Option<String>, ConsoleError> {
    match update {
        Err(warning) if spec.command == "install:api" => {
            warnings.push(warning);
            Ok(None)
        }
        result => result.map(Some),
    }
}

fn enable_sanctum(source: &str) -> Result<Option<String>, ConsoleError> {
    enable_feature(source, "sanctum")
}

pub(super) fn enable_feature(source: &str, feature: &str) -> Result<Option<String>, ConsoleError> {
    use toml_edit::{Array, DocumentMut, Item};

    let invalid = || {
        ConsoleError::new(format!(
            "Cargo.toml must declare the application feature `{feature}` and an array for `features.default`."
        ))
    };
    let mut manifest = source
        .parse::<DocumentMut>()
        .map_err(|error| ConsoleError::new(format!("Invalid Cargo.toml: {error}")))?;
    let features = manifest
        .get_mut("features")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(invalid)?;
    if !features.contains_key(feature) {
        return Err(invalid());
    }
    let defaults = features
        .entry("default")
        .or_insert(toml_edit::value(Array::new()))
        .as_array_mut()
        .ok_or_else(invalid)?;
    if defaults.iter().any(|item| item.as_str() == Some(feature)) {
        return Ok(None);
    }
    defaults.push(feature);
    let updated = manifest.to_string();
    Ok(Some(if source.contains("\r\n") {
        updated.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        updated
    }))
}

fn create_source(path: &Path, stub: &[u8]) -> Result<bool, ConsoleError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(stub) {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(file_error(path, error));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_file() => {
            return Ok(false);
        }
        Err(error) => return Err(file_error(path, error)),
    }
    Ok(true)
}

fn install_passport_after_rebuild(
    root: &Path,
    environment: Option<&str>,
    skip: bool,
    noninteractive: bool,
) -> Result<(), ConsoleError> {
    let executable = build_application(root)?;
    let mut command = std::process::Command::new(executable);
    command
        .current_dir(root)
        .env("APP_BASE_PATH", root)
        .args(["artisan", "passport:install"]);
    if skip {
        command.arg("--without-migration-prompt");
    }
    if noninteractive {
        command.arg("--no-interaction");
    }
    if let Some(environment) = environment {
        command.args(["--env", environment]);
    }
    let status = command
        .status()
        .map_err(|e| ConsoleError::new(format!("Could not run passport:install: {e}")))?;
    if !status.success() {
        return Err(ConsoleError::new(format!(
            "Passport installation is incomplete ({status}). Existing sources and keys were preserved; fix the reported error and rerun passport:install."
        )));
    }
    build_application(root)?;
    Ok(())
}

/// Source and rebuild operations shared with the optional Passport package.
pub struct ApiInstallCommand;
impl ApiInstallCommand {
    pub fn configure_passport(root: &Path) -> Result<(), ConsoleError> {
        let _lock = lock_manifest(root)?;
        configure_passport_unlocked(root)
    }
    pub fn configure_passport_resources(
        installation: &super::installation::SourceInstallation<'_>,
    ) -> Result<(), ConsoleError> {
        configure_passport_unlocked(installation.root())
    }
    pub fn configure_passport_installation(
        installation: &super::installation::SourceInstallation<'_>,
    ) -> Result<(), ConsoleError> {
        let root = installation.root();
        let mut warnings = Vec::new();
        wire_passport(root, &mut warnings)?;
        if !warnings.is_empty() {
            return Err(ConsoleError::new(
                warnings
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        configure_passport_unlocked(root)
    }
    pub fn run_pending_migrations(
        root: &Path,
        environment: Option<&str>,
        noninteractive: bool,
    ) -> Result<(), ConsoleError> {
        let executable = build_application(root)?;
        let mut command = std::process::Command::new(executable);
        command
            .current_dir(root)
            .env("APP_BASE_PATH", root)
            .args(["artisan", "migrate"]);
        if noninteractive {
            command.arg("--no-interaction");
        }
        if let Some(environment) = environment {
            command.args(["--env", environment]);
        }
        let status = command
            .status()
            .map_err(|e| ConsoleError::new(format!("Could not run pending migrations: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(ConsoleError::new(format!(
                "Pending migrations failed: {status}"
            )))
        }
    }
}

fn wire_passport(root: &Path, warnings: &mut Vec<ConsoleError>) -> Result<(), ConsoleError> {
    let model = root.join("app/models/user.rs");
    let source = read(&model)?;
    const IMPL: &str = "impl openrust_passport::HasApiTokens for User {";
    if !source.contains(IMPL.trim_end_matches('{').trim()) {
        match insert_before(
            &source,
            &model,
            "#[cfg(feature = \"passport\")]\nimpl openrust_passport::HasApiTokens for User {\n    fn getProviderName(&self) -> &str { \"users\" }\n}\n",
            |line| line.trim() == "impl Authenticatable for User {",
        ) {
            Ok(updated) => replace(&model, &source, &updated)?,
            Err(error) => warnings.push(error),
        }
    }
    let path = root.join("config/auth.rs");
    let source = read(&path)?;
    if let Some(api) = source
        .lines()
        .find(|line| line.trim_start().starts_with("\"api\":"))
    {
        if !api.contains("\"driver\": \"passport\"") {
            warnings.push(ConsoleError::new("Existing API guard was preserved. Configure its Passport driver and provider manually."));
        }
    } else {
        let updated = insert_before(
            &source,
            &path,
            r#""api": {"driver": "passport", "provider": "users"},"#,
            |line| {
                line.trim_start()
                    .starts_with(r#""web": {"driver": "session""#)
            },
        );
        match updated {
            Ok(updated) => replace(&path, &source, &updated)?,
            Err(error) => warnings.push(error),
        }
    }
    Ok(())
}

fn configure_passport_unlocked(root: &Path) -> Result<(), ConsoleError> {
    let path = root.join("config/mod.rs");
    let source = read(&path)?;
    let updated = insert_config_hook(&source, &path, "mod passport;", |line| {
        line.trim() == "mod services;"
    })?;
    let updated = insert_config_hook(
        &updated,
        &path,
        r#"("passport".to_owned(), passport::load(env)?),"#,
        |line| line.trim_start().starts_with(r#"("services".to_owned(),"#),
    )?;
    replace(&path, &source, &updated)
}
