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
use chrono::{DateTime, Utc};

/// Generates localized sample data for tests and seeders.
pub struct Faker {
    locale: String,
    unique: Mutex<HashMap<&'static str, HashSet<String>>>,
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

    pub fn dateTimeBetween(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeBetween", || self.0.dateTimeBetween(start, end))
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
