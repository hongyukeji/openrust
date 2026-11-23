use super::{
    faker_address::{self, DISTRICTS_ZH_CN},
    faker_file::MIME_TYPES,
    faker_institutions, faker_miscellaneous,
    faker_payment::{
        CARD_VENDORS, CARD_VENDORS_PT_BR, IBAN_FORMATS, account_iban_defaults, card_masks,
        card_masks_pt_br, uses_us_payment_provider,
    },
    faker_provider::{Extensions, Formatters},
    faker_rng::FakerRandom,
    regex_pattern::RegexPattern,
};
use crate::support::timestamp_seconds;
use std::{
    collections::HashMap,
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io,
    net::Ipv4Addr,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use self::modifiers::UniqueHistory;

use crate::{
    container::Container,
    contracts::{config::Repository, container::Container as _},
    database::{Connection, DatabaseError, DatabaseResult},
    support::{Str, calendar},
    translation::canonical_locale,
};
use ::fake::{Fake, faker, locales};
use aws_lc_rs::digest;
use jiff::{
    SignedDuration, Span, Timestamp, Zoned,
    civil::{Date as CivilDate, DateTime as CivilDateTime},
    tz::{AmbiguousOffset, Offset, TimeZone},
};
use md5::{Digest as _, Md5};

#[path = "faker_color.rs"]
mod color;

#[path = "faker_html_lorem.rs"]
mod html_lorem;

#[path = "faker_identity.rs"]
mod identity;

#[path = "faker_license_plate.rs"]
mod license_plate;

#[path = "faker_modifiers.rs"]
mod modifiers;

#[path = "faker_text.rs"]
mod text;

#[path = "faker_user_agent.rs"]
mod user_agent;

/// A sampled latitude and longitude in signed decimal degrees.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

/// The fields returned by the sample credit-card formatter.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditCardDetails {
    pub r#type: &'static str,
    pub number: String,
    pub name: String,
    pub expiration_date: String,
}

// These catalog-only formatters share the same locale and uniqueness contracts.
macro_rules! institution_formatters {
    ($($method:ident [$history:literal] => $locale:literal, $catalog:ident;)*) => {
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.institution_sample($locale, faker_institutions::$catalog)
            })*
        }

        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.try_generate($history, || self.0.$method())
            })*
        }
    };
}

institution_formatters! {
    local_bank ["localBank"] => "ms_MY", MS_MY_LOCAL_BANKS;
    foreign_bank ["foreignBank"] => "ms_MY", MS_MY_FOREIGN_BANKS;
    government_bank ["governmentBank"] => "ms_MY", MS_MY_GOVERNMENT_BANKS;
    insurance ["insurance"] => "ms_MY", MS_MY_INSURANCE_COMPANIES;
    currency_symbol ["currencySymbol"] => "ms_MY", MS_MY_CURRENCY_SYMBOL;
    commercial_bank ["commercialBank"] => "ne_NP", NE_NP_COMMERCIAL_BANKS;
    development_bank ["developmentBank"] => "ne_NP", NE_NP_DEVELOPMENT_BANKS;
    finance_company ["financeCompany"] => "ne_NP", NE_NP_FINANCE_COMPANIES;
    digital_wallet ["digitalWallet"] => "ne_NP", NE_NP_DIGITAL_WALLETS;
}

macro_rules! jpj_catalog_formatters {
    ($($method:ident [$history:literal] => $values:ident),+ $(,)?) => {
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.require_jpj_locale()?;
                Ok(self.random.sample(faker_miscellaneous::$values))
            })+
        }

        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.try_generate($history, || self.0.$method())
            })+
        }
    };
}

jpj_catalog_formatters! {
    peninsular_prefix ["peninsularPrefix"] => PENINSULAR_PREFIX,
    sarawak_prefix ["sarawakPrefix"] => SARAWAK_PREFIX,
    sabah_prefix ["sabahPrefix"] => SABAH_PREFIX,
    special_prefix ["specialPrefix"] => SPECIAL_PREFIX,
    valid_alphabet ["validAlphabet"] => VALID_ALPHABETS,
}

impl Faker {
    pub fn jpj_number_plate(&self) -> DatabaseResult<String> {
        self.require_jpj_locale()?;
        let mut template = self
            .random
            .sample(faker_miscellaneous::JPJ_NUMBER_PLATE_FORMATS);
        let mut plate = String::new();
        while let Some((literal, token)) = template.split_once("{{") {
            plate.push_str(literal);
            let (token, rest) = token.split_once("}}").expect("fixed JPJ plate template");
            if token == "numberSequence" {
                plate.push_str(&self.number_sequence()?.to_string());
            } else {
                let values = match token {
                    "peninsularPrefix" => faker_miscellaneous::PENINSULAR_PREFIX,
                    "sarawakPrefix" => faker_miscellaneous::SARAWAK_PREFIX,
                    "sabahPrefix" => faker_miscellaneous::SABAH_PREFIX,
                    "specialPrefix" => faker_miscellaneous::SPECIAL_PREFIX,
                    "validAlphabet" => faker_miscellaneous::VALID_ALPHABETS,
                    _ => unreachable!("fixed JPJ plate token"),
                };
                plate.push_str(self.random.sample(values));
            }
            template = rest;
        }
        plate.push_str(template);
        Ok(plate)
    }

    pub fn number_sequence(&self) -> DatabaseResult<i64> {
        self.require_jpj_locale()?;
        Ok(self.number_between(1, 9999))
    }

    fn require_jpj_locale(&self) -> DatabaseResult<()> {
        if self.provider_locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker JPJ locale"));
        }
        Ok(())
    }
}

impl UniqueFaker<'_> {
    pub fn jpj_number_plate(&self) -> DatabaseResult<String> {
        self.try_generate("jpjNumberPlate", || self.0.jpj_number_plate())
    }

    pub fn number_sequence(&self) -> DatabaseResult<i64> {
        self.try_generate("numberSequence", || self.0.number_sequence())
    }
}

// Use the same samples for direct calls and full-name templates.
macro_rules! malaysian_person_formatters {
    ($($method:ident [$history:literal]),+ $(,)?) => {
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<String> {
                self.require_malaysian_person()?;
                Ok(south_southeast_asian::malaysian_name_part(&self.random, $history).into_owned())
            })+
        }

        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<String> {
                self.try_generate($history, || self.0.$method())
            })+
        }
    };
}

malaysian_person_formatters! {
    first_name_male_malay ["firstNameMaleMalay"], first_name_female_malay ["firstNameFemaleMalay"], last_name_malay ["lastNameMalay"],
    muhammad_name ["muhammadName"], nur_name ["nurName"], haji ["haji"], hajjah ["hajjah"], title_male_malay ["titleMaleMalay"],
    last_name_chinese ["lastNameChinese"], first_name_male_chinese ["firstNameMaleChinese"], first_name_female_chinese ["firstNameFemaleChinese"],
    first_name_male_christian ["firstNameMaleChristian"], first_name_female_christian ["firstNameFemaleChristian"],
    initial_indian ["initialIndian"], first_name_male_indian ["firstNameMaleIndian"], first_name_female_indian ["firstNameFemaleIndian"], last_name_indian ["lastNameIndian"],
}

/// Generates localized sample data for tests and seeders.
pub struct Faker {
    locale: String,
    provider_locale: String,
    has_local_address_provider: bool,
    application_timezone: TimeZone,
    default_timezone: RwLock<Option<TimeZone>>,
    pub(super) random: FakerRandom,
    pub(super) extensions: RwLock<Extensions>,
    pub(super) formatters: RwLock<Formatters>,
    unique: Mutex<UniqueHistory>,
}

const DATE_RANGE_ERROR: DatabaseError =
    DatabaseError::InvalidQuery("Faker timestamp is out of range");

fn calendar_query_error(error: calendar::CalendarError) -> DatabaseError {
    match error {
        calendar::CalendarError::InvalidExpression => {
            DatabaseError::InvalidQuery("invalid Faker date expression")
        }
        calendar::CalendarError::OutOfRange => DATE_RANGE_ERROR,
        calendar::CalendarError::LimitExceeded => {
            DatabaseError::InvalidQuery("Faker date expression exceeds parser limits")
        }
    }
}

fn faker_timestamp(error: calendar::CalendarError) -> DatabaseResult<i64> {
    // The upstream date parser’s failure value becomes zero in numeric contexts.
    match error {
        calendar::CalendarError::InvalidExpression => Ok(0),
        error => Err(calendar_query_error(error)),
    }
}

fn months_ago(date: CivilDateTime, months: u32) -> DatabaseResult<CivilDateTime> {
    // PHP normalizes an overflowing day into the following month.
    date.with()
        .day(1)
        .build()
        .and_then(|first| first.checked_sub(Span::new().months(i64::from(months))))
        .and_then(|first| first.checked_add(Span::new().days(i64::from(date.day() - 1))))
        .map_err(|_| DATE_RANGE_ERROR)
}

fn resolve_local(zone: TimeZone, local: CivilDateTime) -> DatabaseResult<Timestamp> {
    let offset = match zone.to_ambiguous_timestamp(local).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset,
        AmbiguousOffset::Fold { before, after } => {
            // As in timelib, look up the offset by treating the civil time as UTC.
            let guess = Offset::UTC
                .to_timestamp(local)
                .map_err(|_| DATE_RANGE_ERROR)?;
            if zone.to_offset(guess) == before {
                before
            } else {
                after
            }
        }
        // Apply the pre-transition offset to move forward by the length of the gap.
        AmbiguousOffset::Gap { before, .. } => before,
    };
    offset.to_timestamp(local).map_err(|_| DATE_RANGE_ERROR)
}

macro_rules! generate {
    ($random:expr, $locale:expr, $generator:path, $($arg:expr),* $(,)?) => {{
        let mut rng = $random.fake_rng();
        match $locale {
            "ar_SA" => generate!(@sample rng, $generator(locales::AR_SA, $($arg),*)),
            "cy_GB" => generate!(@sample rng, $generator(locales::CY_GB, $($arg),*)),
            "de_DE" => generate!(@sample rng, $generator(locales::DE_DE, $($arg),*)),
            "fa_IR" => generate!(@sample rng, $generator(locales::FA_IR, $($arg),*)),
            "fr_FR" => generate!(@sample rng, $generator(locales::FR_FR, $($arg),*)),
            "it_IT" => generate!(@sample rng, $generator(locales::IT_IT, $($arg),*)),
            "ja_JP" => generate!(@sample rng, $generator(locales::JA_JP, $($arg),*)),
            "nl_NL" => generate!(@sample rng, $generator(locales::NL_NL, $($arg),*)),
            "pt_BR" => generate!(@sample rng, $generator(locales::PT_BR, $($arg),*)),
            "pt_PT" => generate!(@sample rng, $generator(locales::PT_PT, $($arg),*)),
            "tr_TR" => generate!(@sample rng, $generator(locales::TR_TR, $($arg),*)),
            "zh_CN" => generate!(@sample rng, $generator(locales::ZH_CN, $($arg),*)),
            "zh_TW" => generate!(@sample rng, $generator(locales::ZH_TW, $($arg),*)),
            _ => generate!(@sample rng, $generator(locales::EN, $($arg),*)),
        }
    }};
    (@sample $rng:ident, $generator:expr) => {{
        let formatter = $generator;
        match $rng.as_mut() {
            Some(rng) => formatter.fake_with_rng(rng),
            None => formatter.fake(),
        }
    }};
}

#[path = "faker_company.rs"]
mod company;

#[path = "faker_phone.rs"]
mod faker_phone;

#[path = "faker_person.rs"]
mod faker_person;

#[path = "faker_person_nigerian_ugandan.rs"]
mod nigerian_ugandan;

#[path = "faker_person_indian_nepali.rs"]
mod indian_nepali;

#[path = "faker_person_baltic_montenegrin.rs"]
mod baltic_montenegrin;

#[path = "faker_person_romanian_serbian.rs"]
mod romanian_serbian;

#[path = "faker_person_south_southeast_asian.rs"]
mod south_southeast_asian;

#[path = "faker_username.rs"]
mod username;

#[path = "faker_domains.rs"]
mod domains;

macro_rules! formatters {
    ($($name:ident [$history:literal] => $generator:path),+ $(,)?) => {
        impl Faker {
            $(pub fn $name(&self) -> String {
                generate!(&self.random, self.provider_locale(), $generator,)
            })+
        }

        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<String> {
                self.generate($history, || self.0.$name())
            })+
        }
    };
}

formatters! {
    word ["word"] => faker::lorem::raw::Word,
}

macro_rules! zoned_date_formatters {
    ($($zoned:ident => $base:ident($($argument:ident: $kind:ty),*) as $history:literal),+ $(,)?) => {
        impl Faker {
            $(
                #[doc = concat!("Samples with `", stringify!($base), "` and returns the chosen output time zone.")]
                pub fn $zoned(
                    &self,
                    $($argument: $kind,)*
                    timezone: Option<TimeZone>,
                ) -> DatabaseResult<Zoned> {
                    self.$base($($argument),*)
                        .map(|instant| self.zoned_date(instant, timezone))
                }
            )+
        }

        impl UniqueFaker<'_> {
            $(
                #[doc = concat!("Samples a unique zoned value with `", stringify!($base), "`.")]
                pub fn $zoned(
                    &self,
                    $($argument: $kind,)*
                    timezone: Option<TimeZone>,
                ) -> DatabaseResult<Zoned> {
                    self.generate_zoned($history, || self.0.$zoned($($argument,)* timezone.clone()))
                }
            )+
        }
    };
}

zoned_date_formatters! {
    date_time_between_in_timezone => date_time_between(start: Timestamp, end: Timestamp) as "dateTimeBetween",
    date_time_between_expressions_in_timezone => date_time_between_expressions(start: &str, end: &str) as "dateTimeBetween",
    date_time_in_interval_in_timezone => date_time_in_interval(date: Option<Timestamp>, interval: Option<SignedDuration>) as "dateTimeInInterval",
    date_time_in_interval_expressions_in_timezone => date_time_in_interval_expressions(date: Option<&str>, interval: Option<&str>) as "dateTimeInInterval",
    date_time_this_century_in_timezone => date_time_this_century(max: Option<Timestamp>) as "dateTimeThisCentury",
    date_time_this_decade_in_timezone => date_time_this_decade(max: Option<Timestamp>) as "dateTimeThisDecade",
    date_time_this_year_in_timezone => date_time_this_year(max: Option<Timestamp>) as "dateTimeThisYear",
    date_time_this_month_in_timezone => date_time_this_month(max: Option<Timestamp>) as "dateTimeThisMonth",
}

macro_rules! sample_formatters {
    ($($name:ident [$history:literal] => $samples:ident),+ $(,)?) => {
        impl Faker {
            $(pub fn $name(&self) -> &'static str {
                let samples = faker_miscellaneous::$samples;
                samples[self.random.usize(..samples.len())]
            })+
        }

        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<&'static str> {
                self.generate($history, || self.0.$name())
            })+
        }
    };
}

sample_formatters! {
    language_code ["languageCode"] => LANGUAGE_CODE,
    country_code ["countryCode"] => COUNTRY_CODE,
    country_iso_alpha3 ["countryISOAlpha3"] => COUNTRY_ISOALPHA3,
    currency_code ["currencyCode"] => CURRENCY_CODE,
    emoji ["emoji"] => EMOJI,
}

impl Faker {
    pub fn blood_type(&self) -> &'static str {
        const TYPES: [&str; 4] = ["A", "AB", "B", "O"];
        TYPES[self.random.usize(..TYPES.len())]
    }

    pub fn blood_rh(&self) -> &'static str {
        const FACTORS: [&str; 2] = ["+", "-"];
        FACTORS[self.random.usize(..FACTORS.len())]
    }

    pub fn blood_group(&self) -> String {
        format!("{}{}", self.blood_type(), self.blood_rh())
    }
}

impl UniqueFaker<'_> {
    pub fn blood_type(&self) -> DatabaseResult<&'static str> {
        self.generate("bloodType", || self.0.blood_type())
    }

    pub fn blood_rh(&self) -> DatabaseResult<&'static str> {
        self.generate("bloodRh", || self.0.blood_rh())
    }

    pub fn blood_group(&self) -> DatabaseResult<String> {
        self.generate("bloodGroup", || self.0.blood_group())
    }
}

/// Resolves the application generator for the selected locale.
/// Outside an application scope, creates a fresh generator.
pub fn fake(locale: Option<&str>) -> DatabaseResult<Arc<Faker>> {
    Faker::configured(
        Connection::model_container(None).as_deref(),
        locale,
        "faker",
    )
}

impl Faker {
    pub fn new(locale: &str) -> DatabaseResult<Self> {
        let locale = canonical_locale(locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        Ok(Self::with_canonical_locale(locale, TimeZone::UTC))
    }

    fn with_canonical_locale(locale: String, application_timezone: TimeZone) -> Self {
        // FakerPHP 数据键仅在此消费者边界使用下划线，公开标签保持不变。
        let id = locale
            .parse::<icu_locale_core::Locale>()
            .expect("已验证的语言标签")
            .id;
        let provider_locale = match (
            id.language.as_str(),
            id.script.map(|script| script.to_string()).as_deref(),
        ) {
            ("zh", Some("Hans")) => "zh_CN".to_owned(),
            ("zh", Some("Hant")) => "zh_TW".to_owned(),
            _ => id.to_string().replace('-', "_"),
        };
        let has_local_address_provider = faker_address::ADDRESS_LOCALES
            .binary_search(&provider_locale.as_str())
            .is_ok();
        Self {
            locale,
            provider_locale,
            has_local_address_provider,
            application_timezone,
            default_timezone: RwLock::default(),
            random: FakerRandom::new(),
            extensions: RwLock::default(),
            formatters: RwLock::default(),
            unique: Mutex::default(),
        }
    }

    pub(super) fn for_connection(connection: &Connection) -> DatabaseResult<Arc<Self>> {
        Self::configured(
            Connection::model_container(Some(connection)).as_deref(),
            None,
            "factory.faker",
        )
    }

    fn configured(
        container: Option<&Container>,
        locale: Option<&str>,
        binding: &str,
    ) -> DatabaseResult<Arc<Self>> {
        let Some(container) = container else {
            return Self::new(locale.unwrap_or("en-US")).map(Arc::new);
        };
        let config = container
            .get::<dyn Repository>("config")
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        let locale = match (locale, config.get("app.faker_locale")) {
            (Some(locale), _) => locale.to_owned(),
            (None, Some(serde_json::Value::String(locale))) => locale,
            (None, None) => "en-US".to_owned(),
            _ => return Err(DatabaseError::InvalidConfiguration("app.faker_locale")),
        };
        let locale = canonical_locale(&locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        let key = format!("{binding}:{locale}");
        if let Ok(faker) = container.get::<Self>(key.as_str()) {
            return Ok(faker);
        }
        let application_timezone = match config.get("app.timezone") {
            None => TimeZone::UTC,
            Some(serde_json::Value::String(zone)) => crate::support::Date::timezone(&zone)
                .map_err(|_| DatabaseError::InvalidConfiguration("app.timezone"))?,
            _ => return Err(DatabaseError::InvalidConfiguration("app.timezone")),
        };
        container
            .singleton_if_named(&key, move |_| {
                Ok(Arc::new(Self::with_canonical_locale(
                    locale.clone(),
                    application_timezone.clone(),
                )))
            })
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        container
            .get::<Self>(key)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))
    }

    /// Overrides the time zone for this generator. Pass None to restore the application default.
    pub fn set_default_timezone(&self, timezone: Option<TimeZone>) {
        *self.default_timezone.write().unwrap() = timezone;
    }

    pub fn get_default_timezone(&self) -> Option<TimeZone> {
        self.default_timezone.read().unwrap().clone()
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    pub(super) fn provider_locale(&self) -> &str {
        &self.provider_locale
    }

    pub fn random_locale(&self) -> &'static str {
        static LOCALES: OnceLock<Vec<String>> = OnceLock::new();
        let locales = LOCALES.get_or_init(|| {
            faker_miscellaneous::LOCALE_DATA
                .iter()
                .map(|locale| {
                    canonical_locale(&locale.replace('_', "-")).expect("内置 Faker 语言标签有效")
                })
                .collect()
        });
        &locales[self.random.usize(..locales.len())]
    }

    /// Restarts this generator’s random sequence. Pass None to use fresh entropy.
    ///
    /// Uniqueness history is independent and can be reset with `unique(true)`.
    pub fn seed(&self, seed: impl Into<Option<u64>>) {
        self.random.seed(seed.into());
    }

