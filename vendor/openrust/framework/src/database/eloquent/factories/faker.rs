use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Mutex,
};

use crate::{
    container::Container,
    contracts::{config::Repository, container::Container as _},
    database::{Connection, DatabaseError, DatabaseResult},
    translation::canonical_locale,
};
use ::fake::{Fake, faker, locales};
use chrono::{DateTime, Datelike, Days, Months, TimeDelta, TimeZone, Utc};

/// Generates localized sample data for tests and seeders.
pub struct Faker {
    locale: String,
    unique: Mutex<HashMap<&'static str, HashSet<String>>>,
}

fn months_ago(date: DateTime<Utc>, months: u32) -> DatabaseResult<DateTime<Utc>> {
    // PHP normalizes an overflowing day into the following month.
    date.with_day(1)
        .and_then(|first| first.checked_sub_months(Months::new(months)))
        .and_then(|first| first.checked_add_days(Days::new(u64::from(date.day0()))))
        .ok_or(DatabaseError::InvalidQuery(
            "Faker timestamp is out of range",
        ))
}

macro_rules! generate {
    ($locale:expr, $generator:path, $($arg:expr),* $(,)?) => {{
        match $locale {
            "ar_SA" => $generator(locales::AR_SA, $($arg),*).fake(),
            "cy_GB" => $generator(locales::CY_GB, $($arg),*).fake(),
            "de_DE" => $generator(locales::DE_DE, $($arg),*).fake(),
            "fa_IR" => $generator(locales::FA_IR, $($arg),*).fake(),
            "fr_FR" => $generator(locales::FR_FR, $($arg),*).fake(),
            "it_IT" => $generator(locales::IT_IT, $($arg),*).fake(),
            "ja_JP" => $generator(locales::JA_JP, $($arg),*).fake(),
            "nl_NL" => $generator(locales::NL_NL, $($arg),*).fake(),
            "pt_BR" => $generator(locales::PT_BR, $($arg),*).fake(),
            "pt_PT" => $generator(locales::PT_PT, $($arg),*).fake(),
            "tr_TR" => $generator(locales::TR_TR, $($arg),*).fake(),
            "zh_CN" => $generator(locales::ZH_CN, $($arg),*).fake(),
            "zh_TW" => $generator(locales::ZH_TW, $($arg),*).fake(),
            _ => $generator(locales::EN, $($arg),*).fake(),
        }
    }};
}

macro_rules! formatters {
    ($($name:ident => $generator:path),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $name(&self) -> String {
                generate!(self.locale.as_str(), $generator,)
            })+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<String> {
                self.generate(stringify!($name), || self.0.$name())
            })+
        }
    };
}

formatters! {
    name => faker::name::raw::Name,
    firstName => faker::name::raw::FirstName,
    lastName => faker::name::raw::LastName,
    safeEmail => faker::internet::raw::SafeEmail,
    freeEmail => faker::internet::raw::FreeEmail,
    userName => faker::internet::raw::Username,
    city => faker::address::raw::CityName,
    streetName => faker::address::raw::StreetName,
    postcode => faker::address::raw::PostCode,
    phoneNumber => faker::phone_number::raw::PhoneNumber,
    company => faker::company::raw::CompanyName,
    word => faker::lorem::raw::Word,
}

