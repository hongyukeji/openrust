use std::fmt::Display;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned, ser::Error as _,
};
use serde_json::Value;

use super::{ArrayKey, Collection, CollectionResult, LazyCollection};
use crate::support::{data_get, data_has, escape_html};

impl<'source, T: Serialize + 'source> LazyCollection<'source, T> {
    pub fn to_array(&self) -> CollectionResult<Value> {
        Self::collect(self)?.to_array()
    }

    pub fn json_serialize(&self) -> CollectionResult<Value> {
        self.to_array()
    }

    pub fn to_json(&self) -> CollectionResult<String> {
        Self::collect(self)?.to_json()
    }

    /// Selects HTML escaping for the collection's explicit string conversion.
    pub fn escape_when_casting_to_string(mut self, escape: bool) -> Self {
        self.escape_when_casting_to_string = escape;
        self
    }

    /// Converts the collection to JSON text, optionally escaped for HTML.
    pub fn to_string(&self) -> CollectionResult<String> {
        let json = self.to_json()?;
        if !self.escape_when_casting_to_string {
            return Ok(json);
        }
        let mut output = String::with_capacity(json.len());
        escape_html(&mut output, &json);
        Ok(output)
    }

    pub fn to_pretty_json(&self) -> CollectionResult<String> {
        Self::collect(self)?.to_pretty_json()
    }
}

impl<'source, T: DeserializeOwned + Clone + 'source> LazyCollection<'source, T> {
    pub fn from_json(json: &str) -> CollectionResult<Self> {
        Collection::from_json(json).map(Into::into)
    }
}

impl<'source, T: Serialize + 'source> Serialize for LazyCollection<'source, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Self::collect(self)
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de, 'source, T: Deserialize<'de> + Clone + 'source> Deserialize<'de>
    for LazyCollection<'source, T>
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Collection::deserialize(deserializer).map(Into::into)
    }
}

impl<'source, T: 'source> LazyCollection<'source, T> {
    /// Materializes keyed entries before projecting and joining them.
    pub fn implode_by<U: Display>(
        &self,
        project: impl FnMut(&T, &ArrayKey) -> U,
        glue: &str,
    ) -> CollectionResult<String> {
        Ok(Self::collect(self)?.implode_by(project, glue))
    }
}

impl<'source, T: Display + 'source> LazyCollection<'source, T> {
    pub fn join(&self, glue: &str, final_glue: &str) -> CollectionResult<String> {
        Ok(Self::collect(self)?.join(glue, final_glue))
    }

    pub fn implode(&self, glue: &str) -> CollectionResult<String> {
        self.join(glue, "")
    }
}

impl LazyCollection<'_, Value> {
    pub fn value(&self, path: &str) -> CollectionResult<Option<Value>> {
        Ok(self
            .first_where(|value, _| data_has(value, path))?
            .and_then(|value| data_get(&value, Some(path))))
    }

    /// Evaluates the fallback only when no item contains the path.
    pub fn value_or_else(
        &self,
        path: &str,
        fallback: impl FnOnce() -> Value,
    ) -> CollectionResult<Value> {
        Ok(self.value(path)?.unwrap_or_else(fallback))
    }

    pub fn dot(self) -> Self {
        self.dot_with_depth(None)
    }

    pub fn dot_with_depth(self, depth: Option<usize>) -> Self {
        self.try_passthru(move |items| items.dot_with_depth(depth))
    }

    pub fn undot(self) -> Self {
        self.try_passthru(Collection::undot)
    }
}
