use crate::{
    ActivityBuffer, ActivityLogContext, ActivityLogStatus, ActivityRepository, CauserResolver,
    Redaction, actions::LogActivityAction,
};
use openrust::{
    contracts::{
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;
pub struct ActivitylogServiceProvider;
fn binding() -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "ActivityRepository",
        source: Box::new(crate::Error::InvalidConfiguration),
    }
}
impl ServiceProvider for ActivitylogServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, crate::configuration::load(&env)?, "activitylog")?;
            }
            app.container()
                .singleton_named::<crate::models::ActivityModel, _>(
                    "openrust_activitylog::models::Activity",
                    |_| {
                        Ok(Arc::new(crate::models::ActivityModel::of::<
                            crate::models::Activity,
                        >()))
                    },
                )?;
            app.container().singleton_named::<LogActivityAction, _>(
                "openrust_activitylog::actions::LogActivityAction",
                |_| Ok(Arc::new(LogActivityAction::default())),
            )?;
            app.container()
                .singleton_named::<crate::actions::CleanActivityLogAction, _>(
                    "openrust_activitylog::actions::CleanActivityLogAction",
                    |container| {
                        Ok(Arc::new(crate::actions::CleanActivityLogAction::new(
                            container.make::<ActivityRepository>()?,
                        )))
                    },
                )?;
            app.container()
                .singleton::<ActivityRepository, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let enabled = config
                        .get("activitylog.enabled")
                        .map(|value| match value {
                            serde_json::Value::Null => Ok(true),
                            serde_json::Value::Bool(enabled) => Ok(enabled),
                            _ => Err(binding()),
                        })
                        .transpose()?
                        .unwrap_or(true);
                    let connection = config.get("activitylog.connection");
                    let name = match connection.as_ref() {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(s)) => Some(s.as_str()),
                        _ => return Err(binding()),
                    };
                    let keys =
                        |name: &str| -> std::result::Result<Vec<String>, BindingResolutionError> {
                            let values = config.get(name).ok_or_else(binding)?;
                            values
                                .as_array()
                                .ok_or_else(binding)?
                                .iter()
                                .map(|v| v.as_str().map(str::to_owned).ok_or_else(binding))
                                .collect()
                        };
                    let properties = keys("activitylog.properties")?;
                    let attributes = keys("activitylog.attributes")?;
                    let except = keys("activitylog.except")?;
                    fn refs(v: &[String]) -> Vec<&str> {
                        v.iter().map(String::as_str).collect()
                    }
                    let policy = Redaction::new()
                        .log_properties(&refs(&properties))
                        .log_only(&refs(&attributes))
                        .log_except(&refs(&except));
                    let model_name = config
                        .get("activitylog.activity_model")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .ok_or_else(binding)?;
                    let model =
                        container.get::<crate::models::ActivityModel>(model_name.as_str())?;
                    let connection = container
                        .make::<openrust::database::DatabaseManager>()?
                        .connection(name.or(model.connection.as_deref()))
                        .map_err(|_| binding())?;
                    let mut repository = ActivityRepository::new(connection)
                        .enabled(enabled)
                        .with_model(model);
                    if config
                        .get("activitylog.redaction.enabled")
                        .and_then(|value| value.as_bool())
                        .unwrap_or(false)
                    {
                        repository = repository.with_redaction(policy);
                    }
                    let log_name = match config.get("activitylog.default_log_name") {
                        None | Some(serde_json::Value::Null) => None,
                        Some(serde_json::Value::String(name)) => Some(name),
                        _ => return Err(binding()),
                    };
                    repository = repository
                        .with_default_log_name(log_name)
                        .with_default_except_attributes(keys(
                            "activitylog.default_except_attributes",
                        )?);
                    Ok(Arc::new(repository))
                })?;
            app.container()
                .scoped::<ActivityLogStatus, _>(|container| {
                    let repository = container.make::<ActivityRepository>()?;
                    Ok(Arc::new(ActivityLogStatus::new(repository.is_enabled())))
                })?;
            app.container().scoped::<CauserResolver, _>(|container| {
                let config =
                    container.get::<dyn openrust::contracts::config::Repository>("config")?;
                let driver = match config.get("activitylog.default_auth_driver") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(serde_json::Value::String(name)) => Some(name),
                    _ => return Err(binding()),
                };
                let auth = container.make::<openrust::auth::AuthManager>()?;
                let default_auth = auth.clone();
                let default_driver = driver.clone();
                let id_auth = auth.clone();
                let id_driver = driver.clone();
                let id_config = config.clone();
                Ok(Arc::new(
                    CauserResolver::default()
                        .with_identity_resolvers(
                            Arc::new(move |id| {
                                let auth = id_auth.clone();
                                let driver = id_driver
                                    .clone()
                                    .unwrap_or_else(|| auth.get_default_driver());
                                let config = id_config.clone();
                                Box::pin(async move {
                                    let provider = config
                                        .get(&format!("auth.guards.{driver}.provider"))
                                        .and_then(|v| v.as_str().map(str::to_owned))
                                        .ok_or(crate::Error::InvalidConfiguration)?;
                                    let user = auth
                                        .create_user_provider(&provider)
                                        .map_err(|_| crate::Error::InvalidConfiguration)?
                                        .retrieve_by_id(&id)
                                        .await
                                        .map_err(|_| crate::Error::InvalidActivity)?
                                        .ok_or(crate::Error::InvalidActivity)?;
                                    let kind = auth
                                        .user_morph_class(user.as_ref())
                                        .map_err(|_| crate::Error::InvalidActivity)?
                                        .ok_or(crate::Error::InvalidActivity)?;
                                    let attributes = auth
                                        .user_attributes(user.as_ref())
                                        .map_err(|_| crate::Error::InvalidActivity)?;
                                    Ok((
                                        crate::Subject::new(
                                            kind,
                                            user.get_auth_identifier().to_string(),
                                        )?,
                                        attributes.into(),
                                    ))
                                })
                            }),
                            Arc::new(move |subject| {
                                let Some(user) =
                                    default_auth.resolved_user(default_driver.as_deref())
                                else {
                                    return Ok(serde_json::json!({}));
                                };
                                if user.get_auth_identifier().to_string() != subject.id {
                                    return Ok(serde_json::json!({}));
                                }
                                let kind = default_auth
                                    .user_morph_class(user.as_ref())
                                    .map_err(|_| crate::Error::InvalidActivity)?;
                                if kind != Some(subject.kind.as_str()) {
                                    return Ok(serde_json::json!({}));
                                }
                                default_auth
                                    .user_attributes(user.as_ref())
                                    .map(Into::into)
                                    .map_err(|_| crate::Error::InvalidActivity)
                            }),
                        )
                        .with_default_resolver(move || {
                            let Some(user) = auth.resolved_user(driver.as_deref()) else {
                                return Ok(None);
                            };
                            let kind = auth
                                .user_morph_class(user.as_ref())
                                .map_err(|_| crate::Error::InvalidActivity)?
                                .ok_or(crate::Error::InvalidActivity)?;
                            crate::Subject::new(kind, user.get_auth_identifier().to_string())
                                .map(Some)
                        }),
                ))
            })?;
            app.container()
                .singleton::<LogActivityAction, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let action = config
                        .get("activitylog.actions.log_activity")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .ok_or_else(binding)?;
                    container.get::<LogActivityAction>(action.as_str())
                })?;
            app.container().scoped::<ActivityBuffer, _>(|container| {
                let buffer = Arc::new(ActivityBuffer::new(container.make::<ActivityRepository>()?));
                let pending = buffer.clone();
                let _ = openrust::support::defer(
                    async move { pending.flush().await.map(|_| ()) },
                    None,
                    true,
                )
                .map_err(|_| binding())?;
                Ok(buffer)
            })?;
            app.container()
                .scoped::<ActivityLogContext, _>(|container| {
                    let config =
                        container.get::<dyn openrust::contracts::config::Repository>("config")?;
                    let mut context = ActivityLogContext::new(
                        container.make::<ActivityRepository>()?,
                        container.make::<ActivityLogStatus>()?.as_ref().clone(),
                        container.make::<CauserResolver>()?.as_ref().clone(),
                        container.make::<LogActivityAction>()?.as_ref().clone(),
                    );
                    if config
                        .get("activitylog.buffer.enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        context = context.with_buffer(container.make::<ActivityBuffer>()?);
                    }
                    Ok(Arc::new(context))
                })?;
            Ok(())
        })
    }
    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.publishes(
                app,
                &[(
                    "config/activitylog.rs",
                    include_bytes!("../config/activitylog.rs"),
                )],
                "activitylog-config",
            )?;
            if app
                .config()?
                .get("activitylog.connection")
                .as_ref()
                .is_none_or(serde_json::Value::is_null)
            {
                self.load_migrations_from(app, Self::migrations()).await?;
            }
            self.commands(app, register_commands);
            Ok(())
        })
    }
}
impl ActivitylogServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        static ENTRIES: &[openrust::database::migrations::MigrationEntry] = &[
            openrust::database::migrations::MigrationEntry::new(
                "vendor/openrust-activitylog/database/migrations/2026_11_21_000001_create_activity_log_table.rs",
                &crate::migration::MIGRATION,
            ),
        ];
        ENTRIES
    }
}
fn register_commands() -> std::result::Result<(), openrust::console::ConsoleError> {
    openrust::support::facades::Artisan::add(Box::new(CleanCommand {
        definition: openrust::console::CommandDefinition::new(
            clap::Command::new("activitylog:clean")
                .about("Clean up old records from the activity log.")
                .arg(clap::Arg::new("log").help("The log name that will be cleaned."))
                .arg(
                    clap::Arg::new("days")
                        .long("days")
                        .help("Records older than this number of days will be cleaned."),
                )
                .arg(
                    clap::Arg::new("force")
                        .long("force")
                        .action(clap::ArgAction::SetTrue)
                        .help("Force the operation to run when in production."),
                ),
        ),
    }))
}
struct CleanCommand {
    definition: openrust::console::CommandDefinition,
}
impl openrust::console::Command for CleanCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.definition
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        input: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<
        'a,
        std::result::Result<i32, openrust::console::ConsoleError>,
    > {
        Box::pin(async move {
            let error = || openrust::console::ConsoleError::new("Activity log cleanup failed");
            let config = app.config().map_err(|_| error())?;
            let production = config
                .get("app.env")
                .is_some_and(|value| value.as_str() == Some("production"));
            if !openrust::console::confirm_to_proceed(production, input.matches())? {
                return Ok(1);
            }
            output.comment("Cleaning activity log...");
            let configured = config.get("activitylog.clean_after_days");
            let days = input
                .matches()
                .get_one::<String>("days")
                .map(String::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    configured.as_ref().map(|value| match value {
                        serde_json::Value::String(value) => value.clone(),
                        _ => value.to_string(),
                    })
                });
            let Some(days) = days
                .and_then(|value| value.parse::<i64>().ok())
                .filter(|value| *value > 0)
            else {
                output.error("The days option must be a positive integer.");
                return Ok(1);
            };
            let repository = app
                .make::<ActivityRepository>()
                .await
                .map_err(|_| error())?;
            let _ = repository;
            let name = config
                .get("activitylog.actions.clean_log")
                .and_then(|v| v.as_str().map(str::to_owned))
                .ok_or_else(error)?;
            let action = app
                .container()
                .get::<crate::actions::CleanActivityLogAction>(name.as_str())
                .map_err(|_| error())?;
            let count = action
                .execute(
                    days,
                    input.matches().get_one::<String>("log").map(String::as_str),
                )
                .await
                .map_err(|_| error())?;
            output.info(&format!("Deleted {count} record(s) from the activity log."));
            output.comment("All done!");
            Ok(0)
        })
    }
}