    /// Generates values without repeating previously returned values for each formatter.
    pub fn unique(&self, reset: bool) -> UniqueFaker<'_> {
        self.unique_with_retries(reset, 10_000)
    }

    /// Uses a custom attempt limit with the shared formatter history.
    pub fn unique_with_retries(&self, reset: bool, max_retries: usize) -> UniqueFaker<'_> {
        if reset {
            *self.unique.lock().unwrap() = UniqueHistory::default();
        }
        UniqueFaker(self, max_retries)
    }

    pub fn sentence(&self, words: Range<usize>) -> String {
        generate!(
            &self.random,
            self.provider_locale(),
            faker::lorem::raw::Sentence,
            words
        )
    }

    pub fn words(&self, count: usize) -> Vec<String> {
        (0..count).map(|_| self.word()).collect()
    }

    pub fn words_as_text(&self, count: usize) -> String {
        self.words(count).join(" ")
    }

    /// Generates a sentence with a fixed or approximately specified word count.
    pub fn sentence_with_word_count(&self, count: usize, variable: bool) -> String {
        if count == 0 {
            return String::new();
        }
        let mut words = self.words(lorem_count(&self.random, count, variable));
        words[0] = capitalize_lorem_word(&words[0]);
        words.join(" ") + "."
    }

    pub fn sentences(&self, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| self.sentence_with_word_count(6, true))
            .collect()
    }

    pub fn sentences_as_text(&self, count: usize) -> String {
        self.sentences(count).join(" ")
    }

    pub fn paragraph(&self, count: usize, variable: bool) -> String {
        self.sentences_as_text(lorem_count(&self.random, count, variable))
    }

    pub fn paragraphs(&self, count: usize) -> Vec<String> {
        (0..count).map(|_| self.paragraph(3, true)).collect()
    }

    pub fn paragraphs_as_text(&self, count: usize) -> String {
        self.paragraphs(count).join("\n\n")
    }

    /// Generates text within the requested UTF-8 byte budget.
    pub fn text(&self, max: usize) -> DatabaseResult<String> {
        if max < 5 {
            return Err(DatabaseError::InvalidQuery(
                "Faker text requires at least five bytes",
            ));
        }
        for _ in 0..10_000 {
            let mut text = String::new();
            loop {
                let unit = match max {
                    0..25 => self.word(),
                    25..100 => self.sentence_with_word_count(6, true),
                    _ => self.paragraph(3, true),
                };
                let separator = usize::from(!text.is_empty());
                if unit.is_empty() || unit.len().saturating_add(separator) >= max - text.len() {
                    break;
                }
                if separator != 0 {
                    text.push(' ');
                }
                text.push_str(&unit);
            }
            if !text.is_empty() {
                if max < 25 {
                    text = capitalize_lorem_word(&text);
                    text.push('.');
                }
                return Ok(text);
            }
        }
        Err(DatabaseError::InvalidQuery(
            "Faker text could not fit the byte budget",
        ))
    }

    /// Samples an inclusive integer interval, accepting endpoints in either order.
    pub fn number_between(&self, first: i64, second: i64) -> i64 {
        self.random.i64(first.min(second)..=first.max(second))
    }

    pub fn random_digit(&self) -> u8 {
        self.random.u8(0..=9)
    }

    pub fn random_digit_not_null(&self) -> u8 {
        self.random.u8(1..=9)
    }

    pub fn random_digit_not(&self, except: i64) -> u8 {
        let digit = self.random.u8(0..=8);
        digit + u8::from(i64::from(digit) >= except)
    }

    pub fn boolean(&self, chance_of_getting_true: Option<i64>) -> bool {
        self.number_between(1, 100) <= chance_of_getting_true.unwrap_or(50)
    }

    pub fn latitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.random_float(Some(6), min.unwrap_or(-90.0), Some(max.unwrap_or(90.0)))
    }

    pub fn longitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.random_float(Some(6), min.unwrap_or(-180.0), Some(max.unwrap_or(180.0)))
    }

    pub fn local_latitude(&self) -> DatabaseResult<f64> {
        match self.provider_locale() {
            "zh_TW" => self.latitude(Some(22.0), Some(25.0)),
            _ => self.local_coordinate_latitude(),
        }
    }

    pub fn local_longitude(&self) -> DatabaseResult<f64> {
        match self.provider_locale() {
            "zh_TW" => self.longitude(Some(120.0), Some(122.0)),
            _ => self.local_coordinate_longitude(),
        }
    }

    fn local_coordinate_latitude(&self) -> DatabaseResult<f64> {
        match self.provider_locale() {
            "hu_HU" => self.latitude(Some(46.262740), Some(47.564721)),
            "me_ME" => self.latitude(Some(42.43), Some(42.45)),
            _ => self.latitude(None, None),
        }
    }

    fn local_coordinate_longitude(&self) -> DatabaseResult<f64> {
        match self.provider_locale() {
            "hu_HU" => self.longitude(Some(17.077949), Some(20.604560)),
            "me_ME" => self.longitude(Some(19.16), Some(19.27)),
            _ => self.longitude(None, None),
        }
    }

    pub fn local_coordinates(&self) -> DatabaseResult<Coordinates> {
        Ok(Coordinates {
            latitude: self.local_coordinate_latitude()?,
            longitude: self.local_coordinate_longitude()?,
        })
    }

    fn address_locale(&self) -> &str {
        if self.has_local_address_provider {
            self.provider_locale()
        } else {
            "en_US"
        }
    }

    pub fn city(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.regional_address_format(faker_address::CITY_FORMATS_HU_HU),
            "sr_RS" | "sr_Cyrl_RS" => self.random.sample(faker_address::CITY_NAMES_SR_RS).into(),
            "sr_Latn_RS" => self
                .random
                .sample(faker_address::CITY_NAMES_SR_LATN_RS)
                .into(),
            "me_ME" => self.random.sample(faker_address::CITY_NAMES_ME_ME).into(),
            "hy_AM" => self.random.sample(faker_address::CITY_HY_AM).into(),
            "ka_GE" => self.regional_address_format(faker_address::CITY_FORMATS_KA_GE),
            "lt_LT" => self.random.sample(faker_address::CITY_LT_LT).into(),
            "lv_LV" => self.random.sample(faker_address::CITY_LV_LV).into(),
            "hr_HR" => self.random.sample(faker_address::CITY_NAMES_HR_HR).into(),
            "sl_SI" => self.random.sample(faker_address::CITY_SL_SI).into(),
            "ro_RO" | "ro_MD" => self.regional_address_format(faker_address::CITY_FORMATS_RO_RO),
            "el_CY" => self.city_suffix(),
            "el_GR" => self.regional_address_format(faker_address::CITY_FORMATS_EL_GR),
            "it_CH" => self.random.sample(faker_address::CITY_NAMES_IT_CH).into(),
            "pt_PT" => self.random.sample(faker_address::CITIES_PT_PT).into(),
            "es_ES" => self.regional_address_format(faker_address::CITY_FORMATS_ES_ES),
            "es_AR" | "it_IT" | "pt_BR" => {
                self.regional_address_format(faker_address::CITY_FORMATS_ES_AR)
            }
            "es_PE" => self.regional_address_format(faker_address::CITY_FORMATS_ES_PE),
            "es_VE" => self.regional_address_format(faker_address::CITY_FORMATS_ES_VE),
            "en_UG" => self.random.sample(faker_address::CITY_NAME_EN_UG).into(),
            "en_IN" => self.random.sample(faker_address::CITY_EN_IN).into(),
            "en_PH" => self.random.sample(faker_address::CITY_EN_PH).into(),
            "en_HK" => self.random.sample(faker_address::CITY_EN_HK).into(),
            "ko_KR" => self.random.sample(faker_address::CITY_KO_KR).into(),
            "de_DE" => self.random.sample(faker_address::CITY_DE_DE).into(),
            "de_AT" => self.random.sample(faker_address::CITY_DE_AT).into(),
            "de_CH" => self.random.sample(faker_address::CITY_DE_CH).into(),
            "nl_NL" => self.random.sample(faker_address::CITY_NL_NL).into(),
            "nl_BE" => self.random.sample(faker_address::CITY_NL_BE).into(),
            "nb_NO" => self.random.sample(faker_address::CITY_NB_NO).into(),
            "sv_SE" => self.random.sample(faker_address::CITY_SV_SE).into(),
            "da_DK" => self.random.sample(faker_address::CITY_DA_DK).into(),
            "is_IS" => self.random.sample(faker_address::CITY_IS_IS).into(),
            "fi_FI" => match self.random.u8(0..4) {
                0 => format!(
                    "{}-{}{}",
                    self.city_prefix(),
                    self.first_name(),
                    self.city_suffix()
                ),
                1 => format!("{}-{}", self.city_prefix(), self.first_name()),
                2 => format!("{}{}", self.first_name(), self.city_suffix()),
                _ => format!("{}{}", self.last_name(), self.city_suffix()),
            },
            "pl_PL" => self.random.sample(faker_address::CITY_PL_PL).into(),
            "cs_CZ" => self.random.sample(faker_address::CITY_CS_CZ).into(),
            "sk_SK" => self.random.sample(faker_address::CITY_SK_SK).into(),
            "ru_RU" => self.random.sample(faker_address::CITY_RU_RU).into(),
            "uk_UA" => self
                .random
                .sample(faker_address::CITY_REGIONS_UK_UA)
                .0
                .into(),
            "kk_KZ" => self.random.sample(faker_address::CITY_KK_KZ).into(),
            "tr_TR" => self.random.sample(faker_address::CITY_TR_TR).into(),
            "fa_IR" => {
                let city = self.random.sample(faker_address::CITY_FA_IR);
                if self.boolean(None) {
                    format!("{} {city}", self.city_prefix())
                } else {
                    city.into()
                }
            }
            "he_IL" => self.city_suffix(),
            "ar_EG" => self.random.sample(faker_address::CITY_AR_EG).into(),
            "ar_SA" => self.random.sample(faker_address::CITY_AR_SA).into(),
            "ar_JO" => {
                let city = self.random.sample(faker_address::CITY_AR_JO);
                if self.boolean(None) {
                    format!("{} {city}", self.city_prefix())
                } else {
                    city.into()
                }
            }
            "bn_BD" => format!("{}{}", self.city_prefix(), self.city_suffix()),
            "ne_NP" => self.random.sample(faker_address::CITY_NE_NP).into(),
            "id_ID" => self.random.sample(faker_address::CITY_ID_ID).into(),
            "ms_MY" => self
                .random
                .sample(Self::malaysian_region(&self.random).towns)
                .into(),
            "vi_VN" => self.random.sample(faker_address::CITY_VI_VN).into(),
            "ja_JP" => format!("{}{}", self.last_name(), self.city_suffix()),
            "en_NG" | "en_NZ" | "en_SG" => format!("{}{}", self.first_name(), self.city_suffix()),
            "en_US" | "en_CA" | "en_AU" | "en_GB" | "en_ZA" | "th_TH" => match self.random.u8(0..4)
            {
                0 => format!(
                    "{} {}{}",
                    self.city_prefix(),
                    self.first_name(),
                    self.city_suffix()
                ),
                1 => format!("{} {}", self.city_prefix(), self.first_name()),
                2 => format!("{}{}", self.first_name(), self.city_suffix()),
                _ => format!("{}{}", self.last_name(), self.city_suffix()),
            },
            "fr_BE" => self.random.sample(faker_address::CITY_FR_BE).into(),
            "fr_CH" => self.random.sample(faker_address::CITY_FR_CH).into(),
            "fr_CA" => {
                let mut city = format!("{}{}", self.city_prefix(), self.first_name());
                if self.boolean(None) {
                    city.push_str(&self.city_suffix());
                }
                city
            }
            "fr_FR" => match self.random.u8(0..8) {
                0..=3 => self.last_name(),
                4..=6 => format!("{}{}", self.last_name(), self.city_suffix()),
                _ => format!("{}-sur-{}", self.last_name(), self.last_name()),
            },
            "zh_CN" => self.random.sample(faker_address::CITY_ZH_CN).into(),
            "zh_TW" => {
                let (county, districts) = self.random.sample(faker_address::CITY_ZH_TW);
                format!("{county}{}", self.random.sample(districts))
            }
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::CityName,
            ),
        }
    }

    pub fn street_name(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_HU_HU),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                self.regional_address_format(faker_address::STREET_NAME_FORMATS_SR_RS)
            }
            "hy_AM" | "ka_GE" => {
                self.regional_address_format(faker_address::STREET_NAME_FORMATS_HY_AM)
            }
            "lt_LT" | "lv_LV" => {
                self.regional_address_format(faker_address::STREET_NAME_FORMATS_LT_LT)
            }
            "hr_HR" => self.random.sample(faker_address::STREETS_HR_HR).into(),
            "sl_SI" => self.random.sample(faker_address::STREET_SL_SI).into(),
            "ro_RO" | "ro_MD" => {
                self.regional_address_format(faker_address::STREET_NAME_FORMATS_RO_RO)
            }
            "el_GR" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_EL_GR),
            "el_CY" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_EL_CY),
            "it_IT" | "it_CH" => {
                self.regional_address_format(faker_address::STREET_NAME_FORMATS_IT_IT)
            }
            "pt_BR" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_PT_BR),
            "pt_PT" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_PT_PT),
            "es_ES" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_ES_ES),
            "es_AR" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_ES_AR),
            "es_PE" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_ES_PE),
            "es_VE" => self.regional_address_format(faker_address::STREET_NAME_FORMATS_ES_VE),
            "en_US" | "en_CA" | "en_AU" | "en_NZ" | "en_GB" | "en_UG" | "en_ZA" => {
                let name = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{name} {}", self.street_suffix())
            }
            "fr_BE" => format!("{} {}", self.street_suffix(), self.last_name()),
            "fr_CH" => {
                let prefix = self.random.sample(faker_address::STREET_PREFIX_FR_CH);
                match self.random.u8(0..3) {
                    0 => format!("{prefix} {}", self.last_name()),
                    1 => format!("{prefix} de {}", self.city()),
                    _ => format!("{prefix} de {}", self.last_name()),
                }
            }
            "fr_CA" => {
                let name = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{} {name}", self.street_suffix())
            }
            "fr_FR" => {
                let prefix = self.random.sample(faker_address::STREET_PREFIX_FR_FR);
                match self.random.u8(0..3) {
                    0 => format!("{prefix} {}", self.last_name()),
                    1 => format!("{prefix} {} {}", self.first_name(), self.last_name()),
                    _ => format!("{prefix} de {}", self.last_name()),
                }
            }
            "en_HK" => Self::hong_kong_name(&self.random, faker_address::STREET_NAME_FORMATS_EN_HK),
            "ko_KR" => self.random.sample(faker_address::STREET_NAME_KO_KR).into(),
            "de_DE" | "de_AT" | "de_CH" => self.german_street_name(),
            "nl_NL" | "nl_BE" => format!("{}{}", self.last_name(), self.street_suffix()),
            "da_DK" => {
                let stem = if self.boolean(None) {
                    self.last_name()
                } else {
                    self.middle_name().expect("Danish middle name").into()
                };
                if self.boolean(None) {
                    format!("{stem}{}", self.street_suffix())
                } else {
                    format!(
                        "{stem} {}",
                        self.random.sample(faker_address::STREET_SUFFIX_WORD_DA_DK)
                    )
                }
            }
            "fi_FI" => {
                let stem = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{stem}{}", self.street_suffix())
            }
            "is_IS" => {
                let stem = match self.random.u8(0..4) {
                    0..=1 => self
                        .random
                        .sample(faker_address::STREET_PREFIX_IS_IS)
                        .into(),
                    2 => self.first_name_male(),
                    _ => self.first_name_female(),
                };
                format!("{stem}{}", self.street_suffix())
            }
            "nb_NO" | "sv_SE" => {
                let suffix = self.street_suffix();
                match self.random.u8(0..9) {
                    0..=1 => format!("{}{suffix}", self.last_name()),
                    2..=3 => format!("{}{suffix}", self.first_name()),
                    4..=7 => format!(
                        "{}{suffix}",
                        self.street_prefix().expect("fixed Nordic street locale")
                    ),
                    _ => format!(
                        "{} {}",
                        self.last_name(),
                        self.street_suffix_word()
                            .expect("fixed Nordic street locale")
                    ),
                }
            }
            "pl_PL" => self.random.sample(faker_address::STREET_PL_PL).into(),
            "cs_CZ" => self.random.sample(faker_address::STREET_CS_CZ).into(),
            "sk_SK" => {
                let name = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{name} {}", self.street_suffix())
            }
            "uk_UA" => self.random.sample(faker_address::STREET_UK_UA).into(),
            "ru_RU" | "kk_KZ" => format!("{} {}", self.last_name(), self.street_suffix()),
            "tr_TR" => {
                let name = if self.random.u8(0..3) == 0 {
                    self.last_name()
                } else {
                    self.first_name()
                };
                format!("{name} {}", self.street_suffix())
            }
            "fa_IR" => format!(
                "{} {}",
                self.random.sample(faker_address::STREET_PREFIX_FA_IR),
                self.last_name()
            ),
            "he_IL" => {
                let name = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{} {name}", self.street_suffix())
            }
            "ar_EG" | "ar_JO" | "ar_SA" => format!(
                "{} {} {}",
                self.street_prefix()
                    .expect("fixed Arabic street prefix locale"),
                self.first_name(),
                self.last_name()
            ),
            "bn_BD" => format!(
                "{} {}",
                self.random.sample(faker_address::STREET_NAMES_BN_BD),
                self.street_suffix()
            ),
            "ne_NP" => {
                let name = if self.boolean(None) {
                    self.first_name()
                } else {
                    self.last_name()
                };
                format!("{name}{}", self.street_suffix())
            }
            "id_ID" => self.random.sample(faker_address::STREET_ID_ID).into(),
            "ms_MY" => self.malaysian_address_name(faker_address::STREET_NAME_FORMATS_MS_MY),
            "vi_VN" => self.vietnamese_address_name(faker_address::STREET_NAME_FORMATS_VI_VN),
            "ja_JP" => format!("{}{}", self.last_name(), self.street_suffix()),
            "th_TH" => format!(
                "{} {}",
                self.random.sample(faker_address::STREET_PREFIX_TH_TH),
                self.random.sample(faker_address::STREET_SUFFIX_TH_TH),
            ),
            "zh_CN" | "en_IN" | "en_NG" | "en_PH" | "en_SG" => {
                format!("{} {}", self.last_name(), self.street_suffix())
            }
            "zh_TW" => {
                let mut street = format!(
                    "{}{}",
                    self.random.sample(faker_address::STREET_ZH_TW),
                    self.street_suffix()
                );
                if self.boolean(None) {
                    write!(street, "{}段", self.taiwan_chinese_digit())
                        .expect("writing to String cannot fail");
                }
                street
            }
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::StreetName,
            ),
        }
    }

    fn regional_address_format(&self, formats: &[&str]) -> String {
        let mut format = self.random.sample(formats);
        let mut result = String::with_capacity(format.len());
        while let Some((literal, token)) = format.split_once("{{") {
            result.push_str(literal);
            let (token, remaining) = token
                .split_once("}}")
                .expect("fixed regional address format");
            match token {
                "streetPrefix" => result.push_str(
                    self.street_prefix()
                        .expect("street prefix supported by address template locale"),
                ),
                "firstName" => result.push_str(&self.first_name()),
                "lastName" => result.push_str(&self.last_name()),
                "smallerCity" => result.push_str(
                    self.smaller_city()
                        .expect("smallerCity supported by address template locale"),
                ),
                "bigCity" => result.push_str(
                    self.big_city()
                        .expect("bigCity supported by address template locale"),
                ),
                "capital" => result.push_str(
                    self.capital()
                        .expect("capital supported by address template locale"),
                ),
                "regionGenitiveForm" => result.push_str(
                    self.region_genitive_form()
                        .expect("region genitive form supported by address template locale"),
                ),
                "regionSuffix" => result.push_str(
                    self.region_suffix()
                        .expect("regionSuffix supported by address template locale"),
                ),
                "region" => result.push_str(
                    self.region()
                        .expect("region supported by address template locale"),
                ),
                "street" => result.push_str(
                    self.street()
                        .expect("street supported by address template locale"),
                ),
                "apartmentNumber" => result.push_str(
                    self.apartment_number()
                        .expect("apartmentNumber supported by address template locale"),
                ),
                "floor" => result.push_str(
                    self.floor()
                        .expect("floor supported by address template locale"),
                ),
                "blockSegment" => result.push_str(
                    self.block_segment()
                        .expect("blockSegment supported by address template locale"),
                ),
                "block" => result.push_str(
                    &self
                        .block()
                        .expect("block supported by address template locale"),
                ),
                "county" => result.push_str(
                    self.county()
                        .expect("county supported by address template locale"),
                ),
                "streetPlainName" => result.push_str(
                    self.street_plain_name()
                        .expect("streetPlainName supported by address template locale"),
                ),
                "cityName" => result.push_str(
                    self.city_name()
                        .expect("cityName supported by address template locale"),
                ),
                "lastNameMale" => result.push_str(
                    &self
                        .last_name_male()
                        .expect("male surname supported by address template locale"),
                ),
                "lastNameFemale" => result.push_str(
                    &self
                        .last_name_female()
                        .expect("female surname supported by address template locale"),
                ),
                "prefecture" => result.push_str(
                    self.prefecture()
                        .expect("prefecture supported by address template locale"),
                ),
                "cityPrefix" => result.push_str(&self.city_prefix()),
                "citySuffix" => result.push_str(&self.city_suffix()),
                "streetSuffix" => result.push_str(&self.street_suffix()),
                "buildingNumber" => result.push_str(&self.building_number()),
                "secondaryAddress" => result.push_str(&self.secondary_address()),
                "streetName" => result.push_str(&self.street_name()),
                "streetAddress" => result.push_str(&self.street_address()),
                "city" => result.push_str(&self.city()),
                "state" => result.push_str(&self.state()),
                "stateAbbr" => result.push_str(&self.state_abbr()),
                "postcode" => result.push_str(&self.postcode()),
                _ => unreachable!("fixed regional address token"),
            }
            format = remaining;
        }
        result.push_str(format);
        result
    }

    pub fn city_prefix(&self) -> String {
        match self.address_locale() {
            "hy_AM" => self.random.sample(faker_address::CITY_PREFIX_HY_AM).into(),
            "ka_GE" => self.random.sample(faker_address::CITY_PREFIX_KA_GE).into(),
            "lv_LV" => self.random.sample(faker_address::CITY_PREFIX_LV_LV).into(),
            "it_IT" | "it_CH" => self.random.sample(faker_address::CITY_PREFIX_IT_IT).into(),
            "pt_BR" => self.random.sample(faker_address::CITY_PREFIX_PT_BR).into(),
            "es_ES" => self.random.sample(faker_address::CITY_PREFIX_ES_ES).into(),
            "es_AR" => self.random.sample(faker_address::CITY_PREFIX_ES_AR).into(),
            "es_PE" => self.random.sample(faker_address::CITY_PREFIX_ES_PE).into(),
            "es_VE" => self.random.sample(faker_address::CITY_PREFIX_ES_VE).into(),
            "fr_CA" => self.random.sample(faker_address::CITY_PREFIX_FR_CA).into(),
            "th_TH" => self.random.sample(faker_address::CITY_PREFIX_TH_TH).into(),
            "ru_RU" => "город".into(),
            "uk_UA" => "місто".into(),
            "fa_IR" => self.random.sample(faker_address::CITY_PREFIX_FA_IR).into(),
            "fi_FI" => self.random.sample(faker_address::CITY_PREFIX_FI_FI).into(),
            "ar_EG" => self.random.sample(faker_address::CITY_PREFIX_AR_EG).into(),
            "ar_JO" => self.random.sample(faker_address::CITY_PREFIX_AR_JO).into(),
            "bn_BD" => self.random.sample(faker_address::CITY_PREFIX_BN_BD).into(),
            "en_AU" => self.random.sample(faker_address::CITY_PREFIX_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_GB" | "en_PH" | "en_UG" | "en_ZA" => {
                self.random.sample(faker_address::CITY_PREFIX_EN_US).into()
            }
            "zh_TW" => String::new(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::CityPrefix,
            ),
        }
    }

    pub fn city_suffix(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.random.sample(faker_address::CITY_SUFFIX_HU_HU).into(),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                self.random.sample(faker_address::CITY_SUFFIX_SR_RS).into()
            }
            "hy_AM" | "ka_GE" => self.random.sample(faker_address::CITY_SUFFIX_HY_AM).into(),
            "lt_LT" | "lv_LV" => self.random.sample(faker_address::CITY_SUFFIX_LT_LT).into(),
            "hr_HR" | "sl_SI" => self.random.sample(faker_address::CITY_SUFFIX_HR_HR).into(),
            "ro_RO" | "ro_MD" => self.random.sample(faker_address::CITY_SUFFIX_RO_RO).into(),
            "el_GR" => self.random.sample(faker_address::CITY_SUFFIX_EL_GR).into(),
            "el_CY" => self.random.sample(faker_address::CITY_SUFFIX_EL_CY).into(),
            "it_IT" | "it_CH" => self.random.sample(faker_address::CITY_SUFFIX_IT_IT).into(),
            "pt_BR" => self.random.sample(faker_address::CITY_SUFFIX_PT_BR).into(),
            "pt_PT" => self.random.sample(faker_address::CITY_SUFFIX_PT_PT).into(),
            "es_ES" => self.random.sample(faker_address::CITY_SUFFIX_ES_ES).into(),
            "es_AR" => self.random.sample(faker_address::CITY_SUFFIX_ES_AR).into(),
            "es_PE" => self.random.sample(faker_address::CITY_SUFFIX_ES_PE).into(),
            "es_VE" => self.random.sample(faker_address::CITY_SUFFIX_ES_VE).into(),
            "en_ZA" => self.random.sample(faker_address::CITY_SUFFIX_EN_ZA).into(),
            "en_HK" | "en_IN" | "en_NG" | "en_SG" | "id_ID" | "ko_KR" | "ms_MY" | "ne_NP"
            | "vi_VN" => "Ville".into(),
            "en_GB" => self.random.sample(faker_address::CITY_SUFFIX_EN_GB).into(),
            "en_NZ" => self.random.sample(faker_address::CITY_SUFFIX_EN_NZ).into(),
            "en_US" | "en_CA" | "en_AU" | "en_PH" | "en_UG" => {
                self.random.sample(faker_address::CITY_SUFFIX_EN_US).into()
            }
            "fr_CA" => self.random.sample(faker_address::CITY_SUFFIX_FR_CA).into(),
            "fr_FR" | "fr_BE" | "fr_CH" => {
                self.random.sample(faker_address::CITY_SUFFIX_FR_FR).into()
            }
            "ar_EG" | "ar_JO" | "ar_SA" | "fa_IR" | "ru_RU" | "tr_TR" | "zh_CN" | "pl_PL"
            | "cs_CZ" | "sk_SK" | "de_AT" | "de_CH" | "de_DE" | "nl_NL" | "nl_BE" | "nb_NO"
            | "sv_SE" | "is_IS" => "Ville".into(),
            "da_DK" => self.random.sample(faker_address::CITY_SUFFIX_DA_DK).into(),
            "fi_FI" => self.random.sample(faker_address::CITY_SUFFIX_FI_FI).into(),
            "kk_KZ" => "қаласы".into(),
            "ja_JP" => "市".into(),
            "th_TH" => self.random.sample(faker_address::CITY_SUFFIX_TH_TH).into(),
            "he_IL" => self.random.sample(faker_address::CITY_HE_IL).into(),
            "bn_BD" => self.random.sample(faker_address::CITY_SUFFIX_BN_BD).into(),
            "uk_UA" | "zh_TW" => String::new(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::CitySuffix,
            ),
        }
    }

    pub fn street_suffix(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self
                .random
                .sample(faker_address::STREET_SUFFIX_HU_HU)
                .into(),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => self
                .random
                .sample(faker_address::STREET_SUFFIX_SR_RS)
                .into(),
            "hy_AM" => self
                .random
                .sample(faker_address::STREET_SUFFIX_HY_AM)
                .into(),
            "ka_GE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_KA_GE)
                .into(),
            "lt_LT" => self
                .random
                .sample(faker_address::STREET_SUFFIX_LT_LT)
                .into(),
            "lv_LV" => self
                .random
                .sample(faker_address::STREET_SUFFIX_LV_LV)
                .into(),
            "hr_HR" | "sl_SI" => self
                .random
                .sample(faker_address::STREET_SUFFIX_HR_HR)
                .into(),
            "ro_RO" | "ro_MD" => self
                .random
                .sample(faker_address::STREET_SUFFIX_RO_RO)
                .into(),
            "el_GR" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EL_GR)
                .into(),
            "it_IT" | "it_CH" => self
                .random
                .sample(faker_address::STREET_SUFFIX_IT_IT)
                .into(),
            "es_ES" | "pt_BR" | "pt_PT" => self
                .random
                .sample(faker_address::STREET_SUFFIX_ES_ES)
                .into(),
            "es_AR" | "es_PE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_ES_AR)
                .into(),
            "es_VE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_ES_VE)
                .into(),
            "en_AU" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_AU)
                .into(),
            "en_GB" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_GB)
                .into(),
            "en_SG" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_SG)
                .into(),
            "en_HK" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_HK)
                .into(),
            "en_NZ" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_NZ)
                .into(),
            "en_US" | "en_CA" | "en_PH" | "en_UG" | "en_ZA" | "sk_SK" => self
                .random
                .sample(faker_address::STREET_SUFFIX_EN_US)
                .into(),
            "fr_BE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_FR_BE)
                .into(),
            "fr_CA" => self
                .random
                .sample(faker_address::STREET_SUFFIX_FR_CA)
                .into(),
            "fr_FR" | "fr_CH" | "zh_CN" | "en_IN" | "en_NG" | "id_ID" | "ko_KR" | "vi_VN"
            | "ar_EG" | "ar_JO" | "ar_SA" | "fa_IR" | "ru_RU" | "pl_PL" | "cs_CZ" | "de_AT"
            | "de_CH" | "de_DE" => "Street".into(),
            "zh_TW" => self
                .random
                .sample(faker_address::STREET_SUFFIX_ZH_TW)
                .into(),
            "ja_JP" => "町".into(),
            "th_TH" => self
                .random
                .sample(faker_address::STREET_SUFFIX_TH_TH)
                .into(),
            "he_IL" => self
                .random
                .sample(faker_address::STREET_SUFFIX_HE_IL)
                .into(),
            "nl_NL" | "nl_BE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_NL_NL)
                .into(),
            "nb_NO" => self
                .random
                .sample(faker_address::STREET_SUFFIX_NB_NO)
                .into(),
            "sv_SE" => self
                .random
                .sample(faker_address::STREET_SUFFIX_SV_SE)
                .into(),
            "da_DK" => self
                .random
                .sample(faker_address::STREET_SUFFIX_DA_DK)
                .into(),
            "fi_FI" => self
                .random
                .sample(faker_address::STREET_SUFFIX_FI_FI)
                .into(),
            "is_IS" => self
                .random
                .sample(faker_address::STREET_SUFFIX_IS_IS)
                .into(),
            "kk_KZ" => self
                .random
                .sample(faker_address::STREET_SUFFIX_KK_KZ)
                .into(),
            "el_CY" | "uk_UA" => String::new(),
            "tr_TR" => self
                .random
                .sample(faker_address::STREET_SUFFIX_TR_TR)
                .into(),
            "bn_BD" => self
                .random
                .sample(faker_address::STREET_SUFFIX_BN_BD)
                .into(),
            "ne_NP" => self
                .random
                .sample(faker_address::STREET_SUFFIX_NE_NP)
                .into(),
            "ms_MY" => self
                .random
                .sample(faker_address::STREET_SUFFIX_MS_MY)
                .into(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::StreetSuffix,
            ),
        }
    }

    pub fn country(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.random.sample(faker_address::COUNTRY_HU_HU).into(),
            "sr_RS" | "sr_Cyrl_RS" => self.random.sample(faker_address::COUNTRY_SR_RS).into(),
            "sr_Latn_RS" => self.random.sample(faker_address::COUNTRY_SR_LATN_RS).into(),
            "me_ME" => self.random.sample(faker_address::COUNTRY_ME_ME).into(),
            "hy_AM" => self.random.sample(faker_address::COUNTRY_HY_AM).into(),
            "ka_GE" => self.random.sample(faker_address::COUNTRY_KA_GE).into(),
            "lt_LT" => self.random.sample(faker_address::COUNTRY_LT_LT).into(),
            "lv_LV" => self.random.sample(faker_address::COUNTRY_LV_LV).into(),
            "hr_HR" => self.random.sample(faker_address::COUNTRY_HR_HR).into(),
            "sl_SI" => self.random.sample(faker_address::COUNTRY_SL_SI).into(),
            "ro_RO" | "ro_MD" => self.random.sample(faker_address::COUNTRY_RO_RO).into(),
            "el_GR" | "el_CY" => self.random.sample(faker_address::COUNTRY_EL_GR).into(),
            "it_IT" | "it_CH" => self.random.sample(faker_address::COUNTRY_IT_IT).into(),
            "pt_BR" => self.random.sample(faker_address::COUNTRY_PT_BR).into(),
            "pt_PT" => self.random.sample(faker_address::COUNTRY_PT_PT).into(),
            "es_ES" | "es_AR" | "es_PE" | "es_VE" => {
                self.random.sample(faker_address::COUNTRY_ES_ES).into()
            }
            "en_IN" | "en_NG" => self.random.sample(faker_address::COUNTRY_EN_NG).into(),
            "en_SG" => "SINGAPORE".into(),
            "en_HK" => "Hong Kong".into(),
            "en_GB" => self.random.sample(faker_address::COUNTRY_EN_GB).into(),
            "en_US" | "en_CA" | "en_AU" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" | "ne_NP"
            | "vi_VN" => self.random.sample(faker_address::COUNTRY_EN_US).into(),
            "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA" => {
                self.random.sample(faker_address::COUNTRY_FR_FR).into()
            }
            "zh_CN" => self.random.sample(faker_address::COUNTRY_ZH_CN).into(),
            "zh_TW" => self.random.sample(faker_address::COUNTRY_ZH_TW).into(),
            "ja_JP" => self.random.sample(faker_address::COUNTRY_JA_JP).into(),
            "th_TH" => self.random.sample(faker_address::COUNTRY_TH_TH).into(),
            "fa_IR" => self.random.sample(faker_address::COUNTRY_FA_IR).into(),
            "he_IL" => self.random.sample(faker_address::COUNTRY_HE_IL).into(),
            "de_DE" | "de_AT" => self.random.sample(faker_address::COUNTRY_DE_DE_AT).into(),
            "de_CH" => self.random.sample(faker_address::COUNTRY_DE_CH).into(),
            "nl_NL" => self.random.sample(faker_address::COUNTRY_NL_NL).into(),
            "nl_BE" => self.random.sample(faker_address::COUNTRY_NL_BE).into(),
            "nb_NO" => self.random.sample(faker_address::COUNTRY_NB_NO).into(),
            "sv_SE" => self.random.sample(faker_address::COUNTRY_SV_SE).into(),
            "da_DK" => self.random.sample(faker_address::COUNTRY_DA_DK).into(),
            "fi_FI" => self.random.sample(faker_address::COUNTRY_FI_FI).into(),
            "is_IS" => self.random.sample(faker_address::COUNTRY_IS_IS).into(),
            "pl_PL" => self.random.sample(faker_address::COUNTRY_PL_PL).into(),
            "cs_CZ" => self.random.sample(faker_address::COUNTRY_CS_CZ).into(),
            "sk_SK" => self.random.sample(faker_address::COUNTRY_SK_SK).into(),
            "ru_RU" => self.random.sample(faker_address::COUNTRY_RU_RU).into(),
            "uk_UA" => self.random.sample(faker_address::COUNTRY_UK_UA).into(),
            "kk_KZ" => self.random.sample(faker_address::COUNTRY_KK_KZ).into(),
            "tr_TR" => self.random.sample(faker_address::COUNTRY_TR_TR).into(),
            "ar_EG" | "ar_JO" | "ar_SA" => self.random.sample(faker_address::COUNTRY_AR).into(),
            "bn_BD" => self.random.sample(faker_address::COUNTRY_BN_BD).into(),
            "id_ID" => self.random.sample(faker_address::COUNTRY_ID_ID).into(),
            "ms_MY" => self.random.sample(faker_address::COUNTRY_MS_MY).into(),
            "ko_KR" => self.random.sample(faker_address::COUNTRY_KO_KR).into(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::CountryName,
            ),
        }
    }

    pub fn state(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.random.sample(faker_address::STATE_HU_HU).into(),
            "it_IT" | "it_CH" => self.random.sample(faker_address::STATE_IT_IT).into(),
            "pt_BR" => self.random.sample(faker_address::STATE_PT_BR).into(),
            "es_ES" => self.random.sample(faker_address::STATE_ES_ES).into(),
            "es_AR" => self.random.sample(faker_address::STATE_ES_AR).into(),
            "es_PE" => self.random.sample(faker_address::STATE_ES_PE).into(),
            "es_VE" => self.random.sample(faker_address::STATE_ES_VE).into(),
            "id_ID" => self.random.sample(faker_address::STATE_ID_ID).into(),
            "bn_BD" => self.random.sample(faker_address::STATE_BN_BD).into(),
            "de_DE" => self.random.sample(faker_address::STATE_DE_DE).into(),
            "de_AT" => self.random.sample(faker_address::STATE_DE_AT).into(),
            "fi_FI" => self.random.sample(faker_address::STATE_FI_FI).into(),
            "nl_NL" => self.random.sample(faker_address::STATE_NL_NL).into(),
            "nl_BE" => self.random.sample(faker_address::STATE_NL_BE).into(),
            "pl_PL" => self.random.sample(faker_address::STATE_PL_PL).into(),
            "ms_MY" => self
                .random
                .sample(Self::malaysian_region(&self.random).names)
                .into(),
            "fr_CA" => self.random.sample(faker_address::STATE_FR_CA).into(),
            "ar_JO" => self.random.sample(faker_address::STATE_AR_JO).into(),
            "en_IN" => self.random.sample(faker_address::STATE_EN_IN).into(),
            "en_AU" => self.random.sample(faker_address::STATE_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" => {
                self.random.sample(faker_address::STATE_EN_US).into()
            }
            "zh_CN" => self.random.sample(faker_address::STATE_ZH_CN).into(),
            "zh_TW" => "臺灣省".into(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::StateName,
            ),
        }
    }

    pub fn state_abbr(&self) -> String {
        match self.address_locale() {
            "it_IT" | "it_CH" => self.random.sample(faker_address::STATE_ABBR_IT_IT).into(),
            "pt_BR" => self.random.sample(faker_address::STATE_ABBR_PT_BR).into(),
            "es_AR" => self.random.sample(faker_address::STATE_ABBR_ES_AR).into(),
            "id_ID" => self.random.sample(faker_address::STATE_ABBR_ID_ID).into(),
            "fr_CA" => self.random.sample(faker_address::STATE_ABBR_FR_CA).into(),
            "ar_JO" => self.random.sample(faker_address::STATE_ABBR_AR_JO).into(),
            "en_IN" => self.random.sample(faker_address::STATE_ABBR_EN_IN).into(),
            "en_AU" => self.random.sample(faker_address::STATE_ABBR_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" => {
                self.random.sample(faker_address::STATE_ABBR_EN_US).into()
            }
            "zh_CN" => self.random.sample(faker_address::STATE_ABBR_ZH_CN).into(),
            "zh_TW" => "臺".into(),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::StateAbbr,
            ),
        }
    }

    /// Returns a district from the Simplified Chinese Address provider.
    pub fn area(&self) -> DatabaseResult<&'static str> {
        if self.provider_locale() == "zh_CN" {
            Ok(self.random.sample(DISTRICTS_ZH_CN))
        } else {
            Err(DatabaseError::InvalidQuery("unsupported Faker area locale"))
        }
    }

    /// Returns a street stem from the supported regional catalogs.
    pub fn street(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "sr_RS" | "sr_Cyrl_RS" => faker_address::STREET_SR_RS,
            "sr_Latn_RS" => faker_address::STREET_SR_LATN_RS,
            "me_ME" => faker_address::STREET_ME_ME,
            "hy_AM" => faker_address::STREET_HY_AM,
            "ka_GE" => faker_address::STREET_KA_GE,
            "lt_LT" => faker_address::STREET_LT_LT,
            "lv_LV" => faker_address::STREET_LV_LV,
            "hr_HR" => faker_address::STREETS_HR_HR,
            "zh_TW" => faker_address::STREET_ZH_TW,
            "id_ID" => faker_address::STREET_ID_ID,
            "ru_RU" => faker_address::STREET_RU_RU,
            "kk_KZ" => faker_address::STREET_KK_KZ,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker street locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    /// Samples the autonomous community catalog inherited by the Spanish Address providers.
    pub fn community(&self) -> DatabaseResult<&'static str> {
        if !matches!(
            self.provider_locale(),
            "es_ES" | "es_AR" | "es_PE" | "es_VE"
        ) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker community locale",
            ));
        }
        Ok(self.random.sample(faker_address::COMMUNITY_ES_ES))
    }

    pub fn street_prefix(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "sr_RS" | "sr_Cyrl_RS" => faker_address::STREET_PREFIX_SR_RS,
            "sr_Latn_RS" => faker_address::STREET_PREFIX_SR_LATN_RS,
            "me_ME" => faker_address::STREET_PREFIX_ME_ME,
            "hy_AM" => faker_address::STREET_PREFIX_HY_AM,
            "lv_LV" => faker_address::STREET_PREFIX_LV_LV,
            "ro_RO" => faker_address::STREET_PREFIX_RO_RO,
            "ro_MD" => faker_address::STREET_PREFIX_RO_MD,
            "el_GR" => faker_address::STREET_PREFIX_EL_GR,
            "el_CY" => faker_address::STREET_PREFIX_EL_CY,
            "it_CH" => faker_address::STREET_PREFIX_IT_CH,
            "pt_BR" => faker_address::STREET_PREFIX_PT_BR,
            "pt_PT" => faker_address::STREET_PREFIX_PT_PT,
            "es_ES" | "es_AR" => faker_address::STREET_PREFIX_ES_ES,
            "es_PE" => faker_address::STREET_PREFIX_ES_PE,
            "es_VE" => faker_address::STREET_PREFIX_ES_VE,
            "en_SG" => &["Jalan"],
            "th_TH" => faker_address::STREET_PREFIX_TH_TH,
            "nb_NO" => faker_address::STREET_PREFIX_NB_NO,
            "sv_SE" => faker_address::STREET_PREFIX_SV_SE,
            "is_IS" => faker_address::STREET_PREFIX_IS_IS,
            "ru_RU" => faker_address::STREET_PREFIX_RU_RU,
            "uk_UA" => faker_address::STREET_PREFIX_UK_UA,
            "fa_IR" => faker_address::STREET_PREFIX_FA_IR,
            "ar_EG" | "ar_SA" => faker_address::STREET_PREFIX_AR_EG_SA,
            "ar_JO" => faker_address::STREET_PREFIX_AR_JO,
            "id_ID" => faker_address::STREET_PREFIX_ID_ID,
            "ms_MY" => faker_address::STREET_PREFIX_MS_MY,
            "fr_CH" => faker_address::STREET_PREFIX_FR_CH,
            "fr_FR" | "fr_BE" | "fr_CA" => faker_address::STREET_PREFIX_FR_FR,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker street prefix locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn street_number(&self) -> DatabaseResult<String> {
        if self.provider_locale() == "bn_BD" {
            return Ok(self.bangla_street_number());
        }
        Ok(self.numerify(Some(
            self.singapore_address_value(faker_address::STREET_NUMBER_EN_SG)?,
        )))
    }

    pub fn block_number(&self) -> DatabaseResult<String> {
        Ok(self.numerify(Some(
            self.singapore_address_value(faker_address::BLOCK_NUMBER_EN_SG)?,
        )))
    }

    pub fn floor_number(&self) -> DatabaseResult<&'static str> {
        self.singapore_address_value(faker_address::FLOOR_NUMBER_EN_SG)
    }

    pub fn apartment_number(&self) -> DatabaseResult<&'static str> {
        match self.provider_locale() {
            "ro_RO" | "ro_MD" => Ok(Self::romanian_unit_number(&self.random)),
            _ => self.singapore_address_value(faker_address::APARTMENT_NUMBER_EN_SG),
        }
    }

    pub fn block(&self) -> DatabaseResult<String> {
        self.require_romanian_address_locale()?;
        Ok(self.numerify(Some(self.random.sample(faker_address::BLOCK_RO_RO))))
    }

    pub fn block_segment(&self) -> DatabaseResult<&'static str> {
        self.require_romanian_address_locale()?;
        Ok(self.random.sample(faker_address::BLOCK_SEGMENT_RO_RO))
    }

    pub fn floor(&self) -> DatabaseResult<&'static str> {
        self.require_romanian_address_locale()?;
        Ok(Self::romanian_unit_number(&self.random))
    }

    pub fn street_plain_name(&self) -> DatabaseResult<&'static str> {
        let names = match self.provider_locale() {
            "ro_RO" => faker_address::STREET_PLAIN_NAME_RO_RO,
            "ro_MD" => faker_address::STREET_PLAIN_NAME_RO_MD,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker Romanian address locale",
                ));
            }
        };
        Ok(self.random.sample(names))
    }

    fn require_romanian_address_locale(&self) -> DatabaseResult<()> {
        if !matches!(self.provider_locale(), "ro_RO" | "ro_MD") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Romanian address locale",
            ));
        }
        Ok(())
    }

    fn romanian_unit_number(random: &FakerRandom) -> &'static str {
        // FakerPHP picks the one- or two-digit template before substituting digits.
        let numbers = &faker_address::ROMANIAN_UNIT_NUMBERS;
        if random.bool() {
            random.sample(&numbers[..10])
        } else {
            random.sample(&numbers[10..])
        }
    }

    pub fn town_name(&self) -> DatabaseResult<&'static str> {
        self.singapore_address_value(&["SINGAPORE"])
    }

    fn singapore_address_value(&self, values: &[&'static str]) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "en_SG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Singapore address locale",
            ));
        }
        Ok(self.random.sample(values))
    }

    pub fn locality(&self) -> DatabaseResult<String> {
        self.require_indian_address_locale()?;
        Ok(self.indian_locality())
    }

    pub fn locality_name(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(self.random.sample(faker_address::LOCALITY_NAME_EN_IN))
    }

    pub fn area_suffix(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(self.random.sample(faker_address::AREA_SUFFIX_EN_IN))
    }

    pub fn society_name(&self) -> DatabaseResult<String> {
        self.require_indian_address_locale()?;
        Ok(self.indian_society_name())
    }

    pub fn society_suffix(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(self.random.sample(faker_address::SOCIETY_SUFFIX_EN_IN))
    }

    fn require_indian_address_locale(&self) -> DatabaseResult<()> {
        if self.provider_locale() != "en_IN" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Indian address locale",
            ));
        }
        Ok(())
    }

    fn indian_locality(&self) -> String {
        if self.boolean(None) {
            format!(
                "{}{}",
                self.first_name(),
                self.random.sample(faker_address::AREA_SUFFIX_EN_IN)
            )
        } else {
            self.random
                .sample(faker_address::LOCALITY_NAME_EN_IN)
                .into()
        }
    }

    fn indian_society_name(&self) -> String {
        format!(
            "{} {}",
            self.first_name(),
            self.random.sample(faker_address::SOCIETY_SUFFIX_EN_IN)
        )
    }

    pub fn municipality(&self) -> DatabaseResult<&'static str> {
        if self.provider_locale() == "lt_LT" {
            return Ok(self.random.sample(faker_address::MUNICIPALITY_LT_LT));
        }
        self.require_address_locale("en_PH")?;
        Ok(self.random.sample(faker_address::MUNICIPALITY_EN_PH))
    }

    pub fn barangay(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_PH")?;
        Ok(self.random.sample(faker_address::BARANGAY_EN_PH))
    }

    pub fn town(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(
            &self.random,
            faker_address::TOWN_FORMATS_EN_HK,
        ))
    }

    pub fn village(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(
            &self.random,
            faker_address::VILLAGE_FORMATS_EN_HK,
        ))
    }

    pub fn estate(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(
            &self.random,
            faker_address::ESTATE_FORMATS_EN_HK,
        ))
    }

    pub fn syllable(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(self.random.sample(faker_address::SYLLABLES_EN_HK))
    }

    pub fn direction(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(self.random.sample(faker_address::DIRECTIONS_EN_HK))
    }

    pub fn english_street_name(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(self
            .random
            .sample(faker_address::ENGLISH_STREET_NAMES_EN_HK))
    }

    pub fn village_suffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(self.random.sample(faker_address::VILLAGE_SUFFIX_EN_HK))
    }

    pub fn estate_suffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(self.random.sample(faker_address::ESTATE_SUFFIX_EN_HK))
    }

    fn require_address_locale(&self, locale: &str) -> DatabaseResult<()> {
        if self.provider_locale() != locale {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker address formatter locale",
            ));
        }
        Ok(())
    }

    fn hong_kong_name(random: &FakerRandom, formats: &[&str]) -> String {
        let mut name = String::new();
        Self::write_hong_kong_name(random, &mut name, random.sample(formats));
        name
    }

    fn write_hong_kong_name(random: &FakerRandom, name: &mut String, mut format: &str) {
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, remaining) = token
                .split_once("}}")
                .expect("fixed Hong Kong address template");
            name.push_str(literal);
            match token {
                "town" => Self::write_hong_kong_name(
                    random,
                    name,
                    random.sample(faker_address::TOWN_FORMATS_EN_HK),
                ),
                "syllable" => name.push_str(random.sample(faker_address::SYLLABLES_EN_HK)),
                "direction" => name.push_str(random.sample(faker_address::DIRECTIONS_EN_HK)),
                "englishStreetName" => {
                    name.push_str(random.sample(faker_address::ENGLISH_STREET_NAMES_EN_HK))
                }
                "streetSuffix" => name.push_str(random.sample(faker_address::STREET_SUFFIX_EN_HK)),
                "villageSuffix" => {
                    name.push_str(random.sample(faker_address::VILLAGE_SUFFIX_EN_HK))
                }
                "estateSuffix" => name.push_str(random.sample(faker_address::ESTATE_SUFFIX_EN_HK)),
                _ => unreachable!("unknown fixed Hong Kong address token: {token}"),
            }
            format = remaining;
        }
        name.push_str(format);
    }

    pub fn prefecture(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "el_GR" => faker_address::PREFECTURE_EL_GR,
            "ja_JP" => faker_address::PREFECTURE_JA_JP,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker address formatter locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn ward(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ja_JP")?;
        Ok(self.random.sample(faker_address::WARD_JA_JP))
    }

    pub fn postcode1(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(self.random.i64(faker_address::POSTCODE_PREFIX_JA_JP))
    }

    pub fn postcode2(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(self.random.i64(faker_address::POSTCODE_SUFFIX_JA_JP))
    }

    pub fn area_number(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(self.random.i64(faker_address::AREA_NUMBER_JA_JP))
    }

    pub fn metropolitan_city(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ko_KR")?;
        Ok(self.random.sample(faker_address::METROPOLITAN_CITY_KO_KR))
    }

    pub fn borough(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ko_KR")?;
        Ok(self.random.sample(faker_address::BOROUGH_KO_KR))
    }

    pub fn hamlet_name(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::HAMLET_NAME_FORMATS_VI_VN))
    }

    pub fn hamlet_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(self.random.sample(faker_address::HAMLET_PREFIX_VI_VN))
    }

    pub fn ward_name(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::WARD_NAME_FORMATS_VI_VN))
    }

    pub fn ward_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(self.random.sample(faker_address::WARD_PREFIX_VI_VN))
    }

    pub fn district_name(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::DISTRICT_NAME_FORMATS_VI_VN))
    }

    pub fn district_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(self.random.sample(faker_address::DISTRICT_PREFIX_VI_VN))
    }

    fn vietnamese_locality_name(&self, formats: &[&str]) -> String {
        self.bothify(Some(&self.vietnamese_address_name(formats)))
    }

    fn vietnamese_address_name(&self, formats: &[&str]) -> String {
        let mut format = self.random.sample(formats);
        let mut name = String::new();
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, rest) = token
                .split_once("}}")
                .expect("fixed Vietnamese address template");
            name.push_str(literal);
            match token {
                "firstName" => name.push_str(&self.first_name()),
                "middleName" => name.push_str(self.middle_name().expect("Vietnamese middle name")),
                "lastName" => name.push_str(&self.last_name()),
                "hamletPrefix" => {
                    name.push_str(self.random.sample(faker_address::HAMLET_PREFIX_VI_VN))
                }
                "wardPrefix" => name.push_str(self.random.sample(faker_address::WARD_PREFIX_VI_VN)),
                "districtPrefix" => {
                    name.push_str(self.random.sample(faker_address::DISTRICT_PREFIX_VI_VN))
                }
                _ => unreachable!("unknown fixed Vietnamese address token: {token}"),
            };
            format = rest;
        }
        name.push_str(format);
        name
    }

    pub fn building_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(self.random.sample(faker_address::BUILDING_PREFIX_MS_MY))
    }

    pub fn township(&self) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        Ok(self.malaysian_township())
    }

    pub fn township_prefix_abbr(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(self
            .random
            .sample(faker_address::TOWNSHIP_PREFIX_ABBR_MS_MY))
    }

    pub fn township_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(self.random.sample(faker_address::TOWNSHIP_PREFIX_MS_MY))
    }

    pub fn township_suffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(self.random.sample(faker_address::TOWNSHIP_SUFFIX_MS_MY))
    }

    pub fn town_state(&self) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        Ok(Self::malaysian_town_state(&self.random))
    }

    /// Generates a Malaysian postcode from a FakerPHP state key, such as `perlis`.
    pub fn postcode_with_state(&self, state: &str) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        faker_address::MALAYSIAN_REGIONS
            .iter()
            .find(|region| region.key == state)
            .map(|region| region.postcode(&self.random))
            .ok_or(DatabaseError::InvalidQuery(
                "unknown Faker Malaysian postcode state",
            ))
    }

    fn malaysian_region(random: &FakerRandom) -> &'static faker_address::MalaysianRegion {
        let regions = faker_address::MALAYSIAN_REGIONS;
        &regions[random.usize(..regions.len())]
    }

    fn malaysian_town_state(random: &FakerRandom) -> String {
        let region = Self::malaysian_region(random);
        format!(
            "{} {}, {}",
            region.postcode(random),
            random.sample(region.towns),
            random.sample(region.names)
        )
    }

    fn malaysian_township(&self) -> String {
        self.malaysian_address_name(faker_address::TOWNSHIP_FORMATS_MS_MY)
    }

    fn malaysian_address_name(&self, formats: &[&str]) -> String {
        let formatted = self
            .bothify(Some(self.random.sample(formats)))
            .to_ascii_uppercase();
        let mut template = formatted.as_str();
        let mut name = String::new();
        while let Some((literal, token)) = template.split_once("{{") {
            let (token, rest) = token
                .split_once("}}")
                .expect("fixed Malaysian address template");
            name.push_str(literal);
            let values = match token {
                "STREETPREFIX" => faker_address::STREET_PREFIX_MS_MY,
                "STREETSUFFIX" => faker_address::STREET_SUFFIX_MS_MY,
                "TOWNSHIPPREFIX" => faker_address::TOWNSHIP_PREFIX_MS_MY,
                "TOWNSHIPPREFIXABBR" => faker_address::TOWNSHIP_PREFIX_ABBR_MS_MY,
                "TOWNSHIPSUFFIX" => faker_address::TOWNSHIP_SUFFIX_MS_MY,
                _ => unreachable!("unknown fixed Malaysian address token: {token}"),
            };
            name.push_str(self.random.sample(values));
            template = rest;
        }
        name.push_str(template);
        name
    }

    pub fn bangla_street_name(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("bn_BD")?;
        Ok(self.random.sample(faker_address::STREET_NAMES_BN_BD))
    }

    pub fn ward_number(&self) -> DatabaseResult<String> {
        self.require_address_locale("ne_NP")?;
        Ok(self.nepali_ward_number())
    }

    fn bangla_street_number(&self) -> String {
        const DIGITS: [char; 10] = ['০', '১', '২', '৩', '৪', '৫', '৬', '৭', '৮', '৯'];
        self.number_between(1, 100)
            .to_string()
            .bytes()
            .map(|digit| DIGITS[usize::from(digit - b'0')])
            .collect()
    }

    fn nepali_ward_number(&self) -> String {
        self.numerify(Some(self.random.sample(faker_address::WARD_NUMBER_NE_NP)))
    }

    pub fn governorate(&self) -> DatabaseResult<&'static str> {
        match self.provider_locale() {
            "ar_EG" => Ok(self.random.sample(faker_address::GOVERNORATES_AR_EG).0),
            "ar_SA" => Ok(self.random.sample(faker_address::GOVERNORATES_AR_SA)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker governorate locale",
            )),
        }
    }

    pub fn subdivision(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ar_SA")?;
        Ok(self.random.sample(faker_address::SUBDIVISIONS_AR_SA))
    }

    pub fn building_name_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(self
            .random
            .sample(faker_address::BUILDING_NAME_PREFIX_FA_IR))
    }

    pub fn building_number_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(self
            .random
            .sample(faker_address::BUILDING_NUMBER_PREFIX_FA_IR))
    }

    pub fn postcode_prefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(self.random.sample(faker_address::POSTCODE_PREFIX_FA_IR))
    }

    pub fn building(&self) -> DatabaseResult<String> {
        self.require_address_locale("fa_IR")?;
        Ok(self.persian_building())
    }

    fn persian_building(&self) -> String {
        let mut name = format!(
            "{} {}",
            self.random
                .sample(faker_address::BUILDING_NAME_PREFIX_FA_IR),
            self.first_name()
        );
        if self.boolean(None) {
            write!(
                name,
                " {} {}",
                self.random
                    .sample(faker_address::BUILDING_NUMBER_PREFIX_FA_IR),
                self.building_number()
            )
            .expect("writing to String cannot fail");
        }
        name
    }

    /// Samples a Georgian region name in the genitive form.
    pub fn region_genitive_form(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ka_GE")?;
        Ok(self
            .random
            .sample(faker_address::REGION_GENITIVE_FORM_KA_GE))
    }

    pub fn region_suffix(&self) -> DatabaseResult<&'static str> {
        match self.provider_locale() {
            "hy_AM" => Ok(self.random.sample(faker_address::REGION_SUFFIX_HY_AM)),
            "ka_GE" => Ok(self.random.sample(faker_address::REGION_SUFFIX_KA_GE)),
            "lt_LT" => Ok(self.random.sample(faker_address::REGION_SUFFIX_LT_LT)),
            "lv_LV" => Ok(self.random.sample(faker_address::REGION_SUFFIX_LV_LV)),
            "ru_RU" | "uk_UA" => Ok("область"),
            "kk_KZ" => Ok("облысы"),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker region suffix locale",
            )),
        }
    }

    pub fn city_and_region(&self) -> DatabaseResult<String> {
        self.require_address_locale("uk_UA")?;
        Ok(Self::ukrainian_city_and_region(&self.random))
    }

    fn ukrainian_city_and_region(random: &FakerRandom) -> String {
        let (city, region) = random.sample(faker_address::CITY_REGIONS_UK_UA);
        format!("{region} область, місто {city}")
    }

    pub fn street_suffix_short(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "de_DE" => faker_address::STREET_SUFFIX_SHORT_DE_DE,
            "de_AT" => faker_address::STREET_SUFFIX_SHORT_DE_AT,
            "de_CH" => faker_address::STREET_SUFFIX_SHORT_DE_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker short street suffix locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn street_suffix_long(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "de_DE" => faker_address::STREET_SUFFIX_LONG_DE_DE,
            "de_AT" => faker_address::STREET_SUFFIX_LONG_DE_AT,
            "de_CH" => faker_address::STREET_SUFFIX_LONG_DE_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker long street suffix locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    fn german_street_name(&self) -> String {
        let swiss = self.provider_locale() == "de_CH";
        match self.random.u8(0..if swiss { 3 } else { 2 }) {
            0 => format!(
                "{}{}",
                self.last_name(),
                self.street_suffix_short()
                    .expect("fixed German street locale")
            ),
            1 if swiss => format!(
                "{}{}",
                self.city(),
                self.street_suffix_short()
                    .expect("fixed German street locale")
            ),
            _ => format!(
                "{}-{}-{}",
                self.first_name(),
                self.last_name(),
                self.street_suffix_long()
                    .expect("fixed German street locale")
            ),
        }
    }

    fn german_building_number(&self, formats: &[&str]) -> String {
        let format = self.random.sample(formats);
        if let Some(prefix) = format.strip_suffix("[abc]") {
            let mut number = self.numerify(Some(prefix));
            number.push(self.random.sample(&['a', 'b', 'c']));
            return number;
        }
        self.numerify(Some(format))
    }

    pub fn street_suffix_word(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "nb_NO" => faker_address::STREET_SUFFIX_WORD_NB_NO,
            "sv_SE" => faker_address::STREET_SUFFIX_WORD_SV_SE,
            "da_DK" => faker_address::STREET_SUFFIX_WORD_DA_DK,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker street suffix word locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn building_level(&self) -> DatabaseResult<String> {
        self.require_address_locale("da_DK")?;
        Ok(self.danish_building_level())
    }

    pub fn building_side(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("da_DK")?;
        Ok(self.random.sample(faker_address::BUILDING_SIDE_DA_DK))
    }

    pub fn kommune(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("da_DK")?;
        Ok(self.random.sample(faker_address::KOMMUNE_DA_DK))
    }

    fn danish_building_level(&self) -> String {
        self.numerify(Some(
            self.random.sample(faker_address::BUILDING_LEVEL_DA_DK),
        ))
    }

    pub fn capital(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(self.random.sample(faker_address::CAPITALS_HU_HU))
    }

    pub fn big_city(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(self.random.sample(faker_address::BIG_CITIES_HU_HU))
    }

    pub fn smaller_city(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(self.random.sample(faker_address::SMALLER_CITIES_HU_HU))
    }

    pub fn city_name(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "sr_RS" | "sr_Cyrl_RS" => faker_address::CITY_NAMES_SR_RS,
            "sr_Latn_RS" => faker_address::CITY_NAMES_SR_LATN_RS,
            "me_ME" => faker_address::CITY_NAMES_ME_ME,
            "ka_GE" => faker_address::CITY_KA_GE,
            "hr_HR" => faker_address::CITY_NAMES_HR_HR,
            "sl_SI" => faker_address::CITY_SL_SI,
            "ro_RO" => faker_address::CITY_NAMES_RO_RO,
            "ro_MD" => faker_address::CITY_NAMES_RO_MD,
            "it_CH" => faker_address::CITY_NAMES_IT_CH,
            "en_UG" => faker_address::CITY_NAME_EN_UG,
            "id_ID" => faker_address::CITY_ID_ID,
            "ne_NP" => faker_address::CITY_NE_NP,
            "fa_IR" => faker_address::CITY_FA_IR,
            "tr_TR" => faker_address::CITY_TR_TR,
            "sk_SK" => faker_address::CITY_SK_SK,
            "de_DE" => faker_address::CITY_DE_DE,
            "de_AT" => faker_address::CITY_DE_AT,
            "de_CH" => faker_address::CITY_DE_CH,
            "nl_NL" => faker_address::CITY_NL_NL,
            "nl_BE" => faker_address::CITY_NL_BE,
            "nb_NO" => faker_address::CITY_NB_NO,
            "sv_SE" => faker_address::CITY_SV_SE,
            "da_DK" => faker_address::CITY_DA_DK,
            "is_IS" => faker_address::CITY_IS_IS,
            "ar_EG" => faker_address::CITY_AR_EG,
            "ar_JO" => faker_address::CITY_AR_JO,
            "ar_SA" => faker_address::CITY_AR_SA,
            "fr_BE" => faker_address::CITY_FR_BE,
            "fr_CH" => faker_address::CITY_FR_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker city name locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    /// Samples a county from the British Address provider.
    pub fn county(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "ro_RO" => faker_address::COUNTIES_RO_RO,
            "en_GB" => faker_address::COUNTY_EN_GB,
            "en_NG" => faker_address::COUNTY_EN_NG,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker county locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn province(&self) -> DatabaseResult<&'static str> {
        let provinces = match self.provider_locale() {
            "fr_BE" => faker_address::PROVINCE_FR_BE,
            "en_ZA" => faker_address::PROVINCE_EN_ZA,
            "en_CA" => faker_address::PROVINCE_EN_CA,
            "en_PH" => faker_address::PROVINCE_EN_PH,
            "ko_KR" => faker_address::PROVINCE_KO_KR,
            "vi_VN" => faker_address::PROVINCE_VI_VN,
            "th_TH" => faker_address::PROVINCE_TH_TH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker province locale",
                ));
            }
        };
        Ok(self.random.sample(provinces))
    }

    pub fn province_abbr(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "en_CA" => faker_address::PROVINCE_ABBR_EN_CA,
            "en_ZA" => faker_address::PROVINCE_ABBR_EN_ZA,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker province abbreviation locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    pub fn random_postcode_letter(&self) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "en_CA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker postcode letter locale",
            ));
        }
        Ok(self.random.sample(faker_address::POSTCODE_LETTERS_EN_CA))
    }

    pub fn building_letter(&self) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "en_AU" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker building letter locale",
            ));
        }
        Ok(self.random.sample(faker_address::BUILDING_LETTERS_EN_AU))
    }

    pub fn canton(&self) -> DatabaseResult<(&'static str, &'static str)> {
        let cantons = match self.provider_locale() {
            "it_CH" => faker_address::CANTONS_IT_CH,
            "de_CH" => faker_address::CANTONS_DE_CH,
            "fr_CH" => faker_address::CANTONS_FR_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker canton locale",
                ));
            }
        };
        Ok(self.random.sample(cantons))
    }

    pub fn canton_short(&self) -> DatabaseResult<&'static str> {
        self.canton().map(|(code, _)| code)
    }

    pub fn canton_name(&self) -> DatabaseResult<&'static str> {
        self.canton().map(|(_, name)| name)
    }

    pub fn district(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "en_UG" => faker_address::DISTRICT_EN_UG,
            "ne_NP" => faker_address::DISTRICT_NE_NP,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker district locale",
                ));
            }
        };
        Ok(self.random.sample(values))
    }

    /// Samples a Brazilian region abbreviation.
    pub fn region_abbr(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("pt_BR")?;
        Ok(self.random.sample(faker_address::REGION_ABBR_PT_BR))
    }

    pub fn region(&self) -> DatabaseResult<&'static str> {
        let values = match self.provider_locale() {
            "hy_AM" => Some(faker_address::REGION_HY_AM),
            "ka_GE" => Some(faker_address::REGION_KA_GE),
            "lt_LT" => Some(faker_address::REGION_LT_LT),
            "lv_LV" => Some(faker_address::REGION_LV_LV),
            "pt_BR" => Some(faker_address::REGION_PT_BR),
            "en_NZ" => Some(faker_address::REGION_EN_NZ),
            "en_UG" => Some(faker_address::REGION_EN_UG),
            "en_NG" => Some(faker_address::REGIONS_EN_NG),
            "ru_RU" => Some(faker_address::REGION_RU_RU),
            "cs_CZ" => Some(faker_address::REGION_CS_CZ),
            "da_DK" => Some(faker_address::REGION_DA_DK),
            "is_IS" => Some(faker_address::REGION_IS_IS),
            "kk_KZ" => Some(faker_address::REGION_KK_KZ),
            "uk_UA" => {
                return Ok(self.random.sample(faker_address::CITY_REGIONS_UK_UA).1);
            }
            _ => None,
        };
        if let Some(values) = values {
            return Ok(self.random.sample(values));
        }
        if !matches!(
            self.provider_locale(),
            "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA"
        ) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker region locale",
            ));
        }
        Ok(self.random.sample(faker_address::REGIONS_FR_FR))
    }

    /// Returns the French department code and name from the same catalog entry.
    pub fn department(&self) -> DatabaseResult<(&'static str, &'static str)> {
        if !matches!(
            self.provider_locale(),
            "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA"
        ) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker department locale",
            ));
        }
        Ok(self.random.sample(faker_address::DEPARTMENTS_FR_FR))
    }

    pub fn department_name(&self) -> DatabaseResult<&'static str> {
        self.department().map(|(_, name)| name)
    }

    pub fn department_number(&self) -> DatabaseResult<&'static str> {
        self.department().map(|(number, _)| number)
    }

    pub fn random_chinese_number(&self) -> DatabaseResult<&'static str> {
        self.ensure_taiwan_address_locale()?;
        Ok(self.taiwan_chinese_digit())
    }

    pub fn random_number2(&self) -> DatabaseResult<u8> {
        self.ensure_taiwan_address_locale()?;
        Ok(self.random.u8(1..=100))
    }

    pub fn random_number3(&self) -> DatabaseResult<u16> {
        self.ensure_taiwan_address_locale()?;
        Ok(self.random.u16(1..=1000))
    }

    fn ensure_taiwan_address_locale(&self) -> DatabaseResult<()> {
        if self.provider_locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Taiwanese number locale",
            ));
        }
        Ok(())
    }

    fn taiwan_chinese_digit(&self) -> &'static str {
        self.random
            .sample(&["一", "二", "三", "四", "五", "六", "七", "八", "九"])
    }

    pub fn postcode(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_HU_HU))),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                self.numerify(Some(self.random.sample(faker_address::POSTCODE_SR_RS)))
            }
            "hy_AM" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_HY_AM))),
            "ka_GE" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_KA_GE))),
            "lt_LT" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_LT_LT))),
            "lv_LV" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_LV_LV))),
            "hr_HR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_HR_HR))),
            "sl_SI" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_SL_SI))),
            "ro_RO" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_RO_RO))),
            "ro_MD" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_RO_MD))),
            "el_GR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_EL_GR))),
            "el_CY" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_EL_CY))),
            "it_IT" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_IT_IT))),
            "it_CH" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_IT_CH))),
            "pt_BR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_PT_BR))),
            "pt_PT" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_PT_PT))),
            "es_ES" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_ES_ES))),
            "es_AR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_ES_AR))),
            "es_PE" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_ES_PE))),
            "es_VE" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_ES_VE))),
            "de_CH" | "en_PH" | "en_ZA" => self.numerify(Some("####")),
            "de_DE" => self.numerify(Some("#####")),
            "de_AT" => self.random.sample(faker_address::POSTCODE_DE_AT).into(),
            "nl_NL" => self.random.sample(faker_address::POSTCODE_NL_NL).into(),
            "nl_BE" => self.random.sample(faker_address::POSTCODE_NL_BE).into(),
            "nb_NO" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_NB_NO))),
            "sv_SE" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_SV_SE))),
            "da_DK" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_DA_DK))),
            "fi_FI" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_FI_FI))),
            "is_IS" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_IS_IS))),
            "en_SG" => self.numerify(Some("######")),
            "en_IN" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_EN_IN))),
            "en_UG" => self.numerify(Some(
                self.random.sample(&["#", "##", "###", "####", "#####"]),
            )),
            "en_NG" => self.numerify(Some(self.random.sample(&["#####", "## ###"]))),
            "en_GB" => self.random.sample(faker_address::POSTCODE_EN_GB).into(),
            "en_CA" => {
                let pattern = self.random.sample(&["?#? #?#", "?#?-#?#", "?#?#?#"]);
                let mut postcode = String::with_capacity(pattern.len());
                for character in pattern.chars() {
                    match character {
                        '?' => postcode
                            .push_str(self.random.sample(faker_address::POSTCODE_LETTERS_EN_CA)),
                        '#' => postcode.push(char::from(b'0' + self.random_digit())),
                        literal => postcode.push(literal),
                    }
                }
                postcode
            }
            "en_AU" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_EN_AU))),
            "en_NZ" => self.numerify(Some("####")),
            "en_US" => {
                let format = if self.boolean(None) {
                    "#####"
                } else {
                    "#####-####"
                };
                self.numerify(Some(format))
            }
            "zh_CN" => format!("{:02}{}00", self.random.u8(1..=85), self.random.u8(10..=88)),
            "zh_TW" => self.numerify(Some(self.random.sample(&["###-##", "###"]))),
            "fr_BE" | "fr_CH" => self.numerify(Some("####")),
            "fr_CA" => self
                .bothify(Some(self.random.sample(&["?#? #?#", "?#?#?#"])))
                .to_ascii_uppercase(),
            "en_HK" | "fr_FR" | "ne_NP" | "th_TH" | "tr_TR" | "uk_UA" => {
                self.numerify(Some("#####"))
            }
            "ru_RU" => self.numerify(Some("######")),
            "cs_CZ" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_CS_CZ))),
            "pl_PL" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_PL_PL))),
            "sk_SK" => self.numerify(Some("### ##")),
            "kk_KZ" => self.numerify(Some("0#####")),
            "bn_BD" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_BN_BD))),
            "fa_IR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_FA_IR))),
            "he_IL" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_HE_IL))),
            "ar_EG" | "ar_JO" | "ar_SA" => {
                self.numerify(Some(self.random.sample(faker_address::POSTCODE_AR)))
            }
            "id_ID" => self.numerify(Some("%####")),
            "ms_MY" => Self::malaysian_region(&self.random).postcode(&self.random),
            "ko_KR" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_KO_KR))),
            "vi_VN" => self.numerify(Some(self.random.sample(faker_address::POSTCODE_VI_VN))),
            "ja_JP" => format!(
                "{}{}",
                self.random.i64(faker_address::POSTCODE_PREFIX_JA_JP),
                self.random.i64(faker_address::POSTCODE_SUFFIX_JA_JP)
            ),
            _ => generate!(
                &self.random,
                self.address_locale(),
                faker::address::raw::PostCode,
            ),
        }
    }

    pub fn building_number(&self) -> String {
        let formats: &[&str] = match self.address_locale() {
            "hu_HU" => faker_address::BUILDING_NUMBER_HU_HU,
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => faker_address::BUILDING_NUMBER_SR_RS,
            "hy_AM" | "ka_GE" => faker_address::BUILDING_NUMBER_HY_AM,
            "lt_LT" | "lv_LV" => faker_address::BUILDING_NUMBER_LT_LT,
            "hr_HR" => faker_address::BUILDING_NUMBER_HR_HR,
            "sl_SI" => faker_address::BUILDING_NUMBER_SL_SI,
            "ro_RO" | "ro_MD" => faker_address::BUILDING_NUMBER_RO_RO,
            "el_GR" => faker_address::BUILDING_NUMBER_EL_GR,
            "el_CY" => faker_address::BUILDING_NUMBER_EL_CY,
            "it_IT" => faker_address::BUILDING_NUMBER_IT_IT,
            "it_CH" => faker_address::BUILDING_NUMBER_IT_CH,
            "pt_BR" => faker_address::BUILDING_NUMBER_PT_BR,
            "pt_PT" => faker_address::BUILDING_NUMBER_PT_PT,
            "es_ES" | "es_VE" => faker_address::BUILDING_NUMBER_ES_ES,
            "es_AR" | "es_PE" => faker_address::BUILDING_NUMBER_ES_AR,
            "en_AU" | "en_NZ" | "en_GB" | "tr_TR" => &["%##", "%#", "%"],
            "en_US" | "en_CA" | "en_UG" | "en_ZA" | "sk_SK" => &["%####", "%###", "%##"],
            "fr_FR" | "fr_BE" => &["%", "%#", "%#", "%#", "%##"],
            "fr_CA" => &["%####", "%###", "%##", "%#", "%"],
            "fr_CH" => &["%##", "%#", "%", "#a", "#b", "#c"],
            "zh_CN" | "zh_TW" | "bn_BD" | "en_HK" | "en_IN" | "en_NG" | "en_SG" | "fa_IR"
            | "ne_NP" | "ru_RU" | "uk_UA" | "nl_BE" => &["%#"],
            "en_PH" => faker_address::BUILDING_NUMBER_EN_PH,
            "ko_KR" => faker_address::BUILDING_NUMBER_KO_KR,
            "vi_VN" => faker_address::BUILDING_NUMBER_VI_VN,
            "th_TH" => faker_address::BUILDING_NUMBER_TH_TH,
            "fi_FI" => faker_address::BUILDING_NUMBER_FI_FI,
            "da_DK" | "is_IS" => {
                return self
                    .bothify(Some(
                        self.random.sample(faker_address::BUILDING_NUMBER_DA_DK),
                    ))
                    .to_ascii_uppercase();
            }
            "nb_NO" | "sv_SE" => {
                return self
                    .bothify(Some(
                        self.random.sample(faker_address::BUILDING_NUMBER_NB_NO),
                    ))
                    .to_ascii_uppercase();
            }
            "nl_NL" => {
                return self.bothify(Some(
                    self.random.sample(faker_address::BUILDING_NUMBER_NL_NL),
                ));
            }
            "de_DE" => return self.german_building_number(faker_address::BUILDING_NUMBER_DE_DE),
            "de_AT" | "de_CH" => {
                return self.german_building_number(faker_address::BUILDING_NUMBER_DE_AT_CH);
            }
            "pl_PL" => faker_address::BUILDING_NUMBER_PL_PL,
            "cs_CZ" => faker_address::BUILDING_NUMBER_CS_CZ,
            "kk_KZ" => &["%##"],
            "he_IL" => faker_address::BUILDING_NUMBER_HE_IL,
            "ar_EG" | "ar_JO" | "ar_SA" => faker_address::BUILDING_NUMBER_AR,
            "id_ID" => return self.number_between(1, 999).to_string(),
            "ms_MY" => {
                return self
                    .bothify(Some(
                        self.random.sample(faker_address::BUILDING_NUMBER_MS_MY),
                    ))
                    .to_ascii_uppercase();
            }
            "ja_JP" => return self.number_between(101, 110).to_string(),
            _ => {
                return generate!(
                    &self.random,
                    self.address_locale(),
                    faker::address::raw::BuildingNumber,
                );
            }
        };
        self.numerify(Some(formats[self.random.usize(..formats.len())]))
    }

    pub fn secondary_address(&self) -> String {
        let formats: &[&str] = match self.address_locale() {
            "hu_HU" => faker_address::SECONDARY_ADDRESS_FORMATS_HU_HU,
            "it_IT" | "it_CH" => faker_address::SECONDARY_ADDRESS_FORMATS_IT_IT,
            "pt_BR" => faker_address::SECONDARY_ADDRESS_FORMATS_PT_BR,
            "pt_PT" => faker_address::SECONDARY_ADDRESS_FORMATS_PT_PT,
            "es_ES" => faker_address::SECONDARY_ADDRESS_FORMATS_ES_ES,
            "es_AR" => faker_address::SECONDARY_ADDRESS_FORMATS_ES_AR,
            "es_PE" => faker_address::SECONDARY_ADDRESS_FORMATS_ES_PE,
            "es_VE" => faker_address::SECONDARY_ADDRESS_FORMATS_ES_VE,
            "en_GB" => {
                return self.bothify(Some(
                    self.random
                        .sample(faker_address::SECONDARY_ADDRESS_FORMATS_EN_GB),
                ));
            }
            "ja_JP" => {
                let prefix = self.random.sample(&["ハイツ", "コーポ"]);
                return format!("{prefix}{}{}号", self.last_name(), self.building_number());
            }
            "en_AU" => faker_address::SECONDARY_ADDRESS_FORMATS_EN_AU,
            "ar_EG" | "ar_SA" => faker_address::SECONDARY_ADDRESS_FORMATS_AR_EG_SA,
            "ar_JO" => faker_address::SECONDARY_ADDRESS_FORMATS_AR_JO,
            "he_IL" => faker_address::SECONDARY_ADDRESS_FORMATS_HE_IL,
            "fi_FI" => faker_address::SECONDARY_ADDRESS_FORMATS_FI_FI,
            "en_US" | "en_CA" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" => &["Apt. ###", "Suite ###"],
            "fr_CA" => &["Apt. ###", "Suite ###", "Bureau ###"],
            "fr_FR" | "fr_BE" | "fr_CH" => &[
                "Apt. ###",
                "Suite ###",
                "Étage ###",
                "Bât. ###",
                "Chambre ###",
            ],
            "zh_TW" => {
                return format!(
                    "{}{}",
                    self.random.u8(1..=100),
                    if self.boolean(None) { "室" } else { "房" }
                );
            }
            _ => {
                return generate!(
                    &self.random,
                    self.address_locale(),
                    faker::address::raw::SecondaryAddress,
                );
            }
        };
        self.numerify(Some(formats[self.random.usize(..formats.len())]))
    }

    pub fn street_address(&self) -> String {
        match self.address_locale() {
            "hu_HU" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_HU_HU);
            }
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_SR_RS);
            }
            "hy_AM" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_HY_AM);
            }
            "ka_GE" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_KA_GE);
            }
            "lt_LT" | "lv_LV" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_LT_LT);
            }
            "hr_HR" | "sl_SI" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_HR_HR);
            }
            "ro_RO" | "ro_MD" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_RO_RO);
            }
            "el_GR" | "el_CY" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_EL_GR);
            }
            "it_IT" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_IT_IT);
            }
            "it_CH" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_IT_CH);
            }
            "pt_BR" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_PT_BR);
            }
            "pt_PT" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_PT_PT);
            }
            "es_ES" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_ES_ES);
            }
            "es_AR" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_ES_AR);
            }
            "es_PE" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_ES_PE);
            }
            "es_VE" => {
                return self.regional_address_format(faker_address::STREET_ADDRESS_FORMATS_ES_VE);
            }
            "da_DK" => {
                let mut street = format!("{} {}", self.street_name(), self.building_number());
                let layout = self.random.u8(0..3);
                if layout > 0 {
                    write!(street, ", {}", self.danish_building_level())
                        .expect("writing to String cannot fail");
                }
                if layout == 2 {
                    write!(
                        street,
                        " {}",
                        self.random.sample(faker_address::BUILDING_SIDE_DA_DK)
                    )
                    .expect("writing to String cannot fail");
                }
                return street;
            }
            "fi_FI" => {
                let mut street = format!("{} {}", self.street_name(), self.building_number());
                if self.boolean(None) {
                    write!(street, " {}", self.secondary_address())
                        .expect("writing to String cannot fail");
                }
                return street;
            }
            "ru_RU" => {
                return format!(
                    "{} {}, {}",
                    self.random.sample(faker_address::STREET_PREFIX_RU_RU),
                    self.random.sample(faker_address::STREET_RU_RU),
                    self.building_number(),
                );
            }
            "uk_UA" => {
                return format!(
                    "{} {}, {}",
                    self.random.sample(faker_address::STREET_PREFIX_UK_UA),
                    self.street_name(),
                    self.building_number(),
                );
            }
            "kk_KZ" => {
                return format!(
                    "{} {}, {}",
                    self.random.sample(faker_address::STREET_KK_KZ),
                    self.street_suffix(),
                    self.building_number(),
                );
            }
            "fa_IR" => return format!("{} {}", self.street_name(), self.persian_building()),
            "he_IL" => {
                let mut street = format!("{} {} ", self.street_name(), self.building_number());
                if self.boolean(None) {
                    street.push_str(&self.secondary_address());
                }
                return street;
            }
            "bn_BD" => return format!("{} {}", self.bangla_street_number(), self.street_name()),
            "ne_NP" => return self.street_name(),
            "id_ID" => {
                return format!(
                    "{} {} No. {}",
                    self.random.sample(faker_address::STREET_PREFIX_ID_ID),
                    self.street_name(),
                    self.building_number(),
                );
            }
            "ms_MY" => {
                return format!(
                    "{}{}, {}",
                    self.random.sample(faker_address::BUILDING_PREFIX_MS_MY),
                    self.building_number(),
                    self.street_name(),
                );
            }
            _ => {}
        }
        if self.address_locale() == "en_HK" {
            let number = self.building_number();
            return match self.random.u8(0..3) {
                0 => format!("{number} {}", self.street_name()),
                1 => format!(
                    "{number} {}",
                    Self::hong_kong_name(&self.random, faker_address::VILLAGE_FORMATS_EN_HK)
                ),
                _ => format!(
                    "Block {number}, {}",
                    Self::hong_kong_name(&self.random, faker_address::ESTATE_FORMATS_EN_HK)
                ),
            };
        }
        if self.address_locale() == "en_IN" {
            let number = self.building_number();
            let locality = self.indian_locality();
            return if self.boolean(None) {
                format!("{number}, {locality},")
            } else {
                format!("{number}, {}, {locality}", self.indian_society_name())
            };
        }
        if self.address_locale() == "en_SG" {
            let street = self.street_name();
            return if self.boolean(None) {
                format!("Jalan {street}")
            } else {
                format!("{street} {}", self.street_suffix())
            };
        }
        let street = self.street_name();
        match self.address_locale() {
            "ja_JP" => format!(
                "{street}{}{}-{}-{}",
                self.last_name(),
                self.random.i64(faker_address::AREA_NUMBER_JA_JP),
                self.random.i64(faker_address::AREA_NUMBER_JA_JP),
                self.random.i64(faker_address::AREA_NUMBER_JA_JP),
            ),
            "en_GB" if self.random.u8(0..3) == 0 => {
                format!("{}\n{street}", self.secondary_address())
            }
            "en_GB" | "en_NZ" => format!("{} {street}", self.building_number()),
            "en_AU" => {
                let number = self.building_number();
                match self.random.u8(0..3) {
                    0 => format!("{number} {street}"),
                    1 => format!(
                        "{number}{} {street}",
                        self.random.sample(faker_address::BUILDING_LETTERS_EN_AU)
                    ),
                    _ => format!("{} {number} {street}", self.secondary_address()),
                }
            }
            "en_US" | "en_CA" | "fr_CA" | "en_PH" | "en_UG" | "en_ZA" | "ar_EG" | "ar_JO"
            | "ar_SA" => {
                let mut address = format!("{} {street}", self.building_number());
                if self.boolean(None) {
                    write!(address, " {}", self.secondary_address())
                        .expect("writing to String cannot fail");
                }
                address
            }
            "cs_CZ" if self.random.u8(0..5) == 0 => street,
            "cs_CZ" | "de_AT" | "de_CH" | "de_DE" | "fr_BE" | "fr_CH" | "pl_PL" | "sk_SK"
            | "tr_TR" | "nl_NL" | "nl_BE" | "nb_NO" | "sv_SE" | "is_IS" => {
                format!("{street} {}", self.building_number())
            }
            "fr_FR" if self.random.u8(0..6) == 0 => street,
            "fr_FR" => format!("{}, {street}", self.building_number()),
            "zh_TW" => {
                let mut address = street;
                let format = self.random.u8(0..6);
                if format >= 2 {
                    write!(address, "{}巷", self.random.u16(1..=1000))
                        .expect("writing to String cannot fail");
                }
                if format >= 4 {
                    write!(address, "{}弄", self.random.u16(1..=1000))
                        .expect("writing to String cannot fail");
                }
                write!(address, "{}號", self.random.u16(1..=1000))
                    .expect("writing to String cannot fail");
                if format % 2 == 1 {
                    write!(address, "{}樓", self.random.u8(1..=100))
                        .expect("writing to String cannot fail");
                }
                address
            }
            _ => format!("{} {street}", self.building_number()),
        }
    }

    pub fn address(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.regional_address_format(faker_address::ADDRESS_FORMATS_HU_HU),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                self.regional_address_format(faker_address::ADDRESS_FORMATS_SR_RS)
            }
            "hy_AM" => self.regional_address_format(faker_address::ADDRESS_FORMATS_HY_AM),
            "ka_GE" => self.regional_address_format(faker_address::ADDRESS_FORMATS_KA_GE),
            "lt_LT" => self.regional_address_format(faker_address::ADDRESS_FORMATS_LT_LT),
            "lv_LV" => self.regional_address_format(faker_address::ADDRESS_FORMATS_LV_LV),
            "hr_HR" => self.regional_address_format(faker_address::ADDRESS_FORMATS_HR_HR),
            "sl_SI" => self.regional_address_format(faker_address::ADDRESS_FORMATS_SL_SI),
            "ro_RO" => self.regional_address_format(faker_address::ADDRESS_FORMATS_RO_RO),
            "ro_MD" => self.regional_address_format(faker_address::ADDRESS_FORMATS_RO_MD),
            "el_GR" => self.regional_address_format(faker_address::ADDRESS_FORMATS_EL_GR),
            "el_CY" => self.regional_address_format(faker_address::ADDRESS_FORMATS_EL_CY),
            "it_IT" => self.regional_address_format(faker_address::ADDRESS_FORMATS_IT_IT),
            "it_CH" => self.regional_address_format(faker_address::ADDRESS_FORMATS_IT_CH),
            "pt_BR" => self.regional_address_format(faker_address::ADDRESS_FORMATS_PT_BR),
            "pt_PT" => self.regional_address_format(faker_address::ADDRESS_FORMATS_PT_PT),
            "es_ES" => self.regional_address_format(faker_address::ADDRESS_FORMATS_ES_ES),
            "es_AR" => self.regional_address_format(faker_address::ADDRESS_FORMATS_ES_AR),
            "es_PE" => self.regional_address_format(faker_address::ADDRESS_FORMATS_ES_PE),
            "es_VE" => self.regional_address_format(faker_address::ADDRESS_FORMATS_ES_VE),
            "fi_FI" => {
                let mut address = format!(
                    "{}\n{} {}",
                    self.street_address(),
                    self.postcode(),
                    self.city()
                );
                if self.boolean(None) {
                    write!(address, ", {}", self.state()).expect("writing to String cannot fail");
                }
                address
            }
            "nl_BE" => format!(
                "{}\n {} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            "de_AT" | "de_CH" | "de_DE" | "nl_NL" | "nb_NO" | "sv_SE" | "da_DK" | "is_IS" => {
                format!(
                    "{}\n{} {}",
                    self.street_address(),
                    self.postcode(),
                    self.city()
                )
            }
            "pl_PL" => format!(
                "{}, {} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            "cs_CZ" => {
                let street = self.street_address();
                let postcode = self.postcode();
                let city = self.city();
                match self.random.u8(0..7) {
                    0 => {
                        let region = self.random.sample(faker_address::REGION_CS_CZ);
                        format!("{street}\n{region}\n{postcode} {city}")
                    }
                    1 => format!("{street}\n{postcode} {city}\nČeská republika"),
                    _ => format!("{street}\n{postcode} {city}"),
                }
            }
            // FakerPHP uses a single-quoted template here, so its separators are literal.
            "sk_SK" => format!(
                r"{}\n {}\n {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            "ru_RU" => format!(
                "{}, {} область, город {}, {}",
                self.postcode(),
                self.random.sample(faker_address::REGION_RU_RU),
                self.city(),
                self.street_address(),
            ),
            "uk_UA" => format!(
                "{}, {}, {}",
                self.postcode(),
                Self::ukrainian_city_and_region(&self.random),
                self.street_address()
            ),
            "kk_KZ" => format!(
                "{}, {} облысы, {} қаласы, {}",
                self.postcode(),
                self.random.sample(faker_address::REGION_KK_KZ),
                self.city(),
                self.street_address(),
            ),
            "tr_TR" => format!(
                "{}\n{} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            "fa_IR" => {
                let mut address = format!("{} {}", self.city(), self.street_address());
                if self.boolean(None) {
                    write!(
                        address,
                        " {} {}",
                        self.random.sample(faker_address::POSTCODE_PREFIX_FA_IR),
                        self.postcode()
                    )
                    .expect("writing to String cannot fail");
                }
                address
            }
            "he_IL" => format!(
                "{}\n{}, {}",
                self.street_address(),
                self.city(),
                self.postcode()
            ),
            "ar_EG" | "ar_JO" | "ar_SA" => format!("{}\n{}", self.street_address(), self.city()),
            "bn_BD" => format!(
                "{}, {} {}",
                self.street_address(),
                self.city(),
                self.state()
            ),
            "ne_NP" => format!(
                "{}-{}, {}, {} {}",
                self.city(),
                self.nepali_ward_number(),
                self.street_address(),
                self.random.sample(faker_address::DISTRICT_NE_NP),
                self.postcode(),
            ),
            "id_ID" => format!(
                "{}, {} {}, {}",
                self.street_address(),
                self.city(),
                self.postcode(),
                self.state_abbr()
            ),
            "ms_MY" => format!(
                "{}, {}, {}",
                self.street_address(),
                self.malaysian_township(),
                Self::malaysian_town_state(&self.random)
            ),
            "th_TH" => format!(
                "{}\n{}, {}",
                self.street_address(),
                self.city(),
                self.postcode()
            ),
            "vi_VN" => {
                let (street, region) = match self.random.u8(0..3) {
                    0 => (
                        self.street_address(),
                        self.random.sample(faker_address::CITY_VI_VN),
                    ),
                    1 => (
                        self.street_address(),
                        self.random.sample(faker_address::PROVINCE_VI_VN),
                    ),
                    _ => (
                        format!(
                            "{}, {}",
                            self.building_number(),
                            self.vietnamese_locality_name(faker_address::HAMLET_NAME_FORMATS_VI_VN)
                        ),
                        self.random.sample(faker_address::PROVINCE_VI_VN),
                    ),
                };
                let ward = self.vietnamese_locality_name(faker_address::WARD_NAME_FORMATS_VI_VN);
                let district =
                    self.vietnamese_locality_name(faker_address::DISTRICT_NAME_FORMATS_VI_VN);
                format!("{street}, {ward}, {district}\n{region}")
            }
            "ko_KR" => {
                let (region, locality) = if self.boolean(None) {
                    (
                        self.random.sample(faker_address::METROPOLITAN_CITY_KO_KR),
                        faker_address::BOROUGH_KO_KR,
                    )
                } else {
                    (
                        self.random.sample(faker_address::PROVINCE_KO_KR),
                        faker_address::CITY_KO_KR,
                    )
                };
                format!(
                    "{region} {} {} {}",
                    self.random.sample(locality),
                    self.street_name(),
                    self.building_number(),
                )
            }
            "ja_JP" => {
                let mut address = format!(
                    "{}  {}{}{}{}",
                    self.postcode(),
                    self.random.sample(faker_address::PREFECTURE_JA_JP),
                    self.city(),
                    self.random.sample(faker_address::WARD_JA_JP),
                    self.street_address(),
                );
                if self.boolean(None) {
                    write!(address, " {}", self.secondary_address())
                        .expect("writing to String cannot fail");
                }
                address
            }
            "en_HK" => format!(
                "{}\n{}\n{}",
                self.street_address(),
                Self::hong_kong_name(&self.random, faker_address::TOWN_FORMATS_EN_HK),
                self.city()
            ),
            "en_PH" => {
                let street = self.street_address();
                let postcode = self.postcode();
                let province = self.random.sample(faker_address::PROVINCE_EN_PH);
                if self.boolean(None) {
                    let barangay = self.random.sample(faker_address::BARANGAY_EN_PH);
                    format!(
                        "{street}, {barangay}, {} {postcode} {province}",
                        self.city()
                    )
                } else {
                    let municipality = self.random.sample(faker_address::MUNICIPALITY_EN_PH);
                    format!("{street}, {municipality} {postcode} {province}")
                }
            }
            "en_IN" => format!(
                "{} {} - {}",
                self.street_address(),
                self.city(),
                self.postcode()
            ),
            "en_SG" => {
                let street = self.street_address();
                let postcode = self.postcode();
                if self.boolean(None) {
                    let number =
                        self.numerify(Some(self.random.sample(faker_address::STREET_NUMBER_EN_SG)));
                    format!("{number} {street}\nSINGAPORE {postcode}")
                } else {
                    let block =
                        self.numerify(Some(self.random.sample(faker_address::BLOCK_NUMBER_EN_SG)));
                    let floor = self.random.sample(faker_address::FLOOR_NUMBER_EN_SG);
                    let apartment = self.random.sample(faker_address::APARTMENT_NUMBER_EN_SG);
                    format!("{block} {street}\n{floor} {apartment}\nSINGAPORE {postcode}")
                }
            }
            "en_ZA" => format!(
                "{}\n{}, {} {}",
                self.street_address(),
                self.city(),
                self.random.sample(faker_address::PROVINCE_ABBR_EN_ZA),
                self.postcode()
            ),
            "en_GB" => format!(
                "{}\n{}\n{}",
                self.street_address(),
                self.city(),
                self.postcode()
            ),
            "en_CA" => format!(
                "{}\n{}, {}  {}",
                self.street_address(),
                self.city(),
                self.random.sample(faker_address::PROVINCE_ABBR_EN_CA),
                self.postcode()
            ),
            "en_NZ" => format!(
                "{}, {}, {}, {}",
                self.street_address(),
                self.city(),
                self.random.sample(faker_address::REGION_EN_NZ),
                self.postcode()
            ),
            "en_US" | "en_AU" | "en_UG" => format!(
                "{}\n{}, {} {}",
                self.street_address(),
                self.city(),
                self.state_abbr(),
                self.postcode()
            ),
            "zh_CN" => {
                let districts = DISTRICTS_ZH_CN;
                format!(
                    "{}{}",
                    self.city(),
                    districts[self.random.usize(..districts.len())]
                )
            }
            "zh_TW" => format!(
                "{} {}{}",
                self.postcode(),
                self.city(),
                self.street_address()
            ),
            "fr_BE" => format!(
                "{}\n {} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            "fr_CA" => format!(
                "{}, {}, {} {}",
                self.street_address(),
                self.city(),
                self.state_abbr(),
                self.postcode()
            ),
            "fr_FR" | "fr_CH" => format!(
                "{}\n{} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
            _ => format!(
                "{} {} {}",
                self.street_address(),
                self.postcode(),
                self.city()
            ),
        }
    }

    pub fn name(&self) -> String {
        self.name_with_gender(None)
    }

    pub fn first_name(&self) -> String {
        self.first_name_with_gender(None)
    }

    pub fn last_name(&self) -> String {
        if self.provider_locale() == "nl_NL" {
            return company::dutch::dutch_surname(&self.random).to_owned();
        }
        match self.provider_locale() {
            "ru_RU" => return self.russian_surname(self.random.bool()),
            "is_IS" => return company::nordic::icelandic_surname(&self.random),
            // The pinned Lithuanian no-argument surname template samples given names.
            "lt_LT" => return self.first_name_with_gender(None),
            "ms_MY" => return south_southeast_asian::malaysian_last_name(&self.random).to_owned(),
            "pl_PL" | "cs_CZ" | "sk_SK" | "el_GR" | "bg_BG" | "id_ID" => {
                return self
                    .gendered_surname(self.random.bool())
                    .expect("fixed gendered surname locale");
            }
            _ => {}
        }
        let surnames =
            faker_person::surnames(faker_person::provider_locale(self.provider_locale()))
                .expect("fixed Faker surname catalog");
        surnames[self.random.usize(..surnames.len())].to_owned()
    }

    pub fn middle_name(&self) -> DatabaseResult<&'static str> {
        if matches!(self.provider_locale(), "ru_RU" | "uk_UA") {
            return self.gendered_middle_name(self.random.bool());
        }
        let names = match self.provider_locale() {
            "da_DK" => company::nordic::DA_DK_MIDDLE,
            "is_IS" => company::nordic::IS_IS_MIDDLE,
            // The pinned Vietnamese no-argument middle-name template samples given names.
            "vi_VN" => {
                let catalog = faker_person::catalog(self.provider_locale());
                if self.random.bool() {
                    catalog.male
                } else {
                    catalog.female
                }
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker middle name locale",
                ));
            }
        };
        Ok(names[self.random.usize(..names.len())])
    }

    pub fn middle_name_with_gender(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        match gender {
            Some("male") => self.middle_name_male(),
            Some("female") => self.middle_name_female(),
            _ => self.middle_name(),
        }
    }

    pub fn middle_name_male(&self) -> DatabaseResult<&'static str> {
        self.gendered_middle_name(true)
    }

    pub fn middle_name_female(&self) -> DatabaseResult<&'static str> {
        self.gendered_middle_name(false)
    }

    fn gendered_middle_name(&self, male: bool) -> DatabaseResult<&'static str> {
        let names = match (self.provider_locale(), male) {
            ("ru_RU", true) => faker_person::RU_RU_MIDDLE_MALE,
            ("ru_RU", false) => faker_person::RU_RU_MIDDLE_FEMALE,
            ("uk_UA", true) => faker_person::UK_UA_MIDDLE_MALE,
            ("uk_UA", false) => faker_person::UK_UA_MIDDLE_FEMALE,
            ("vi_VN", true) => south_southeast_asian::VI_VN_MIDDLE_MALE,
            ("vi_VN", false) => south_southeast_asian::VI_VN_MIDDLE_FEMALE,
            ("en_IN", true) => indian_nepali::EN_IN_MIDDLE_MALE,
            ("ne_NP", true) => indian_nepali::NE_NP_MIDDLE_MALE,
            ("ne_NP", false) => indian_nepali::NE_NP_MIDDLE_FEMALE,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker gendered middle name locale",
                ));
            }
        };
        Ok(names[self.random.usize(..names.len())])
    }

    fn russian_surname(&self, male: bool) -> String {
        let names = faker_person::RU_RU_SURNAMES;
        let name = names[self.random.usize(..names.len())];
        if male {
            name.to_owned()
        } else {
            format!("{name}а")
        }
    }

    pub fn last_name_with_gender(&self, gender: Option<&str>) -> String {
        if matches!(
            self.provider_locale(),
            "lt_LT" | "pl_PL" | "cs_CZ" | "sk_SK" | "el_GR" | "bg_BG" | "ru_RU" | "id_ID" | "en_UG"
        ) {
            match gender {
                Some("male") => return self.last_name_male().expect("fixed male surname locale"),
                Some("female") => {
                    return self
                        .last_name_female()
                        .expect("fixed female surname locale");
                }
                _ => {}
            }
        }
        self.last_name()
    }

    pub fn last_name_male(&self) -> DatabaseResult<String> {
        self.gendered_surname(true)
    }

    pub fn last_name_female(&self) -> DatabaseResult<String> {
        self.gendered_surname(false)
    }

    fn gendered_surname(&self, male: bool) -> DatabaseResult<String> {
        let names = match (self.provider_locale(), male) {
            ("ru_RU", _) => return Ok(self.russian_surname(male)),
            ("lt_LT", true) => faker_person::LT_LT_LAST_MALE,
            ("lt_LT", false) => faker_person::LT_LT_LAST_FEMALE,
            ("cs_CZ", true) => company::CS_CZ_MALE_SURNAMES,
            ("cs_CZ", false) => company::CS_CZ_FEMALE_SURNAMES,
            ("sk_SK", true) => company::central::SK_SK_MALE_SURNAMES,
            ("sk_SK", false) => company::central::SK_SK_FEMALE_SURNAMES,
            ("el_GR", true) => company::balkan::GREEK_MALE_NAMES,
            ("el_GR", false) => company::balkan::GREEK_FEMALE_NAMES,
            ("bg_BG", true) => faker_person::BG_MALE_SURNAMES,
            ("bg_BG", false) => faker_person::BG_FEMALE_SURNAMES,
            ("sl_SI", _) => company::lt_sl_tr::SL_SI_SURNAMES,
            ("pl_PL", true) => faker_person::PL_PL_LAST_MALE,
            ("pl_PL", false) => faker_person::PL_PL_LAST_FEMALE,
            ("id_ID", true) => company::south_southeast_asian::ID_MALE_SURNAMES,
            ("id_ID", false) => company::south_southeast_asian::ID_FEMALE_SURNAMES,
            ("en_UG", true) => nigerian_ugandan::EN_UG_SURNAMES_MALE,
            ("en_UG", false) => nigerian_ugandan::EN_UG_SURNAMES_FEMALE,
            ("is_IS", _) => {
                let ending = if male { "son" } else { "dóttir" };
                return Ok(format!("{}{ending}", self.last_name()));
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker gendered surname locale",
                ));
            }
        };
        Ok(names[self.random.usize(..names.len())].to_owned())
    }

    pub fn first_name_male_ne(&self) -> DatabaseResult<&'static str> {
        self.hungarian_married_name(faker_person::HU_MARRIED_FIRST_NAMES)
    }

    pub fn last_name_female_married(&self) -> DatabaseResult<&'static str> {
        self.hungarian_married_name(faker_person::HU_MARRIED_SURNAMES)
    }

    fn hungarian_married_name(
        &self,
        names: &'static [&'static str],
    ) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "hu_HU" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker married name locale",
            ));
        }
        Ok(names[self.random.usize(..names.len())])
    }

    pub fn first_name_male(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.provider_locale()).male)
    }

    pub fn first_name_female(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.provider_locale()).female)
    }

    fn require_malaysian_person(&self) -> DatabaseResult<()> {
        if self.provider_locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Malaysian Person locale",
            ));
        }
        Ok(())
    }

    pub fn my_kad_number(
        &self,
        gender: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        self.require_malaysian_person()?;
        let separator = if formatted {
            separator.unwrap_or("-")
        } else {
            ""
        };
        let year = self.number_between(0, 99);
        // The upstream month and day are sampled separately.
        let month = self.date_time_in_timezone(None, None).month();
        let day = self.date_time_in_timezone(None, None).day();
        let mut place = self.number_between(1, 55);
        if place >= 17 {
            place += 4;
        }
        let serial = self.number_between(0, 999);
        let digit = self.number_between(0, 9);
        let digit = match gender {
            Some("male") => digit | 1,
            Some("female") => digit & !1,
            _ => digit,
        };
        Ok(format!(
            "{year:02}{month:02}{day:02}{separator}{place:02}{separator}{serial:03}{digit}"
        ))
    }

    pub fn cnp(
        &self,
        gender: Option<&str>,
        date_of_birth: Option<&str>,
        county: Option<&str>,
        is_resident: bool,
    ) -> DatabaseResult<String> {
        if self.provider_locale() != "ro_RO" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker CNP locale"));
        }
        romanian_serbian::cnp(&self.random, gender, date_of_birth, county, is_resident)
    }

    pub fn first_name_with_gender(&self, gender: Option<&str>) -> String {
        match gender {
            Some("male") => self.first_name_male(),
            Some("female") => self.first_name_female(),
            _ if self.provider_locale() == "ms_MY" => {
                south_southeast_asian::malaysian_first_name(&self.random)
            }
            _ if self.boolean(None) => self.first_name_male(),
            _ => self.first_name_female(),
        }
    }

    pub fn name_with_gender(&self, gender: Option<&str>) -> String {
        let catalog = faker_person::catalog(self.provider_locale());
        let formats = match gender {
            Some("male") => catalog.male_formats,
            Some("female") => catalog.female_formats,
            _ => {
                let index = self
                    .random
                    .usize(..catalog.male_formats.len() + catalog.female_formats.len());
                if let Some(format) = catalog.male_formats.get(index) {
                    return self.render_person_name(format, catalog);
                }
                return self.render_person_name(
                    catalog.female_formats[index - catalog.male_formats.len()],
                    catalog,
                );
            }
        };
        let format = formats[self.random.usize(..formats.len())];
        self.render_person_name(format, catalog)
    }

    /// Concatenates independent draws from a Taiwanese name pool.
    pub fn random_name(&self, pool: &[&str], count: usize) -> DatabaseResult<String> {
        if self.provider_locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker random name locale",
            ));
        }
        let Some(max_bytes) = pool.iter().map(|name| name.len()).max() else {
            return Ok(String::new());
        };
        let capacity_error =
            || DatabaseError::InvalidQuery("Faker name length exceeds allocation capacity");
        let capacity = count
            .checked_mul(max_bytes.max(1))
            .ok_or_else(capacity_error)?;
        let mut name = String::new();
        name.try_reserve_exact(capacity)
            .map_err(|_| capacity_error())?;
        for _ in 0..count {
            name.push_str(self.random.sample(pool));
        }
        Ok(name)
    }

    fn sample_given_name(&self, names: &[&str]) -> String {
        if self.provider_locale() == "zh_TW" {
            return self
                .random_name(names, self.random.usize(1..=2))
                .expect("fixed random name locale");
        }
        names[self.random.usize(..names.len())].to_owned()
    }

    fn render_person_name(&self, mut format: &str, catalog: &faker_person::PersonNames) -> String {
        let mut name = String::new();
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, remaining) = token.split_once("}}").expect("fixed Faker name template");
            name.push_str(literal);
            match token {
                "firstNameMale" => name.push_str(&self.first_name_male()),
                "firstNameFemale" => name.push_str(&self.first_name_female()),
                "middleNameFemale" => name.push_str(
                    self.middle_name_female()
                        .expect("fixed gendered middle-name locale"),
                ),
                "middleNameMale" => name.push_str(
                    self.middle_name_male()
                        .expect("fixed gendered middle-name locale"),
                ),
                "middleName" => {
                    name.push_str(self.middle_name().expect("fixed middle-name locale"))
                }
                "lastNameMale" => {
                    name.push_str(&self.last_name_male().expect("fixed male surname locale"))
                }
                "lastNameFemale" => name.push_str(
                    &self
                        .last_name_female()
                        .expect("fixed female surname locale"),
                ),
                "firstNameMaleNe" => name.push_str(
                    self.first_name_male_ne()
                        .expect("fixed married name locale"),
                ),
                "lastNameFemaleMarried" => name.push_str(
                    self.last_name_female_married()
                        .expect("fixed married name locale"),
                ),
                "lastName" => name.push_str(&self.last_name()),
                "titleMale" => name.push_str(self.title_male()),
                "titleFemale" => name.push_str(self.title_female()),
                "title" => name.push_str(self.title(None)),
                "namePrefix" => name.push(self.name_prefix().expect("fixed name-prefix locale")),
                "prefix" => {
                    name.push_str(catalog.prefixes[self.random.usize(..catalog.prefixes.len())])
                }
                "suffix" => {
                    name.push_str(catalog.suffixes[self.random.usize(..catalog.suffixes.len())])
                }
                _ if self.provider_locale() == "ms_MY" => {
                    name.push_str(&south_southeast_asian::malaysian_name_part(
                        &self.random,
                        token,
                    ));
                }
                _ => unreachable!("fixed Faker name template token"),
            }
            format = remaining;
        }
        name.push_str(format);
        name
    }

    /// Samples a Japanese full name in katakana.
    pub fn kana_name(&self, gender: Option<&str>) -> DatabaseResult<String> {
        Ok(format!(
            "{} {}",
            self.last_kana_name()?,
            self.first_kana_name(gender)?
        ))
    }

    pub fn first_kana_name(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        match gender {
            Some("male") => self.first_kana_name_male(),
            Some("female") => self.first_kana_name_female(),
            _ if self.random.bool() => self.first_kana_name_male(),
            _ => self.first_kana_name_female(),
        }
    }

    pub fn first_kana_name_male(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_MALE)
    }

    pub fn first_kana_name_female(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_FEMALE)
    }

    pub fn last_kana_name(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_SURNAMES)
    }

    fn sample_kana(&self, names: &'static [&'static str]) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "ja_JP" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker kana name locale",
            ));
        }
        Ok(names[self.random.usize(..names.len())])
    }

    /// Samples a name prefix from the selected Person provider.
    pub fn prefix(&self) -> DatabaseResult<&'static str> {
        let catalog = match self.provider_locale() {
            "ar_EG" | "ar_JO" | "ar_SA" | "fr_CH" | "fr_FR" | "nl_NL" => {
                faker_person::catalog(self.provider_locale()).prefixes
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker name prefix locale",
                ));
            }
        };
        Ok(catalog[self.random.usize(..catalog.len())])
    }

    /// Samples a name suffix, including the upstream null and empty-string values.
    pub fn suffix(&self) -> DatabaseResult<Option<&'static str>> {
        let locale = faker_person::provider_locale(self.provider_locale());
        let catalog = match locale {
            "it_CH" | "it_IT" => return Ok(None),
            "hy_AM" | "zh_TW" => return Ok(Some("")),
            "de_AT" | "de_CH" | "de_DE" | "en_US" | "es_AR" | "es_ES" | "es_PE" | "es_VE"
            | "hu_HU" | "id_ID" | "nl_NL" | "pt_BR" | "sk_SK" | "th_TH" => {
                faker_person::catalog(locale).suffixes
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker name suffix locale",
                ));
            }
        };
        Ok(Some(catalog[self.random.usize(..catalog.len())]))
    }

    pub fn title(&self, gender: Option<&str>) -> &'static str {
        if self.provider_locale() == "sl_SI" {
            let titles = faker_person::SLOVENIAN_TITLES;
            return titles[self.random.usize(..titles.len())];
        }
        match gender {
            Some("male") => self.title_male(),
            Some("female") => self.title_female(),
            _ if self.boolean(None) => self.title_male(),
            _ => self.title_female(),
        }
    }

    pub fn title_male(&self) -> &'static str {
        let titles: &[&str] = match self.provider_locale() {
            "en_ZA" => &["Mr.", "Dr.", "Prof.", "Rev.", "Hon."],
            "fr_BE" => &["M.", "Dr.", "Pr.", "Me.", "Mgr"],
            "fr_CH" => &["M.", "Dr.", "Pr.", "Me."],
            "it_CH" => &["Sig.", "Dott.", "Dr.", "Ing."],
            "es_AR" | "es_PE" => &["Sr.", "Dn.", "Dr.", "Lic.", "Ing."],
            "es_ES" => &["Sr.", "D.", "Dr.", "Lic.", "Ing."],
            "es_VE" => &["Sr.", "Dn.", "Dr.", "Lcdo.", "Ing."],
            "de_AT" => &[
                "Herr",
                "Dr.",
                "Mag.",
                "Ing.",
                "Dipl.-Ing.",
                "Prof.",
                "Univ.Prof.",
            ],
            "de_CH" => &["Herr", "Herr Dr.", "Herr Prof.", "Herr Prof. Dr."],
            "cs_CZ" => &[
                "Bc.", "Ing.", "MUDr.", "MVDr.", "Mgr.", "JUDr.", "PhDr.", "RNDr.", "doc.", "Dr.",
            ],
            "sk_SK" => &[
                "Bc.",
                "Ing.",
                "MUDr.",
                "MVDr.",
                "Mgr.",
                "JUDr.",
                "PhDr.",
                "RNDr.",
                "doc.",
                "Dr.",
                "BcA.",
                "ICDr.",
                "Ing.",
                "Ing. arch.",
                "JUDr.",
                "Mgr. art.",
                "MSDr.",
                "PaedDr.",
                "PharmDr.",
                "PhDr.",
                "PhMr.",
                "RNDr.",
                "RSDr.",
                "ThDr.",
                "ThLic.",
                "prof.",
                "Dr. h. c.",
            ],
            "hu_HU" => &["Dr.", "Prof.", "Id.", "Ifj.", "Báró", "Gróf"],
            "el_CY" | "el_GR" => &["κος.", "κ."],
            "bg_BG" => &["Г-н", "Др."],
            "he_IL" => &["מר.", "ד\"ר", "פרופ"],
            "hy_AM" => &["Պարոն"],
            "ka_GE" => &["ბნ.", "დოქ.", "პროფ."],
            "lt_LT" => &["p.", "ponas"],
            "ro_RO" | "ro_MD" => &["dl.", "ing.", "dr."],
            "bn_BD" => &["মি."],
            "id_ID" => &["dr.", "drg.", "Dr.", "Drs.", "Ir.", "H."],
            "vi_VN" => &["Cụ", "Ông", "Bác", "Chú", "Anh", "Em"],
            "ms_MY" => &[
                "En.",
                "Dr.",
                "Prof.",
                "Datuk",
                "Dato'",
                "Datuk Seri",
                "Dato' Sri",
                "Tan Sri",
                "Tun",
            ],
            "pl_PL" => &["mgr", "inż.", "dr", "doc."],
            "fi_FI" => &["Hra.", "Tri."],
            "nl_NL" => faker_person::NL_NL_TITLES,
            "tr_TR" => faker_person::TR_TR_TITLES,
            "ar_EG" | "ar_JO" | "ar_SA" => &["السيد", "الأستاذ", "الدكتور", "المهندس"],
            "de_DE" => &["Herr", "Herr Dr.", "Herr Prof.", "Herr Prof. Dr."],
            "fa_IR" => &["آقای", "استاد", "دکتر", "مهندس"],
            "fr_FR" => &["M.", "Dr.", "Pr.", "Me."],
            "it_IT" => &["Sig.", "Dott.", "Dr.", "Ing."],
            "pt_BR" => &["Sr.", "Dr."],
            "pt_PT" => &["Sr.", "Dr.", "Exmo.", "Eng.", "Eng.º", "Ex.", "Exº"],
            "zh_CN" => &["先生", "医生", "老师", "教授", "律师", "经理"],
            "zh_TW" => &["先生", "博士", "教授"],
            _ => &["Mr.", "Dr.", "Prof."],
        };
        titles[self.random.usize(..titles.len())]
    }

    pub fn title_female(&self) -> &'static str {
        let titles: &[&str] = match self.provider_locale() {
            "en_ZA" => &["Mrs.", "Ms.", "Miss", "Dr.", "Prof.", "Rev.", "Hon."],
            "fr_BE" => &["Mme.", "Mlle", "Dr.", "Pr.", "Me."],
            "fr_CH" => &["Mme.", "Mlle", "Dr.", "Pr.", "Me."],
            "it_CH" => &["Sig.ra", "Dott.", "Dr.", "Ing."],
            "es_AR" => &["Sra.", "Srita.", "Dr.", "Lic.", "Ing."],
            "es_ES" => &["Sra.", "Srta.", "Dña", "Dr.", "Lic.", "Ing."],
            "es_PE" => &["Sra.", "Srta.", "Dra.", "Lic.", "Ing."],
            "es_VE" => &["Sra.", "Srita.", "Dra.", "Lcda.", "Ing."],
            "de_AT" => &[
                "Frau",
                "Dr.",
                "Maga.",
                "Ing.",
                "Dipl.-Ing.",
                "Prof.",
                "Univ.Prof.",
            ],
            "de_CH" => &["Frau", "Frau Dr.", "Frau Prof.", "Frau Prof. Dr."],
            "cs_CZ" => &[
                "Bc.", "Ing.", "MUDr.", "MVDr.", "Mgr.", "JUDr.", "PhDr.", "RNDr.", "doc.", "Dr.",
            ],
            "sk_SK" => &[
                "Bc.",
                "Ing.",
                "MUDr.",
                "MVDr.",
                "Mgr.",
                "JUDr.",
                "PhDr.",
                "RNDr.",
                "doc.",
                "Dr.",
                "BcA.",
                "ICDr.",
                "Ing.",
                "Ing. arch.",
                "JUDr.",
                "Mgr. art.",
                "MSDr.",
                "PaedDr.",
                "PharmDr.",
                "PhDr.",
                "PhMr.",
                "RNDr.",
                "RSDr.",
                "ThDr.",
                "ThLic.",
                "prof.",
                "Dr. h. c.",
            ],
            "hu_HU" => &["Özv.", "Dr.", "Prof."],
            "el_CY" | "el_GR" => &["δις.", "δνις.", "κα."],
            "bg_BG" => &["Г-жа", "Г-ца", "Др."],
            "he_IL" => &["גב.", "ד\"ר", "פרופ"],
            "hy_AM" => &["տիկին", "օրիորդ"],
            "ka_GE" => &["ქნ.", "დოქ.", "პროფ."],
            "lt_LT" => &["p.", "ponia", "panelė"],
            "ro_RO" | "ro_MD" => &["d-na.", "d-șoara", "ing.", "dr."],
            "bn_BD" => &["মিসেস.", "মিস."],
            "id_ID" => &["dr.", "drg.", "Dr.", "Hj."],
            "vi_VN" => &["Cụ", "Bà", "Bác", "Cô", "Chị", "Em"],
            "ms_MY" => &[
                "Pn.",
                "Cik",
                "Dr.",
                "Prof.",
                "Datin",
                "Datin Paduka",
                "Datin Paduka Seri",
                "Puan Sri",
                "Toh Puan",
            ],
            "pl_PL" => &["mgr", "inż.", "dr", "doc."],
            "fi_FI" => &["Rva.", "Nti.", "Tri."],
            "nl_NL" => faker_person::NL_NL_TITLES,
            "tr_TR" => faker_person::TR_TR_TITLES,
            "ar_EG" | "ar_JO" | "ar_SA" => &["السيدة", "الآنسة", "الدكتورة", "المهندسة"],
            "de_DE" => &["Frau", "Frau Dr.", "Frau Prof.", "Frau Prof. Dr."],
            "fa_IR" => &["خانم", "استاد", "دکتر", "مهندس"],
            "fr_FR" => &["Mme.", "Mlle", "Dr.", "Pr.", "Me."],
            "it_IT" => &["Sig.ra", "Dott.", "Dr.", "Ing."],
            "pt_BR" => &["Sra.", "Srta.", "Dr."],
            "pt_PT" => &["Sra.", "Dra.", "Exma", "Eng.ª", "Exª"],
            "zh_CN" => &[
                "女士", "小姐", "夫人", "太太", "医生", "老师", "教授", "律师", "经理",
            ],
            "zh_TW" => &["小姐", "太太", "博士", "教授"],
            _ => &["Mrs.", "Ms.", "Miss", "Dr.", "Prof."],
        };
        titles[self.random.usize(..titles.len())]
    }

    pub fn first_name_ascii(&self) -> DatabaseResult<&'static str> {
        username::ascii_given_name(&self.random, self.provider_locale()).ok_or(
            DatabaseError::InvalidQuery("unsupported Faker ASCII name locale"),
        )
    }

    pub fn last_name_ascii(&self) -> DatabaseResult<&'static str> {
        username::ascii_surname(&self.random, self.provider_locale()).ok_or(
            DatabaseError::InvalidQuery("unsupported Faker ASCII name locale"),
        )
    }

    pub fn user_name(&self) -> String {
        let candidate = username::candidate(&self.random, self);
        let candidate = internet_ascii(&candidate).replace("..", ".");
        if candidate.trim_matches(['.', '_']).is_empty() {
            return self.random_letter().to_string();
        }
        candidate.trim_end_matches('.').to_owned()
    }

    pub fn safe_email_domain(&self) -> &'static str {
        ["example.com", "example.org", "example.net"][self.random.usize(0..3)]
    }

    pub fn free_email_domain(&self) -> &'static str {
        let domains = domains::free_email_domains(self.provider_locale());
        domains[self.random.usize(..domains.len())]
    }

    pub fn safe_email(&self) -> String {
        format!("{}@{}", self.user_name(), self.safe_email_domain())
    }

    pub fn free_email(&self) -> String {
        format!("{}@{}", self.user_name(), self.free_email_domain())
    }

    pub fn company_email(&self) -> DatabaseResult<String> {
        Ok(format!("{}@{}", self.user_name(), self.domain_name()?))
    }

    pub fn email(&self) -> DatabaseResult<String> {
        match self.provider_locale() {
            "zh_CN" => Ok(self.free_email()),
            "ne_NP" => match self.random.u8(0..6) {
                0..=1 => self.company_email(),
                2 => Ok(self.free_email()),
                _ => Ok(format!("{}.np", self.company_email()?)),
            },
            _ if self.boolean(None) => Ok(self.free_email()),
            _ => self.company_email(),
        }
    }

    pub fn password(
        &self,
        min_length: Option<usize>,
        max_length: Option<usize>,
    ) -> DatabaseResult<String> {
        let min = min_length.unwrap_or(6);
        let max = max_length.unwrap_or(20);
        let length = self.random.usize(min.min(max)..=min.max(max));
        let mut result = String::new();
        result.try_reserve_exact(length).map_err(|_| {
            DatabaseError::InvalidQuery("Faker password length exceeds string capacity")
        })?;
        result.extend((0..length).map(|_| self.random_ascii()));
        Ok(result)
    }

    pub(super) fn de_phone_area_code(&self) -> String {
        phone_area_code(&self.random, &DE_AREA_CODES, faker_phone::AREA_DE_DE)
    }

    pub(super) fn us_phone_area_code(&self) -> String {
        phone_area_code(&self.random, &US_AREA_CODES, faker_phone::AREA_EN_US)
    }

    pub(super) fn us_phone_exchange_code(&self) -> String {
        let first = self.random.u8(2..=9);
        let middle = self.random_digit();
        let last = if middle == 1 {
            self.random_digit_not(1)
        } else {
            self.random_digit()
        };
        format!("{first}{middle}{last}")
    }

    pub fn e164_phone_number(&self) -> String {
        match self.provider_locale() {
            locale if faker_phone::uses_us_phone_provider(locale) => format!(
                "+1{}{}{}",
                self.us_phone_area_code(),
                self.us_phone_exchange_code(),
                self.numerify(Some("####"))
            ),
            "de_DE" => {
                let area = self.de_phone_area_code();
                format!("+49{area}{}", self.numerify(Some("#######")))
            }
            locale => {
                let formats = match locale {
                    "de_AT" => faker_phone::INTERNATIONAL_DE_AT,
                    "de_CH" => faker_phone::INTERNATIONAL_DE_CH,
                    "en_GB" => faker_phone::INTERNATIONAL_EN_GB,
                    "fr_FR" => faker_phone::INTERNATIONAL_FR_FR,
                    "tr_TR" => faker_phone::INTERNATIONAL_TR_TR,
                    "uk_UA" => faker_phone::INTERNATIONAL_UK_UA,
                    _ => faker_phone::INTERNATIONAL,
                };
                self.numerify(Some(formats[self.random.usize(..formats.len())]))
            }
        }
    }

    pub fn ean13(&self) -> String {
        ean_with_checksum(self.numerify(Some("############")))
    }

    pub fn ean8(&self) -> String {
        ean_with_checksum(self.numerify(Some("#######")))
    }

    pub fn isbn10(&self) -> String {
        let mut code = self.numerify(Some("#########"));
        let sum: u32 = code
            .bytes()
            .enumerate()
            .map(|(index, digit)| (10 - index as u32) * u32::from(digit - b'0'))
            .sum();
        let check = ((11 - sum % 11) % 11) as u8;
        code.push(if check == 10 {
            'X'
        } else {
            char::from(b'0' + check)
        });
        code
    }

    pub fn isbn13(&self) -> String {
        let prefix = if self.random.bool() { "978" } else { "979" };
        let mut code = String::with_capacity(13);
        code.push_str(prefix);
        code.push_str(&self.numerify(Some("#########")));
        ean_with_checksum(code)
    }

    pub fn credit_card_type(&self) -> &'static str {
        let vendors = if self.provider_locale() == "pt_BR" {
            CARD_VENDORS_PT_BR
        } else {
            CARD_VENDORS
        };
        vendors[self.random.usize(..vendors.len())]
    }

    pub fn credit_card_number(
        &self,
        card_type: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        let vendor = card_type.unwrap_or_else(|| self.credit_card_type());
        let masks = if self.provider_locale() == "pt_BR" {
            card_masks_pt_br(vendor)
        } else {
            card_masks(vendor)
        }
        .ok_or(DatabaseError::InvalidQuery(
            "unknown Faker credit card type",
        ))?;
        let mut number = self.numerify(Some(masks[self.random.usize(..masks.len())]));
        number.push(luhn_check_digit(&number));
        if formatted {
            let separator = separator.unwrap_or("-");
            Ok([&number[..4], &number[4..8], &number[8..12], &number[12..]].join(separator))
        } else {
            Ok(number)
        }
    }

    pub fn credit_card_expiration_date(&self, valid: bool) -> DatabaseResult<Zoned> {
        let start = if valid { "now" } else { "-36 months" };
        let zone = self
            .get_default_timezone()
            .unwrap_or_else(|| self.application_timezone.clone());
        self.date_time_between_expressions(start, "+36 months")
            .map(|date| date.to_zoned(zone.clone()))
    }

    pub fn credit_card_expiration_date_string(
        &self,
        valid: bool,
        expiration_date_format: Option<&str>,
    ) -> DatabaseResult<String> {
        let date = self.credit_card_expiration_date(valid)?;
        jiff::fmt::strtime::format(expiration_date_format.unwrap_or("%m/%y"), &date)
            .map_err(|_| DatabaseError::InvalidQuery("Faker 日期格式无效"))
    }

    pub fn credit_card_details(&self, valid: bool) -> DatabaseResult<CreditCardDetails> {
        let card_type = self.credit_card_type();
        Ok(CreditCardDetails {
            r#type: card_type,
            number: self.credit_card_number(Some(card_type), false, None)?,
            name: self.name(),
            expiration_date: self.credit_card_expiration_date_string(valid, None)?,
        })
    }

    fn institution_sample(
        &self,
        locale: &str,
        catalog: &'static [&'static str],
    ) -> DatabaseResult<&'static str> {
        if self.provider_locale() != locale {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker institution locale",
            ));
        }
        Ok(catalog[self.random.usize(..catalog.len())])
    }

    /// Selects a bank from the locale's catalog.
    pub fn bank(&self) -> DatabaseResult<&'static str> {
        if self.provider_locale() == "ms_MY" {
            return [Self::local_bank, Self::foreign_bank, Self::government_bank]
                [self.random.usize(..3)](self);
        }
        let catalog = faker_institutions::banks(self.provider_locale())
            .ok_or(DatabaseError::InvalidQuery("unsupported Faker bank locale"))?;
        Ok(catalog[self.random.usize(..catalog.len())])
    }

    pub fn micro_finance(&self) -> DatabaseResult<String> {
        let name = self.institution_sample("ne_NP", faker_institutions::NE_NP_MICRO_FINANCES)?;
        Ok(format!("{name} Laghubitta Bittiya Sanstha"))
    }

    /// Generates a SWIFT code using the Malaysian or Nepali provider.
    pub fn swift_code(&self) -> DatabaseResult<String> {
        match self.provider_locale() {
            "ms_MY" => {
                let pattern =
                    self.institution_sample("ms_MY", faker_institutions::MS_MY_SWIFT_CODES)?;
                Ok(self.lexify(Some(pattern)).to_ascii_uppercase())
            }
            "ne_NP" => self
                .institution_sample("ne_NP", faker_institutions::NE_NP_SWIFT_CODES)
                .map(str::to_owned),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker SWIFT locale",
            )),
        }
    }

    /// Generates a VAT sample using the configured locale's provider.
    pub fn vat(&self, spaced_national_prefix: Option<bool>) -> DatabaseResult<String> {
        let separator = if spaced_national_prefix.unwrap_or(true) {
            " "
        } else {
            ""
        };
        let digits = |count| self.random_number(Some(count), true);
        match self.provider_locale() {
            "zh_TW" => Ok(self.taiwan_vat()?.to_string()),
            "bg_BG" => Ok(format!(
                "BG{separator}{}{}",
                digits(5)?,
                digits(self.random.u32(4..=5))?
            )),
            "at_AT" | "de_AT" => Ok(format!("AT{separator}U{}", digits(8)?)),
            "es_ES" => {
                const PREFIXES: &[u8] = b"ABCDEFGHJNPQRSUVW";
                let prefix = char::from(PREFIXES[self.random.usize(..PREFIXES.len())]);
                Ok(format!("{prefix}{}", self.numerify(Some("########"))))
            }
            "fr_BE" => Ok(format!("BE{separator}0{}", digits(9)?)),
            "nl_BE" => {
                let body = digits(7)?;
                Ok(format!("BE{separator}0{body}{:02}", 97 - body % 97))
            }
            "fr_FR" => {
                let siren = self.siren(false)?;
                let number = siren.parse::<u32>().expect("SIREN contains nine digits");
                let key = (12 + 3 * (number % 97)) % 97;
                Ok(format!(
                    "FR{separator}{key:02}{separator}{}{separator}{}{separator}{}",
                    &siren[..3],
                    &siren[3..6],
                    &siren[6..]
                ))
            }
            "en_GB" => self.vat_with_type(None),
            "it_IT" => {
                let mut number = format!(
                    "{}{:03}",
                    self.numerify(Some("#######")),
                    self.number_between(1, 121)
                );
                number.push(luhn_check_digit(&number));
                Ok(format!("IT{number}"))
            }
            "nl_NL" => Ok(format!("NL{}B{}", digits(9)?, digits(2)?)),
            _ => Err(DatabaseError::InvalidQuery("unsupported Faker VAT locale")),
        }
    }

    /// Selects the UK provider's standard, branch, government or health VAT sample.
    pub fn vat_with_type(&self, vat_type: Option<&str>) -> DatabaseResult<String> {
        if self.provider_locale() != "en_GB" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker VAT type locale",
            ));
        }
        match vat_type {
            Some("gov") => Ok(format!("GBGD{:03}", self.number_between(0, 499))),
            Some("health") => Ok(format!("GBHA{}", self.number_between(500, 999))),
            _ => {
                let first = self.number_between(100, 999);
                let second = self.random_number(Some(4), true)?;
                let key = Self::calculate_modulus97(&format!("{first}{second}"), true)?;
                let mut vat = format!("GB{first} {second} {key}");
                if vat_type == Some("branch") {
                    write!(vat, " {}", self.random_number(Some(3), true)?)
                        .expect("writing to String cannot fail");
                }
                Ok(vat)
            }
        }
    }

    /// Computes the UK provider's modulus 97 or 9755 check digits.
    pub fn calculate_modulus97(input: &str, use9755: bool) -> DatabaseResult<u8> {
        if input.len() != 7 || !input.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err(DatabaseError::InvalidQuery(
                "invalid Faker VAT modulus input",
            ));
        }
        let weighted: u16 = input
            .bytes()
            .zip((2..=8).rev())
            .map(|(digit, factor)| u16::from(digit - b'0') * factor)
            .sum();
        let total = weighted + if use9755 { 55 } else { 0 };
        Ok(((97 - total % 97) % 97) as u8)
    }

    pub fn siren(&self, formatted: bool) -> DatabaseResult<String> {
        if self.provider_locale() != "fr_FR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker SIREN locale",
            ));
        }
        let mut number = self.numerify(Some("%#######"));
        number.push(luhn_check_digit(&number));
        if formatted {
            Ok(format!(
                "{} {} {}",
                &number[..3],
                &number[3..6],
                &number[6..]
            ))
        } else {
            Ok(number)
        }
    }

    pub fn vat_id(&self) -> DatabaseResult<String> {
        if self.provider_locale() != "it_IT" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker vatId locale",
            ));
        }
        self.vat(None)
    }

    pub fn btw(&self) -> DatabaseResult<String> {
        if self.provider_locale() != "nl_NL" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker btw locale"));
        }
        self.vat(None)
    }

    /// Generates the bank account format provided by the selected or fallback provider.
    pub fn bank_account_number(
        &self,
        prefix: &str,
        country_code: Option<&str>,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        if let Some((country, default_length)) = account_iban_defaults(self.provider_locale()) {
            return self.iban(
                Some(country_code.unwrap_or(country)),
                prefix,
                length.or(default_length),
            );
        }
        if !prefix.is_empty() || country_code.is_some() || length.is_some() {
            return Err(DatabaseError::InvalidQuery(
                "Faker local bank account takes no arguments",
            ));
        }
        match self.provider_locale() {
            "ar_EG" => self.iban(Some("EG"), "", Some(25)),
            locale if uses_us_payment_provider(locale) => {
                let length = 5 + (0..4).map(|_| self.random.usize(0..=3)).sum::<usize>();
                Ok(self.numerify(Some(&"#".repeat(length))))
            }
            "ms_MY" => Ok(self.numerify(Some(&"#".repeat(self.random.usize(10..=16))))),
            "ne_NP" => self.regexify(if self.random.bool() {
                "[A-Z][1-9]{8,19}"
            } else {
                "00[1-9]{7,18}"
            }),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker bank account locale",
            )),
        }
    }

    /// Generates a routing number when the US Payment provider is selected.
    pub fn bank_routing_number(&self) -> DatabaseResult<String> {
        if !uses_us_payment_provider(self.provider_locale()) {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker bank routing locale",
            ));
        }
        let bank_type = [0, 0, 0, 0, 20, 20, 60][self.random.usize(..7)];
        let district = self.number_between(1, 12);
        let clearing_center = self.random_digit_not_null();
        let state = self.random_digit();
        let institution = self.number_between(1000, 9999);
        let routing = format!(
            "{:02}{clearing_center}{state}{institution}",
            district + bank_type
        );
        let checksum = Self::calculate_routing_number_checksum(&routing)?;
        Ok(format!("{routing}{checksum}"))
    }

    /// Computes the check digit from the first eight routing-number digits.
    pub fn calculate_routing_number_checksum(routing: &str) -> DatabaseResult<u8> {
        let digits = routing
            .as_bytes()
            .get(..8)
            .filter(|digits| digits.iter().all(u8::is_ascii_digit))
            .ok_or(DatabaseError::InvalidQuery("invalid Faker routing number"))?;
        let weights = [7, 3, 9, 7, 3, 9, 7, 3];
        let sum: u16 = digits
            .iter()
            .zip(weights)
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum();
        Ok((sum % 10) as u8)
    }

    /// Generates a sample IBAN using the selected country's account layout.
    pub fn iban(
        &self,
        country_code: Option<&str>,
        prefix: &str,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        let country = country_code.map_or_else(
            || {
                IBAN_FORMATS[self.random.usize(..IBAN_FORMATS.len())]
                    .0
                    .to_owned()
            },
            str::to_ascii_uppercase,
        );
        if country.len() != 2 || !country.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(DatabaseError::InvalidQuery("Faker IBAN country code"));
        }
        if !prefix
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            return Err(DatabaseError::InvalidQuery("Faker IBAN prefix"));
        }
        let layout = IBAN_FORMATS
            .binary_search_by_key(&country.as_str(), |(code, _)| *code)
            .ok()
            .map(|index| IBAN_FORMATS[index].1);
        let length = layout.map_or_else(|| length.unwrap_or(24), <[u8]>::len);
        if length > 30 || prefix.len() > 30 {
            return Err(DatabaseError::InvalidQuery("Faker IBAN account length"));
        }
        let mut account = String::with_capacity(length.max(prefix.len()));
        account.push_str(prefix);
        for index in prefix.len()..length {
            let class = layout.map_or(b'n', |layout| layout[index]);
            let letter = class == b'a' || (class == b'c' && self.random.bool());
            account.push(if letter {
                char::from(self.random.u8(b'A'..=b'Z'))
            } else {
                char::from(self.random.u8(b'0'..=b'9'))
            });
        }
        let remainder =
            account
                .bytes()
                .chain(country.bytes())
                .chain(*b"00")
                .fold(0_u32, |remainder, byte| {
                    if byte.is_ascii_digit() {
                        (10 * remainder + u32::from(byte - b'0')) % 97
                    } else {
                        (100 * remainder + u32::from(byte - b'A') + 10) % 97
                    }
                });
        Ok(format!("{country}{:02}{account}", 98 - remainder))
    }

    /// Generates a sample SWIFT/BIC code with an optional three-character branch suffix.
    pub fn swift_bic_number(&self) -> String {
        const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let location_length = if self.random.bool() { 2 } else { 5 };
        let mut code = String::with_capacity(6 + location_length);
        code.extend((0..6).map(|_| char::from(self.random.u8(b'A'..=b'Z'))));
        code.extend(
            (0..location_length)
                .map(|_| char::from(ALPHANUMERIC[self.random.usize(..ALPHANUMERIC.len())])),
        );
        code
    }

    pub fn imei(&self) -> String {
        let mut digits = self.numerify(Some("##############"));
        let sum: u32 = digits
            .bytes()
            .rev()
            .enumerate()
            .map(|(index, byte)| {
                let digit = u32::from(byte - b'0');
                let weighted = digit * (2 - (index % 2) as u32);
                weighted / 10 + weighted % 10
            })
            .sum();
        digits.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
        digits
    }

    pub fn domain_word(&self) -> DatabaseResult<String> {
        let mut name = internet_ascii(&self.last_name());
        if name.trim_matches(['.', '_']).is_empty() {
            return Err(DatabaseError::InvalidQuery(
                "Faker surname cannot be transliterated to a domain word",
            ));
        }
        name.truncate(name.trim_end_matches('.').len());
        Ok(name)
    }

    pub fn tld(&self) -> &'static str {
        let suffixes = domains::tlds(self.provider_locale());
        suffixes[self.random.usize(..suffixes.len())]
    }

    pub fn domain_name(&self) -> DatabaseResult<String> {
        let name = match username::ascii_surname(&self.random, self.provider_locale()) {
            Some(surname) => surname.to_owned(),
            None => self.domain_word()?,
        };
        Ok(format!("{name}.{}", self.tld()))
    }

    pub fn slug(&self, word_count: Option<usize>, variable_word_count: bool) -> String {
        self.words(lorem_count(
            &self.random,
            word_count.unwrap_or(6),
            variable_word_count,
        ))
        .join("-")
    }

    pub fn url(&self) -> DatabaseResult<String> {
        if self.provider_locale() == "ne_NP" {
            let format = match self.random.u8(0..11) {
                0 | 1 => ("http://www.", ".np", false, ""),
                2 | 3 => ("http://", ".np", false, ""),
                4 => ("http://www.", ".np", true, ""),
                5 => ("http://www.", ".np", true, ".html"),
                6 | 7 => ("http://", ".np", true, ""),
                8 => ("http://", "", true, ".html"),
                9 => ("http://www.", "", false, ""),
                _ => ("http://", "", false, ""),
            };
            return self.render_url(format);
        }

        let format = match self.random.u8(0..10) {
            0 => ("http://www.", "", false, ""),
            1 => ("http://", "", false, ""),
            2 | 3 => ("http://www.", "", true, ""),
            4 => ("https://www.", "", true, ""),
            5 => ("http://www.", "", true, ".html"),
            6 | 7 => ("http://", "", true, ""),
            8 => ("http://", "", true, ".html"),
            _ => ("https://", "", true, ".html"),
        };
        self.render_url(format)
    }

    fn render_url(
        &self,
        (prefix, domain_suffix, path, extension): (&str, &str, bool, &str),
    ) -> DatabaseResult<String> {
        let mut url = format!("{prefix}{}{domain_suffix}/", self.domain_name()?);
        if path {
            url.push_str(&self.slug(None, true));
            url.push_str(extension);
        }
        Ok(url)
    }

    pub fn ipv4(&self) -> String {
        let address = if self.boolean(None) {
            self.random.u32(0x8000_0000..=0xffff_fffe)
        } else {
            self.random.u32(0x0100_0000..=0x7fff_ffff)
        };
        Ipv4Addr::from(address).to_string()
    }

    pub fn ipv6(&self) -> String {
        let mut address = String::with_capacity(39);
        for index in 0..8 {
            if index != 0 {
                address.push(':');
            }
            write!(address, "{:x}", self.random.u16(..)).expect("writing to String cannot fail");
        }
        address
    }

    pub fn local_ipv4(&self) -> String {
        let (network, host_mask) = match self.random.u8(0..3) {
            0 => (0x0a00_0000, 0x00ff_ffff),
            1 => (0xac10_0000, 0x000f_ffff),
            _ => (0xc0a8_0000, 0x0000_ffff),
        };
        Ipv4Addr::from(network | self.random.u32(0..=host_mask)).to_string()
    }

    pub fn mac_address(&self) -> String {
        let mut address = String::with_capacity(17);
        for index in 0..6 {
            if index != 0 {
                address.push(':');
            }
            write!(address, "{:02X}", self.random.u8(..)).expect("writing to String cannot fail");
        }
        address
    }

    /// Samples a MIME type from the fixed FakerPHP file catalog.
    pub fn mime_type(&self) -> &'static str {
        MIME_TYPES[self.random.usize(..MIME_TYPES.len())].0
    }

    /// Samples a MIME group, then one of its extensions without the dot.
    pub fn file_extension(&self) -> &'static str {
        let extensions = MIME_TYPES[self.random.usize(..MIME_TYPES.len())].1;
        extensions[self.random.usize(..extensions.len())]
    }

    /// Creates an empty temporary file. The caller owns the persisted path.
    pub fn file_path(&self) -> io::Result<PathBuf> {
        let (_, path) = self.create_temporary_file()?.keep()?;
        Ok(path)
    }

    pub(super) fn create_temporary_file(&self) -> io::Result<tempfile::NamedTempFile> {
        tempfile::Builder::new().prefix("faker").tempfile()
    }

    /// Generates a semantic version with optional prerelease and build metadata.
    pub fn semver(&self, pre_release: bool, build: bool) -> DatabaseResult<String> {
        let mut version = format!(
            "{}.{}.{}",
            self.number_between(0, 9),
            self.number_between(0, 99),
            self.number_between(0, 99),
        );
        if pre_release && self.random.bool() {
            version.push('-');
            version.push_str(self.random.sample(&["alpha", "beta", "rc"]));
            if self.random.bool() {
                write!(version, ".{}", self.number_between(1, 99)).expect("writing to String");
            }
        }
        if build && self.random.bool() {
            version.push('+');
            if self.random.bool() {
                let letters = self.lexify(Some("??????"));
                let digest = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, letters.as_bytes());
                version.push_str(&digest_hex(digest.as_ref())[..7]);
            } else {
                version.push_str(&self.date_with_php_format("YmdHis", None)?);
            }
        }
        Ok(version)
    }

    /// Generates the version 3 sample UUID used by the FakerPHP file provider.
    pub fn uuid(&self) -> uuid::Uuid {
        let seed = format!(
            "{}#{}",
            self.number_between(0, i32::MAX.into()),
            self.number_between(0, i32::MAX.into())
        );
        let mut bytes: [u8; 16] = Md5::digest(seed).into();
        bytes[6] = (bytes[6] & 0x0f) | 0x30;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(bytes)
    }

    /// Copies one readable file from a directory without recursing into subdirectories.
    pub fn file(
        &self,
        source_directory: &Path,
        target_directory: &Path,
        full_path: bool,
    ) -> io::Result<PathBuf> {
        let source = fs::canonicalize(source_directory)?;
        let target = fs::canonicalize(target_directory)?;
        if !source.is_dir() || !target.is_dir() || source == target {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Faker file requires two different directories",
            ));
        }
        let mut candidates = fs::read_dir(&source)?.collect::<io::Result<Vec<_>>>()?;
        if self.random.is_seeded() {
            candidates.sort_unstable_by_key(fs::DirEntry::file_name);
        }
        self.random.shuffle(&mut candidates);
        let (source_path, mut source_file) = candidates
            .into_iter()
            .find_map(|entry| {
                let path = entry.path();
                if !path.is_file() {
                    return None;
                }
                File::open(&path).ok().map(|file| (path, file))
            })
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "Faker source directory has no readable files",
                )
            })?;
        loop {
            let mut name = std::ffi::OsString::from(format!("{}.", self.uuid()));
            if let Some(extension) = source_path.extension() {
                name.push(extension);
            } else if let Some(extension) = source_path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix('.'))
            {
                name.push(extension);
            }
            let path = target_directory.join(&name);
            let mut output = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                result => result?,
            };
            if let Err(error) = io::copy(&mut source_file, &mut output) {
                drop(output);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            return Ok(if full_path { path } else { PathBuf::from(name) });
        }
    }

    pub fn md5(&self) -> String {
        let value = self.number_between(0, i64::from(i32::MAX)).to_string();
        digest_hex(&Md5::digest(value.as_bytes()))
    }

    pub fn sha1(&self) -> String {
        self.digest_sample(&digest::SHA1_FOR_LEGACY_USE_ONLY)
    }

    pub fn sha256(&self) -> String {
        self.digest_sample(&digest::SHA256)
    }

    fn digest_sample(&self, algorithm: &'static digest::Algorithm) -> String {
        let value = self.number_between(0, i64::from(i32::MAX)).to_string();
        digest_hex(digest::digest(algorithm, value.as_bytes()).as_ref())
    }

    pub fn random_element<'a, T>(&self, values: &'a [T]) -> Option<&'a T> {
        self.random_key(values).map(|index| &values[index])
    }

    /// Selects a positional key; pass an explicit slice of keys to sample a keyed collection.
    pub fn random_key<T>(&self, values: &[T]) -> Option<usize> {
        (!values.is_empty()).then(|| self.random.usize(..values.len()))
    }

    pub fn random_elements<'a, T>(
        &self,
        values: &'a [T],
        count: Option<usize>,
        allow_duplicates: bool,
    ) -> DatabaseResult<Vec<&'a T>> {
        let size = values.len();
        let count = match count {
            Some(0) => return Ok(Vec::new()),
            _ if size == 0 => {
                return Err(DatabaseError::InvalidQuery(
                    "cannot sample an empty Faker collection",
                ));
            }
            Some(count) => count,
            None => self.random.usize(1..=size),
        };
        if allow_duplicates {
            let mut selected = Vec::new();
            selected.try_reserve_exact(count).map_err(|_| {
                DatabaseError::InvalidQuery("Faker sample count exceeds allocation capacity")
            })?;
            selected.extend((0..count).map(|_| &values[self.random.usize(..size)]));
            return Ok(selected);
        }
        if count > size {
            return Err(DatabaseError::InvalidQuery(
                "Faker sample count exceeds collection length",
            ));
        }

        // A sparse partial Fisher–Yates shuffle samples positions in O(count) space.
        let mut swaps = HashMap::with_capacity(count);
        let mut selected = Vec::with_capacity(count);
        for remaining in (size - count + 1..=size).rev() {
            let index = self.random.usize(..remaining);
            let last = remaining - 1;
            let picked = swaps.remove(&index).unwrap_or(index);
            let replacement = swaps.remove(&last).unwrap_or(last);
            if index != last {
                swaps.insert(index, replacement);
            }
            selected.push(&values[picked]);
        }
        Ok(selected)
    }

    pub fn shuffle_array<T: Clone>(&self, values: &[T]) -> Vec<T> {
        let mut shuffled = values.to_vec();
        self.random.shuffle(&mut shuffled);
        shuffled
    }

    pub fn shuffle_string(&self, value: &str) -> String {
        let mut characters: Vec<_> = value.chars().collect();
        self.random.shuffle(&mut characters);
        characters.into_iter().collect()
    }

    pub fn random_letter(&self) -> char {
        char::from(self.random.u8(b'a'..=b'z'))
    }

    pub fn random_ascii(&self) -> char {
        char::from(self.random.u8(b'!'..=b'~'))
    }

    pub fn numerify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("###")
            .chars()
            .map(|character| match character {
                '#' => char::from(b'0' + self.random_digit()),
                '%' => char::from(b'0' + self.random_digit_not_null()),
                literal => literal,
            })
            .collect()
    }

    pub fn lexify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("????")
            .chars()
            .map(|character| match character {
                '?' => self.random_letter(),
                literal => literal,
            })
            .collect()
    }

    pub fn bothify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("## ??")
            .chars()
            .map(|character| match character {
                '#' => char::from(b'0' + self.random_digit()),
                '%' => char::from(b'0' + self.random_digit_not_null()),
                '?' => self.random_letter(),
                '*' if self.random.bool() => self.random_letter(),
                '*' => char::from(b'0' + self.random_digit()),
                literal => literal,
            })
            .collect()
    }

    pub fn regexify(&self, pattern: &str) -> DatabaseResult<String> {
        Ok(RegexPattern::parse(pattern)?.sample(&self.random))
    }

    pub fn asciify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("****")
            .chars()
            .map(|character| match character {
                '*' => self.random_ascii(),
                literal => literal,
            })
            .collect()
    }

    /// Generates an integer with at most the requested number of digits.
    pub fn random_number(&self, digits: Option<u32>, strict: bool) -> DatabaseResult<i64> {
        let digits = digits.unwrap_or_else(|| u32::from(self.random_digit_not_null()));
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
        Ok(self.number_between(min, max))
    }

    /// Samples an inclusive interval at Unix-second precision.
    pub fn date_time_between(&self, start: Timestamp, end: Timestamp) -> DatabaseResult<Timestamp> {
        let (start, end) = (timestamp_seconds(start), timestamp_seconds(end));
        if start > end {
            return Err(DatabaseError::InvalidQuery(
                "Faker start date exceeds end date",
            ));
        }
        Timestamp::new(self.number_between(start, end), 0)
            .map_err(|_| DatabaseError::InvalidQuery("Faker timestamp is out of range"))
    }

    /// Resolves PHP-style date expressions in the application time zone and samples their range.
    pub fn date_time_between_expressions(
        &self,
        start: &str,
        end: &str,
    ) -> DatabaseResult<Timestamp> {
        let now = Timestamp::now();
        let start = self
            .timestamp_expression(start, now)
            .or_else(faker_timestamp)?;
        let end = self
            .resolve_timestamp_max(end, now)
            .or_else(faker_timestamp)?;
        let start = Timestamp::new(start, 0).map_err(|_| DATE_RANGE_ERROR)?;
        let end = Timestamp::new(end, 0).map_err(|_| DATE_RANGE_ERROR)?;
        self.date_time_between(start, end)
    }

    /// Resolves a PHP-style maximum for any formatter that takes a UTC date bound.
    pub fn resolve_date_max(&self, max: &str) -> DatabaseResult<Timestamp> {
        self.resolve_date_max_at(max, Timestamp::now())
    }

    fn timestamp_expression(
        &self,
        expression: &str,
        now: Timestamp,
    ) -> Result<i64, calendar::CalendarError> {
        calendar::parse_timestamp_expression(expression, now, self.application_timezone.clone())
    }

    pub(super) fn resolve_date_max_at(
        &self,
        max: &str,
        now: Timestamp,
    ) -> DatabaseResult<Timestamp> {
        let timestamp = self
            .resolve_timestamp_max(max, now)
            .map_err(calendar_query_error)?;
        Timestamp::new(timestamp, 0).map_err(|_| DATE_RANGE_ERROR)
    }

    fn resolve_timestamp_max(
        &self,
        max: &str,
        now: Timestamp,
    ) -> Result<i64, calendar::CalendarError> {
        if max.is_empty() {
            return Ok(timestamp_seconds(now));
        }
        let numeric = max.trim_matches(calendar::php_whitespace);
        if calendar::is_numeric(numeric) {
            if let Ok(integer) = numeric.parse::<i64>() {
                return Ok(integer);
            }
            let number = numeric
                .parse::<f64>()
                .map_err(|_| calendar::CalendarError::OutOfRange)?;
            // PHP's numeric-string cast saturates finite values and maps infinity to zero.
            return Ok(if number.is_finite() { number as i64 } else { 0 });
        }
        self.timestamp_expression(max, now)
    }

    /// Samples between the supplied UTC instant and that instant plus a signed interval.
    pub fn date_time_in_interval(
        &self,
        date: Option<Timestamp>,
        interval: Option<SignedDuration>,
    ) -> DatabaseResult<Timestamp> {
        let zone = self.application_timezone.clone();
        let uses_local_calendar = date.is_none();
        let date = match date {
            Some(date) => date,
            None => {
                let now = Timestamp::now().to_zoned(zone.clone()).datetime();
                resolve_local(zone.clone(), months_ago(now, 30 * 12)?)?
            }
        };
        let other = match interval {
            Some(interval) => date.checked_add(interval).map_err(|_| DATE_RANGE_ERROR)?,
            None if uses_local_calendar => {
                let local = date
                    .to_zoned(zone.clone())
                    .datetime()
                    .checked_add(Span::new().days(5))
                    .map_err(|_| DATE_RANGE_ERROR)?;
                resolve_local(zone, local)?
            }
            None => date
                .checked_add(SignedDuration::from_secs(5 * 86400))
                .map_err(|_| DATE_RANGE_ERROR)?,
        };
        self.date_time_between(date.min(other), date.max(other))
    }

    /// Samples a range using PHP-style date and relative interval expressions.
    pub fn date_time_in_interval_expressions(
        &self,
        date: Option<&str>,
        interval: Option<&str>,
    ) -> DatabaseResult<Timestamp> {
        let (start, end) = calendar::date_interval(
            match date {
                Some("") => "now",
                Some(date) => date,
                None => "-30 years",
            },
            interval.unwrap_or("+5 days"),
            Timestamp::now(),
            self.application_timezone.clone(),
        )
        .map_err(|_| DatabaseError::InvalidQuery("invalid Faker date or interval expression"))?;
        self.date_time_between(start.min(end), start.max(end))
    }

    pub(super) fn calendar_period_start(
        &self,
        now: Timestamp,
        years: i32,
    ) -> DatabaseResult<Timestamp> {
        let year = i32::from(now.to_zoned(self.application_timezone.clone()).year());
        let first_year = year.div_euclid(years) * years;
        self.resolve_date_max_at(&format!("first day of january {first_year}"), now)
    }

    pub fn date_time_this_century(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.date_time_since_months_ago(100 * 12, max)
    }

    pub fn date_time_this_decade(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.date_time_since_months_ago(10 * 12, max)
    }

    pub fn date_time_this_year(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        let now = Timestamp::now();
        let zone = self.application_timezone.clone();
        let year = now.to_zoned(zone.clone()).year();
        let start = CivilDate::new(year, 1, 1)
            .map(|date| date.at(0, 0, 0, 0))
            .map_err(|_| DATE_RANGE_ERROR)?;
        let start = resolve_local(zone, start)?;
        self.date_time_between(start, max.unwrap_or(now))
    }

    pub fn date_time_this_month(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.date_time_since_months_ago(1, max)
    }

    fn date_time_since_months_ago(
        &self,
        months: u32,
        max: Option<Timestamp>,
    ) -> DatabaseResult<Timestamp> {
        let now = Timestamp::now();
        let zone = self.application_timezone.clone();
        let start = months_ago(now.to_zoned(zone.clone()).datetime(), months)?;
        self.date_time_between(resolve_local(zone, start)?, max.unwrap_or(now))
    }

    /// Samples whole seconds between the Unix epoch and the supplied upper endpoint.
    pub fn unix_time(&self, max: Option<Timestamp>) -> i64 {
        let end = max.map_or_else(|| timestamp_seconds(Timestamp::now()), timestamp_seconds);
        self.number_between(0, end)
    }

    /// Samples Unix seconds with a string endpoint, including dates outside the native date range.
    pub fn unix_time_expression(&self, max: &str) -> DatabaseResult<i64> {
        let end = self
            .resolve_timestamp_max(max, Timestamp::now())
            .or_else(faker_timestamp)?;
        Ok(self.number_between(0, end))
    }

    /// Returns a sampled instant in UTC.
    pub fn date_time(&self, max: Option<Timestamp>) -> Timestamp {
        Timestamp::new(self.unix_time(max), 0).expect("sample lies between valid timestamps")
    }

    /// Returns a sampled date in the explicit time zone or the generator's default.
    pub fn date_time_in_timezone(
        &self,
        max: Option<Timestamp>,
        timezone: Option<TimeZone>,
    ) -> Zoned {
        self.zoned_date(self.date_time(max), timezone)
    }

    pub(super) fn zoned_date(&self, instant: Timestamp, timezone: Option<TimeZone>) -> Zoned {
        let timezone = timezone
            .or_else(|| self.get_default_timezone())
            .unwrap_or_else(|| self.application_timezone.clone());
        instant.to_zoned(timezone)
    }

    pub fn date(&self, format: Option<&str>, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%Y-%m-%d"), max)
    }

    pub fn time(&self, format: Option<&str>, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%H:%M:%S"), max)
    }

    /// Renders a sampled date with PHP DateTime format tokens.
    pub fn date_with_php_format(
        &self,
        format: &str,
        max: Option<Timestamp>,
    ) -> DatabaseResult<String> {
        let timezone = self
            .get_default_timezone()
            .unwrap_or_else(|| self.application_timezone.clone());
        calendar::format_php_date(self.date_time(max), timezone, format)
            .map_err(|_| DatabaseError::InvalidQuery("Faker PHP date format could not be rendered"))
    }

    /// Renders a sampled time with PHP DateTime format tokens.
    pub fn time_with_php_format(
        &self,
        format: &str,
        max: Option<Timestamp>,
    ) -> DatabaseResult<String> {
        self.date_with_php_format(format, max)
    }

    pub fn iso8601(&self, max: Option<Timestamp>) -> String {
        let mut text =
            super::faker_provider::rfc3339_seconds(&self.date_time_in_timezone(max, None));
        text.remove(text.len() - 3);
        text
    }

    /// Samples from the upstream 64-bit AD lower bound to the supplied endpoint.
    pub fn date_time_ad(&self, max: Option<Timestamp>) -> Timestamp {
        const FIRST_AD_TIMESTAMP: i64 = -62_135_597_361;
        let end = timestamp_seconds(max.unwrap_or_else(Timestamp::now));
        Timestamp::new(self.number_between(FIRST_AD_TIMESTAMP, end), 0)
            .expect("sample lies between valid timestamps")
    }

    /// Samples an AD date and returns it in the chosen output time zone.
    pub fn date_time_ad_in_timezone(
        &self,
        max: Option<Timestamp>,
        timezone: Option<TimeZone>,
    ) -> Zoned {
        self.zoned_date(self.date_time_ad(max), timezone)
    }

    pub fn am_pm(&self, max: Option<Timestamp>) -> String {
        let date = self.date_time_in_timezone(max, None);
        let morning = date.hour() < 12;
        match (self.provider_locale(), morning) {
            ("tr_TR", true) => "öö".into(),
            ("tr_TR", false) => "ös".into(),
            ("zh_CN" | "zh_TW", true) => "上午".into(),
            ("zh_CN" | "zh_TW", false) => "下午".into(),
            _ => date.strftime("%P").to_string(),
        }
    }

    pub fn day_of_month(&self, max: Option<Timestamp>) -> String {
        let pattern = if self.provider_locale() == "cs_CZ" {
            "%-d"
        } else {
            "%d"
        };
        self.date_time_in_timezone(max, None)
            .strftime(pattern)
            .to_string()
    }

    pub fn day_of_week(&self, max: Option<Timestamp>) -> String {
        let date = self.date_time_in_timezone(max, None);
        let names: &[&str] = match self.provider_locale() {
            "cs_CZ" => &[
                "neděle",
                "pondělí",
                "úterý",
                "středa",
                "čtvrtek",
                "pátek",
                "sobota",
            ],
            "tr_TR" => &[
                "Pazar",
                "Pazartesi",
                "Salı",
                "Çarşamba",
                "Perşembe",
                "Cuma",
                "Cumartesi",
            ],
            "zh_CN" | "zh_TW" => &[
                "星期日",
                "星期一",
                "星期二",
                "星期三",
                "星期四",
                "星期五",
                "星期六",
            ],
            "ka_GE" => &[
                "კვირა",
                "ორშაბათი",
                "სამშაბათი",
                "ოთხშაბათი",
                "ხუთშაბათი",
                "პარასკევი",
                "შაბათი",
            ],
            _ => return date.strftime("%A").to_string(),
        };
        names[date.weekday().to_sunday_zero_offset() as usize].to_owned()
    }

    pub fn month(&self, max: Option<Timestamp>) -> String {
        self.date_time_in_timezone(max, None)
            .strftime("%m")
            .to_string()
    }

    pub fn month_name(&self, max: Option<Timestamp>) -> String {
        let date = self.date_time_in_timezone(max, None);
        let names: &[&str] = match self.provider_locale() {
            "cs_CZ" => &[
                "leden",
                "únor",
                "březen",
                "duben",
                "květen",
                "červen",
                "červenec",
                "srpen",
                "září",
                "říjen",
                "listopad",
                "prosinec",
            ],
            "tr_TR" => &[
                "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül",
                "Ekim", "Kasım", "Aralık",
            ],
            "zh_CN" | "zh_TW" => &[
                "一月",
                "二月",
                "三月",
                "四月",
                "五月",
                "六月",
                "七月",
                "八月",
                "九月",
                "十月",
                "十一月",
                "十二月",
            ],
            "ka_GE" => &[
                "იანვარი",
                "თებერვალი",
                "მარტი",
                "აპრილი",
                "მაისი",
                "ივნისი",
                "ივლისი",
                "აგვისტო",
                "სექტემბერი",
                "ოქტომბერი",
                "ნოემბერი",
                "დეკემბერი",
            ],
            _ => return date.strftime("%B").to_string(),
        };
        names[(date.month() - 1) as usize].to_owned()
    }

    /// Samples the Czech genitive form of a month name.
    pub fn month_name_genitive(&self, max: Option<Timestamp>) -> DatabaseResult<&'static str> {
        if self.provider_locale() != "cs_CZ" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker genitive month locale",
            ));
        }
        const NAMES: [&str; 12] = [
            "ledna",
            "února",
            "března",
            "dubna",
            "května",
            "června",
            "července",
            "srpna",
            "září",
            "října",
            "listopadu",
            "prosince",
        ];
        Ok(NAMES[(self.date_time_in_timezone(max, None).month() - 1) as usize])
    }

    /// Samples the Czech display date, drawing each of its three fields separately.
    pub fn formatted_date(&self) -> DatabaseResult<String> {
        if self.provider_locale() != "cs_CZ" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker formatted date locale",
            ));
        }
        let day = self.day_of_month(None);
        let month = self.month_name_genitive(None)?;
        Ok(format!("{day}. {month} {}", self.year(None)))
    }

    pub fn year(&self, max: Option<Timestamp>) -> String {
        let year = self
            .date_time_in_timezone(max, None)
            .strftime("%Y")
            .to_string();
        match year.strip_prefix('+') {
            Some(unsigned) => unsigned.to_owned(),
            None => year,
        }
    }

    pub fn century(&self) -> &'static str {
        const CENTURIES: [&str; 21] = [
            "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI", "XII", "XIII",
            "XIV", "XV", "XVI", "XVII", "XVIII", "XIX", "XX", "XXI",
        ];
        CENTURIES[self.random.usize(..CENTURIES.len())]
    }

    /// Picks an IANA time zone, optionally restricted to an ISO country code.
    pub fn timezone(&self, country_code: Option<&str>) -> Option<&'static str> {
        type ZoneLists = (Vec<&'static str>, HashMap<&'static str, Vec<&'static str>>);
        static ZONES: OnceLock<ZoneLists> = OnceLock::new();
        let (all, by_country) = ZONES.get_or_init(|| {
            let mut all = vec!["UTC"];
            let mut by_country: HashMap<&str, Vec<&str>> = HashMap::new();
            // Public-domain IANA tzdb 2025b zone.tab used by the Faker country list.
            for line in include_str!("zone.tab")
                .lines()
                .filter(|line| !line.starts_with('#'))
            {
                let mut columns = line.split_ascii_whitespace();
                if let (Some(country), Some(zone)) = (columns.next(), columns.nth(1)) {
                    all.push(zone);
                    by_country.entry(country).or_default().push(zone);
                }
            }
            all.sort_unstable();
            all.dedup();
            (all, by_country)
        });
        let candidates = match country_code {
            None | Some("" | "0") => all,
            Some(code) => by_country.get(code)?,
        };
        candidates
            .get(self.random.usize(0..candidates.len()))
            .copied()
    }

    fn format_date(&self, format: &str, max: Option<Timestamp>) -> DatabaseResult<String> {
        jiff::fmt::strtime::format(format, &self.date_time_in_timezone(max, None))
            .map_err(|_| DatabaseError::InvalidQuery("Faker 日期格式无效"))
    }

    /// Samples between the supplied bounds, then rounds to the requested decimal place.
    pub fn random_float(
        &self,
        decimals: Option<i32>,
        min: f64,
        max: Option<f64>,
    ) -> DatabaseResult<f64> {
        let decimals = decimals.unwrap_or_else(|| i32::from(self.random_digit()));
        let max = match max {
            Some(max) => max,
            None => (self.random_number(None, false)? as f64).max(min),
        };
        if !min.is_finite() || !max.is_finite() {
            return Err(DatabaseError::InvalidQuery(
                "Faker float bounds must be finite",
            ));
        }
        let (min, max) = (min.min(max), min.max(max));
        let ratio = f64::from(self.random.i32(0..=i32::MAX)) / f64::from(i32::MAX);
        let span = max - min;
        let value = if span.is_finite() {
            (min + ratio * span).clamp(min, max)
        } else {
            min * (1.0 - ratio) + max * ratio
        };
        round_decimal(value, decimals)
    }
}

