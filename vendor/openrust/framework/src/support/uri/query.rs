use std::collections::HashSet;

use serde_json::{Map, Value};

use super::{InvalidUri, Uri, invalid_uri};
use crate::support::{Arr, data_get};

/// A borrowed URI query string with RFC 3986 decoding.
#[derive(Clone, Copy)]
pub struct UriQueryString<'a>(&'a str);

impl<'a> UriQueryString<'a> {
    pub fn value(self) -> &'a str {
        self.0
    }

    pub fn decode(self) -> Result<String, InvalidUri> {
        percent_encoding::percent_decode_str(self.0)
            .decode_utf8()
            .map(std::borrow::Cow::into_owned)
            .map_err(|_| invalid_uri())
    }

    pub fn all(self) -> Result<Value, InvalidUri> {
        crate::http::parse_uri_query(self.0).map_err(|_| invalid_uri())
    }

    pub fn get(self, key: &str) -> Result<Option<Value>, InvalidUri> {
        Ok(data_get(&self.all()?, Some(key)))
    }

    pub fn has(self, key: &str) -> Result<bool, InvalidUri> {
        Ok(Arr::has(&self.all()?, key))
    }

    pub fn missing(self, key: &str) -> Result<bool, InvalidUri> {
        Ok(!self.has(key)?)
    }
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Uri {
    pub fn query(&self) -> UriQueryString<'_> {
        UriQueryString(self.queryString().unwrap_or_default())
    }

    pub fn replaceQuery(&self, parameters: &Value) -> Result<Self, InvalidUri> {
        self.update_query(Value::Object(Map::new()), parameters, false)
    }

    pub fn with_query(&self, parameters: &Value) -> Result<Self, InvalidUri> {
        self.update_query(self.query().all()?, parameters, false)
    }

    pub fn withQueryIfMissing(&self, parameters: &Value) -> Result<Self, InvalidUri> {
        self.update_query(self.query().all()?, parameters, true)
    }

    pub fn withoutQuery(&self, keys: &[&str]) -> Result<Self, InvalidUri> {
        let mut query = self.query().all()?;
        for key in keys {
            Arr::forget(&mut query, key).map_err(|_| invalid_uri())?;
        }
        self.encode_query(query)
    }

    pub fn pushOntoQuery(&self, key: &str, value: Value) -> Result<Self, InvalidUri> {
        let mut query = self.query().all()?;
        super::super::data::check_depth(&value, 0).map_err(|_| invalid_uri())?;
        let current = data_get(&query, Some(key)).unwrap_or(Value::Null);
        let deduplicate = current.is_array();
        let mut values = Map::new();
        for source in [current, value] {
            for (entry, value) in entries(source) {
                if let Some(entry) = entry {
                    values.insert(entry, value);
                } else {
                    let next = values.keys().filter(|key| numeric_key(key)).count();
                    values.insert(next.to_string(), value);
                }
            }
        }
        let values = if deduplicate {
            let mut seen = HashSet::new();
            let mut list = Vec::new();
            for value in values.into_iter().map(|(_, value)| value) {
                let key = match &value {
                    Value::Array(_) | Value::Object(_) => return Err(invalid_uri()),
                    Value::Null | Value::Bool(false) => String::new(),
                    Value::Bool(true) => "1".into(),
                    Value::String(value) => value.clone(),
                    value => value.to_string(),
                };
                if seen.insert(key) {
                    list.push(value);
                }
            }
            Value::Array(list)
        } else {
            Value::Object(values)
        };
        crate::support::data::set(&mut query, key, values, true, false)
            .map_err(|_| invalid_uri())?;
        self.encode_query(query)
    }

    fn update_query(
        &self,
        mut current: Value,
        additional: &Value,
        missing_only: bool,
    ) -> Result<Self, InvalidUri> {
        super::super::data::check_depth(additional, 0).map_err(|_| invalid_uri())?;
        let parameters = match additional {
            Value::Object(parameters) => parameters.clone(),
            Value::Array(parameters) => parameters
                .iter()
                .enumerate()
                .map(|(i, value)| (i.to_string(), value.clone()))
                .collect(),
            _ => return Err(invalid_uri()),
        };
        for (key, value) in parameters {
            if !missing_only || Arr::get(&current, &key).is_none() {
                crate::support::data::set(&mut current, &key, value, true, false)
                    .map_err(|_| invalid_uri())?;
            }
        }
        self.encode_query(current)
    }

    fn encode_query(&self, value: Value) -> Result<Self, InvalidUri> {
        let encoded = crate::routing::encode_query_value(value).map_err(|_| invalid_uri())?;
        let mut parts = self.parts.clone();
        parts.query = (!encoded.is_empty()).then_some(encoded);
        Self::checked_parts(parts)
    }
}

fn numeric_key(key: &str) -> bool {
    key.parse::<i64>()
        .is_ok_and(|value| value.to_string() == key)
}

fn entries(value: Value) -> Vec<(Option<String>, Value)> {
    match value {
        Value::Null => Vec::new(),
        Value::Object(values) => values
            .into_iter()
            .map(|(key, value)| ((!numeric_key(&key)).then_some(key), value))
            .collect(),
        Value::Array(values) => values.into_iter().map(|value| (None, value)).collect(),
        value => vec![(None, value)],
    }
}
