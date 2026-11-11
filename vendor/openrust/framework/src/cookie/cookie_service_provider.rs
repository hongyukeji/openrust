use std::sync::Arc;

use crate::{
    config::{ConfigError, Value},
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{CookieJar, SameSite};

/// Registers the application's cookie factory and defaults.
pub struct CookieServiceProvider;

impl ServiceProvider for CookieServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let container = app.container();
            container.singletonNamed("cookie", |container| {
                let config = container.get::<dyn Repository>("config")?;
                defaults(config.as_ref()).map(Arc::new).map_err(|source| {
                    BindingResolutionError::FactoryFailed {
                        service: "cookie",
                        source: Box::new(source),
                    }
                })
            })?;
            container.alias("cookie", crate::container::ServiceKey::typed::<CookieJar>())?;
            container.singleton::<dyn crate::contracts::cookie::Factory, _>(|container| {
                let jar = container.get::<CookieJar>("cookie")?;
                Ok(jar)
            })?;
            Ok(())
        })
    }
}

fn defaults(config: &dyn Repository) -> Result<CookieJar, ConfigError> {
    let path = text(config, "session.path")?;
    let domain = text(config, "session.domain")?;
    let secure = match config.get("session.secure") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(secure)) => Some(secure),
        _ => return Err(invalid("session.secure", "a boolean or null")),
    };
    let same_site = match config.get("session.same_site") {
        None => Some(SameSite::Lax),
        Some(Value::Null) => None,
        Some(Value::String(value)) => match value.to_ascii_lowercase().as_str() {
            "" => None,
            "lax" => Some(SameSite::Lax),
            "strict" => Some(SameSite::Strict),
            "none" => Some(SameSite::None),
            _ => return Err(invalid("session.same_site", "lax, strict, none or null")),
        },
        _ => return Err(invalid("session.same_site", "lax, strict, none or null")),
    };
    let mut jar = CookieJar::new();
    jar.setDefaultPathAndDomain(
        path.as_deref().unwrap_or("/"),
        domain.as_deref(),
        secure,
        same_site,
    );
    Ok(jar)
}

fn text(config: &dyn Repository, key: &str) -> Result<Option<String>, ConfigError> {
    match config.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(invalid(key, "a string or null")),
    }
}

fn invalid(key: &str, expected: &'static str) -> ConfigError {
    ConfigError::InvalidType {
        key: key.to_owned(),
        expected,
    }
}
