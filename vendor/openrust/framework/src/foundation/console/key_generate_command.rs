use std::path::PathBuf;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use clap::{Arg, ArgAction};
use zeroize::Zeroizing;

use crate::{
    console::{
        Application as Artisan, Command, CommandDefinition, ConsoleError, ConsoleFuture, Input,
        Output, confirm_production,
    },
    contracts::{container::Container as _, foundation::Application as _},
    encryption::{Crypt, Encrypter, configured_cipher},
    foundation::Application,
    support::facades::Facade,
};

#[path = "key_file.rs"]
mod key_file;

struct KeyGenerateCommand(CommandDefinition);

impl Command for KeyGenerateCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }

    fn handle<'a>(
        &'a self,
        application: &'a Application,
        input: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let config = application
                .configuration_repository()
                .map_err(console_error)?;
            let cipher = configured_cipher(config.as_ref()).map_err(console_error)?;
            let key = Zeroizing::new(Encrypter::generateKey(cipher).map_err(console_error)?);
            let encoded = Zeroizing::new(STANDARD.encode(&*key));
            let key = Zeroizing::new(format!("base64:{}", encoded.as_str()));
            let matches = input.matches();
            if matches.get_flag("show") {
                output.comment(key.as_str());
                return Ok(0);
            }

            let current = Zeroizing::new(match config.get("app.key") {
                Some(serde_json::Value::String(key)) => key,
                None | Some(serde_json::Value::Null) => String::new(),
                _ => return Err(ConsoleError::new("Invalid application key configuration.")),
            });
            let container = application.container();
            let environment = container.get::<str>("env").map_err(console_error)?;
            if !current.is_empty()
                && !confirm_production(environment.as_ref() == "production", matches)?
            {
                output.stderr.push_str("Command cancelled.\n");
                return Ok(0);
            }

            let path = container
                .get::<PathBuf>("env.file")
                .map_err(console_error)?;
            key_file::stage_key_file(
                &path,
                "APP_KEY",
                &current,
                &key,
                std::env::var_os("APP_KEY").is_some(),
            )?
            .commit()?;
            config
                .set("app.key", serde_json::Value::String(key.to_string()))
                .map_err(console_error)?;
            container.forgetInstance("encrypter");
            container.forgetInstance(crate::container::ServiceKey::typed::<
                dyn crate::contracts::encryption::Encrypter,
            >());
            let _ = Crypt::clearResolvedInstance(None);
            output.info("Application key set successfully.");
            Ok(0)
        })
    }
}

fn console_error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}

pub(super) fn register(artisan: &mut Artisan) -> Result<(), ConsoleError> {
    let flag = |name: &'static str| Arg::new(name).long(name).action(ArgAction::SetTrue);
    artisan.add_command(Box::new(KeyGenerateCommand(CommandDefinition::new(
        clap::Command::new("key:generate")
            .about("Set the application key")
            .arg(flag("show").help("Display a new key instead of modifying files"))
            .arg(flag("force").help("Force the operation to run when in production"))
            .arg(
                flag("no-interaction")
                    .short('n')
                    .help("Do not ask any interactive question"),
            ),
    ))))
}
