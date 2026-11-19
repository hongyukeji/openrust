//! Passport 13.10.0 passport:client options and branch precedence.
use crate::{ClientRecord, ClientRepository, Passport};
use clap::{Arg, ArgAction, Command as Syntax};
use openrust::contracts::foundation::Application as _;
use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    foundation::Application,
};
use std::io::IsTerminal as _;
use std::sync::atomic::Ordering;

pub struct ClientCommand {
    definition: CommandDefinition,
}
impl Default for ClientCommand {
    fn default() -> Self {
        let mut syntax =
            Syntax::new("passport:client").about("Create a client for issuing access tokens");
        for (name, description) in [
            ("personal", "Create a personal access token client"),
            ("password", "Create a password grant client"),
            ("client", "Create a client credentials grant client"),
            ("implicit", "Create an implicit grant client"),
            ("device", "Create a device authorization grant client"),
            ("public", "Create a public client (without secret)"),
        ] {
            syntax = syntax.arg(
                Arg::new(name)
                    .long(name)
                    .action(ArgAction::SetTrue)
                    .help(description),
            );
        }
        for (name, description) in [
            ("name", "The name of the client"),
            ("provider", "The name of the user provider"),
            ("redirect_uri", "The URI to redirect to after authorization"),
        ] {
            syntax = syntax.arg(Arg::new(name).long(name).help(description));
        }
        Self {
            definition: CommandDefinition::new(syntax),
        }
    }
}
impl Command for ClientCommand {
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
            let config = app.config().map_err(error)?;
            let matches = input.matches();
            let interactive = std::io::stdin().is_terminal()
                && !matches
                    .try_get_one::<bool>("no-interaction")
                    .ok()
                    .flatten()
                    .copied()
                    .unwrap_or(false);
            let prompt = Prompt { interactive };
            let app_name = config
                .get("app.name")
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let name = option(input, "name")
                .map(Ok)
                .unwrap_or_else(|| prompt.ask("What should we name the client?", &app_name))?;
            let clients = app.make::<ClientRepository>().await.map_err(error)?;
            let confidential = !matches.get_flag("public");
            let client: Option<ClientRecord> = if matches.get_flag("personal") {
                let provider = provider(input, &prompt, config.as_ref())?;
                clients
                    .createPersonalAccessGrantClient(&name, provider.as_deref())
                    .await
                    .map_err(error)?;
                None
            } else if matches.get_flag("password") {
                let provider = provider(input, &prompt, config.as_ref())?;
                Some(
                    clients
                        .createPasswordGrantClient(&name, provider.as_deref(), confidential)
                        .await
                        .map_err(error)?,
                )
            } else if matches.get_flag("client") {
                Some(
                    clients
                        .createClientCredentialsGrantClient(&name)
                        .await
                        .map_err(error)?,
                )
            } else if matches.get_flag("implicit") {
                let redirects = redirects(input, &prompt)?;
                Some(
                    clients
                        .createImplicitGrantClient(&name, &redirects)
                        .await
                        .map_err(error)?,
                )
            } else if matches.get_flag("device") {
                Some(
                    clients
                        .createDeviceAuthorizationGrantClient(&name, confidential, None)
                        .await
                        .map_err(error)?,
                )
            } else {
                let redirects = redirects(input, &prompt)?;
                let passport = app.make::<Passport>().await.map_err(error)?;
                let device = passport.deviceCodeGrantEnabled.load(Ordering::Relaxed)
                    && prompt.confirm(
                        "Would you like to enable the device authorization flow for this client?",
                        false,
                    )?;
                Some(
                    clients
                        .createAuthorizationCodeGrantClient(
                            &name,
                            &redirects,
                            confidential,
                            None,
                            device,
                        )
                        .await
                        .map_err(error)?,
                )
            };
            output.info("New client created successfully.");
            if let Some(client) = client {
                let id = client
                    .getKey()
                    .await
                    .map(|v| match v {
                        openrust::database::Value::Text(s) => s,
                        openrust::database::Value::Integer(id) => id.to_string(),
                        openrust::database::Value::Unsigned(id) => id.to_string(),
                        other => format!("{other:?}"),
                    })
                    .unwrap_or_default();
                output
                    .writeRaw(&format!(
                        "  Client ID ................................ {id}\n"
                    ))
                    .map_err(error)?;
                if client.confidential().await {
                    let plain = client
                        .getAttributeValue("plain_secret")
                        .await
                        .map_err(error)?;
                    let plain = match plain {
                        Some(openrust::database::Value::Text(s)) => zeroize::Zeroizing::new(s),
                        _ => zeroize::Zeroizing::new(String::new()),
                    };
                    output
                        .writeRaw(&format!(
                            "  Client Secret ............................ {}\n",
                            plain.as_str()
                        ))
                        .map_err(error)?;
                    output.comment("The client secret will not be shown again, so don't lose it!");
                }
            }
            Ok(0)
        })
    }
}
fn error(error: impl std::fmt::Display) -> ConsoleError {
    ConsoleError::new(error.to_string())
}
fn option(input: &Input, key: &str) -> Option<String> {
    input
        .matches()
        .get_one::<String>(key)
        .filter(|s| !s.is_empty() && s.as_str() != "0")
        .cloned()
}
struct Prompt {
    interactive: bool,
}
impl Prompt {
    fn ask(&self, question: &str, default: &str) -> Result<String, ConsoleError> {
        if !self.interactive {
            return Ok(default.into());
        }
        let term = ::console::Term::stderr();
        term.write_str(&format!("{question} [{default}]: "))
            .map_err(error)?;
        let answer = term.read_line().map_err(error)?;
        let answer = answer.trim().to_owned();
        Ok(if answer.is_empty() {
            default.into()
        } else {
            answer
        })
    }
    fn confirm(&self, question: &str, default: bool) -> Result<bool, ConsoleError> {
        if !self.interactive {
            return Ok(default);
        }
        loop {
            let answer = self.ask(question, if default { "yes" } else { "no" })?;
            match answer.trim().to_ascii_lowercase().as_str() {
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => {}
            }
        }
    }
}
fn redirects(input: &Input, prompt: &Prompt) -> Result<Vec<String>, ConsoleError> {
    let redirect = match option(input, "redirect_uri") {
        Some(value) => value,
        None => {
            let default =
                openrust::support::facades::URL::to("/auth/callback", &[], None).map_err(error)?;
            prompt.ask(
                "Where should we redirect the request after authorization?",
                &default,
            )?
        }
    };
    Ok(redirect.split(',').map(str::to_owned).collect())
}
fn provider(
    input: &Input,
    prompt: &Prompt,
    config: &dyn openrust::contracts::config::Repository,
) -> Result<Option<String>, ConsoleError> {
    if let Some(provider) = option(input, "provider") {
        return Ok(Some(provider));
    }
    let mut providers = config
        .get("auth.guards")
        .and_then(|v| v.as_object().cloned())
        .map(|guards| {
            guards
                .values()
                .filter(|guard| guard.get("driver").and_then(|v| v.as_str()) == Some("passport"))
                .filter_map(|guard| {
                    guard
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if providers.is_empty() {
        providers = config
            .get("auth.providers")
            .and_then(|v| v.as_object().map(|v| v.keys().cloned().collect()))
            .unwrap_or_default();
    }
    if providers.is_empty() {
        return Err(ConsoleError::new("No user providers are configured."));
    }
    let default = config
        .get("auth.guards.api.provider")
        .and_then(|v| v.as_str().map(str::to_owned));
    if !prompt.interactive {
        return match default {
            None => Ok(None),
            Some(default) if providers.contains(&default) => Ok(Some(default)),
            Some(_) => Err(ConsoleError::new(
                "Default user provider is not a configured choice.",
            )),
        };
    }
    loop {
        let answer = prompt.ask(
            &format!(
                "Which user provider should this client use to retrieve users? ({})",
                providers.join(", ")
            ),
            default.as_deref().unwrap_or_default(),
        )?;
        if providers.contains(&answer) {
            return Ok(Some(answer));
        }
        if let Ok(index) = answer.parse::<usize>()
            && let Some(value) = providers.get(index)
        {
            return Ok(Some(value.clone()));
        }
    }
}
