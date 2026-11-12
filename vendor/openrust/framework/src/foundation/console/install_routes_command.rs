use std::{
    fs::{self, OpenOptions},
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
        next_steps: "Add your API endpoints and rebuild the application.",
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
    let _manifest = lock_manifest(root)?;
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
