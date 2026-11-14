use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker};

const DEFAULT_RETRIES: usize = 10_000;

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    /// Draws from an inclusive integer range with the supplied acceptance curve.
    /// The default curve is the square root.
    pub fn biasedNumberBetween(
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
            let draw = fastrand::u64(..);
            let x = (draw >> 11) as f64 / SCALE as f64;
            let acceptance = function.map_or_else(|| x.sqrt(), |curve| curve(x));
            if !(0.0..=1.0).contains(&acceptance) {
                return Err(DatabaseError::InvalidQuery(
                    "Faker bias must be finite and between 0 and 1",
                ));
            }
            if fastrand::f64() < acceptance {
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
        if f64::from(fastrand::u8(1..=100)) <= weight * 100.0 {
            generate(self).map(Some)
        } else {
            Ok(default)
        }
    }

    /// Retries a formatter until its value passes the supplied predicate.
    pub fn valid<T>(
        &self,
        mut validator: impl FnMut(&T) -> bool,
        maxRetries: Option<usize>,
        mut generate: impl FnMut(&Faker) -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        let maxRetries = maxRetries.unwrap_or(DEFAULT_RETRIES);
        for attempt in 0..=maxRetries {
            let value = generate(self)?;
            if attempt < maxRetries && validator(&value) {
                return Ok(value);
            }
        }
        Err(DatabaseError::InvalidQuery("Faker valid values exhausted"))
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn biasedNumberBetween(
        &self,
        min: Option<i64>,
        max: Option<i64>,
        function: Option<&dyn Fn(f64) -> f64>,
    ) -> DatabaseResult<i64> {
        self.try_generate("biasedNumberBetween", || {
            self.0.biasedNumberBetween(min, max, function)
        })
    }
}
