//! Package installation with explicit propagation of every required step's failure.
use crate::Passport;
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::foundation::Application as _,
    foundation::{Application, console::ApiInstallCommand},
    support::facades::Artisan,
};
use std::{
    ffi::OsString,
    io::{self, IsTerminal as _, Write as _},
};

pub struct InstallCommand {
    definition: CommandDefinition,
}
impl Default for InstallCommand {
    fn default() -> Self {
        Self {
            definition: CommandDefinition::new(
                Syntax::new("passport:install")
                    .about("Run the commands necessary to prepare Passport for use")
                    .arg(
                        Arg::new("force")
                            .long("force")
                            .action(ArgAction::SetTrue)
                            .help("Overwrite keys they already exist"),
                    )
                    .arg(
                        Arg::new("length")
                            .long("length")
                            .default_value("4096")
                            .allow_hyphen_values(true)
                            .help("The length of the private key"),
                    )
                    .arg(
                        Arg::new("without-migration-prompt")
                            .long("without-migration-prompt")
                            .action(ArgAction::SetTrue)
                            .help("Publish resources without running pending migrations"),
                    ),
            ),
        }
    }
}
impl Command for InstallCommand {
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
            let passport = app.make::<Passport>().await.map_err(error)?;
            let public = passport.keyPath("oauth-public.key");
            let private = passport.keyPath("oauth-private.key");
            let force = input.matches().get_flag("force");
            match (
                public.try_exists().map_err(error)?,
                private.try_exists().map_err(error)?,
            ) {
                (true, true) if !force => {
                    output.info("Existing Passport encryption keys were preserved.")
                }
                (true, false) | (false, true) if !force => {
                    return Err(ConsoleError::new(
                        "Passport key pair is incomplete. Restore the missing key or explicitly regenerate with passport:install --force.",
                    ));
                }
                _ => {
                    let mut args = vec![
                        OsString::from("--length"),
                        OsString::from(
                            input
                                .matches()
                                .get_one::<String>("length")
                                .expect("default length"),
                        ),
                    ];
                    if force {
                        args.push("--force".into());
                    }
                    if Artisan::call("passport:keys", &args, output).await? != 0 {
                        return Err(ConsoleError::new("Passport key generation failed."));
                    }
                }
            }
            let root = app.base_path().to_owned();
            {
                let installation =
                    openrust::foundation::console::installation::SourceInstallation::new(app)?;
                publish_resources(&installation, false, output).await?;
            }
            let noninteractive = input.matches().get_flag("no-interaction");
            let interactive =
                !noninteractive && output.streams_to_terminal() && io::stdin().is_terminal();
            let run = !input.matches().get_flag("without-migration-prompt")
                && if noninteractive {
                    true
                } else if interactive {
                    tokio::task::spawn_blocking(|| {
                        confirm("Would you like to run all pending database migrations?")
                    })
                    .await
                    .map_err(error)??
                } else {
                    false
                };
            if !run {
                output.info("Passport resources installed. Rebuild the application, then run artisan migrate; migrations are pending.");
                return Ok(0);
            }
            let environment = input
                .matches()
                .get_many::<String>("env")
                .and_then(|mut values| values.next())
                .cloned();
            tokio::task::spawn_blocking(move || {
                ApiInstallCommand::runPendingMigrations(
                    &root,
                    environment.as_deref(),
                    noninteractive,
                )
            })
            .await
            .map_err(error)??;
            let personal = if noninteractive {
                true
            } else {
                tokio::task::spawn_blocking(|| {
                    confirm("Would you like to create the personal access grant client?")
                })
                .await
                .map_err(error)??
            };
            if personal {
                let name = app
                    .config()
                    .map_err(error)?
                    .get("app.name")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default();
                if Artisan::call(
                    "passport:client",
                    &[
                        "--personal".into(),
                        "--name".into(),
                        name.into(),
                        "--no-interaction".into(),
                    ],
                    output,
                )
                .await?
                    != 0
                {
                    return Err(ConsoleError::new("Personal access client creation failed."));
                }
            }
            output.info("Passport installation completed; pending migrations ran successfully.");
            Ok(0)
        })
    }
}
fn confirm(question: &str) -> Result<bool, ConsoleError> {
    let mut stderr = io::stderr().lock();
    loop {
        write!(stderr, "{question} [Y/n] ")
            .and_then(|_| stderr.flush())
            .map_err(error)?;
        let mut answer = String::new();
        if io::stdin().read_line(&mut answer).map_err(error)? == 0 {
            return Ok(false);
        }
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => writeln!(stderr, "Please answer yes or no.").map_err(error)?,
        }
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(crate) fn initialize<'a>(
    installation: &'a openrust::foundation::console::installation::SourceInstallation<'a>,
    options: openrust::foundation::console::installation::InstallationOptions<'a>,
    output: &'a mut Output,
) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
    Box::pin(async move {
        let passport = installation
            .application()
            .make::<Passport>()
            .await
            .map_err(error)?;
        let public = passport.keyPath("oauth-public.key");
        let private = passport.keyPath("oauth-private.key");
        match (
            public.try_exists().map_err(error)?,
            private.try_exists().map_err(error)?,
        ) {
            (true, true) => output.info("Existing Passport encryption keys were preserved."),
            (false, false) => {
                for path in [&public, &private] {
                    let directory = path
                        .parent()
                        .ok_or_else(|| ConsoleError::new("Invalid Passport key path."))?;
                    std::fs::create_dir_all(directory).map_err(error)?;
                }
                tokio::task::spawn_blocking(move || {
                    super::keys_command::generate_keys(&public, &private, 4096, false)
                })
                .await
                .map_err(error)?
                .map_err(error)?;
                output.info("Passport encryption keys generated.");
            }
            _ => {
                return Err(ConsoleError::new(
                    "Passport key pair is incomplete. Restore the missing key before retrying initialization.",
                ));
            }
        }
        publish_resources(installation, options.force, output).await?;
        ApiInstallCommand::configurePassportInstallation(installation)
    })
}

async fn publish_resources(
    installation: &openrust::foundation::console::installation::SourceInstallation<'_>,
    force: bool,
    output: &mut Output,
) -> Result<(), ConsoleError> {
    for tag in ["passport-config", "passport-migrations"] {
        installation
            .publish(tag, force && tag != "passport-migrations", output)
            .await?;
    }
    ApiInstallCommand::configurePassportResources(installation)
}
