use indexmap::IndexMap;
use serde_json::Value;

use super::super::json::ResourceResult;

type Attribute<'a> = Box<dyn FnOnce() -> ResourceResult<Value> + 'a>;

/// JSON:API attributes evaluated only after applying the sparse fieldset.
#[derive(Default)]
pub struct JsonApiAttributes<'a>(pub(super) IndexMap<&'a str, Attribute<'a>>);

impl<'a> JsonApiAttributes<'a> {
    #[must_use]
    pub fn attribute(
        mut self,
        name: &'a str,
        resolve: impl FnOnce() -> ResourceResult<Value> + 'a,
    ) -> Self {
        self.0.insert(name, Box::new(resolve));
        self
    }
}