fn ean_with_checksum(mut code: String) -> String {
    let sum: u32 = code
        .bytes()
        .rev()
        .enumerate()
        .map(|(index, digit)| (3 - 2 * (index % 2) as u32) * u32::from(digit - b'0'))
        .sum();
    code.push(char::from(b'0' + ((10 - sum % 10) % 10) as u8));
    code
}

fn lorem_count(random: &FakerRandom, count: usize, variable: bool) -> usize {
    if !variable || count == 0 {
        return count;
    }
    let percentage = random.u128(60..=140);
    usize::try_from(count as u128 * percentage / 100 + 1).unwrap_or(usize::MAX)
}

fn capitalize_lorem_word(word: &str) -> String {
    // Capitalize an ASCII initial without changing the UTF-8 byte length.
    let mut output = word.to_owned();
    if let Some(first) = output.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    output
}

// Decimal scaling follows PHP 8.5's half-away-from-zero rounding order.
fn round_decimal(value: f64, decimals: i32) -> DatabaseResult<f64> {
    if value == 0.0 || decimals > 308 {
        return Ok(value);
    }
    if decimals < -308 {
        return Ok(0.0);
    }
    let exponent = 10_f64.powf(f64::from(decimals.unsigned_abs()));
    let positive_precision = decimals > 0;
    let unscale = |number: f64| {
        if positive_precision {
            number / exponent
        } else {
            number * exponent
        }
    };
    let scaled = if positive_precision {
        value * exponent
    } else {
        value / exponent
    };
    let mut integral = scaled.trunc();
    let adjacent = integral + 1.0_f64.copysign(value);
    if unscale(adjacent) == value {
        integral = adjacent;
    }
    if integral.abs() >= 1e16 {
        return Ok(value);
    }
    let edge = unscale(integral + 0.5_f64.copysign(integral)).abs();
    if value.abs() >= edge {
        integral += 1.0_f64.copysign(integral);
    }

    if decimals.unsigned_abs() >= 23 {
        // Reparse the decimal result to avoid a second binary rounding during rescaling.
        let rounded: f64 = format!("{integral:.0}e{}", -decimals)
            .parse()
            .map_err(|_| DatabaseError::InvalidQuery("Faker rounded float is out of range"))?;
        return Ok(if rounded.is_finite() { rounded } else { value });
    }
    Ok(unscale(integral))
}

