use crate::FeatureManager;
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
    support::facades::Artisan,
};

struct PurgeCommand(CommandDefinition);
pub(crate) fn register() -> Result<(), ConsoleError> {
    Artisan::add(Box::new(FeatureMakeCommand(CommandDefinition::new(
        Syntax::new("pennant:feature")
            .about("Create a new feature class")
            .arg(Arg::new("name").required(true))
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue),
            ),
    ))))?;
    Artisan::add(Box::new(PurgeCommand(CommandDefinition::new(
        Syntax::new("pennant:purge")
            .alias("pennant:clear")
            .about("Delete Pennant features from storage")
            .arg(Arg::new("features").num_args(0..))
            .arg(Arg::new("except").long("except").action(ArgAction::Append))
            .arg(
                Arg::new("except-registered")
                    .long("except-registered")
                    .action(ArgAction::SetTrue),
            )
            .arg(Arg::new("store").long("store")),
    ))))
}
impl Command for PurgeCommand {
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
            let manager = app.make::<FeatureManager>().await.map_err(error)?;
            let store = manager
                .store(
                    input
                        .matches()
                        .get_one::<String>("store")
                        .map(String::as_str),
                )
                .map_err(error)?;
            let features = input
                .matches()
                .get_many::<String>("features")
                .map(|values| values.cloned().collect::<Vec<_>>());
            let mut except: Vec<_> = input
                .matches()
                .get_many::<String>("except")
                .into_iter()
                .flatten()
                .cloned()
                .collect();
            if input.matches().get_flag("except-registered") {
                except.extend(store.defined().map_err(error)?);
            }
            let features = if except.is_empty() {
                features
            } else {
                Some(
                    (match features {
                        Some(features) => features,
                        None => store.stored().await.map_err(error)?,
                    })
                    .into_iter()
                    .filter(|name| !except.contains(name))
                    .collect(),
                )
            };
            store.purge(features.as_deref()).await.map_err(error)?;
            if features.as_ref().is_some_and(|names| !names.is_empty()) {
                output.info(&format!(
                    "{} successfully purged from storage.",
                    features.unwrap().join(", ")
                ));
            } else if !except.is_empty() {
                output.info("No features to purge from storage.");
            } else {
                output.info("All features successfully purged from storage.");
            }
            Ok(0)
        })
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

struct FeatureMakeCommand(CommandDefinition);
impl Command for FeatureMakeCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        use openrust::console::{GeneratorCommand as _, generator_command::write_generated};
        Box::pin(async move {
            let name = input
                .matches()
                .get_one::<String>("name")
                .expect("required name");
            let path = self.get_path(app, name, input)?;
            let code = self.get_stub(app, input)?;
            let _installation =
                openrust::foundation::console::installation::SourceInstallation::new(app)?;
            write_generated(&path, code.as_bytes(), input.matches().get_flag("force"))
                .map_err(error)?;
            output.info(&format!(
                "Feature [{}] created successfully.",
                path.display()
            ));
            output.stdout.push_str("Declare the generated module and register its factory using Feature::defineClass.\n");
            Ok(0)
        })
    }
}
impl openrust::console::GeneratorCommand for FeatureMakeCommand {
    fn get_path(
        &self,
        app: &Application,
        name: &str,
        _: &Input,
    ) -> Result<std::path::PathBuf, ConsoleError> {
        use openrust::{
            console::generator_command::{snake_case, source_name_parts},
            contracts::foundation::Application as _,
        };
        let mut path = app.base_path().join("app/features");
        path.extend(
            source_name_parts(name, "Use a PascalCase feature name, such as NewApi.")?
                .map(snake_case),
        );
        Ok(path.with_extension("rs"))
    }
    fn get_stub(
        &self,
        app: &Application,
        input: &Input,
    ) -> Result<std::borrow::Cow<'static, str>, ConsoleError> {
        use openrust::{
            console::generator_command::source_name_parts, contracts::foundation::Application as _,
        };
        let class = source_name_parts(
            input
                .matches()
                .get_one::<String>("name")
                .expect("required name"),
            "Use a PascalCase feature name.",
        )?
        .last()
        .expect("validated name");
        let path = app.base_path().join("stubs/feature.rs.stub");
        let stub = match std::fs::read_to_string(&path) {
            Ok(stub) => stub,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                include_str!("../stubs/feature.rs.stub").into()
            }
            Err(source) => return Err(error(source)),
        };
        Ok(std::borrow::Cow::Owned(stub.replace("{{class}}", class)))
    }
}
