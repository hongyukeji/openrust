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

    pub fn name(&self) -> String {
        generate!(self.locale.as_str(), faker::name::raw::Name,)
    }

    pub fn safeEmail(&self) -> String {
        generate!(self.locale.as_str(), faker::internet::raw::SafeEmail,)
    }

    pub fn city(&self) -> String {
        generate!(self.locale.as_str(), faker::address::raw::CityName,)
    }

    pub fn sentence(&self, words: Range<usize>) -> String {
        generate!(self.locale.as_str(), faker::lorem::raw::Sentence, words)
    }
}

/// A uniqueness modifier that shares the generator's formatter history.
pub struct UniqueFaker<'a>(&'a Faker);

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn name(&self) -> DatabaseResult<String> {
        self.generate("name", || self.0.name())
    }

    pub fn safeEmail(&self) -> DatabaseResult<String> {
        self.generate("safeEmail", || self.0.safeEmail())
    }

    pub fn city(&self) -> DatabaseResult<String> {
        self.generate("city", || self.0.city())
    }

    pub fn sentence(&self, words: Range<usize>) -> DatabaseResult<String> {
        self.generate("sentence", || self.0.sentence(words.clone()))
    }

    fn generate(
        &self,
        formatter: &'static str,
        mut value: impl FnMut() -> String,
    ) -> DatabaseResult<String> {
        for _ in 0..10_000 {
            let value = value();
            if self
                .0
                .unique
                .lock()
                .unwrap()
                .entry(formatter)
                .or_default()
                .insert(value.clone())
            {
                return Ok(value);
            }
        }
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    }
}
