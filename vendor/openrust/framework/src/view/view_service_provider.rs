use std::{path::Path, sync::Arc};

use crate::{
    config::ConfigError,
    contracts::{
        config::Repository,
        container::{BindingResolutionError, Container as _},
        foundation::Application as _,
    },
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};

use super::{Factory, FileViewFinder};

/// Registers the view factory for the current application.
pub struct ViewServiceProvider;

impl ServiceProvider for ViewServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let base = app.base_path().to_path_buf();
            let container = app.container();
            container.singletonNamed("view", move |container| {
                let config = container.get::<dyn Repository>("config")?;
                configured_factory(&base, config.as_ref())
                    .map(Arc::new)
                    .map_err(|source| BindingResolutionError::FactoryFailed {
                        service: "view",
                        source: Box::new(source),
                    })
            })?;
            container.alias("view", crate::container::ServiceKey::typed::<Factory>())?;
            container.singletonNamed::<dyn crate::contracts::view::Factory, _>(
                "view.contract",
                |container| {
                    let concrete = container.get::<Factory>("view")?;
                    Ok(concrete as Arc<dyn crate::contracts::view::Factory>)
                },
            )?;
            container.alias(
                "view.contract",
                crate::container::ServiceKey::typed::<dyn crate::contracts::view::Factory>(),
            )?;
            Ok(())
        })
    }
}

fn configured_factory(base: &Path, config: &dyn Repository) -> Result<Factory, ConfigError> {
    let paths = match config.get("view.paths") {
        Some(value) => {
            let invalid = || ConfigError::InvalidType {
                key: "view.paths".to_owned(),
                expected: "an array of non-empty directory paths",
            };
            value
                .as_array()
                .ok_or_else(invalid)?
                .iter()
                .map(|path| {
                    path.as_str()
                        .filter(|path| !path.trim().is_empty())
                        .map(|path| base.join(path))
                        .ok_or_else(invalid)
                })
                .collect::<Result<Vec<_>, _>>()?
        }
        None => vec![base.join("resources/views")],
    };
    let compiled = match config.get("view.compiled") {
        Some(value) => base.join(
            value
                .as_str()
                .filter(|path| !path.trim().is_empty())
                .ok_or(ConfigError::InvalidType {
                    key: "view.compiled".to_owned(),
                    expected: "a non-empty directory path",
                })?,
        ),
        None => base.join("storage/framework/views"),
    };
    Ok(Factory::new(FileViewFinder::new(paths), compiled))
}
