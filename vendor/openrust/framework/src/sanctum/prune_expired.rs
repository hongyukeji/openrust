use chrono::{TimeDelta, Utc};

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output,
    },
    contracts::{container::Container as _, foundation::Application as _},
    database::DatabaseManager,
    foundation::Application,
};

pub(crate) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    artisan.add_command(Box::new(PruneExpired(CommandDefinition::new(
        clap::Command::new("sanctum:prune-expired")
            .about("Prune tokens expired for more than specified number of hours")
            .arg(
                clap::Arg::new("hours")
                    .long("hours")
                    .default_value("24")
                    .value_parser(clap::value_parser!(i64).range(0..))
                    .help("The number of hours to retain expired Sanctum tokens"),
            ),
    ))))
}

struct PruneExpired(CommandDefinition);

impl Command for PruneExpired {
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
            let hours = *input
                .matches()
                .get_one::<i64>("hours")
                .expect("default hours");
            let now = Utc::now();
            let retention = TimeDelta::try_hours(hours)
                .ok_or_else(|| ConsoleError::new("The retention interval is too large."))?;
            let cutoff = now
                .checked_sub_signed(retention)
                .ok_or_else(|| ConsoleError::new("The retention interval is too large."))?;
            let config = app.config().map_err(error)?;
            let expiration = super::guard::expiration(config.as_ref()).map_err(error)?;
            let created_cutoff = expiration
                .map(|age| {
                    cutoff
                        .checked_sub_signed(age)
                        .ok_or_else(|| ConsoleError::new("The expiration interval is too large."))
                })
                .transpose()?;
            let database = app.make::<DatabaseManager>().await.map_err(error)?;
            let sanctum = app.container().make::<super::Sanctum>().map_err(error)?;
            let tokens = sanctum.personalAccessTokens(&database, "").map_err(error)?;
            output.comment("Pruning tokens with expired expires_at timestamps");
            tokens.prune("expires_at", cutoff).await.map_err(error)?;
            if let Some(cutoff) = created_cutoff {
                output.comment(
                    "Pruning tokens with expired expiration value based on configuration file",
                );
                tokens.prune("created_at", cutoff).await.map_err(error)?;
            } else {
                output.comment("Expiration value not specified in configuration file.");
            }
            output.info(&format!(
                "Tokens expired for more than [{hours} hours] pruned successfully."
            ));
            Ok(0)
        })
    }
}

fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
