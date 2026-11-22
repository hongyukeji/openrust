use fixed_decimal::{Decimal, FloatPrecision, Sign, SignedRoundingMode, UnsignedRoundingMode};
use icu_decimal::DecimalFormatter;
use icu_experimental::dimension::{currency::CurrencyType, percent::formatter::PercentFormatter};
use icu_locale_core::Locale;

mod currency;
mod parse;
mod spellout;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberValue {
    Integer(i64),
    Float(f64),
}

impl From<i64> for NumberValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<i32> for NumberValue {
    fn from(value: i32) -> Self {
        i64::from(value).into()
    }
}

impl From<f64> for NumberValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl NumberValue {
    fn as_float(self) -> f64 {
        match self {
            Self::Integer(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn decimal(self) -> Result<Decimal, NumberError> {
        match self {
            Self::Integer(value) => Ok(value.into()),
            Self::Float(value) => Decimal::try_from_f64(value, FloatPrecision::RoundTrip)
                .map_err(|_| NumberError::NonFinite),
        }
    }

    fn at_least(self, other: Self) -> bool {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => left >= right,
            _ => self.as_float() >= other.as_float(),
        }
    }

    fn less(self, other: Self) -> bool {
        match (self, other) {
            (Self::Integer(left), Self::Integer(right)) => left < right,
            _ => self.as_float() < other.as_float(),
        }
    }
}

#[derive(Debug)]
pub enum NumberError {
    InvalidLocale,
    InvalidCurrency,
    LocaleData(icu_provider::DataError),
    NonFinite,
    InvalidNumber,
    UnsupportedSpellout,
    InvalidStep,
}

impl std::fmt::Display for NumberError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidLocale => "无效的 BCP 47 语言标签",
            Self::InvalidCurrency => "invalid ISO currency code",
            Self::LocaleData(_) => "number formatting data unavailable",
            Self::NonFinite => "this operation requires a finite number",
            Self::InvalidNumber => "invalid localized number or number out of range",
            Self::UnsupportedSpellout => "number spellout is unavailable for this locale or value",
            Self::InvalidStep => "pair step must be nonzero and advance without overflowing",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for NumberError {}

impl From<icu_provider::DataError> for NumberError {
    fn from(error: icu_provider::DataError) -> Self {
        Self::LocaleData(error)
    }
}

/// Locale and currency defaults for a caller or application.
#[derive(Clone, Debug)]
pub struct Number {
    locale: Locale,
    currency: CurrencyType,
}

impl Default for Number {
    fn default() -> Self {
        Self {
            locale: icu_locale_core::locale!("en"),
            currency: CurrencyType::try_from_str("USD").expect("valid USD currency code"),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Number API")]
impl Number {
    pub fn useLocale(&mut self, locale: &str) -> Result<(), NumberError> {
        self.locale = crate::translation::canonical_locale(locale)
            .map_err(|_| NumberError::InvalidLocale)?
            .parse()
            .map_err(|_| NumberError::InvalidLocale)?;
        Ok(())
    }

    pub fn useCurrency(&mut self, currency: &str) -> Result<(), NumberError> {
        self.currency =
            CurrencyType::try_from_str(currency).map_err(|_| NumberError::InvalidCurrency)?;
        Ok(())
    }

    pub fn withLocale<T>(
        &self,
        locale: &str,
        callback: impl FnOnce(&Self) -> T,
    ) -> Result<T, NumberError> {
        let mut scoped = self.clone();
        scoped.useLocale(locale)?;
        Ok(callback(&scoped))
    }

    pub fn withCurrency<T>(
        &self,
        currency: &str,
        callback: impl FnOnce(&Self) -> T,
    ) -> Result<T, NumberError> {
        let mut scoped = self.clone();
        scoped.useCurrency(currency)?;
        Ok(callback(&scoped))
    }

    pub fn defaultLocale(&self) -> &Locale {
        &self.locale
    }

    pub fn defaultCurrency(&self) -> CurrencyType {
        self.currency
    }

    fn formatter(&self) -> Result<DecimalFormatter, NumberError> {
        Ok(DecimalFormatter::try_new(
            self.locale.clone().into(),
            Default::default(),
        )?)
    }

    pub fn format(
        &self,
        number: impl Into<NumberValue>,
        precision: Option<u8>,
        max_precision: Option<u8>,
    ) -> Result<String, NumberError> {
        let formatter = self.formatter()?;
        let number = number.into();
        let Some(special) = special_value(number) else {
            let value = round(number.decimal()?, precision, max_precision, 3);
            return Ok(formatter.format_to_string(&value));
        };
        let zero = formatter.format_to_string(&Decimal::from(0));
        let sample = formatter.format_to_string(&Decimal::from(0).with_sign(if special == "-∞" {
            Sign::Negative
        } else {
            Sign::None
        }));
        Ok(sample.replacen(&zero, special.trim_start_matches('-'), 1))
    }

    pub fn percentage(
        &self,
        number: impl Into<NumberValue>,
        precision: u8,
        max_precision: Option<u8>,
    ) -> Result<String, NumberError> {
        let decimal_formatter = self.formatter()?;
        let formatter = PercentFormatter::try_new_with_decimal_formatter(
            self.locale.clone().into(),
            decimal_formatter.clone(),
            Default::default(),
        )?;
        let number = number.into();
        match special_value(number) {
            Some(special) => {
                let zero = decimal_formatter.format_to_string(&Decimal::from(0));
                let sample = Decimal::from(0).with_sign(if special == "-∞" {
                    Sign::Negative
                } else {
                    Sign::None
                });
                Ok(formatter.format(&sample).to_string().replacen(
                    &zero,
                    special.trim_start_matches('-'),
                    1,
                ))
            }
            None => {
                let value = round(number.decimal()?, Some(precision), max_precision, 0);
                Ok(formatter.format(&value).to_string())
            }
        }
    }

    pub fn currency(
        &self,
        number: impl Into<NumberValue>,
        currency: Option<&str>,
        precision: Option<u8>,
    ) -> Result<String, NumberError> {
        let currency = currency
            .filter(|code| !code.is_empty())
            .map(CurrencyType::try_from_str)
            .transpose()
            .map_err(|_| NumberError::InvalidCurrency)?
            .unwrap_or(self.currency);
        currency::format(&self.locale, number.into(), currency, precision)
    }

    pub fn parse(&self, input: &str) -> Result<f64, NumberError> {
        self.parseFloat(input)
    }

    pub fn parseInt(&self, input: &str) -> Result<i64, NumberError> {
        let value = parse::decimal(&self.formatter()?, input)?;
        let integer = value
            .split_once('.')
            .map_or(value.as_str(), |(integer, _)| integer);
        integer.parse().map_err(|_| NumberError::InvalidNumber)
    }

    pub fn parseFloat(&self, input: &str) -> Result<f64, NumberError> {
        parse::decimal(&self.formatter()?, input)?
            .parse()
            .map_err(|_| NumberError::InvalidNumber)
    }

    pub fn spell(
        &self,
        number: impl Into<NumberValue>,
        after: Option<i64>,
        until: Option<i64>,
    ) -> Result<String, NumberError> {
        let number = number.into();
        if after.is_some_and(|limit| NumberValue::from(limit).at_least(number))
            || until.is_some_and(|limit| number.at_least(limit.into()))
        {
            return self.format(number, None, None);
        }
        if number == NumberValue::Integer(i64::MIN) {
            return self.format(number, None, None);
        }
        spellout::format(&self.locale, number, false)
    }

    pub fn ordinal(&self, number: impl Into<NumberValue>) -> Result<String, NumberError> {
        let number = ordinal_integer(number.into())?;
        let locale = self.locale.to_string();
        let ordinal = intl::number::format_ordinal(&locale, number);
        // The RBNF helper chooses the suffix from the integer, then renders
        // the numeric part as f64. Keep the suffix and render exact digits.
        let prefix = intl::number::format_decimal(&locale, number as f64);
        let suffix = ordinal.strip_prefix(&prefix).unwrap_or_default();
        Ok(format!("{}{suffix}", self.format(number, None, None)?))
    }

    pub fn spellOrdinal(&self, number: impl Into<NumberValue>) -> Result<String, NumberError> {
        let number = number.into();
        if number == NumberValue::Integer(i64::MIN) {
            return self.format(number, None, None);
        }
        spellout::format(&self.locale, number, true)
    }

    pub fn fileSize(
        &self,
        bytes: impl Into<NumberValue>,
        precision: u8,
        max_precision: Option<u8>,
    ) -> Result<String, NumberError> {
        let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
        let bytes = bytes.into();
        let mut scaled = bytes.as_float();
        let mut index = 0;
        if scaled.is_finite() {
            while scaled.abs() / 1024.0 > 0.9 && index + 1 < units.len() {
                scaled /= 1024.0;
                index += 1;
            }
        }
        let amount = if index == 0 { bytes } else { scaled.into() };
        Ok(format!(
            "{} {}",
            self.format(amount, Some(precision), max_precision)?,
            units[index]
        ))
    }

    pub fn abbreviate(
        &self,
        number: impl Into<NumberValue>,
        precision: u8,
        max_precision: Option<u8>,
    ) -> Result<String, NumberError> {
        self.forHumans(number, precision, max_precision, true)
    }

    pub fn forHumans(
        &self,
        number: impl Into<NumberValue>,
        precision: u8,
        max_precision: Option<u8>,
        abbreviate: bool,
    ) -> Result<String, NumberError> {
        let units = if abbreviate {
            ["", "K", "M", "B", "T", "Q"]
        } else {
            [
                "",
                " thousand",
                " million",
                " billion",
                " trillion",
                " quadrillion",
            ]
        };
        let number = number.into();
        if special_value(number).is_some() {
            return self.format(number, Some(precision), max_precision);
        }
        let decimal = number.decimal()?;
        if decimal.is_zero() && precision == 0 {
            return Ok("0".into());
        }
        let negative = decimal.sign() == Sign::Negative;
        let mut decimal = decimal.with_sign(Sign::None);
        let mut units_used = Vec::new();
        while decimal.nonzero_magnitude_start() >= 15 {
            decimal.multiply_pow10(-15);
            units_used.push(units[5]);
        }
        let mut unit = usize::try_from(decimal.nonzero_magnitude_start().max(0) / 3).unwrap_or(0);
        let unrounded = decimal.clone();
        decimal.multiply_pow10(-3 * i16::try_from(unit).unwrap_or(0));
        decimal = round(decimal, Some(precision), max_precision, 0);
        if decimal.nonzero_magnitude_start() >= 3 && unit + 1 < units.len() {
            unit += 1;
            decimal = unrounded;
            decimal.multiply_pow10(-3 * i16::try_from(unit).unwrap_or(0));
            decimal = round(decimal, Some(precision), max_precision, 0);
        }
        let sign = if negative && !decimal.is_zero() {
            "-"
        } else {
            ""
        };
        Ok(format!(
            "{sign}{}{}{}",
            self.formatter()?.format_to_string(&decimal),
            units[unit],
            units_used.concat()
        ))
    }

    /// Selects an original operand, preserving its integer or float type.
    pub fn clamp(
        number: impl Into<NumberValue>,
        min: impl Into<NumberValue>,
        max: impl Into<NumberValue>,
    ) -> NumberValue {
        let (number, min, max) = (number.into(), min.into(), max.into());
        let lower = if number.at_least(min) { number } else { min };
        if lower.less(max) { lower } else { max }
    }

    pub fn pairs(
        to: i64,
        by: i64,
        start: i64,
        offset: i64,
    ) -> Result<Vec<(i64, i64)>, NumberError> {
        let step = i128::from(by).abs();
        if step == 0 {
            return Err(NumberError::InvalidStep);
        }
        let (mut lower, end) = (i128::from(start), i128::from(to));
        let mut output = Vec::new();
        while lower < end {
            let upper = (lower + step - i128::from(offset)).min(end);
            output.push((
                i64::try_from(lower).map_err(|_| NumberError::InvalidStep)?,
                i64::try_from(upper).map_err(|_| NumberError::InvalidStep)?,
            ));
            lower += step;
        }
        if lower == end && output.last().is_some_and(|&(_, upper)| upper < to) {
            output.push((to, to));
        }
        Ok(output)
    }

    /// Splits a floating-point range, with the same final singleton rule as `pairs`.
    pub fn pairsFloat(
        to: f64,
        by: f64,
        start: f64,
        offset: f64,
    ) -> Result<Vec<(f64, f64)>, NumberError> {
        if ![to, by, start, offset].into_iter().all(f64::is_finite) {
            return Err(NumberError::NonFinite);
        }
        let step = by.abs();
        if step == 0.0 {
            return Err(NumberError::InvalidStep);
        }
        let mut lower = start;
        let mut output = Vec::new();
        while lower < to {
            let next = lower + step;
            if next <= lower || !next.is_finite() {
                return Err(NumberError::InvalidStep);
            }
            let upper = (next - offset).min(to);
            if !upper.is_finite() {
                return Err(NumberError::InvalidStep);
            }
            output.push((lower, upper));
            lower = next;
        }
        if lower == to && output.last().is_some_and(|&(_, upper)| upper < to) {
            output.push((to, to));
        }
        Ok(output)
    }

    pub fn trim(number: impl Into<NumberValue>) -> NumberValue {
        let number = number.into();
        match number {
            NumberValue::Float(value) if value.fract() == 0.0 => {
                Decimal::try_from_f64(value, FloatPrecision::Integer)
                    .ok()
                    .and_then(|decimal| decimal.to_string().parse::<i64>().ok())
                    .map_or(number, NumberValue::Integer)
            }
            _ => number,
        }
    }
}

pub(super) fn special_value(number: NumberValue) -> Option<&'static str> {
    match number {
        NumberValue::Float(value) if value.is_nan() => Some("NaN"),
        NumberValue::Float(f64::INFINITY) => Some("∞"),
        NumberValue::Float(f64::NEG_INFINITY) => Some("-∞"),
        _ => None,
    }
}

fn round(
    mut value: Decimal,
    precision: Option<u8>,
    max_precision: Option<u8>,
    default: u8,
) -> Decimal {
    let digits = -i16::from(max_precision.or(precision).unwrap_or(default));
    if *value.magnitude_range().start() < digits {
        value.round_with_mode(
            digits,
            SignedRoundingMode::Unsigned(UnsignedRoundingMode::HalfEven),
        );
    }
    value.trim_end();
    if let (Some(digits), None) = (precision, max_precision) {
        value.pad_end(-i16::from(digits));
    }
    if value.is_zero() {
        value.with_sign(Sign::None)
    } else {
        value
    }
}

fn ordinal_integer(number: NumberValue) -> Result<i64, NumberError> {
    let value = number.decimal()?;
    let text = value.to_string();
    text.split_once('.')
        .map_or(text.as_str(), |(integer, _)| integer)
        .parse()
        .map_err(|_| NumberError::InvalidNumber)
}
