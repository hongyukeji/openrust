use crate::{
    config::ConfigError,
    container::{Container, ScopeSnapshot},
    contracts::{config::Repository, container::Container as _},
};

use super::{Localizer, TranslationError, TranslationResource, TranslationResult};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfiguredResource {
    locale: String,
    group: String,
    namespace: Option<String>,
}

impl ConfiguredResource {
    pub(super) fn as_resource(&self) -> TranslationResource<'_> {
        TranslationResource {
            locale: &self.locale,
            group: &self.group,
            namespace: self.namespace.as_deref(),
        }
    }
}

pub(super) fn resources(container: &Container) -> TranslationResult<Vec<ConfiguredResource>> {
    let value = ScopeSnapshot::without(container).run(|| {
        Ok::<_, TranslationError>(
            container
                .get::<dyn Repository>("config")?
                .get("lang.resources"),
        )
    })?;
    match value {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value).map_err(|_| {
            TranslationError::Source(Box::new(ConfigError::InvalidType {
                key: "lang.resources".into(),
                expected: "语言、组和可选命名空间的数组",
            }))
        }),
    }
}

pub(crate) async fn boot(container: &Container) -> TranslationResult<()> {
    let resources = resources(container)?;
    if !resources.is_empty() {
        container
            .make::<Localizer>()?
            .refresh_configured(container, &resources)
            .await?;
    }
    Ok(())
}
