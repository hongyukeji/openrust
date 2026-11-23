use std::future::Future;

use serde_json::{Map, Value};

use crate::{
    database::{
        Value as AttributeValue,
        eloquent::{Collection, Model},
    },
    http::StatusCode,
};

use super::super::json::ResourceResult;

/// The data projected into a JSON:API resource object.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait JsonApiSource: Sized + Send {
    fn identifier(&self) -> Option<String>;

    fn attributes(&self) -> ResourceResult<Map<String, Value>>;

    fn status(&self) -> StatusCode {
        StatusCode::OK
    }

    fn relationLoaded(&self, _name: &str) -> bool {
        false
    }

    fn loadMissingPaths<'a>(
        _sources: Vec<&'a mut Self>,
        _paths: Vec<&'static str>,
    ) -> impl Future<Output = ResourceResult<()>> + Send + 'a {
        async { Ok(()) }
    }
}

impl<M: Model> JsonApiSource for M {
    fn identifier(&self) -> Option<String> {
        match self.getKey()? {
            AttributeValue::Text(value) => Some(value.clone()),
            AttributeValue::Integer(value) => Some(value.to_string()),
            AttributeValue::Unsigned(value) => Some(value.to_string()),
            _ => None,
        }
    }

    fn attributes(&self) -> ResourceResult<Map<String, Value>> {
        let mut attributes = self.attributesToArray()?;
        attributes.shift_remove(self.getKeyName());
        Ok(attributes)
    }

    fn status(&self) -> StatusCode {
        if self.state().wasRecentlyCreated() {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        }
    }

    fn relationLoaded(&self, name: &str) -> bool {
        Model::relationLoaded(self, name)
    }

    async fn loadMissingPaths(
        sources: Vec<&mut Self>,
        paths: Vec<&'static str>,
    ) -> ResourceResult<()> {
        let mut models = Collection::new(
            sources
                .iter()
                .map(|model| M::from_state(model.state().clone()))
                .collect(),
        );
        models.loadMissingPaths(&paths).await?;
        for (source, mut loaded) in sources.into_iter().zip(models) {
            *source.state_mut() = std::mem::take(loaded.state_mut());
        }
        Ok(())
    }
}

impl JsonApiSource for Map<String, Value> {
    fn identifier(&self) -> Option<String> {
        match self.get("id")? {
            Value::String(id) => Some(id.clone()),
            Value::Number(id) if id.is_i64() || id.is_u64() => Some(id.to_string()),
            _ => None,
        }
    }

    fn attributes(&self) -> ResourceResult<Map<String, Value>> {
        let mut attributes = self.clone();
        attributes.shift_remove("id");
        Ok(attributes)
    }
}
