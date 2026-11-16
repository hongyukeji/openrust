use std::fmt::Display;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned, ser::Error as _,
};
use serde_json::Value;

use super::{Collection, CollectionResult, LazyCollection};
use crate::support::{data_get, data_has};

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: Serialize + 'source> LazyCollection<'source, T> {
    pub fn toArray(&self) -> CollectionResult<Value> {
        Self::collect(self)?.toArray()
    }

    pub fn jsonSerialize(&self) -> CollectionResult<Value> {
        self.toArray()
    }

    pub fn toJson(&self) -> CollectionResult<String> {
        Self::collect(self)?.toJson()
    }

    pub fn toPrettyJson(&self) -> CollectionResult<String> {
        Self::collect(self)?.toPrettyJson()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl<'source, T: DeserializeOwned + Clone + 'source> LazyCollection<'source, T> {
    pub fn fromJson(json: &str) -> CollectionResult<Self> {
        Collection::fromJson(json).map(Into::into)
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

impl<'source, T: Display + 'source> LazyCollection<'source, T> {
    pub fn join(&self, glue: &str, final_glue: &str) -> CollectionResult<String> {
        Ok(Self::collect(self)?.join(glue, final_glue))
    }

    pub fn implode(&self, glue: &str) -> CollectionResult<String> {
        self.join(glue, "")
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's LazyCollection API")]
impl LazyCollection<'_, Value> {
    pub fn value(&self, path: &str) -> CollectionResult<Option<Value>> {
        Ok(self
            .firstWhere(|value, _| data_has(value, path))?
            .and_then(|value| data_get(&value, Some(path))))
    }

    pub fn dot(self) -> Self {
        self.dotWithDepth(None)
    }

    pub fn dotWithDepth(self, depth: Option<usize>) -> Self {
        self.try_passthru(move |items| items.dotWithDepth(depth))
    }

    pub fn undot(self) -> Self {
        self.try_passthru(Collection::undot)
    }
}
