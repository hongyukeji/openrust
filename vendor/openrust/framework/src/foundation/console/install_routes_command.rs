use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write as _},
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

const INSTALLERS: &[InstallSpec] = &[InstallSpec {
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
}];

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    for spec in INSTALLERS {
        artisan.add_command(Box::new(InstallRoutesCommand {
            definition: CommandDefinition::new(
                clap::Command::new(spec.command).about(spec.description),
            ),
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
        _input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let root = app.base_path().to_owned();
            let spec = self.spec;
            tokio::task::spawn_blocking(move || install(&root, spec))
                .await
                .map_err(|error| ConsoleError::new(error.to_string()))??;
            output.stdout.push_str(&format!(
                "routes/{}.rs is installed. {}\n",
                spec.module, spec.next_steps
            ));
            Ok(0)
        })
    }
}

fn install(root: &Path, spec: &InstallSpec) -> Result<(), ConsoleError> {
    // The manifest stays in place while installers replace the route and bootstrap files.
    let manifest_path = root.join("Cargo.toml");
    let manifest = File::open(&manifest_path).map_err(|error| file_error(&manifest_path, error))?;
    manifest
        .lock()
        .map_err(|error| file_error(&manifest_path, error))?;
    let routes_module = root.join("routes/mod.rs");
    let bootstrap = root.join("bootstrap/app.rs");
    let routes = root.join("routes").join(format!("{}.rs", spec.module));
    let modules = read(&routes_module)?;
    let startup = read(&bootstrap)?;
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

    create_routes(&routes, spec.stub)?;
    if let Some(updated) = modules_update {
        replace(&routes_module, &modules, &updated)?;
    }
    if let Some(updated) = startup_update {
        replace(&bootstrap, &startup, &updated)?;
    }
    Ok(())
}

fn insert_before(
    source: &str,
    path: &Path,
    statement: &str,
    is_anchor: impl Fn(&str) -> bool,
) -> Result<String, ConsoleError> {
    let mut offset = 0;
    let mut found = None;
    for line in source.split_inclusive('\n') {
        if is_anchor(line) {
            let indent = &line[..line.len() - line.trim_start().len()];
            if found.replace((offset, indent)).is_some() {
                return Err(manual_registration(path, statement));
            }
        }
        offset += line.len();
    }
    let (offset, indent) = found.ok_or_else(|| manual_registration(path, statement))?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut updated = source.to_owned();
    updated.insert_str(offset, &format!("{indent}{statement}{newline}"));
    Ok(updated)
}

fn manual_registration(path: &Path, statement: &str) -> ConsoleError {
    ConsoleError::new(format!(
        "Cannot locate the registration point in {}. Add `{statement}` manually.",
        path.display()
    ))
}

fn create_routes(path: &Path, stub: &[u8]) -> Result<(), ConsoleError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = file.write_all(stub) {
                drop(file);
                let _ = fs::remove_file(path);
                return Err(file_error(path, error));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && path.is_file() => {}
        Err(error) => return Err(file_error(path, error)),
    }
    Ok(())
}

fn replace(path: &Path, previous: &str, updated: &str) -> Result<(), ConsoleError> {
    let current = read(path)?;
    if current == updated {
        return Ok(());
    }
    if current != previous {
        return Err(ConsoleError::new(format!(
            "{} changed during installation. Retry with the updated file.",
            path.display()
        )));
    }
    let temporary = path.with_extension(format!("openrust-{:016x}.tmp", fastrand::u64(..)));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| file_error(path, error))?;
    let result = (|| {
        file.set_permissions(fs::metadata(path)?.permissions())?;
        file.write_all(updated.as_bytes())?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(temporary);
        return Err(file_error(path, error));
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, ConsoleError> {
    fs::read_to_string(path).map_err(|error| file_error(path, error))
}

fn file_error(path: &Path, error: io::Error) -> ConsoleError {
    ConsoleError::new(format!("{}: {error}", path.display()))
}
