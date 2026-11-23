use std::collections::BTreeMap;

use serde_json::Number;

use crate::{
    session::{SessionError, SessionFuture, SessionResult, Value, remove_attribute},
    support::Arr,
};

/// Accesses the state loaded for an HTTP request.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
pub trait Session: Send + Sync {
    fn get_name(&self) -> &str;
    fn set_name(&mut self, name: &str) -> SessionResult<()>;
    fn get_id(&self) -> &str;
    fn set_id(&mut self, id: Option<&str>) -> SessionResult<()>;
    fn is_started(&self) -> bool;
    fn start(&mut self) -> SessionFuture<'_, SessionResult<()>>;
    fn save(&mut self) -> SessionFuture<'_, SessionResult<()>>;

    fn all(&self) -> &BTreeMap<String, Value>;
    fn exists(&self, key: &str) -> bool;
    fn has(&self, key: &str) -> bool;
    fn get(&self, key: &str) -> Option<&Value>;
    fn pull(&mut self, key: &str) -> Option<Value>;
    fn put(&mut self, key: &str, value: Value) -> SessionResult<()>;
    fn forget(&mut self, keys: &[&str]);
    fn flush(&mut self);

    /// Stores each key and value using the same path rules as `put`.
    fn putMany(&mut self, values: &[(String, Value)]) -> SessionResult<()> {
        for (key, value) in values {
            self.put(key, value.clone())?;
        }
        Ok(())
    }

    /// Replaces the supplied attributes and keeps all other attributes.
    fn replace(&mut self, values: &[(String, Value)]) -> SessionResult<()> {
        self.putMany(values)
    }

    /// Returns matching top-level keys, including explicitly null values.
    fn only(&self, keys: &[&str]) -> BTreeMap<String, Value> {
        let attributes = self.all();
        keys.iter()
            .filter_map(|key| {
                attributes
                    .get(*key)
                    .map(|value| ((*key).into(), value.clone()))
            })
            .collect()
    }

    /// Returns a copy with the given keys or dot paths removed.
    fn except(&self, keys: &[&str]) -> BTreeMap<String, Value> {
        let mut attributes = self.all().clone();
        for key in keys {
            remove_attribute(&mut attributes, key);
        }
        attributes
    }

    fn missing(&self, key: &str) -> bool {
        !self.exists(key)
    }

    fn has_any(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| self.has(key))
    }

    /// Calculates and stores a value if the key is missing or null.
    fn remember(
        &mut self,
        key: &str,
        calculate: impl FnOnce() -> SessionResult<Value>,
    ) -> SessionResult<Value>
    where
        Self: Sized,
    {
        if let Some(value) = self.get(key).filter(|value| !value.is_null()) {
            return Ok(value.clone());
        }
        let value = calculate()?;
        self.put(key, value.clone())?;
        Ok(value)
    }

    fn push(&mut self, key: &str, item: Value) -> SessionResult<()> {
        let mut value = match self.get(key) {
            None | Some(Value::Null | Value::Bool(false)) => Value::Array(Vec::new()),
            Some(value) => value.clone(),
        };
        Arr::push(&mut value, None, [item]).map_err(|_| SessionError::InvalidValue)?;
        self.put(key, value)
    }

    fn increment(&mut self, key: &str, amount: i64) -> SessionResult<Value> {
        let value = add(self.get(key), amount.into())?;
        self.put(key, value.clone())?;
        Ok(value)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "PHP promotes the negated minimum integer to a double"
    )]
    fn decrement(&mut self, key: &str, amount: i64) -> SessionResult<Value> {
        let negative = amount
            .checked_neg()
            .map(Number::from)
            .unwrap_or_else(|| Number::from_f64(-(amount as f64)).expect("finite integer"));
        let value = add(self.get(key), negative)?;
        self.put(key, value.clone())?;
        Ok(value)
    }

    fn flash(&mut self, key: &str, value: Value) -> SessionResult<()>;
    fn reflash(&mut self);
    fn keep(&mut self, keys: &[&str]);

    fn token(&self) -> Option<&str>;
    fn regenerate_token(&mut self) -> SessionResult<()>;
    fn migrate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>>;
    fn regenerate(&mut self, destroy: bool) -> SessionFuture<'_, SessionResult<()>>;
    fn invalidate(&mut self) -> SessionFuture<'_, SessionResult<()>>;
}

fn add(current: Option<&Value>, amount: Number) -> SessionResult<Value> {
    let left = match current {
        None | Some(Value::Null | Value::Bool(false)) => Some(0.into()),
        Some(Value::Bool(true)) => Some(1.into()),
        Some(Value::Number(number)) => Some(number.clone()),
        Some(Value::String(text)) => {
            let text = text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
            text.parse::<i64>()
                .ok()
                .map(Number::from)
                .or_else(|| text.parse::<f64>().ok().and_then(Number::from_f64))
        }
        Some(Value::Array(_) | Value::Object(_)) => None,
    }
    .ok_or(SessionError::InvalidValue)?;
    if let (Some(left), Some(right)) = (left.as_i64(), amount.as_i64())
        && let Some(sum) = left.checked_add(right)
    {
        return Ok(sum.into());
    }
    let sum = left.as_f64().ok_or(SessionError::InvalidValue)?
        + amount.as_f64().ok_or(SessionError::InvalidValue)?;
    Number::from_f64(sum)
        .map(Value::Number)
        .ok_or(SessionError::InvalidValue)
}
