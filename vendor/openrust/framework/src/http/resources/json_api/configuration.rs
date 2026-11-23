use std::sync::Arc;

use serde_json::{Map, Value};

use crate::{contracts::container::Container as _, support::facades::current_container};

use super::super::json::ResourceResult;

const DEFAULT_DEPTH: usize = 5;

/// Application-local settings for JSON:API documents.
pub struct JsonApi {
    information: Map<String, Value>,
    depth: usize,
}

impl Default for JsonApi {
    fn default() -> Self {
        Self {
            information: Map::new(),
            depth: DEFAULT_DEPTH,
        }
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl JsonApi {
    pub fn configure(
        version: Option<String>,
        extensions: Vec<String>,
        profiles: Vec<String>,
        meta: Map<String, Value>,
    ) -> Self {
        let mut settings = Self::default();
        if let Some(version) = version.filter(|version| !version.is_empty()) {
            settings
                .information
                .insert("version".into(), Value::String(version));
        }
        for (name, values) in [("ext", extensions), ("profile", profiles)] {
            if !values.is_empty() {
                settings.information.insert(
                    name.into(),
                    Value::Array(values.into_iter().map(Value::String).collect()),
                );
            }
        }
        if !meta.is_empty() {
            settings
                .information
                .insert("meta".into(), Value::Object(meta));
        }
        settings
    }

    #[must_use]
    pub const fn maxRelationshipDepth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    pub(super) fn depth() -> ResourceResult<usize> {
        Self::current().map(|settings| settings.map_or(DEFAULT_DEPTH, |settings| settings.depth))
    }

    pub(in crate::http::resources) fn information() -> ResourceResult<Option<Value>> {
        let settings = Self::current()?;
        Ok(settings
            .filter(|settings| !settings.information.is_empty())
            .map(|settings| Value::Object(settings.information.clone())))
    }

    fn current() -> ResourceResult<Option<Arc<Self>>> {
        let Some(container) = current_container() else {
            return Ok(None);
        };
        if !container.bound::<Self>() {
            return Ok(None);
        }
        container.make::<Self>().map(Some).map_err(Into::into)
    }
}
