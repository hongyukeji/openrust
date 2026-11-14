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
    foundation::Application,
};

use super::source_edit::{file_error, insert_before, lock_manifest, read, replace};

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
        registration_methods: &[".channels(", ".withBroadcasting("],
        equivalent: Some(".withBroadcasting(routes::channels::register)"),
        bootstrap_anchor: ".health(",
        stub: include_bytes!("stubs/broadcasting-routes.stub"),
        next_steps: "Configure a broadcast driver, add your channel rules, and rebuild the application.",
    },
    InstallSpec {
        command: "install:grpc",
        description: "Install the application gRPC scaffolding",
        module: "grpc",
        module_anchor: "web",
        registration: ".withGrpc(routes::grpc::register)",
        registration_methods: &[".withGrpc("],
        equivalent: None,
        bootstrap_anchor: ".withMiddleware(",
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
            let existing = tokio::task::spawn_blocking(move || install(&root, spec, force))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))??;
            if api && existing && !force {
                output.stderr.push_str("API routes file already exists.\n");
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
                if cfg!(feature = "sanctum") {
                    return Ok(app.share().run_console(&["migrate".into()], output).await);
                }
                let root = app.base_path().to_owned();
                return tokio::task::spawn_blocking(move || migrate_after_rebuild(&root))
                    .await
                    .map_err(|error| ConsoleError::new(error.to_string()))?;
            }
            Ok(0)
        })
    }
}

fn migrate_after_rebuild(root: &Path) -> Result<i32, ConsoleError> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = std::process::Command::new(cargo);
    command
        .current_dir(root)
        .args(["run", "--quiet", "--locked"]);
    // Windows locks the installer executable until this command returns.
    #[cfg(windows)]
    let target = tempfile::Builder::new()
        .prefix("openrust-install-api-")
        .tempdir()
        .map_err(|error| {
            ConsoleError::new(format!("Could not prepare the migration build: {error}"))
        })?;
    #[cfg(windows)]
    command.arg("--target-dir").arg(target.path());
    let status = command
        .args(["--", "artisan", "migrate"])
        .status()
        .map_err(|error| {
            ConsoleError::new(format!(
                "Could not rebuild and run pending migrations: {error}"
            ))
        })?;
    Ok(status.code().unwrap_or(1))
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

fn install(root: &Path, spec: &InstallSpec, force: bool) -> Result<bool, ConsoleError> {
    let _manifest = lock_manifest(root)?;
    let routes_module = root.join("routes/mod.rs");
    let bootstrap = root.join("bootstrap/app.rs");
    let routes = root.join("routes").join(format!("{}.rs", spec.module));
    let modules = read(&routes_module)?;
    let startup = read(&bootstrap)?;
    let manifest_path = root.join("Cargo.toml");
    let manifest_update = if spec.command == "install:api" {
        let manifest = read(&manifest_path)?;
        enable_sanctum(&manifest)?.map(|updated| (manifest, updated))
    } else {
        None
    };
    let module = format!("mod {};", spec.module);
    let module_statement = format!("pub(crate) {module}");
    let anchor = format!("pub(crate) mod {};", spec.module_anchor);
    let modules_update = if modules.lines().any(|line| {
        let line = line.trim();
        line == module
            || line == module_statement
            || line.strip_prefix("pub ") == Some(module.as_str())
    }) {
        None
    } else {
        Some(insert_before(
            &modules,
            &routes_module,
            &module_statement,
            |line| line.trim() == anchor,
        )?)
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
            return Err(ConsoleError::new(format!(
                "bootstrap/app.rs already has a custom {} registration. Add `{}` manually if needed.",
                spec.module, spec.registration
            )));
        }
        Some(insert_before(
            &startup,
            &bootstrap,
            spec.registration,
            |line| line.trim_start().starts_with(spec.bootstrap_anchor),
        )?)
    };

    let existing = !create_routes(&routes, spec.stub)?;
    if existing && force {
        let previous = read(&routes)?;
        let stub = std::str::from_utf8(spec.stub).expect("API stub is UTF-8");
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
    Ok(existing)
}

fn enable_sanctum(source: &str) -> Result<Option<String>, ConsoleError> {
    use toml_edit::{Array, DocumentMut, Item};

    let invalid = || {
        ConsoleError::new(
            "Cargo.toml must declare the application feature `sanctum` and an array for `features.default`.",
        )
    };
    let mut manifest = source
        .parse::<DocumentMut>()
        .map_err(|error| ConsoleError::new(format!("Invalid Cargo.toml: {error}")))?;
    let features = manifest
        .get_mut("features")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(invalid)?;
    if !features.contains_key("sanctum") {
        return Err(invalid());
    }
    let defaults = features
        .entry("default")
        .or_insert(toml_edit::value(Array::new()))
        .as_array_mut()
        .ok_or_else(invalid)?;
    if defaults
        .iter()
        .any(|feature| feature.as_str() == Some("sanctum"))
    {
        return Ok(None);
    }
    defaults.push("sanctum");
    let updated = manifest.to_string();
    Ok(Some(if source.contains("\r\n") {
        updated.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        updated
    }))
}

fn create_routes(path: &Path, stub: &[u8]) -> Result<bool, ConsoleError> {
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
