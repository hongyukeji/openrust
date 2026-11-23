use std::collections::{HashMap, HashSet};

use crate::database::Value;

use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker};

#[derive(Default)]
pub(super) struct UniqueHistory {
    pub(super) typed: HashMap<&'static str, HashSet<String>>,
    named: HashMap<String, HashSet<Sample>>,
}

#[derive(Eq, Hash, PartialEq)]
enum Sample {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Real(u64),
    Text(String),
    Bytes(Vec<u8>),
    Json(serde_json::Value),
}

impl From<&Value> for Sample {
    fn from(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Integer(value) => Self::Integer(*value),
            Value::Unsigned(value) => Self::Unsigned(*value),
            Value::Real(value) if value.is_nan() => Self::Real(f64::NAN.to_bits()),
            Value::Real(value) => Self::Real(value.to_bits()),
            Value::Text(value) => Self::Text(value.clone()),
            Value::Bytes(value) => Self::Bytes(value.clone()),
            Value::Json(value) => Self::Json(value.clone()),
        }
    }
}

const DEFAULT_RETRIES: usize = 10_000;

impl Faker {
    /// Draws from an inclusive integer range with the supplied acceptance curve.
    /// The default curve is the square root.
    pub fn biased_number_between(
        &self,
        min: Option<i64>,
        max: Option<i64>,
        function: Option<&dyn Fn(f64) -> f64>,
    ) -> DatabaseResult<i64> {
        let (min, max) = (min.unwrap_or(0), max.unwrap_or(100));
        if min > max {
            return Err(DatabaseError::InvalidQuery(
                "Faker biased number range is reversed",
            ));
        }

        const SCALE: u64 = 1 << 53;
        let width = (i128::from(max) - i128::from(min) + 1) as u128;
        for _ in 0..DEFAULT_RETRIES {
            let draw = self.random.u64(..);
            let x = (draw >> 11) as f64 / SCALE as f64;
            let acceptance = function.map_or_else(|| x.sqrt(), |curve| curve(x));
            if !(0.0..=1.0).contains(&acceptance) {
                return Err(DatabaseError::InvalidQuery(
                    "Faker bias must be finite and between 0 and 1",
                ));
            }
            if self.random.f64() < acceptance {
                let offset = (u128::from(draw) * width) >> 64;
                return Ok((i128::from(min) + offset as i128) as i64);
            }
        }
        Err(DatabaseError::InvalidQuery(
            "Faker biased number retries exhausted",
        ))
    }

    /// Runs a formatter with the supplied probability, or returns the default.
    pub fn optional<T>(
        &self,
        weight: Option<f64>,
        default: Option<T>,
        generate: impl FnOnce(&Faker) -> DatabaseResult<T>,
    ) -> DatabaseResult<Option<T>> {
        let weight = weight.unwrap_or(0.5);
        let weight = if weight > 1.0 { weight / 100.0 } else { weight };
        if f64::from(self.random.u8(1..=100)) <= weight * 100.0 {
            generate(self).map(Some)
        } else {
            Ok(default)
        }
    }

    /// Retries a formatter until its value passes the supplied predicate.
    pub fn valid<T>(
        &self,
        mut validator: impl FnMut(&T) -> bool,
        max_retries: Option<usize>,
        mut generate: impl FnMut(&Faker) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        let max_retries = max_retries.unwrap_or(DEFAULT_RETRIES);
        for attempt in 0..=max_retries {
            let value = generate(self)?;
            if attempt < max_retries && validator(&value) {
                return Ok(value);
            }
        }
        Err(DatabaseError::InvalidQuery("Faker valid values exhausted"))
    }
}

impl UniqueFaker<'_> {
    /// Generates a distinct result for a named formatter.
    ///
    /// As in FakerPHP, the supplied spelling identifies the history even though
    /// formatter lookup ignores ASCII case. Arguments and provider replacements
    /// do not start a new history.
    pub fn format(&self, name: &str, arguments: &[Value]) -> DatabaseResult<Value> {
        self.try_unique(
            || self.0.format(name, arguments),
            |value| {
                let sample = Sample::from(value);
                let mut history = self.0.unique.lock().unwrap();
                let samples = match history.named.get_mut(name) {
                    Some(samples) => samples,
                    None => history.named.entry(name.to_owned()).or_default(),
                };
                Ok(samples.insert(sample))
            },
        )
    }

    pub fn biased_number_between(
        &self,
        min: Option<i64>,
        max: Option<i64>,
        function: Option<&dyn Fn(f64) -> f64>,
    ) -> DatabaseResult<i64> {
        self.try_generate("biasedNumberBetween", || {
            self.0.biased_number_between(min, max, function)
        })
    }
}