/// A uniqueness modifier that shares the generator's formatter history.
pub struct UniqueFaker<'a>(&'a Faker, usize);

impl UniqueFaker<'_> {
    pub fn sentence(&self, words: Range<usize>) -> DatabaseResult<String> {
        self.generate("sentence", || self.0.sentence(words.clone()))
    }

    pub fn sentence_with_word_count(&self, count: usize, variable: bool) -> DatabaseResult<String> {
        self.generate("sentence", || {
            self.0.sentence_with_word_count(count, variable)
        })
    }

    pub fn paragraph(&self, count: usize, variable: bool) -> DatabaseResult<String> {
        self.generate("paragraph", || self.0.paragraph(count, variable))
    }

    pub fn text(&self, max: usize) -> DatabaseResult<String> {
        self.try_generate("text", || self.0.text(max))
    }

    pub fn words(&self, count: usize) -> DatabaseResult<Vec<String>> {
        self.generate_keyed(
            "words",
            || Ok(self.0.words(count)),
            |items| Ok(format!("{items:?}")),
        )
    }

    pub fn words_as_text(&self, count: usize) -> DatabaseResult<String> {
        self.generate("wordsAsText", || self.0.words_as_text(count))
    }

    pub fn sentences(&self, count: usize) -> DatabaseResult<Vec<String>> {
        self.generate_keyed(
            "sentences",
            || Ok(self.0.sentences(count)),
            |items| Ok(format!("{items:?}")),
        )
    }

    pub fn sentences_as_text(&self, count: usize) -> DatabaseResult<String> {
        self.generate("sentencesAsText", || self.0.sentences_as_text(count))
    }

    pub fn paragraphs(&self, count: usize) -> DatabaseResult<Vec<String>> {
        self.generate_keyed(
            "paragraphs",
            || Ok(self.0.paragraphs(count)),
            |items| Ok(format!("{items:?}")),
        )
    }

    pub fn paragraphs_as_text(&self, count: usize) -> DatabaseResult<String> {
        self.generate("paragraphsAsText", || self.0.paragraphs_as_text(count))
    }

    pub fn number_between(&self, first: i64, second: i64) -> DatabaseResult<i64> {
        self.generate("numberBetween", || self.0.number_between(first, second))
    }

    pub fn random_digit(&self) -> DatabaseResult<u8> {
        self.generate("randomDigit", || self.0.random_digit())
    }

    pub fn random_digit_not_null(&self) -> DatabaseResult<u8> {
        self.generate("randomDigitNotNull", || self.0.random_digit_not_null())
    }

    pub fn random_digit_not(&self, except: i64) -> DatabaseResult<u8> {
        self.generate("randomDigitNot", || self.0.random_digit_not(except))
    }

    pub fn boolean(&self, chance_of_getting_true: Option<i64>) -> DatabaseResult<bool> {
        self.generate("boolean", || self.0.boolean(chance_of_getting_true))
    }

    pub fn latitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.try_generate("latitude", || {
            self.0.latitude(min, max).map(|value| value + 0.0)
        })
    }

    pub fn longitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.try_generate("longitude", || {
            self.0.longitude(min, max).map(|value| value + 0.0)
        })
    }

    pub fn local_latitude(&self) -> DatabaseResult<f64> {
        self.try_generate("localLatitude", || {
            self.0.local_latitude().map(|value| value + 0.0)
        })
    }

    pub fn local_longitude(&self) -> DatabaseResult<f64> {
        self.try_generate("localLongitude", || {
            self.0.local_longitude().map(|value| value + 0.0)
        })
    }

    pub fn local_coordinates(&self) -> DatabaseResult<Coordinates> {
        self.generate_keyed(
            "localCoordinates",
            || self.0.local_coordinates(),
            |value| {
                Ok(format!(
                    "{},{}",
                    value.latitude + 0.0,
                    value.longitude + 0.0
                ))
            },
        )
    }

    pub fn city(&self) -> DatabaseResult<String> {
        self.generate("city", || self.0.city())
    }

    pub fn street_name(&self) -> DatabaseResult<String> {
        self.generate("streetName", || self.0.street_name())
    }

    pub fn city_prefix(&self) -> DatabaseResult<String> {
        self.generate("cityPrefix", || self.0.city_prefix())
    }

    pub fn city_suffix(&self) -> DatabaseResult<String> {
        self.generate("citySuffix", || self.0.city_suffix())
    }

    pub fn street_suffix(&self) -> DatabaseResult<String> {
        self.generate("streetSuffix", || self.0.street_suffix())
    }

    pub fn country(&self) -> DatabaseResult<String> {
        self.generate("country", || self.0.country())
    }

    pub fn state(&self) -> DatabaseResult<String> {
        self.generate("state", || self.0.state())
    }

    pub fn state_abbr(&self) -> DatabaseResult<String> {
        self.generate("stateAbbr", || self.0.state_abbr())
    }

    pub fn area(&self) -> DatabaseResult<&'static str> {
        self.try_generate("area", || self.0.area())
    }

    pub fn street(&self) -> DatabaseResult<&'static str> {
        self.try_generate("street", || self.0.street())
    }

    pub fn community(&self) -> DatabaseResult<&'static str> {
        self.try_generate("community", || self.0.community())
    }

    pub fn street_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetPrefix", || self.0.street_prefix())
    }

    pub fn street_number(&self) -> DatabaseResult<String> {
        self.try_generate("streetNumber", || self.0.street_number())
    }

    pub fn block_number(&self) -> DatabaseResult<String> {
        self.try_generate("blockNumber", || self.0.block_number())
    }

    pub fn floor_number(&self) -> DatabaseResult<&'static str> {
        self.try_generate("floorNumber", || self.0.floor_number())
    }

    pub fn apartment_number(&self) -> DatabaseResult<&'static str> {
        self.try_generate("apartmentNumber", || self.0.apartment_number())
    }

    pub fn street_plain_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetPlainName", || self.0.street_plain_name())
    }

    pub fn floor(&self) -> DatabaseResult<&'static str> {
        self.try_generate("floor", || self.0.floor())
    }

    pub fn block_segment(&self) -> DatabaseResult<&'static str> {
        self.try_generate("blockSegment", || self.0.block_segment())
    }

    pub fn block(&self) -> DatabaseResult<String> {
        self.try_generate("block", || self.0.block())
    }

    pub fn town_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townName", || self.0.town_name())
    }

    pub fn locality(&self) -> DatabaseResult<String> {
        self.try_generate("locality", || self.0.locality())
    }

    pub fn locality_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("localityName", || self.0.locality_name())
    }

    pub fn area_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("areaSuffix", || self.0.area_suffix())
    }

    pub fn society_name(&self) -> DatabaseResult<String> {
        self.try_generate("societyName", || self.0.society_name())
    }

    pub fn society_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("societySuffix", || self.0.society_suffix())
    }

    pub fn municipality(&self) -> DatabaseResult<&'static str> {
        self.try_generate("municipality", || self.0.municipality())
    }

    pub fn barangay(&self) -> DatabaseResult<&'static str> {
        self.try_generate("barangay", || self.0.barangay())
    }

    pub fn town(&self) -> DatabaseResult<String> {
        self.try_generate("town", || self.0.town())
    }

    pub fn village(&self) -> DatabaseResult<String> {
        self.try_generate("village", || self.0.village())
    }

    pub fn estate(&self) -> DatabaseResult<String> {
        self.try_generate("estate", || self.0.estate())
    }

    pub fn syllable(&self) -> DatabaseResult<&'static str> {
        self.try_generate("syllable", || self.0.syllable())
    }

    pub fn direction(&self) -> DatabaseResult<&'static str> {
        self.try_generate("direction", || self.0.direction())
    }

    pub fn english_street_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("englishStreetName", || self.0.english_street_name())
    }

    pub fn village_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("villageSuffix", || self.0.village_suffix())
    }

    pub fn estate_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("estateSuffix", || self.0.estate_suffix())
    }

    pub fn prefecture(&self) -> DatabaseResult<&'static str> {
        self.try_generate("prefecture", || self.0.prefecture())
    }

    pub fn ward(&self) -> DatabaseResult<&'static str> {
        self.try_generate("ward", || self.0.ward())
    }

    pub fn postcode1(&self) -> DatabaseResult<i64> {
        self.try_generate("postcode1", || self.0.postcode1())
    }

    pub fn postcode2(&self) -> DatabaseResult<i64> {
        self.try_generate("postcode2", || self.0.postcode2())
    }

    pub fn area_number(&self) -> DatabaseResult<i64> {
        self.try_generate("areaNumber", || self.0.area_number())
    }

    pub fn metropolitan_city(&self) -> DatabaseResult<&'static str> {
        self.try_generate("metropolitanCity", || self.0.metropolitan_city())
    }

    pub fn borough(&self) -> DatabaseResult<&'static str> {
        self.try_generate("borough", || self.0.borough())
    }

    pub fn hamlet_name(&self) -> DatabaseResult<String> {
        self.try_generate("hamletName", || self.0.hamlet_name())
    }

    pub fn hamlet_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("hamletPrefix", || self.0.hamlet_prefix())
    }

    pub fn ward_name(&self) -> DatabaseResult<String> {
        self.try_generate("wardName", || self.0.ward_name())
    }

    pub fn ward_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("wardPrefix", || self.0.ward_prefix())
    }

    pub fn district_name(&self) -> DatabaseResult<String> {
        self.try_generate("districtName", || self.0.district_name())
    }

    pub fn district_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("districtPrefix", || self.0.district_prefix())
    }

    pub fn building_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingPrefix", || self.0.building_prefix())
    }

    pub fn township(&self) -> DatabaseResult<String> {
        self.try_generate("township", || self.0.township())
    }

    pub fn township_prefix_abbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipPrefixAbbr", || self.0.township_prefix_abbr())
    }

    pub fn township_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipPrefix", || self.0.township_prefix())
    }

    pub fn township_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipSuffix", || self.0.township_suffix())
    }

    pub fn town_state(&self) -> DatabaseResult<String> {
        self.try_generate("townState", || self.0.town_state())
    }

    pub fn postcode_with_state(&self, state: &str) -> DatabaseResult<String> {
        self.try_generate("postcode", || self.0.postcode_with_state(state))
    }

    pub fn bangla_street_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("banglaStreetName", || self.0.bangla_street_name())
    }

    pub fn ward_number(&self) -> DatabaseResult<String> {
        self.try_generate("wardNumber", || self.0.ward_number())
    }

    pub fn governorate(&self) -> DatabaseResult<&'static str> {
        self.try_generate("governorate", || self.0.governorate())
    }

    pub fn subdivision(&self) -> DatabaseResult<&'static str> {
        self.try_generate("subdivision", || self.0.subdivision())
    }

    pub fn building_name_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingNamePrefix", || self.0.building_name_prefix())
    }

    pub fn building_number_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingNumberPrefix", || self.0.building_number_prefix())
    }

    pub fn postcode_prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("postcodePrefix", || self.0.postcode_prefix())
    }

    pub fn building(&self) -> DatabaseResult<String> {
        self.try_generate("building", || self.0.building())
    }

    pub fn region_genitive_form(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionGenitiveForm", || self.0.region_genitive_form())
    }

    pub fn region_suffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionSuffix", || self.0.region_suffix())
    }

    pub fn city_and_region(&self) -> DatabaseResult<String> {
        self.try_generate("cityAndRegion", || self.0.city_and_region())
    }

    pub fn street_suffix_short(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixShort", || self.0.street_suffix_short())
    }

    pub fn street_suffix_long(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixLong", || self.0.street_suffix_long())
    }

    pub fn street_suffix_word(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixWord", || self.0.street_suffix_word())
    }

    pub fn building_level(&self) -> DatabaseResult<String> {
        self.try_generate("buildingLevel", || self.0.building_level())
    }

    pub fn building_side(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingSide", || self.0.building_side())
    }

    pub fn kommune(&self) -> DatabaseResult<&'static str> {
        self.try_generate("kommune", || self.0.kommune())
    }

    pub fn capital(&self) -> DatabaseResult<&'static str> {
        self.try_generate("capital", || self.0.capital())
    }

    pub fn big_city(&self) -> DatabaseResult<&'static str> {
        self.try_generate("bigCity", || self.0.big_city())
    }

    pub fn smaller_city(&self) -> DatabaseResult<&'static str> {
        self.try_generate("smallerCity", || self.0.smaller_city())
    }

    pub fn city_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cityName", || self.0.city_name())
    }

    pub fn province_abbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("provinceAbbr", || self.0.province_abbr())
    }

    pub fn random_postcode_letter(&self) -> DatabaseResult<&'static str> {
        self.try_generate("randomPostcodeLetter", || self.0.random_postcode_letter())
    }

    pub fn building_letter(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingLetter", || self.0.building_letter())
    }

    pub fn county(&self) -> DatabaseResult<&'static str> {
        self.try_generate("county", || self.0.county())
    }

    pub fn province(&self) -> DatabaseResult<&'static str> {
        self.try_generate("province", || self.0.province())
    }

    pub fn canton_short(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cantonShort", || self.0.canton_short())
    }

    pub fn canton_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cantonName", || self.0.canton_name())
    }

    pub fn canton(&self) -> DatabaseResult<(&'static str, &'static str)> {
        self.generate_keyed("canton", || self.0.canton(), |(code, _)| Ok((*code).into()))
    }

    pub fn district(&self) -> DatabaseResult<&'static str> {
        self.try_generate("district", || self.0.district())
    }

    pub fn region_abbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionAbbr", || self.0.region_abbr())
    }

    pub fn region(&self) -> DatabaseResult<&'static str> {
        self.try_generate("region", || self.0.region())
    }

    pub fn department_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("departmentName", || self.0.department_name())
    }

    pub fn department_number(&self) -> DatabaseResult<&'static str> {
        self.try_generate("departmentNumber", || self.0.department_number())
    }

    pub fn random_chinese_number(&self) -> DatabaseResult<&'static str> {
        self.try_generate("randomChineseNumber", || self.0.random_chinese_number())
    }

    pub fn random_number2(&self) -> DatabaseResult<u8> {
        self.try_generate("randomNumber2", || self.0.random_number2())
    }

    pub fn random_number3(&self) -> DatabaseResult<u16> {
        self.try_generate("randomNumber3", || self.0.random_number3())
    }

    pub fn department(&self) -> DatabaseResult<(&'static str, &'static str)> {
        self.generate_keyed(
            "department",
            || self.0.department(),
            |(number, _)| Ok((*number).into()),
        )
    }

    pub fn postcode(&self) -> DatabaseResult<String> {
        self.generate("postcode", || self.0.postcode())
    }

    pub fn building_number(&self) -> DatabaseResult<String> {
        self.generate("buildingNumber", || self.0.building_number())
    }

    pub fn secondary_address(&self) -> DatabaseResult<String> {
        self.generate("secondaryAddress", || self.0.secondary_address())
    }

    pub fn street_address(&self) -> DatabaseResult<String> {
        self.generate("streetAddress", || self.0.street_address())
    }

    pub fn address(&self) -> DatabaseResult<String> {
        self.generate("address", || self.0.address())
    }

    pub fn title_male(&self) -> DatabaseResult<&'static str> {
        self.generate("titleMale", || self.0.title_male())
    }

    pub fn title_female(&self) -> DatabaseResult<&'static str> {
        self.generate("titleFemale", || self.0.title_female())
    }

    pub fn name(&self) -> DatabaseResult<String> {
        self.generate("name", || self.0.name())
    }

    pub fn first_name(&self) -> DatabaseResult<String> {
        self.generate("firstName", || self.0.first_name())
    }

    pub fn last_name(&self) -> DatabaseResult<String> {
        self.generate("lastName", || self.0.last_name())
    }

    pub fn middle_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleName", || self.0.middle_name())
    }

    pub fn middle_name_with_gender(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.try_generate("middleName", || self.0.middle_name_with_gender(gender))
    }

    pub fn middle_name_male(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleNameMale", || self.0.middle_name_male())
    }

    pub fn middle_name_female(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleNameFemale", || self.0.middle_name_female())
    }

    pub fn last_name_with_gender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("lastName", || self.0.last_name_with_gender(gender))
    }

    pub fn last_name_male(&self) -> DatabaseResult<String> {
        self.try_generate("lastNameMale", || self.0.last_name_male())
    }

    pub fn last_name_female(&self) -> DatabaseResult<String> {
        self.try_generate("lastNameFemale", || self.0.last_name_female())
    }

    pub fn first_name_male_ne(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstNameMaleNe", || self.0.first_name_male_ne())
    }

    pub fn last_name_female_married(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastNameFemaleMarried", || {
            self.0.last_name_female_married()
        })
    }

    pub fn first_name_male(&self) -> DatabaseResult<String> {
        self.generate("firstNameMale", || self.0.first_name_male())
    }

    pub fn first_name_female(&self) -> DatabaseResult<String> {
        self.generate("firstNameFemale", || self.0.first_name_female())
    }

    pub fn my_kad_number(
        &self,
        gender: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("myKadNumber", || {
            self.0.my_kad_number(gender, formatted, separator)
        })
    }

    pub fn cnp(
        &self,
        gender: Option<&str>,
        date_of_birth: Option<&str>,
        county: Option<&str>,
        is_resident: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("cnp", || {
            self.0.cnp(gender, date_of_birth, county, is_resident)
        })
    }

    pub fn first_name_with_gender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("firstName", || self.0.first_name_with_gender(gender))
    }

    pub fn name_with_gender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("name", || self.0.name_with_gender(gender))
    }

    pub fn kana_name(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("kanaName", || self.0.kana_name(gender))
    }

    pub fn first_kana_name(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaName", || self.0.first_kana_name(gender))
    }

    pub fn first_kana_name_male(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaNameMale", || self.0.first_kana_name_male())
    }

    pub fn first_kana_name_female(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaNameFemale", || self.0.first_kana_name_female())
    }

    pub fn last_kana_name(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastKanaName", || self.0.last_kana_name())
    }

    pub fn random_name(&self, pool: &[&str], count: usize) -> DatabaseResult<String> {
        self.try_generate("randomName", || self.0.random_name(pool, count))
    }

    pub fn prefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("prefix", || self.0.prefix())
    }

    pub fn suffix(&self) -> DatabaseResult<Option<&'static str>> {
        self.generate_keyed("suffix", || self.0.suffix(), faker_json_key)
    }

    pub fn title(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.generate("title", || self.0.title(gender))
    }

    pub fn first_name_ascii(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstNameAscii", || self.0.first_name_ascii())
    }

    pub fn last_name_ascii(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastNameAscii", || self.0.last_name_ascii())
    }

    pub fn user_name(&self) -> DatabaseResult<String> {
        self.generate("userName", || self.0.user_name())
    }

    pub fn safe_email_domain(&self) -> DatabaseResult<&'static str> {
        self.generate("safeEmailDomain", || self.0.safe_email_domain())
    }

    pub fn free_email_domain(&self) -> DatabaseResult<&'static str> {
        self.generate("freeEmailDomain", || self.0.free_email_domain())
    }

    pub fn safe_email(&self) -> DatabaseResult<String> {
        self.generate("safeEmail", || self.0.safe_email())
    }

    pub fn free_email(&self) -> DatabaseResult<String> {
        self.generate("freeEmail", || self.0.free_email())
    }

    pub fn company_email(&self) -> DatabaseResult<String> {
        self.try_generate("companyEmail", || self.0.company_email())
    }

    pub fn email(&self) -> DatabaseResult<String> {
        self.try_generate("email", || self.0.email())
    }

    pub fn password(
        &self,
        min_length: Option<usize>,
        max_length: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("password", || self.0.password(min_length, max_length))
    }

    pub fn phone_number(&self) -> DatabaseResult<String> {
        self.generate("phoneNumber", || self.0.phone_number())
    }

    pub fn e164_phone_number(&self) -> DatabaseResult<String> {
        self.generate("e164PhoneNumber", || self.0.e164_phone_number())
    }

    pub fn ean13(&self) -> DatabaseResult<String> {
        self.generate("ean13", || self.0.ean13())
    }

    pub fn ean8(&self) -> DatabaseResult<String> {
        self.generate("ean8", || self.0.ean8())
    }

    pub fn isbn10(&self) -> DatabaseResult<String> {
        self.generate("isbn10", || self.0.isbn10())
    }

    pub fn isbn13(&self) -> DatabaseResult<String> {
        self.generate("isbn13", || self.0.isbn13())
    }

    pub fn credit_card_type(&self) -> DatabaseResult<&'static str> {
        self.generate("creditCardType", || self.0.credit_card_type())
    }

    pub fn credit_card_number(
        &self,
        card_type: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("creditCardNumber", || {
            self.0.credit_card_number(card_type, formatted, separator)
        })
    }

    pub fn credit_card_expiration_date(&self, valid: bool) -> DatabaseResult<Zoned> {
        self.try_generate("creditCardExpirationDate", || {
            self.0.credit_card_expiration_date(valid)
        })
    }

    pub fn credit_card_expiration_date_string(
        &self,
        valid: bool,
        expiration_date_format: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("creditCardExpirationDateString", || {
            self.0
                .credit_card_expiration_date_string(valid, expiration_date_format)
        })
    }

    pub fn credit_card_details(&self, valid: bool) -> DatabaseResult<CreditCardDetails> {
        self.generate_keyed(
            "creditCardDetails",
            || self.0.credit_card_details(valid),
            faker_json_key,
        )
    }

    pub fn bank(&self) -> DatabaseResult<&'static str> {
        self.try_generate("bank", || self.0.bank())
    }

    pub fn micro_finance(&self) -> DatabaseResult<String> {
        self.try_generate("microFinance", || self.0.micro_finance())
    }

    pub fn swift_code(&self) -> DatabaseResult<String> {
        self.try_generate("swiftCode", || self.0.swift_code())
    }

    pub fn vat(&self, spaced_national_prefix: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("vat", || self.0.vat(spaced_national_prefix))
    }

    pub fn vat_with_type(&self, vat_type: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("vat", || self.0.vat_with_type(vat_type))
    }

    pub fn siren(&self, formatted: bool) -> DatabaseResult<String> {
        self.try_generate("siren", || self.0.siren(formatted))
    }

    pub fn vat_id(&self) -> DatabaseResult<String> {
        self.try_generate("vatId", || self.0.vat_id())
    }

    pub fn btw(&self) -> DatabaseResult<String> {
        self.try_generate("btw", || self.0.btw())
    }

    pub fn bank_account_number(
        &self,
        prefix: &str,
        country_code: Option<&str>,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("bankAccountNumber", || {
            self.0.bank_account_number(prefix, country_code, length)
        })
    }

    pub fn bank_routing_number(&self) -> DatabaseResult<String> {
        self.try_generate("bankRoutingNumber", || self.0.bank_routing_number())
    }

    pub fn iban(
        &self,
        country_code: Option<&str>,
        prefix: &str,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("iban", || self.0.iban(country_code, prefix, length))
    }

    pub fn swift_bic_number(&self) -> DatabaseResult<String> {
        self.generate("swiftBicNumber", || self.0.swift_bic_number())
    }

    pub fn imei(&self) -> DatabaseResult<String> {
        self.generate("imei", || self.0.imei())
    }

    pub fn domain_word(&self) -> DatabaseResult<String> {
        self.try_generate("domainWord", || self.0.domain_word())
    }

    pub fn tld(&self) -> DatabaseResult<&'static str> {
        self.generate("tld", || self.0.tld())
    }

    pub fn domain_name(&self) -> DatabaseResult<String> {
        self.try_generate("domainName", || self.0.domain_name())
    }

    pub fn slug(
        &self,
        word_count: Option<usize>,
        variable_word_count: bool,
    ) -> DatabaseResult<String> {
        self.generate("slug", || self.0.slug(word_count, variable_word_count))
    }

    pub fn url(&self) -> DatabaseResult<String> {
        self.try_generate("url", || self.0.url())
    }

    pub fn ipv4(&self) -> DatabaseResult<String> {
        self.generate("ipv4", || self.0.ipv4())
    }

    pub fn ipv6(&self) -> DatabaseResult<String> {
        self.generate("ipv6", || self.0.ipv6())
    }

    pub fn local_ipv4(&self) -> DatabaseResult<String> {
        self.generate("localIpv4", || self.0.local_ipv4())
    }

    pub fn mac_address(&self) -> DatabaseResult<String> {
        self.generate("macAddress", || self.0.mac_address())
    }

    pub fn image_url(&self, options: &super::ImageOptions<'_>) -> DatabaseResult<String> {
        self.try_generate("imageUrl", || self.0.image_url(options))
    }

    pub fn mime_type(&self) -> DatabaseResult<&'static str> {
        self.generate("mimeType", || self.0.mime_type())
    }

    pub fn file_extension(&self) -> DatabaseResult<&'static str> {
        self.generate("fileExtension", || self.0.file_extension())
    }

    pub fn semver(&self, pre_release: bool, build: bool) -> DatabaseResult<String> {
        self.try_generate("semver", || self.0.semver(pre_release, build))
    }

    pub fn uuid(&self) -> DatabaseResult<uuid::Uuid> {
        self.generate("uuid", || self.0.uuid())
    }

    pub fn md5(&self) -> DatabaseResult<String> {
        self.generate("md5", || self.0.md5())
    }

    pub fn sha1(&self) -> DatabaseResult<String> {
        self.generate("sha1", || self.0.sha1())
    }

    pub fn sha256(&self) -> DatabaseResult<String> {
        self.generate("sha256", || self.0.sha256())
    }

    pub fn random_element<'a, T: serde::Serialize>(
        &self,
        values: &'a [T],
    ) -> DatabaseResult<Option<&'a T>> {
        self.generate_keyed(
            "randomElement",
            || Ok(self.0.random_element(values)),
            faker_json_key,
        )
    }

    pub fn random_elements<'a, T: serde::Serialize>(
        &self,
        values: &'a [T],
        count: Option<usize>,
        allow_duplicates: bool,
    ) -> DatabaseResult<Vec<&'a T>> {
        self.generate_keyed(
            "randomElements",
            || self.0.random_elements(values, count, allow_duplicates),
            faker_json_key,
        )
    }

    pub fn shuffle_array<T: Clone + serde::Serialize>(
        &self,
        values: &[T],
    ) -> DatabaseResult<Vec<T>> {
        self.generate_keyed(
            "shuffleArray",
            || Ok(self.0.shuffle_array(values)),
            faker_json_key,
        )
    }

    pub fn random_key<T>(&self, values: &[T]) -> DatabaseResult<Option<usize>> {
        self.generate_keyed(
            "randomKey",
            || Ok(self.0.random_key(values)),
            |key| Ok(format!("{key:?}")),
        )
    }

    pub fn shuffle_string(&self, value: &str) -> DatabaseResult<String> {
        self.generate("shuffleString", || self.0.shuffle_string(value))
    }

    pub fn random_letter(&self) -> DatabaseResult<char> {
        self.generate("randomLetter", || self.0.random_letter())
    }

    pub fn random_ascii(&self) -> DatabaseResult<char> {
        self.generate("randomAscii", || self.0.random_ascii())
    }

    pub fn numerify(&self, pattern: Option<&str>) -> DatabaseResult<String> {
        self.generate("numerify", || self.0.numerify(pattern))
    }

    pub fn lexify(&self, pattern: Option<&str>) -> DatabaseResult<String> {
        self.generate("lexify", || self.0.lexify(pattern))
    }

    pub fn bothify(&self, pattern: Option<&str>) -> DatabaseResult<String> {
        self.generate("bothify", || self.0.bothify(pattern))
    }

    pub fn regexify(&self, pattern: &str) -> DatabaseResult<String> {
        let pattern = RegexPattern::parse(pattern)?;
        self.generate("regexify", || pattern.sample(&self.0.random))
    }

    pub fn asciify(&self, pattern: Option<&str>) -> DatabaseResult<String> {
        self.generate("asciify", || self.0.asciify(pattern))
    }

    pub fn random_number(&self, digits: Option<u32>, strict: bool) -> DatabaseResult<i64> {
        self.try_generate("randomNumber", || self.0.random_number(digits, strict))
    }

    pub fn random_float(
        &self,
        decimals: Option<i32>,
        min: f64,
        max: Option<f64>,
    ) -> DatabaseResult<f64> {
        self.try_generate("randomFloat", || {
            self.0
                .random_float(decimals, min, max)
                .map(|value| value + 0.0)
        })
    }

    pub fn date_time_between(&self, start: Timestamp, end: Timestamp) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeBetween", || self.0.date_time_between(start, end))
    }

    pub fn date_time_between_expressions(
        &self,
        start: &str,
        end: &str,
    ) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeBetween", || {
            self.0.date_time_between_expressions(start, end)
        })
    }

    pub fn date_time_in_interval(
        &self,
        date: Option<Timestamp>,
        interval: Option<SignedDuration>,
    ) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeInInterval", || {
            self.0.date_time_in_interval(date, interval)
        })
    }

    pub fn date_time_in_interval_expressions(
        &self,
        date: Option<&str>,
        interval: Option<&str>,
    ) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeInInterval", || {
            self.0.date_time_in_interval_expressions(date, interval)
        })
    }

    pub fn date_time_this_century(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeThisCentury", || self.0.date_time_this_century(max))
    }

    pub fn date_time_this_decade(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeThisDecade", || self.0.date_time_this_decade(max))
    }

    pub fn date_time_this_year(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeThisYear", || self.0.date_time_this_year(max))
    }

    pub fn date_time_this_month(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.try_generate("dateTimeThisMonth", || self.0.date_time_this_month(max))
    }

    pub fn unix_time(&self, max: Option<Timestamp>) -> DatabaseResult<i64> {
        self.generate("unixTime", || self.0.unix_time(max))
    }

    pub fn unix_time_expression(&self, max: &str) -> DatabaseResult<i64> {
        let end = self
            .0
            .resolve_timestamp_max(max, Timestamp::now())
            .or_else(faker_timestamp)?;
        self.generate("unixTime", || self.0.number_between(0, end))
    }

    pub fn date_time(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.generate("dateTime", || self.0.date_time(max))
    }

    pub fn date(&self, format: Option<&str>, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.try_generate("date", || self.0.date(format, max))
    }

    pub fn time(&self, format: Option<&str>, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.try_generate("time", || self.0.time(format, max))
    }

    pub fn date_with_php_format(
        &self,
        format: &str,
        max: Option<Timestamp>,
    ) -> DatabaseResult<String> {
        self.try_generate("date", || self.0.date_with_php_format(format, max))
    }

    pub fn time_with_php_format(
        &self,
        format: &str,
        max: Option<Timestamp>,
    ) -> DatabaseResult<String> {
        self.try_generate("time", || self.0.time_with_php_format(format, max))
    }

    pub fn iso8601(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("iso8601", || self.0.iso8601(max))
    }

    pub fn date_time_ad(&self, max: Option<Timestamp>) -> DatabaseResult<Timestamp> {
        self.generate("dateTimeAD", || self.0.date_time_ad(max))
    }

    pub fn date_time_ad_in_timezone(
        &self,
        max: Option<Timestamp>,
        timezone: Option<TimeZone>,
    ) -> DatabaseResult<Zoned> {
        self.generate_zoned("dateTimeAD", || {
            Ok(self.0.date_time_ad_in_timezone(max, timezone.clone()))
        })
    }

    pub fn am_pm(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("amPm", || self.0.am_pm(max))
    }

    pub fn day_of_month(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("dayOfMonth", || self.0.day_of_month(max))
    }

    pub fn day_of_week(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("dayOfWeek", || self.0.day_of_week(max))
    }

    pub fn month(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("month", || self.0.month(max))
    }

    pub fn month_name(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("monthName", || self.0.month_name(max))
    }

    pub fn month_name_genitive(&self, max: Option<Timestamp>) -> DatabaseResult<&'static str> {
        self.try_generate("monthNameGenitive", || self.0.month_name_genitive(max))
    }

    pub fn formatted_date(&self) -> DatabaseResult<String> {
        self.try_generate("formattedDate", || self.0.formatted_date())
    }

    pub fn year(&self, max: Option<Timestamp>) -> DatabaseResult<String> {
        self.generate("year", || self.0.year(max))
    }

    pub fn century(&self) -> DatabaseResult<&'static str> {
        self.generate("century", || self.0.century())
    }

    pub fn timezone(&self, country_code: Option<&str>) -> DatabaseResult<Option<&'static str>> {
        // The empty key represents PHP's null result for an unknown country.
        let zone = self.generate("timezone", || {
            self.0.timezone(country_code).unwrap_or_default()
        })?;
        Ok((!zone.is_empty()).then_some(zone))
    }

    pub fn date_time_in_timezone(
        &self,
        max: Option<Timestamp>,
        timezone: Option<TimeZone>,
    ) -> DatabaseResult<Zoned> {
        self.generate_zoned("dateTime", || {
            Ok(self.0.date_time_in_timezone(max, timezone.clone()))
        })
    }

    fn generate_zoned(
        &self,
        formatter: &'static str,
        value: impl FnMut() -> DatabaseResult<Zoned>,
    ) -> DatabaseResult<Zoned> {
        self.generate_keyed(formatter, value, |date| {
            Ok(if date.time_zone().iana_name() == Some("UTC") {
                date.timestamp().to_string()
            } else {
                // PHP serializes named DateTime results as a wall clock and a zone name.
                let local_seconds =
                    timestamp_seconds(date.timestamp()) + i64::from(date.offset().seconds());
                format!(
                    "{local_seconds}:{} [{}]",
                    date.timestamp().as_nanosecond().rem_euclid(1_000_000_000),
                    date.time_zone()
                        .iana_name()
                        .map(str::to_owned)
                        .unwrap_or_else(|| date.offset().to_string())
                )
            })
        })
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
        value: impl FnMut() -> DatabaseResult<T>,
    ) -> DatabaseResult<T> {
        self.generate_keyed(formatter, value, |value| Ok(value.to_string()))
    }

    fn generate_keyed<T>(
        &self,
        formatter: &'static str,
        value: impl FnMut() -> DatabaseResult<T>,
        key: impl Fn(&T) -> DatabaseResult<String>,
    ) -> DatabaseResult<T> {
        self.try_unique(value, |value| {
            let key = key(value)?;
            Ok(self
                .0
                .unique
                .lock()
                .unwrap()
                .typed
                .entry(formatter)
                .or_default()
                .insert(key))
        })
    }

    fn try_unique<T>(
        &self,
        mut generate: impl FnMut() -> DatabaseResult<T>,
        mut accept: impl FnMut(&T) -> DatabaseResult<bool>,
    ) -> DatabaseResult<T> {
        for attempt in 0..=self.1 {
            let value = generate()?;
            if attempt == self.1 {
                break;
            }
            if accept(&value)? {
                return Ok(value);
            }
        }
        Err(DatabaseError::InvalidQuery("unique Faker values exhausted"))
    }
}