/// Creates a generator using an explicit locale or the scoped application configuration.
pub fn fake(locale: Option<&str>) -> DatabaseResult<Faker> {
    match locale {
        Some(locale) => Faker::new(locale),
        None => Faker::configured(Connection::model_container(None).as_deref()),
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn new(locale: &str) -> DatabaseResult<Self> {
        let locale = canonical_locale(locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        Ok(Self {
            locale,
            unique: Mutex::default(),
        })
    }

    pub(super) fn for_connection(connection: &Connection) -> DatabaseResult<Self> {
        Self::configured(Connection::model_container(Some(connection)).as_deref())
    }

    fn configured(container: Option<&Container>) -> DatabaseResult<Self> {
        let Some(container) = container else {
            return Self::new("en_US");
        };
        let config = container
            .get::<dyn Repository>("config")
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        match config.get("app.faker_locale") {
            Some(serde_json::Value::String(locale)) => Self::new(&locale),
            None => Self::new("en_US"),
            _ => Err(DatabaseError::InvalidConfiguration("app.faker_locale")),
        }
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Generates values without repeating previously returned values for each formatter.
    pub fn unique(&self, reset: bool) -> UniqueFaker<'_> {
        if reset {
            self.unique.lock().unwrap().clear();
        }
        UniqueFaker(self)
    }

    pub fn sentence(&self, words: Range<usize>) -> String {
        generate!(self.locale.as_str(), faker::lorem::raw::Sentence, words)
    }

    /// Samples an inclusive integer interval, accepting endpoints in either order.
    pub fn numberBetween(&self, first: i64, second: i64) -> i64 {
        fastrand::i64(first.min(second)..=first.max(second))
    }

    pub fn randomDigit(&self) -> u8 {
        fastrand::u8(0..=9)
    }

    pub fn randomDigitNotNull(&self) -> u8 {
        fastrand::u8(1..=9)
    }

    pub fn randomDigitNot(&self, except: i64) -> u8 {
        let digit = fastrand::u8(0..=8);
        digit + u8::from(i64::from(digit) >= except)
    }

    /// Generates an integer with at most the requested number of digits.
    pub fn randomNumber(&self, digits: Option<u32>, strict: bool) -> DatabaseResult<i64> {
        let digits = digits.unwrap_or_else(|| u32::from(self.randomDigitNotNull()));
        let max = 10_i64
            .checked_pow(digits)
            .map(|limit| limit - 1)
            .filter(|&limit| limit <= i64::from(i32::MAX))
            .ok_or(DatabaseError::InvalidQuery(
                "Faker random number exceeds digit limit",
            ))?;
        let min = if strict {
            10_i64.pow(digits.saturating_sub(1))
        } else {
            0
        };
        if min > max {
            return Err(DatabaseError::InvalidQuery(
                "Faker strict digit count is zero",
            ));
        }
        Ok(self.numberBetween(min, max))
    }

    /// Samples an inclusive interval at Unix-second precision.
    pub fn dateTimeBetween(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> DatabaseResult<DateTime<Utc>> {
        let (start, end) = (start.timestamp(), end.timestamp());
        if start > end {
            return Err(DatabaseError::InvalidQuery(
                "Faker start date exceeds end date",
            ));
        }
        DateTime::from_timestamp(self.numberBetween(start, end), 0).ok_or(
            DatabaseError::InvalidQuery("Faker timestamp is out of range"),
        )
    }

    /// Samples between the supplied UTC instant and that instant plus a signed interval.
    pub fn dateTimeInInterval(
        &self,
        date: Option<DateTime<Utc>>,
        interval: Option<TimeDelta>,
    ) -> DatabaseResult<DateTime<Utc>> {
        let date = match date {
            Some(date) => date,
            None => months_ago(Utc::now(), 30 * 12)?,
        };
        let other = date
            .checked_add_signed(interval.unwrap_or_else(|| TimeDelta::days(5)))
            .ok_or(DatabaseError::InvalidQuery(
                "Faker timestamp is out of range",
            ))?;
        self.dateTimeBetween(date.min(other), date.max(other))
    }

    pub fn dateTimeThisCentury(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.date_time_since_months_ago(100 * 12, max)
    }

    pub fn dateTimeThisDecade(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.date_time_since_months_ago(10 * 12, max)
    }

    pub fn dateTimeThisYear(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        let now = Utc::now();
        let start = Utc
            .with_ymd_and_hms(now.year(), 1, 1, 0, 0, 0)
            .single()
            .ok_or(DatabaseError::InvalidQuery(
                "Faker timestamp is out of range",
            ))?;
        self.dateTimeBetween(start, max.unwrap_or(now))
    }

    pub fn dateTimeThisMonth(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.date_time_since_months_ago(1, max)
    }

    fn date_time_since_months_ago(
        &self,
        months: u32,
        max: Option<DateTime<Utc>>,
    ) -> DatabaseResult<DateTime<Utc>> {
        let now = Utc::now();
        self.dateTimeBetween(months_ago(now, months)?, max.unwrap_or(now))
    }

    /// Samples whole seconds between the Unix epoch and the supplied upper endpoint.
    pub fn unixTime(&self, max: Option<DateTime<Utc>>) -> i64 {
        let end = max.map_or_else(|| Utc::now().timestamp(), |date| date.timestamp());
        self.numberBetween(0, end)
    }

    /// Returns a sampled instant in UTC.
    pub fn dateTime(&self, max: Option<DateTime<Utc>>) -> DateTime<Utc> {
        DateTime::from_timestamp(self.unixTime(max), 0)
            .expect("sample lies between valid timestamps")
    }

    pub fn date(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%Y-%m-%d"), max)
    }

    pub fn time(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%H:%M:%S"), max)
    }

    pub fn iso8601(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%Y-%m-%dT%H:%M:%S%z").to_string()
    }

    /// Samples between the FakerPHP AD lower bound and the supplied endpoint.
    pub fn dateTimeAD(&self, max: Option<DateTime<Utc>>) -> DateTime<Utc> {
        const FIRST_AD_TIMESTAMP: i64 = -62_135_597_361;
        let end = max.unwrap_or_else(Utc::now).timestamp();
        DateTime::from_timestamp(self.numberBetween(FIRST_AD_TIMESTAMP, end), 0)
            .expect("sample lies between valid timestamps")
    }

    pub fn amPm(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%P").to_string()
    }

    pub fn dayOfMonth(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%d").to_string()
    }

    pub fn dayOfWeek(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%A").to_string()
    }

    pub fn month(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%m").to_string()
    }

    pub fn monthName(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTime(max).format("%B").to_string()
    }

    pub fn year(&self, max: Option<DateTime<Utc>>) -> String {
        let year = self.dateTime(max).year();
        let sign = if year < 0 { "-" } else { "" };
        format!("{sign}{:04}", year.unsigned_abs())
    }

    pub fn century(&self) -> &'static str {
        const CENTURIES: [&str; 21] = [
            "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII",
            "XIV", "XV", "XVI", "XVII", "XVIII", "XIX", "XX", "XXI",
        ];
        CENTURIES[(0..CENTURIES.len()).fake::<usize>()]
    }

    fn format_date(&self, format: &str, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        use std::fmt::Write as _;
        let mut output = String::new();
        write!(output, "{}", self.dateTime(max).format(format))
            .map_err(|_| DatabaseError::InvalidQuery("invalid Faker date format"))?;
        Ok(output)
    }

    /// Samples between the supplied bounds, then rounds to the requested decimal place.
    pub fn randomFloat(
        &self,
        decimals: Option<i32>,
        min: f64,
        max: Option<f64>,
    ) -> DatabaseResult<f64> {
        let decimals = decimals.unwrap_or_else(|| i32::from(self.randomDigit()));
        let max = match max {
            Some(max) => max,
            None => (self.randomNumber(None, false)? as f64).max(min),
        };
        if !min.is_finite() || !max.is_finite() {
            return Err(DatabaseError::InvalidQuery(
                "Faker float bounds must be finite",
            ));
        }
        let (min, max) = (min.min(max), min.max(max));
        let ratio = f64::from(fastrand::i32(0..=i32::MAX)) / f64::from(i32::MAX);
        let span = max - min;
        let value = if span.is_finite() {
            (min + ratio * span).clamp(min, max)
        } else {
            min * (1.0 - ratio) + max * ratio
        };
        round_decimal(value, decimals)
    }
}

fn round_decimal(value: f64, decimals: i32) -> DatabaseResult<f64> {
    if decimals < -308 {
        return Ok(0.0_f64.copysign(value));
    }
    if decimals > 323 {
        return Ok(value);
    }
    let place = 10_f64.powi(decimals.clamp(-308, 308).abs());
    let extra = 10_f64.powi((decimals - 308).max(0));
    let (scaled, step) = if decimals < 0 {
        (value / place, place)
    } else {
        (value * place * extra, 1.0 / place / extra)
    };
    if !scaled.is_finite() || scaled.abs() >= 1e16 {
        return Ok(value);
    }
    // Recover decimal midpoints that fall just below a half after scaling.
    let midpoint = scaled.trunc() + 0.5_f64.copysign(scaled);
    let unscale = |number: f64| {
        if decimals < 0 {
            number * step
        } else {
            number / extra / place
        }
    };
    let rounded = if unscale(midpoint) == value {
        midpoint.round()
    } else {
        scaled.round()
    };
    let value = unscale(rounded);
    if !value.is_finite() {
        return Err(DatabaseError::InvalidQuery(
            "Faker rounded float is out of range",
        ));
    }
    Ok(value)
}

/// A uniqueness modifier that shares the generator's formatter history.
pub struct UniqueFaker<'a>(&'a Faker);

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn sentence(&self, words: Range<usize>) -> DatabaseResult<String> {
        self.generate("sentence", || self.0.sentence(words.clone()))
    }

    pub fn numberBetween(&self, first: i64, second: i64) -> DatabaseResult<i64> {
        self.generate("numberBetween", || self.0.numberBetween(first, second))
    }

    pub fn randomDigit(&self) -> DatabaseResult<u8> {
        self.generate("randomDigit", || self.0.randomDigit())
    }

    pub fn randomDigitNotNull(&self) -> DatabaseResult<u8> {
        self.generate("randomDigitNotNull", || self.0.randomDigitNotNull())
    }

    pub fn randomDigitNot(&self, except: i64) -> DatabaseResult<u8> {
        self.generate("randomDigitNot", || self.0.randomDigitNot(except))
    }

    pub fn randomNumber(&self, digits: Option<u32>, strict: bool) -> DatabaseResult<i64> {
        self.try_generate("randomNumber", || self.0.randomNumber(digits, strict))
    }

    pub fn randomFloat(
        &self,
        decimals: Option<i32>,
        min: f64,
        max: Option<f64>,
    ) -> DatabaseResult<f64> {
        self.try_generate("randomFloat", || {
            self.0
                .randomFloat(decimals, min, max)
                .map(|value| value + 0.0)
        })
    }

    pub fn dateTimeBetween(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeBetween", || self.0.dateTimeBetween(start, end))
    }

    pub fn dateTimeInInterval(
        &self,
        date: Option<DateTime<Utc>>,
        interval: Option<TimeDelta>,
    ) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeInInterval", || {
            self.0.dateTimeInInterval(date, interval)
        })
    }

    pub fn dateTimeThisCentury(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeThisCentury", || self.0.dateTimeThisCentury(max))
    }

    pub fn dateTimeThisDecade(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeThisDecade", || self.0.dateTimeThisDecade(max))
    }

    pub fn dateTimeThisYear(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeThisYear", || self.0.dateTimeThisYear(max))
    }

    pub fn dateTimeThisMonth(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeThisMonth", || self.0.dateTimeThisMonth(max))
    }

    pub fn unixTime(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<i64> {
        self.generate("unixTime", || self.0.unixTime(max))
    }

    pub fn dateTime(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.generate("dateTime", || self.0.dateTime(max))
    }

    pub fn date(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.try_generate("date", || self.0.date(format, max))
    }

    pub fn time(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.try_generate("time", || self.0.time(format, max))
    }

    pub fn iso8601(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("iso8601", || self.0.iso8601(max))
    }

    pub fn dateTimeAD(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.generate("dateTimeAD", || self.0.dateTimeAD(max))
    }

    pub fn amPm(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("amPm", || self.0.amPm(max))
    }

    pub fn dayOfMonth(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("dayOfMonth", || self.0.dayOfMonth(max))
    }

    pub fn dayOfWeek(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("dayOfWeek", || self.0.dayOfWeek(max))
    }

    pub fn month(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("month", || self.0.month(max))
    }

    pub fn monthName(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("monthName", || self.0.monthName(max))
    }

    pub fn year(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("year", || self.0.year(max))
    }

    pub fn century(&self) -> DatabaseResult<&'static str> {
        self.generate("century", || self.0.century())
    }

    fn generate<T: ToString>(
        &self,
        formatter: &'static str,
        mut value: impl FnMut() -> T,
    ) -> DatabaseResult<T> {
        self.try_generate(formatter, || Ok(value()))
    }

    fn try_generate<T: ToString>(
        &self,
        formatter: &'static str,
        mut value: impl FnMut() -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        for _ in 0..10_000 {
            let value = value()?;
            if self
                .0
                .unique
                .lock()
                .unwrap()
                .entry(formatter)
                .or_default()
                .insert(value.to_string())
            {
                return Ok(value);
            }
        }
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    }
}