fn luhn_check_digit(number: &str) -> char {
    let sum: u32 = number
        .bytes()
        .rev()
        .enumerate()
        .map(|(index, digit)| {
            let value = u32::from(digit - b'0');
            if index % 2 == 0 {
                let doubled = value * 2;
                doubled / 10 + doubled % 10
            } else {
                value
            }
        })
        .sum();
    char::from(b'0' + ((10 - sum % 10) % 10) as u8)
}

fn faker_json_key<T: serde::Serialize>(value: &T) -> DatabaseResult<String> {
    serde_json::to_string(value).map_err(|_| {
        DatabaseError::InvalidQuery("Faker sample could not be serialized for uniqueness")
    })
}

fn digest_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn internet_ascii(value: &str) -> String {
    let mut output = Str::transliterate_with_unknown(value, Some(""));
    output.retain(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'));
    output.make_ascii_lowercase();
    output
}

static US_AREA_CODES: OnceLock<Vec<RegexPattern>> = OnceLock::new();
static DE_AREA_CODES: OnceLock<Vec<RegexPattern>> = OnceLock::new();

fn phone_area_code(
    random: &FakerRandom,
    cache: &OnceLock<Vec<RegexPattern>>,
    formats: &[&str],
) -> String {
    let patterns = cache.get_or_init(|| {
        formats
            .iter()
            .map(|pattern| RegexPattern::parse(pattern).expect("fixed Faker area-code pattern"))
            .collect()
    });
    let digit = random.u8(2..=9);
    format!("{digit}{}", patterns[usize::from(digit - 2)].sample(random))
}

impl UniqueFaker<'_> {
    pub fn random_locale(&self) -> DatabaseResult<&'static str> {
        self.generate("randomLocale", || self.0.random_locale())
    }
}
