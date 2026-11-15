use super::{
    faker_address::{self, DISTRICTS_ZH_CN},
    faker_file::MIME_TYPES,
    faker_institutions, faker_miscellaneous,
    faker_payment::{
        CARD_VENDORS, CARD_VENDORS_PT_BR, IBAN_FORMATS, account_iban_defaults, card_masks,
        card_masks_pt_br,
    },
    regex_pattern::RegexPattern,
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    fs::{self, File, OpenOptions},
    io,
    net::Ipv4Addr,
    ops::Range,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, RwLock},
};

use crate::{
    container::Container,
    contracts::{config::Repository, container::Container as _},
    database::{Connection, DatabaseError, DatabaseResult},
    support::{Str, calendar},
    translation::canonical_locale,
};
use ::fake::{Fake, faker, locales};
use aws_lc_rs::digest;
use chrono::{
    DateTime, Datelike, Days, LocalResult, Months, NaiveDate, NaiveDateTime, Offset, TimeDelta,
    TimeZone, Timelike, Utc,
};
use chrono_tz::{GapInfo, Tz, UTC};
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
    ($($method:ident => $locale:literal, $catalog:ident;)*) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.institution_sample($locale, faker_institutions::$catalog)
            })*
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.try_generate(stringify!($method), || self.0.$method())
            })*
        }
    };
}

institution_formatters! {
    localBank => "ms_MY", MS_MY_LOCAL_BANKS;
    foreignBank => "ms_MY", MS_MY_FOREIGN_BANKS;
    governmentBank => "ms_MY", MS_MY_GOVERNMENT_BANKS;
    insurance => "ms_MY", MS_MY_INSURANCE_COMPANIES;
    currencySymbol => "ms_MY", MS_MY_CURRENCY_SYMBOL;
    commercialBank => "ne_NP", NE_NP_COMMERCIAL_BANKS;
    developmentBank => "ne_NP", NE_NP_DEVELOPMENT_BANKS;
    financeCompany => "ne_NP", NE_NP_FINANCE_COMPANIES;
    digitalWallet => "ne_NP", NE_NP_DIGITAL_WALLETS;
}

macro_rules! jpj_catalog_formatters {
    ($($method:ident => $values:ident),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.require_jpj_locale()?;
                Ok(address_sample(faker_miscellaneous::$values))
            })+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<&'static str> {
                self.try_generate(stringify!($method), || self.0.$method())
            })+
        }
    };
}

jpj_catalog_formatters! {
    peninsularPrefix => PENINSULAR_PREFIX,
    sarawakPrefix => SARAWAK_PREFIX,
    sabahPrefix => SABAH_PREFIX,
    specialPrefix => SPECIAL_PREFIX,
    validAlphabet => VALID_ALPHABETS,
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn jpjNumberPlate(&self) -> DatabaseResult<String> {
        self.require_jpj_locale()?;
        let mut template = address_sample(faker_miscellaneous::JPJ_NUMBER_PLATE_FORMATS);
        let mut plate = String::new();
        while let Some((literal, token)) = template.split_once("{{") {
            plate.push_str(literal);
            let (token, rest) = token.split_once("}}").expect("fixed JPJ plate template");
            if token == "numberSequence" {
                plate.push_str(&self.numberSequence()?.to_string());
            } else {
                let values = match token {
                    "peninsularPrefix" => faker_miscellaneous::PENINSULAR_PREFIX,
                    "sarawakPrefix" => faker_miscellaneous::SARAWAK_PREFIX,
                    "sabahPrefix" => faker_miscellaneous::SABAH_PREFIX,
                    "specialPrefix" => faker_miscellaneous::SPECIAL_PREFIX,
                    "validAlphabet" => faker_miscellaneous::VALID_ALPHABETS,
                    _ => unreachable!("fixed JPJ plate token"),
                };
                plate.push_str(address_sample(values));
            }
            template = rest;
        }
        plate.push_str(template);
        Ok(plate)
    }

    pub fn numberSequence(&self) -> DatabaseResult<i64> {
        self.require_jpj_locale()?;
        Ok(self.numberBetween(1, 9999))
    }

    fn require_jpj_locale(&self) -> DatabaseResult<()> {
        if self.locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker JPJ locale"));
        }
        Ok(())
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn jpjNumberPlate(&self) -> DatabaseResult<String> {
        self.try_generate("jpjNumberPlate", || self.0.jpjNumberPlate())
    }

    pub fn numberSequence(&self) -> DatabaseResult<i64> {
        self.try_generate("numberSequence", || self.0.numberSequence())
    }
}

// Use the same samples for direct calls and full-name templates.
macro_rules! malaysian_person_formatters {
    ($($method:ident),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $method(&self) -> DatabaseResult<String> {
                self.require_malaysian_person()?;
                Ok(south_southeast_asian::malaysian_name_part(stringify!($method)).into_owned())
            })+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $method(&self) -> DatabaseResult<String> {
                self.try_generate(stringify!($method), || self.0.$method())
            })+
        }
    };
}

malaysian_person_formatters! {
    firstNameMaleMalay, firstNameFemaleMalay, lastNameMalay,
    muhammadName, nurName, haji, hajjah, titleMaleMalay,
    lastNameChinese, firstNameMaleChinese, firstNameFemaleChinese,
    firstNameMaleChristian, firstNameFemaleChristian,
    initialIndian, firstNameMaleIndian, firstNameFemaleIndian, lastNameIndian,
}

/// Generates localized sample data for tests and seeders.
pub struct Faker {
    locale: String,
    has_local_address_provider: bool,
    application_timezone: Tz,
    default_timezone: RwLock<Option<Tz>>,
    unique: Mutex<HashMap<&'static str, HashSet<String>>>,
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

fn months_ago(date: NaiveDateTime, months: u32) -> DatabaseResult<NaiveDateTime> {
    // PHP normalizes an overflowing day into the following month.
    date.with_day(1)
        .and_then(|first| first.checked_sub_months(Months::new(months)))
        .and_then(|first| first.checked_add_days(Days::new(u64::from(date.day0()))))
        .ok_or(DATE_RANGE_ERROR)
}

fn resolve_local(zone: Tz, local: NaiveDateTime) -> DatabaseResult<DateTime<Utc>> {
    match zone.from_local_datetime(&local) {
        LocalResult::Single(date) => Ok(date.to_utc()),
        LocalResult::Ambiguous(first, second) => {
            // As in timelib, look up the offset by treating the civil time as UTC.
            let guess = zone.offset_from_utc_datetime(&local).fix();
            Ok(if first.offset().fix() == guess {
                first
            } else {
                second
            }
            .to_utc())
        }
        LocalResult::None => {
            // Apply the pre-transition offset to move forward by the length of the gap.
            let (_, offset) = GapInfo::new(&local, &zone)
                .and_then(|gap| gap.begin)
                .ok_or(DATE_RANGE_ERROR)?;
            local
                .checked_sub_signed(TimeDelta::seconds(i64::from(
                    offset.fix().local_minus_utc(),
                )))
                .map(|date| date.and_utc())
                .ok_or(DATE_RANGE_ERROR)
        }
    }
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
    word => faker::lorem::raw::Word,
}

fn address_sample<T: Copy>(values: &[T]) -> T {
    values[fastrand::usize(..values.len())]
}

macro_rules! zoned_date_formatters {
    ($($zoned:ident => $base:ident($($argument:ident: $kind:ty),*) as $history:literal),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(
                #[doc = concat!("Samples with `", stringify!($base), "` and returns the chosen output time zone.")]
                pub fn $zoned(
                    &self,
                    $($argument: $kind,)*
                    timezone: Option<Tz>,
                ) -> DatabaseResult<DateTime<Tz>> {
                    self.$base($($argument),*)
                        .map(|instant| self.zoned_date(instant, timezone))
                }
            )+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(
                #[doc = concat!("Samples a unique zoned value with `", stringify!($base), "`.")]
                pub fn $zoned(
                    &self,
                    $($argument: $kind,)*
                    timezone: Option<Tz>,
                ) -> DatabaseResult<DateTime<Tz>> {
                    self.generate_zoned($history, || self.0.$zoned($($argument,)* timezone))
                }
            )+
        }
    };
}

zoned_date_formatters! {
    dateTimeBetweenInTimezone => dateTimeBetween(start: DateTime<Utc>, end: DateTime<Utc>) as "dateTimeBetween",
    dateTimeBetweenExpressionsInTimezone => dateTimeBetweenExpressions(start: &str, end: &str) as "dateTimeBetween",
    dateTimeInIntervalInTimezone => dateTimeInInterval(date: Option<DateTime<Utc>>, interval: Option<TimeDelta>) as "dateTimeInInterval",
    dateTimeInIntervalExpressionsInTimezone => dateTimeInIntervalExpressions(date: Option<&str>, interval: Option<&str>) as "dateTimeInInterval",
    dateTimeThisCenturyInTimezone => dateTimeThisCentury(max: Option<DateTime<Utc>>) as "dateTimeThisCentury",
    dateTimeThisDecadeInTimezone => dateTimeThisDecade(max: Option<DateTime<Utc>>) as "dateTimeThisDecade",
    dateTimeThisYearInTimezone => dateTimeThisYear(max: Option<DateTime<Utc>>) as "dateTimeThisYear",
    dateTimeThisMonthInTimezone => dateTimeThisMonth(max: Option<DateTime<Utc>>) as "dateTimeThisMonth",
}

macro_rules! sample_formatters {
    ($($name:ident => $samples:ident),+ $(,)?) => {
        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl Faker {
            $(pub fn $name(&self) -> &'static str {
                let samples = faker_miscellaneous::$samples;
                samples[fastrand::usize(..samples.len())]
            })+
        }

        #[allow(non_snake_case, reason = "Matches Faker formatter names")]
        impl UniqueFaker<'_> {
            $(pub fn $name(&self) -> DatabaseResult<&'static str> {
                self.generate(stringify!($name), || self.0.$name())
            })+
        }
    };
}

sample_formatters! {
    randomLocale => LOCALE_DATA,
    languageCode => LANGUAGE_CODE,
    countryCode => COUNTRY_CODE,
    countryISOAlpha3 => COUNTRY_ISOALPHA3,
    currencyCode => CURRENCY_CODE,
    emoji => EMOJI,
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn bloodType(&self) -> &'static str {
        const TYPES: [&str; 4] = ["A", "AB", "B", "O"];
        TYPES[fastrand::usize(..TYPES.len())]
    }

    pub fn bloodRh(&self) -> &'static str {
        const FACTORS: [&str; 2] = ["+", "-"];
        FACTORS[fastrand::usize(..FACTORS.len())]
    }

    pub fn bloodGroup(&self) -> String {
        format!("{}{}", self.bloodType(), self.bloodRh())
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn bloodType(&self) -> DatabaseResult<&'static str> {
        self.generate("bloodType", || self.0.bloodType())
    }

    pub fn bloodRh(&self) -> DatabaseResult<&'static str> {
        self.generate("bloodRh", || self.0.bloodRh())
    }

    pub fn bloodGroup(&self) -> DatabaseResult<String> {
        self.generate("bloodGroup", || self.0.bloodGroup())
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

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn new(locale: &str) -> DatabaseResult<Self> {
        let locale = canonical_locale(locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        Ok(Self::with_canonical_locale(locale, UTC))
    }

    fn with_canonical_locale(locale: String, application_timezone: Tz) -> Self {
        let has_local_address_provider = faker_address::ADDRESS_LOCALES
            .binary_search(&locale.as_str())
            .is_ok();
        Self {
            locale,
            has_local_address_provider,
            application_timezone,
            default_timezone: RwLock::default(),
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
            return Self::new(locale.unwrap_or("en_US")).map(Arc::new);
        };
        let config = container
            .get::<dyn Repository>("config")
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        let locale = match (locale, config.get("app.faker_locale")) {
            (Some(locale), _) => locale.to_owned(),
            (None, Some(serde_json::Value::String(locale))) => locale,
            (None, None) => "en_US".to_owned(),
            _ => return Err(DatabaseError::InvalidConfiguration("app.faker_locale")),
        };
        let locale = canonical_locale(&locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        let key = format!("{binding}:{locale}");
        if let Ok(faker) = container.get::<Self>(key.as_str()) {
            return Ok(faker);
        }
        let application_timezone = match config.get("app.timezone") {
            None => UTC,
            Some(serde_json::Value::String(zone)) => zone
                .parse()
                .map_err(|_| DatabaseError::InvalidConfiguration("app.timezone"))?,
            _ => return Err(DatabaseError::InvalidConfiguration("app.timezone")),
        };
        container
            .singletonIfNamed(&key, move |_| {
                Ok(Arc::new(Self::with_canonical_locale(
                    locale.clone(),
                    application_timezone,
                )))
            })
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        container
            .get::<Self>(key)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))
    }

    /// Overrides the time zone for this generator. Pass None to restore the application default.
    pub fn setDefaultTimezone(&self, timezone: Option<Tz>) {
        *self.default_timezone.write().unwrap() = timezone;
    }

    pub fn getDefaultTimezone(&self) -> Option<Tz> {
        *self.default_timezone.read().unwrap()
    }

    pub fn locale(&self) -> &str {
        &self.locale
    }

    /// Generates values without repeating previously returned values for each formatter.
    pub fn unique(&self, reset: bool) -> UniqueFaker<'_> {
        self.uniqueWithRetries(reset, 10_000)
    }

    /// Uses a custom attempt limit with the shared formatter history.
    pub fn uniqueWithRetries(&self, reset: bool, maxRetries: usize) -> UniqueFaker<'_> {
        if reset {
            self.unique.lock().unwrap().clear();
        }
        UniqueFaker(self, maxRetries)
    }

    pub fn sentence(&self, words: Range<usize>) -> String {
        generate!(self.locale.as_str(), faker::lorem::raw::Sentence, words)
    }

    pub fn words(&self, count: usize) -> Vec<String> {
        (0..count).map(|_| self.word()).collect()
    }

    pub fn wordsAsText(&self, count: usize) -> String {
        self.words(count).join(" ")
    }

    /// Generates a sentence with a fixed or approximately specified word count.
    pub fn sentenceWithWordCount(&self, count: usize, variable: bool) -> String {
        if count == 0 {
            return String::new();
        }
        let mut words = self.words(lorem_count(count, variable));
        words[0] = capitalize_lorem_word(&words[0]);
        words.join(" ") + "."
    }

    pub fn sentences(&self, count: usize) -> Vec<String> {
        (0..count)
            .map(|_| self.sentenceWithWordCount(6, true))
            .collect()
    }

    pub fn sentencesAsText(&self, count: usize) -> String {
        self.sentences(count).join(" ")
    }

    pub fn paragraph(&self, count: usize, variable: bool) -> String {
        self.sentencesAsText(lorem_count(count, variable))
    }

    pub fn paragraphs(&self, count: usize) -> Vec<String> {
        (0..count).map(|_| self.paragraph(3, true)).collect()
    }

    pub fn paragraphsAsText(&self, count: usize) -> String {
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
                    25..100 => self.sentenceWithWordCount(6, true),
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

    pub fn boolean(&self, chanceOfGettingTrue: Option<i64>) -> bool {
        self.numberBetween(1, 100) <= chanceOfGettingTrue.unwrap_or(50)
    }

    pub fn latitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.randomFloat(Some(6), min.unwrap_or(-90.0), Some(max.unwrap_or(90.0)))
    }

    pub fn longitude(&self, min: Option<f64>, max: Option<f64>) -> DatabaseResult<f64> {
        self.randomFloat(Some(6), min.unwrap_or(-180.0), Some(max.unwrap_or(180.0)))
    }

    pub fn localLatitude(&self) -> DatabaseResult<f64> {
        match self.locale() {
            "zh_TW" => self.latitude(Some(22.0), Some(25.0)),
            _ => self.local_coordinate_latitude(),
        }
    }

    pub fn localLongitude(&self) -> DatabaseResult<f64> {
        match self.locale() {
            "zh_TW" => self.longitude(Some(120.0), Some(122.0)),
            _ => self.local_coordinate_longitude(),
        }
    }

    fn local_coordinate_latitude(&self) -> DatabaseResult<f64> {
        match self.locale() {
            "hu_HU" => self.latitude(Some(46.262740), Some(47.564721)),
            "me_ME" => self.latitude(Some(42.43), Some(42.45)),
            _ => self.latitude(None, None),
        }
    }

    fn local_coordinate_longitude(&self) -> DatabaseResult<f64> {
        match self.locale() {
            "hu_HU" => self.longitude(Some(17.077949), Some(20.604560)),
            "me_ME" => self.longitude(Some(19.16), Some(19.27)),
            _ => self.longitude(None, None),
        }
    }

    pub fn localCoordinates(&self) -> DatabaseResult<Coordinates> {
        Ok(Coordinates {
            latitude: self.local_coordinate_latitude()?,
            longitude: self.local_coordinate_longitude()?,
        })
    }

    fn address_locale(&self) -> &str {
        if self.has_local_address_provider {
            self.locale()
        } else {
            "en_US"
        }
    }

    pub fn city(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.regional_address_format(faker_address::CITY_FORMATS_HU_HU),
            "sr_RS" | "sr_Cyrl_RS" => address_sample(faker_address::CITY_NAMES_SR_RS).into(),
            "sr_Latn_RS" => address_sample(faker_address::CITY_NAMES_SR_LATN_RS).into(),
            "me_ME" => address_sample(faker_address::CITY_NAMES_ME_ME).into(),
            "hy_AM" => address_sample(faker_address::CITY_HY_AM).into(),
            "ka_GE" => self.regional_address_format(faker_address::CITY_FORMATS_KA_GE),
            "lt_LT" => address_sample(faker_address::CITY_LT_LT).into(),
            "lv_LV" => address_sample(faker_address::CITY_LV_LV).into(),
            "hr_HR" => address_sample(faker_address::CITY_NAMES_HR_HR).into(),
            "sl_SI" => address_sample(faker_address::CITY_SL_SI).into(),
            "ro_RO" | "ro_MD" => self.regional_address_format(faker_address::CITY_FORMATS_RO_RO),
            "el_CY" => self.citySuffix(),
            "el_GR" => self.regional_address_format(faker_address::CITY_FORMATS_EL_GR),
            "it_CH" => address_sample(faker_address::CITY_NAMES_IT_CH).into(),
            "pt_PT" => address_sample(faker_address::CITIES_PT_PT).into(),
            "es_ES" => self.regional_address_format(faker_address::CITY_FORMATS_ES_ES),
            "es_AR" | "it_IT" | "pt_BR" => {
                self.regional_address_format(faker_address::CITY_FORMATS_ES_AR)
            }
            "es_PE" => self.regional_address_format(faker_address::CITY_FORMATS_ES_PE),
            "es_VE" => self.regional_address_format(faker_address::CITY_FORMATS_ES_VE),
            "en_UG" => address_sample(faker_address::CITY_NAME_EN_UG).into(),
            "en_IN" => address_sample(faker_address::CITY_EN_IN).into(),
            "en_PH" => address_sample(faker_address::CITY_EN_PH).into(),
            "en_HK" => address_sample(faker_address::CITY_EN_HK).into(),
            "ko_KR" => address_sample(faker_address::CITY_KO_KR).into(),
            "de_DE" => address_sample(faker_address::CITY_DE_DE).into(),
            "de_AT" => address_sample(faker_address::CITY_DE_AT).into(),
            "de_CH" => address_sample(faker_address::CITY_DE_CH).into(),
            "nl_NL" => address_sample(faker_address::CITY_NL_NL).into(),
            "nl_BE" => address_sample(faker_address::CITY_NL_BE).into(),
            "nb_NO" => address_sample(faker_address::CITY_NB_NO).into(),
            "sv_SE" => address_sample(faker_address::CITY_SV_SE).into(),
            "da_DK" => address_sample(faker_address::CITY_DA_DK).into(),
            "is_IS" => address_sample(faker_address::CITY_IS_IS).into(),
            "fi_FI" => match fastrand::u8(0..4) {
                0 => format!(
                    "{}-{}{}",
                    self.cityPrefix(),
                    self.firstName(),
                    self.citySuffix()
                ),
                1 => format!("{}-{}", self.cityPrefix(), self.firstName()),
                2 => format!("{}{}", self.firstName(), self.citySuffix()),
                _ => format!("{}{}", self.lastName(), self.citySuffix()),
            },
            "pl_PL" => address_sample(faker_address::CITY_PL_PL).into(),
            "cs_CZ" => address_sample(faker_address::CITY_CS_CZ).into(),
            "sk_SK" => address_sample(faker_address::CITY_SK_SK).into(),
            "ru_RU" => address_sample(faker_address::CITY_RU_RU).into(),
            "uk_UA" => address_sample(faker_address::CITY_REGIONS_UK_UA).0.into(),
            "kk_KZ" => address_sample(faker_address::CITY_KK_KZ).into(),
            "tr_TR" => address_sample(faker_address::CITY_TR_TR).into(),
            "fa_IR" => {
                let city = address_sample(faker_address::CITY_FA_IR);
                if self.boolean(None) {
                    format!("{} {city}", self.cityPrefix())
                } else {
                    city.into()
                }
            }
            "he_IL" => self.citySuffix(),
            "ar_EG" => address_sample(faker_address::CITY_AR_EG).into(),
            "ar_SA" => address_sample(faker_address::CITY_AR_SA).into(),
            "ar_JO" => {
                let city = address_sample(faker_address::CITY_AR_JO);
                if self.boolean(None) {
                    format!("{} {city}", self.cityPrefix())
                } else {
                    city.into()
                }
            }
            "bn_BD" => format!("{}{}", self.cityPrefix(), self.citySuffix()),
            "ne_NP" => address_sample(faker_address::CITY_NE_NP).into(),
            "id_ID" => address_sample(faker_address::CITY_ID_ID).into(),
            "ms_MY" => address_sample(Self::malaysian_region().towns).into(),
            "vi_VN" => address_sample(faker_address::CITY_VI_VN).into(),
            "ja_JP" => format!("{}{}", self.lastName(), self.citySuffix()),
            "en_NG" | "en_NZ" | "en_SG" => format!("{}{}", self.firstName(), self.citySuffix()),
            "en_US" | "en_CA" | "en_AU" | "en_GB" | "en_ZA" | "th_TH" => match fastrand::u8(0..4) {
                0 => format!(
                    "{} {}{}",
                    self.cityPrefix(),
                    self.firstName(),
                    self.citySuffix()
                ),
                1 => format!("{} {}", self.cityPrefix(), self.firstName()),
                2 => format!("{}{}", self.firstName(), self.citySuffix()),
                _ => format!("{}{}", self.lastName(), self.citySuffix()),
            },
            "fr_BE" => address_sample(faker_address::CITY_FR_BE).into(),
            "fr_CH" => address_sample(faker_address::CITY_FR_CH).into(),
            "fr_CA" => {
                let mut city = format!("{}{}", self.cityPrefix(), self.firstName());
                if self.boolean(None) {
                    city.push_str(&self.citySuffix());
                }
                city
            }
            "fr_FR" => match fastrand::u8(0..8) {
                0..=3 => self.lastName(),
                4..=6 => format!("{}{}", self.lastName(), self.citySuffix()),
                _ => format!("{}-sur-{}", self.lastName(), self.lastName()),
            },
            "zh_CN" => address_sample(faker_address::CITY_ZH_CN).into(),
            "zh_TW" => {
                let (county, districts) = address_sample(faker_address::CITY_ZH_TW);
                format!("{county}{}", address_sample(districts))
            }
            _ => generate!(self.address_locale(), faker::address::raw::CityName,),
        }
    }

    pub fn streetName(&self) -> String {
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
            "hr_HR" => address_sample(faker_address::STREETS_HR_HR).into(),
            "sl_SI" => address_sample(faker_address::STREET_SL_SI).into(),
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
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{name} {}", self.streetSuffix())
            }
            "fr_BE" => format!("{} {}", self.streetSuffix(), self.lastName()),
            "fr_CH" => {
                let prefix = address_sample(faker_address::STREET_PREFIX_FR_CH);
                match fastrand::u8(0..3) {
                    0 => format!("{prefix} {}", self.lastName()),
                    1 => format!("{prefix} de {}", self.city()),
                    _ => format!("{prefix} de {}", self.lastName()),
                }
            }
            "fr_CA" => {
                let name = if self.boolean(None) {
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{} {name}", self.streetSuffix())
            }
            "fr_FR" => {
                let prefix = address_sample(faker_address::STREET_PREFIX_FR_FR);
                match fastrand::u8(0..3) {
                    0 => format!("{prefix} {}", self.lastName()),
                    1 => format!("{prefix} {} {}", self.firstName(), self.lastName()),
                    _ => format!("{prefix} de {}", self.lastName()),
                }
            }
            "en_HK" => Self::hong_kong_name(faker_address::STREET_NAME_FORMATS_EN_HK),
            "ko_KR" => address_sample(faker_address::STREET_NAME_KO_KR).into(),
            "de_DE" | "de_AT" | "de_CH" => self.german_street_name(),
            "nl_NL" | "nl_BE" => format!("{}{}", self.lastName(), self.streetSuffix()),
            "da_DK" => {
                let stem = if self.boolean(None) {
                    self.lastName()
                } else {
                    self.middleName().expect("Danish middle name").into()
                };
                if self.boolean(None) {
                    format!("{stem}{}", self.streetSuffix())
                } else {
                    format!(
                        "{stem} {}",
                        address_sample(faker_address::STREET_SUFFIX_WORD_DA_DK)
                    )
                }
            }
            "fi_FI" => {
                let stem = if self.boolean(None) {
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{stem}{}", self.streetSuffix())
            }
            "is_IS" => {
                let stem = match fastrand::u8(0..4) {
                    0..=1 => address_sample(faker_address::STREET_PREFIX_IS_IS).into(),
                    2 => self.firstNameMale(),
                    _ => self.firstNameFemale(),
                };
                format!("{stem}{}", self.streetSuffix())
            }
            "nb_NO" | "sv_SE" => {
                let suffix = self.streetSuffix();
                match fastrand::u8(0..9) {
                    0..=1 => format!("{}{suffix}", self.lastName()),
                    2..=3 => format!("{}{suffix}", self.firstName()),
                    4..=7 => format!(
                        "{}{suffix}",
                        self.streetPrefix().expect("fixed Nordic street locale")
                    ),
                    _ => format!(
                        "{} {}",
                        self.lastName(),
                        self.streetSuffixWord().expect("fixed Nordic street locale")
                    ),
                }
            }
            "pl_PL" => address_sample(faker_address::STREET_PL_PL).into(),
            "cs_CZ" => address_sample(faker_address::STREET_CS_CZ).into(),
            "sk_SK" => {
                let name = if self.boolean(None) {
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{name} {}", self.streetSuffix())
            }
            "uk_UA" => address_sample(faker_address::STREET_UK_UA).into(),
            "ru_RU" | "kk_KZ" => format!("{} {}", self.lastName(), self.streetSuffix()),
            "tr_TR" => {
                let name = if fastrand::u8(0..3) == 0 {
                    self.lastName()
                } else {
                    self.firstName()
                };
                format!("{name} {}", self.streetSuffix())
            }
            "fa_IR" => format!(
                "{} {}",
                address_sample(faker_address::STREET_PREFIX_FA_IR),
                self.lastName()
            ),
            "he_IL" => {
                let name = if self.boolean(None) {
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{} {name}", self.streetSuffix())
            }
            "ar_EG" | "ar_JO" | "ar_SA" => format!(
                "{} {} {}",
                self.streetPrefix()
                    .expect("fixed Arabic street prefix locale"),
                self.firstName(),
                self.lastName()
            ),
            "bn_BD" => format!(
                "{} {}",
                address_sample(faker_address::STREET_NAMES_BN_BD),
                self.streetSuffix()
            ),
            "ne_NP" => {
                let name = if self.boolean(None) {
                    self.firstName()
                } else {
                    self.lastName()
                };
                format!("{name}{}", self.streetSuffix())
            }
            "id_ID" => address_sample(faker_address::STREET_ID_ID).into(),
            "ms_MY" => self.malaysian_address_name(faker_address::STREET_NAME_FORMATS_MS_MY),
            "vi_VN" => self.vietnamese_address_name(faker_address::STREET_NAME_FORMATS_VI_VN),
            "ja_JP" => format!("{}{}", self.lastName(), self.streetSuffix()),
            "th_TH" => format!(
                "{} {}",
                address_sample(faker_address::STREET_PREFIX_TH_TH),
                address_sample(faker_address::STREET_SUFFIX_TH_TH),
            ),
            "zh_CN" | "en_IN" | "en_NG" | "en_PH" | "en_SG" => {
                format!("{} {}", self.lastName(), self.streetSuffix())
            }
            "zh_TW" => {
                let mut street = format!(
                    "{}{}",
                    address_sample(faker_address::STREET_ZH_TW),
                    self.streetSuffix()
                );
                if self.boolean(None) {
                    write!(street, "{}段", self.taiwan_chinese_digit())
                        .expect("writing to String cannot fail");
                }
                street
            }
            _ => generate!(self.address_locale(), faker::address::raw::StreetName,),
        }
    }

    fn regional_address_format(&self, formats: &[&str]) -> String {
        let mut format = address_sample(formats);
        let mut result = String::with_capacity(format.len());
        while let Some((literal, token)) = format.split_once("{{") {
            result.push_str(literal);
            let (token, remaining) = token
                .split_once("}}")
                .expect("fixed regional address format");
            match token {
                "streetPrefix" => result.push_str(
                    self.streetPrefix()
                        .expect("street prefix supported by address template locale"),
                ),
                "firstName" => result.push_str(&self.firstName()),
                "lastName" => result.push_str(&self.lastName()),
                "smallerCity" => result.push_str(
                    self.smallerCity()
                        .expect("smallerCity supported by address template locale"),
                ),
                "bigCity" => result.push_str(
                    self.bigCity()
                        .expect("bigCity supported by address template locale"),
                ),
                "capital" => result.push_str(
                    self.capital()
                        .expect("capital supported by address template locale"),
                ),
                "regionGenitiveForm" => result.push_str(
                    self.regionGenitiveForm()
                        .expect("region genitive form supported by address template locale"),
                ),
                "regionSuffix" => result.push_str(
                    self.regionSuffix()
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
                    self.apartmentNumber()
                        .expect("apartmentNumber supported by address template locale"),
                ),
                "floor" => result.push_str(
                    self.floor()
                        .expect("floor supported by address template locale"),
                ),
                "blockSegment" => result.push_str(
                    self.blockSegment()
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
                    self.streetPlainName()
                        .expect("streetPlainName supported by address template locale"),
                ),
                "cityName" => result.push_str(
                    self.cityName()
                        .expect("cityName supported by address template locale"),
                ),
                "lastNameMale" => result.push_str(
                    &self
                        .lastNameMale()
                        .expect("male surname supported by address template locale"),
                ),
                "lastNameFemale" => result.push_str(
                    &self
                        .lastNameFemale()
                        .expect("female surname supported by address template locale"),
                ),
                "prefecture" => result.push_str(
                    self.prefecture()
                        .expect("prefecture supported by address template locale"),
                ),
                "cityPrefix" => result.push_str(&self.cityPrefix()),
                "citySuffix" => result.push_str(&self.citySuffix()),
                "streetSuffix" => result.push_str(&self.streetSuffix()),
                "buildingNumber" => result.push_str(&self.buildingNumber()),
                "secondaryAddress" => result.push_str(&self.secondaryAddress()),
                "streetName" => result.push_str(&self.streetName()),
                "streetAddress" => result.push_str(&self.streetAddress()),
                "city" => result.push_str(&self.city()),
                "state" => result.push_str(&self.state()),
                "stateAbbr" => result.push_str(&self.stateAbbr()),
                "postcode" => result.push_str(&self.postcode()),
                _ => unreachable!("fixed regional address token"),
            }
            format = remaining;
        }
        result.push_str(format);
        result
    }

    pub fn cityPrefix(&self) -> String {
        match self.address_locale() {
            "hy_AM" => address_sample(faker_address::CITY_PREFIX_HY_AM).into(),
            "ka_GE" => address_sample(faker_address::CITY_PREFIX_KA_GE).into(),
            "lv_LV" => address_sample(faker_address::CITY_PREFIX_LV_LV).into(),
            "it_IT" | "it_CH" => address_sample(faker_address::CITY_PREFIX_IT_IT).into(),
            "pt_BR" => address_sample(faker_address::CITY_PREFIX_PT_BR).into(),
            "es_ES" => address_sample(faker_address::CITY_PREFIX_ES_ES).into(),
            "es_AR" => address_sample(faker_address::CITY_PREFIX_ES_AR).into(),
            "es_PE" => address_sample(faker_address::CITY_PREFIX_ES_PE).into(),
            "es_VE" => address_sample(faker_address::CITY_PREFIX_ES_VE).into(),
            "fr_CA" => address_sample(faker_address::CITY_PREFIX_FR_CA).into(),
            "th_TH" => address_sample(faker_address::CITY_PREFIX_TH_TH).into(),
            "ru_RU" => "город".into(),
            "uk_UA" => "місто".into(),
            "fa_IR" => address_sample(faker_address::CITY_PREFIX_FA_IR).into(),
            "fi_FI" => address_sample(faker_address::CITY_PREFIX_FI_FI).into(),
            "ar_EG" => address_sample(faker_address::CITY_PREFIX_AR_EG).into(),
            "ar_JO" => address_sample(faker_address::CITY_PREFIX_AR_JO).into(),
            "bn_BD" => address_sample(faker_address::CITY_PREFIX_BN_BD).into(),
            "en_AU" => address_sample(faker_address::CITY_PREFIX_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_GB" | "en_PH" | "en_UG" | "en_ZA" => {
                address_sample(faker_address::CITY_PREFIX_EN_US).into()
            }
            "zh_TW" => String::new(),
            _ => generate!(self.address_locale(), faker::address::raw::CityPrefix,),
        }
    }

    pub fn citySuffix(&self) -> String {
        match self.address_locale() {
            "hu_HU" => address_sample(faker_address::CITY_SUFFIX_HU_HU).into(),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                address_sample(faker_address::CITY_SUFFIX_SR_RS).into()
            }
            "hy_AM" | "ka_GE" => address_sample(faker_address::CITY_SUFFIX_HY_AM).into(),
            "lt_LT" | "lv_LV" => address_sample(faker_address::CITY_SUFFIX_LT_LT).into(),
            "hr_HR" | "sl_SI" => address_sample(faker_address::CITY_SUFFIX_HR_HR).into(),
            "ro_RO" | "ro_MD" => address_sample(faker_address::CITY_SUFFIX_RO_RO).into(),
            "el_GR" => address_sample(faker_address::CITY_SUFFIX_EL_GR).into(),
            "el_CY" => address_sample(faker_address::CITY_SUFFIX_EL_CY).into(),
            "it_IT" | "it_CH" => address_sample(faker_address::CITY_SUFFIX_IT_IT).into(),
            "pt_BR" => address_sample(faker_address::CITY_SUFFIX_PT_BR).into(),
            "pt_PT" => address_sample(faker_address::CITY_SUFFIX_PT_PT).into(),
            "es_ES" => address_sample(faker_address::CITY_SUFFIX_ES_ES).into(),
            "es_AR" => address_sample(faker_address::CITY_SUFFIX_ES_AR).into(),
            "es_PE" => address_sample(faker_address::CITY_SUFFIX_ES_PE).into(),
            "es_VE" => address_sample(faker_address::CITY_SUFFIX_ES_VE).into(),
            "en_ZA" => address_sample(faker_address::CITY_SUFFIX_EN_ZA).into(),
            "en_HK" | "en_IN" | "en_NG" | "en_SG" | "id_ID" | "ko_KR" | "ms_MY" | "ne_NP"
            | "vi_VN" => "Ville".into(),
            "en_GB" => address_sample(faker_address::CITY_SUFFIX_EN_GB).into(),
            "en_NZ" => address_sample(faker_address::CITY_SUFFIX_EN_NZ).into(),
            "en_US" | "en_CA" | "en_AU" | "en_PH" | "en_UG" => {
                address_sample(faker_address::CITY_SUFFIX_EN_US).into()
            }
            "fr_CA" => address_sample(faker_address::CITY_SUFFIX_FR_CA).into(),
            "fr_FR" | "fr_BE" | "fr_CH" => address_sample(faker_address::CITY_SUFFIX_FR_FR).into(),
            "ar_EG" | "ar_JO" | "ar_SA" | "fa_IR" | "ru_RU" | "tr_TR" | "zh_CN" | "pl_PL"
            | "cs_CZ" | "sk_SK" | "de_AT" | "de_CH" | "de_DE" | "nl_NL" | "nl_BE" | "nb_NO"
            | "sv_SE" | "is_IS" => "Ville".into(),
            "da_DK" => address_sample(faker_address::CITY_SUFFIX_DA_DK).into(),
            "fi_FI" => address_sample(faker_address::CITY_SUFFIX_FI_FI).into(),
            "kk_KZ" => "қаласы".into(),
            "ja_JP" => "市".into(),
            "th_TH" => address_sample(faker_address::CITY_SUFFIX_TH_TH).into(),
            "he_IL" => address_sample(faker_address::CITY_HE_IL).into(),
            "bn_BD" => address_sample(faker_address::CITY_SUFFIX_BN_BD).into(),
            "uk_UA" | "zh_TW" => String::new(),
            _ => generate!(self.address_locale(), faker::address::raw::CitySuffix,),
        }
    }

    pub fn streetSuffix(&self) -> String {
        match self.address_locale() {
            "hu_HU" => address_sample(faker_address::STREET_SUFFIX_HU_HU).into(),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                address_sample(faker_address::STREET_SUFFIX_SR_RS).into()
            }
            "hy_AM" => address_sample(faker_address::STREET_SUFFIX_HY_AM).into(),
            "ka_GE" => address_sample(faker_address::STREET_SUFFIX_KA_GE).into(),
            "lt_LT" => address_sample(faker_address::STREET_SUFFIX_LT_LT).into(),
            "lv_LV" => address_sample(faker_address::STREET_SUFFIX_LV_LV).into(),
            "hr_HR" | "sl_SI" => address_sample(faker_address::STREET_SUFFIX_HR_HR).into(),
            "ro_RO" | "ro_MD" => address_sample(faker_address::STREET_SUFFIX_RO_RO).into(),
            "el_GR" => address_sample(faker_address::STREET_SUFFIX_EL_GR).into(),
            "it_IT" | "it_CH" => address_sample(faker_address::STREET_SUFFIX_IT_IT).into(),
            "es_ES" | "pt_BR" | "pt_PT" => {
                address_sample(faker_address::STREET_SUFFIX_ES_ES).into()
            }
            "es_AR" | "es_PE" => address_sample(faker_address::STREET_SUFFIX_ES_AR).into(),
            "es_VE" => address_sample(faker_address::STREET_SUFFIX_ES_VE).into(),
            "en_AU" => address_sample(faker_address::STREET_SUFFIX_EN_AU).into(),
            "en_GB" => address_sample(faker_address::STREET_SUFFIX_EN_GB).into(),
            "en_SG" => address_sample(faker_address::STREET_SUFFIX_EN_SG).into(),
            "en_HK" => address_sample(faker_address::STREET_SUFFIX_EN_HK).into(),
            "en_NZ" => address_sample(faker_address::STREET_SUFFIX_EN_NZ).into(),
            "en_US" | "en_CA" | "en_PH" | "en_UG" | "en_ZA" | "sk_SK" => {
                address_sample(faker_address::STREET_SUFFIX_EN_US).into()
            }
            "fr_BE" => address_sample(faker_address::STREET_SUFFIX_FR_BE).into(),
            "fr_CA" => address_sample(faker_address::STREET_SUFFIX_FR_CA).into(),
            "fr_FR" | "fr_CH" | "zh_CN" | "en_IN" | "en_NG" | "id_ID" | "ko_KR" | "vi_VN"
            | "ar_EG" | "ar_JO" | "ar_SA" | "fa_IR" | "ru_RU" | "pl_PL" | "cs_CZ" | "de_AT"
            | "de_CH" | "de_DE" => "Street".into(),
            "zh_TW" => address_sample(faker_address::STREET_SUFFIX_ZH_TW).into(),
            "ja_JP" => "町".into(),
            "th_TH" => address_sample(faker_address::STREET_SUFFIX_TH_TH).into(),
            "he_IL" => address_sample(faker_address::STREET_SUFFIX_HE_IL).into(),
            "nl_NL" | "nl_BE" => address_sample(faker_address::STREET_SUFFIX_NL_NL).into(),
            "nb_NO" => address_sample(faker_address::STREET_SUFFIX_NB_NO).into(),
            "sv_SE" => address_sample(faker_address::STREET_SUFFIX_SV_SE).into(),
            "da_DK" => address_sample(faker_address::STREET_SUFFIX_DA_DK).into(),
            "fi_FI" => address_sample(faker_address::STREET_SUFFIX_FI_FI).into(),
            "is_IS" => address_sample(faker_address::STREET_SUFFIX_IS_IS).into(),
            "kk_KZ" => address_sample(faker_address::STREET_SUFFIX_KK_KZ).into(),
            "el_CY" | "uk_UA" => String::new(),
            "tr_TR" => address_sample(faker_address::STREET_SUFFIX_TR_TR).into(),
            "bn_BD" => address_sample(faker_address::STREET_SUFFIX_BN_BD).into(),
            "ne_NP" => address_sample(faker_address::STREET_SUFFIX_NE_NP).into(),
            "ms_MY" => address_sample(faker_address::STREET_SUFFIX_MS_MY).into(),
            _ => generate!(self.address_locale(), faker::address::raw::StreetSuffix,),
        }
    }

    pub fn country(&self) -> String {
        match self.address_locale() {
            "hu_HU" => address_sample(faker_address::COUNTRY_HU_HU).into(),
            "sr_RS" | "sr_Cyrl_RS" => address_sample(faker_address::COUNTRY_SR_RS).into(),
            "sr_Latn_RS" => address_sample(faker_address::COUNTRY_SR_LATN_RS).into(),
            "me_ME" => address_sample(faker_address::COUNTRY_ME_ME).into(),
            "hy_AM" => address_sample(faker_address::COUNTRY_HY_AM).into(),
            "ka_GE" => address_sample(faker_address::COUNTRY_KA_GE).into(),
            "lt_LT" => address_sample(faker_address::COUNTRY_LT_LT).into(),
            "lv_LV" => address_sample(faker_address::COUNTRY_LV_LV).into(),
            "hr_HR" => address_sample(faker_address::COUNTRY_HR_HR).into(),
            "sl_SI" => address_sample(faker_address::COUNTRY_SL_SI).into(),
            "ro_RO" | "ro_MD" => address_sample(faker_address::COUNTRY_RO_RO).into(),
            "el_GR" | "el_CY" => address_sample(faker_address::COUNTRY_EL_GR).into(),
            "it_IT" | "it_CH" => address_sample(faker_address::COUNTRY_IT_IT).into(),
            "pt_BR" => address_sample(faker_address::COUNTRY_PT_BR).into(),
            "pt_PT" => address_sample(faker_address::COUNTRY_PT_PT).into(),
            "es_ES" | "es_AR" | "es_PE" | "es_VE" => {
                address_sample(faker_address::COUNTRY_ES_ES).into()
            }
            "en_IN" | "en_NG" => address_sample(faker_address::COUNTRY_EN_NG).into(),
            "en_SG" => "SINGAPORE".into(),
            "en_HK" => "Hong Kong".into(),
            "en_GB" => address_sample(faker_address::COUNTRY_EN_GB).into(),
            "en_US" | "en_CA" | "en_AU" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" | "ne_NP"
            | "vi_VN" => address_sample(faker_address::COUNTRY_EN_US).into(),
            "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA" => {
                address_sample(faker_address::COUNTRY_FR_FR).into()
            }
            "zh_CN" => address_sample(faker_address::COUNTRY_ZH_CN).into(),
            "zh_TW" => address_sample(faker_address::COUNTRY_ZH_TW).into(),
            "ja_JP" => address_sample(faker_address::COUNTRY_JA_JP).into(),
            "th_TH" => address_sample(faker_address::COUNTRY_TH_TH).into(),
            "fa_IR" => address_sample(faker_address::COUNTRY_FA_IR).into(),
            "he_IL" => address_sample(faker_address::COUNTRY_HE_IL).into(),
            "de_DE" | "de_AT" => address_sample(faker_address::COUNTRY_DE_DE_AT).into(),
            "de_CH" => address_sample(faker_address::COUNTRY_DE_CH).into(),
            "nl_NL" => address_sample(faker_address::COUNTRY_NL_NL).into(),
            "nl_BE" => address_sample(faker_address::COUNTRY_NL_BE).into(),
            "nb_NO" => address_sample(faker_address::COUNTRY_NB_NO).into(),
            "sv_SE" => address_sample(faker_address::COUNTRY_SV_SE).into(),
            "da_DK" => address_sample(faker_address::COUNTRY_DA_DK).into(),
            "fi_FI" => address_sample(faker_address::COUNTRY_FI_FI).into(),
            "is_IS" => address_sample(faker_address::COUNTRY_IS_IS).into(),
            "pl_PL" => address_sample(faker_address::COUNTRY_PL_PL).into(),
            "cs_CZ" => address_sample(faker_address::COUNTRY_CS_CZ).into(),
            "sk_SK" => address_sample(faker_address::COUNTRY_SK_SK).into(),
            "ru_RU" => address_sample(faker_address::COUNTRY_RU_RU).into(),
            "uk_UA" => address_sample(faker_address::COUNTRY_UK_UA).into(),
            "kk_KZ" => address_sample(faker_address::COUNTRY_KK_KZ).into(),
            "tr_TR" => address_sample(faker_address::COUNTRY_TR_TR).into(),
            "ar_EG" | "ar_JO" | "ar_SA" => address_sample(faker_address::COUNTRY_AR).into(),
            "bn_BD" => address_sample(faker_address::COUNTRY_BN_BD).into(),
            "id_ID" => address_sample(faker_address::COUNTRY_ID_ID).into(),
            "ms_MY" => address_sample(faker_address::COUNTRY_MS_MY).into(),
            "ko_KR" => address_sample(faker_address::COUNTRY_KO_KR).into(),
            _ => generate!(self.address_locale(), faker::address::raw::CountryName,),
        }
    }

    pub fn state(&self) -> String {
        match self.address_locale() {
            "hu_HU" => address_sample(faker_address::STATE_HU_HU).into(),
            "it_IT" | "it_CH" => address_sample(faker_address::STATE_IT_IT).into(),
            "pt_BR" => address_sample(faker_address::STATE_PT_BR).into(),
            "es_ES" => address_sample(faker_address::STATE_ES_ES).into(),
            "es_AR" => address_sample(faker_address::STATE_ES_AR).into(),
            "es_PE" => address_sample(faker_address::STATE_ES_PE).into(),
            "es_VE" => address_sample(faker_address::STATE_ES_VE).into(),
            "id_ID" => address_sample(faker_address::STATE_ID_ID).into(),
            "bn_BD" => address_sample(faker_address::STATE_BN_BD).into(),
            "de_DE" => address_sample(faker_address::STATE_DE_DE).into(),
            "de_AT" => address_sample(faker_address::STATE_DE_AT).into(),
            "fi_FI" => address_sample(faker_address::STATE_FI_FI).into(),
            "nl_NL" => address_sample(faker_address::STATE_NL_NL).into(),
            "nl_BE" => address_sample(faker_address::STATE_NL_BE).into(),
            "pl_PL" => address_sample(faker_address::STATE_PL_PL).into(),
            "ms_MY" => address_sample(Self::malaysian_region().names).into(),
            "fr_CA" => address_sample(faker_address::STATE_FR_CA).into(),
            "ar_JO" => address_sample(faker_address::STATE_AR_JO).into(),
            "en_IN" => address_sample(faker_address::STATE_EN_IN).into(),
            "en_AU" => address_sample(faker_address::STATE_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" => {
                address_sample(faker_address::STATE_EN_US).into()
            }
            "zh_CN" => address_sample(faker_address::STATE_ZH_CN).into(),
            "zh_TW" => "臺灣省".into(),
            _ => generate!(self.address_locale(), faker::address::raw::StateName,),
        }
    }

    pub fn stateAbbr(&self) -> String {
        match self.address_locale() {
            "it_IT" | "it_CH" => address_sample(faker_address::STATE_ABBR_IT_IT).into(),
            "pt_BR" => address_sample(faker_address::STATE_ABBR_PT_BR).into(),
            "es_AR" => address_sample(faker_address::STATE_ABBR_ES_AR).into(),
            "id_ID" => address_sample(faker_address::STATE_ABBR_ID_ID).into(),
            "fr_CA" => address_sample(faker_address::STATE_ABBR_FR_CA).into(),
            "ar_JO" => address_sample(faker_address::STATE_ABBR_AR_JO).into(),
            "en_IN" => address_sample(faker_address::STATE_ABBR_EN_IN).into(),
            "en_AU" => address_sample(faker_address::STATE_ABBR_EN_AU).into(),
            "en_US" | "en_CA" | "en_NZ" | "en_PH" | "en_UG" | "en_ZA" => {
                address_sample(faker_address::STATE_ABBR_EN_US).into()
            }
            "zh_CN" => address_sample(faker_address::STATE_ABBR_ZH_CN).into(),
            "zh_TW" => "臺".into(),
            _ => generate!(self.address_locale(), faker::address::raw::StateAbbr,),
        }
    }

    /// Returns a district from the Simplified Chinese Address provider.
    pub fn area(&self) -> DatabaseResult<&'static str> {
        if self.locale() == "zh_CN" {
            Ok(address_sample(DISTRICTS_ZH_CN))
        } else {
            Err(DatabaseError::InvalidQuery("unsupported Faker area locale"))
        }
    }

    /// Returns a street stem from the supported regional catalogs.
    pub fn street(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
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
        Ok(address_sample(values))
    }

    /// Samples the autonomous community catalog inherited by the Spanish Address providers.
    pub fn community(&self) -> DatabaseResult<&'static str> {
        if !matches!(self.locale(), "es_ES" | "es_AR" | "es_PE" | "es_VE") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker community locale",
            ));
        }
        Ok(address_sample(faker_address::COMMUNITY_ES_ES))
    }

    pub fn streetPrefix(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
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
        Ok(address_sample(values))
    }

    pub fn streetNumber(&self) -> DatabaseResult<String> {
        if self.locale() == "bn_BD" {
            return Ok(self.bangla_street_number());
        }
        Ok(self.numerify(Some(
            self.singapore_address_value(faker_address::STREET_NUMBER_EN_SG)?,
        )))
    }

    pub fn blockNumber(&self) -> DatabaseResult<String> {
        Ok(self.numerify(Some(
            self.singapore_address_value(faker_address::BLOCK_NUMBER_EN_SG)?,
        )))
    }

    pub fn floorNumber(&self) -> DatabaseResult<&'static str> {
        self.singapore_address_value(faker_address::FLOOR_NUMBER_EN_SG)
    }

    pub fn apartmentNumber(&self) -> DatabaseResult<&'static str> {
        match self.locale() {
            "ro_RO" | "ro_MD" => Ok(Self::romanian_unit_number()),
            _ => self.singapore_address_value(faker_address::APARTMENT_NUMBER_EN_SG),
        }
    }

    pub fn block(&self) -> DatabaseResult<String> {
        self.require_romanian_address_locale()?;
        Ok(self.numerify(Some(address_sample(faker_address::BLOCK_RO_RO))))
    }

    pub fn blockSegment(&self) -> DatabaseResult<&'static str> {
        self.require_romanian_address_locale()?;
        Ok(address_sample(faker_address::BLOCK_SEGMENT_RO_RO))
    }

    pub fn floor(&self) -> DatabaseResult<&'static str> {
        self.require_romanian_address_locale()?;
        Ok(Self::romanian_unit_number())
    }

    pub fn streetPlainName(&self) -> DatabaseResult<&'static str> {
        let names = match self.locale() {
            "ro_RO" => faker_address::STREET_PLAIN_NAME_RO_RO,
            "ro_MD" => faker_address::STREET_PLAIN_NAME_RO_MD,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker Romanian address locale",
                ));
            }
        };
        Ok(address_sample(names))
    }

    fn require_romanian_address_locale(&self) -> DatabaseResult<()> {
        if !matches!(self.locale(), "ro_RO" | "ro_MD") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Romanian address locale",
            ));
        }
        Ok(())
    }

    fn romanian_unit_number() -> &'static str {
        // FakerPHP picks the one- or two-digit template before substituting digits.
        let numbers = &faker_address::ROMANIAN_UNIT_NUMBERS;
        if fastrand::bool() {
            address_sample(&numbers[..10])
        } else {
            address_sample(&numbers[10..])
        }
    }

    pub fn townName(&self) -> DatabaseResult<&'static str> {
        self.singapore_address_value(&["SINGAPORE"])
    }

    fn singapore_address_value(&self, values: &[&'static str]) -> DatabaseResult<&'static str> {
        if self.locale() != "en_SG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Singapore address locale",
            ));
        }
        Ok(address_sample(values))
    }

    pub fn locality(&self) -> DatabaseResult<String> {
        self.require_indian_address_locale()?;
        Ok(self.indian_locality())
    }

    pub fn localityName(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(address_sample(faker_address::LOCALITY_NAME_EN_IN))
    }

    pub fn areaSuffix(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(address_sample(faker_address::AREA_SUFFIX_EN_IN))
    }

    pub fn societyName(&self) -> DatabaseResult<String> {
        self.require_indian_address_locale()?;
        Ok(self.indian_society_name())
    }

    pub fn societySuffix(&self) -> DatabaseResult<&'static str> {
        self.require_indian_address_locale()?;
        Ok(address_sample(faker_address::SOCIETY_SUFFIX_EN_IN))
    }

    fn require_indian_address_locale(&self) -> DatabaseResult<()> {
        if self.locale() != "en_IN" {
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
                self.firstName(),
                address_sample(faker_address::AREA_SUFFIX_EN_IN)
            )
        } else {
            address_sample(faker_address::LOCALITY_NAME_EN_IN).into()
        }
    }

    fn indian_society_name(&self) -> String {
        format!(
            "{} {}",
            self.firstName(),
            address_sample(faker_address::SOCIETY_SUFFIX_EN_IN)
        )
    }

    pub fn municipality(&self) -> DatabaseResult<&'static str> {
        if self.locale() == "lt_LT" {
            return Ok(address_sample(faker_address::MUNICIPALITY_LT_LT));
        }
        self.require_address_locale("en_PH")?;
        Ok(address_sample(faker_address::MUNICIPALITY_EN_PH))
    }

    pub fn barangay(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_PH")?;
        Ok(address_sample(faker_address::BARANGAY_EN_PH))
    }

    pub fn town(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(faker_address::TOWN_FORMATS_EN_HK))
    }

    pub fn village(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(faker_address::VILLAGE_FORMATS_EN_HK))
    }

    pub fn estate(&self) -> DatabaseResult<String> {
        self.require_address_locale("en_HK")?;
        Ok(Self::hong_kong_name(faker_address::ESTATE_FORMATS_EN_HK))
    }

    pub fn syllable(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(address_sample(faker_address::SYLLABLES_EN_HK))
    }

    pub fn direction(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(address_sample(faker_address::DIRECTIONS_EN_HK))
    }

    pub fn englishStreetName(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(address_sample(faker_address::ENGLISH_STREET_NAMES_EN_HK))
    }

    pub fn villageSuffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(address_sample(faker_address::VILLAGE_SUFFIX_EN_HK))
    }

    pub fn estateSuffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("en_HK")?;
        Ok(address_sample(faker_address::ESTATE_SUFFIX_EN_HK))
    }

    fn require_address_locale(&self, locale: &str) -> DatabaseResult<()> {
        if self.locale() != locale {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker address formatter locale",
            ));
        }
        Ok(())
    }

    fn hong_kong_name(formats: &[&str]) -> String {
        let mut name = String::new();
        Self::write_hong_kong_name(&mut name, address_sample(formats));
        name
    }

    fn write_hong_kong_name(name: &mut String, mut format: &str) {
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, remaining) = token
                .split_once("}}")
                .expect("fixed Hong Kong address template");
            name.push_str(literal);
            match token {
                "town" => Self::write_hong_kong_name(
                    name,
                    address_sample(faker_address::TOWN_FORMATS_EN_HK),
                ),
                "syllable" => name.push_str(address_sample(faker_address::SYLLABLES_EN_HK)),
                "direction" => name.push_str(address_sample(faker_address::DIRECTIONS_EN_HK)),
                "englishStreetName" => {
                    name.push_str(address_sample(faker_address::ENGLISH_STREET_NAMES_EN_HK))
                }
                "streetSuffix" => name.push_str(address_sample(faker_address::STREET_SUFFIX_EN_HK)),
                "villageSuffix" => {
                    name.push_str(address_sample(faker_address::VILLAGE_SUFFIX_EN_HK))
                }
                "estateSuffix" => name.push_str(address_sample(faker_address::ESTATE_SUFFIX_EN_HK)),
                _ => unreachable!("unknown fixed Hong Kong address token: {token}"),
            }
            format = remaining;
        }
        name.push_str(format);
    }

    pub fn prefecture(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "el_GR" => faker_address::PREFECTURE_EL_GR,
            "ja_JP" => faker_address::PREFECTURE_JA_JP,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker address formatter locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    pub fn ward(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ja_JP")?;
        Ok(address_sample(faker_address::WARD_JA_JP))
    }

    pub fn postcode1(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(fastrand::i64(faker_address::POSTCODE_PREFIX_JA_JP))
    }

    pub fn postcode2(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(fastrand::i64(faker_address::POSTCODE_SUFFIX_JA_JP))
    }

    pub fn areaNumber(&self) -> DatabaseResult<i64> {
        self.require_address_locale("ja_JP")?;
        Ok(fastrand::i64(faker_address::AREA_NUMBER_JA_JP))
    }

    pub fn metropolitanCity(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ko_KR")?;
        Ok(address_sample(faker_address::METROPOLITAN_CITY_KO_KR))
    }

    pub fn borough(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ko_KR")?;
        Ok(address_sample(faker_address::BOROUGH_KO_KR))
    }

    pub fn hamletName(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::HAMLET_NAME_FORMATS_VI_VN))
    }

    pub fn hamletPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(address_sample(faker_address::HAMLET_PREFIX_VI_VN))
    }

    pub fn wardName(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::WARD_NAME_FORMATS_VI_VN))
    }

    pub fn wardPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(address_sample(faker_address::WARD_PREFIX_VI_VN))
    }

    pub fn districtName(&self) -> DatabaseResult<String> {
        self.require_address_locale("vi_VN")?;
        Ok(self.vietnamese_locality_name(faker_address::DISTRICT_NAME_FORMATS_VI_VN))
    }

    pub fn districtPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("vi_VN")?;
        Ok(address_sample(faker_address::DISTRICT_PREFIX_VI_VN))
    }

    fn vietnamese_locality_name(&self, formats: &[&str]) -> String {
        self.bothify(Some(&self.vietnamese_address_name(formats)))
    }

    fn vietnamese_address_name(&self, formats: &[&str]) -> String {
        let mut format = address_sample(formats);
        let mut name = String::new();
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, rest) = token
                .split_once("}}")
                .expect("fixed Vietnamese address template");
            name.push_str(literal);
            match token {
                "firstName" => name.push_str(&self.firstName()),
                "middleName" => name.push_str(self.middleName().expect("Vietnamese middle name")),
                "lastName" => name.push_str(&self.lastName()),
                "hamletPrefix" => name.push_str(address_sample(faker_address::HAMLET_PREFIX_VI_VN)),
                "wardPrefix" => name.push_str(address_sample(faker_address::WARD_PREFIX_VI_VN)),
                "districtPrefix" => {
                    name.push_str(address_sample(faker_address::DISTRICT_PREFIX_VI_VN))
                }
                _ => unreachable!("unknown fixed Vietnamese address token: {token}"),
            };
            format = rest;
        }
        name.push_str(format);
        name
    }

    pub fn buildingPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(address_sample(faker_address::BUILDING_PREFIX_MS_MY))
    }

    pub fn township(&self) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        Ok(self.malaysian_township())
    }

    pub fn townshipPrefixAbbr(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(address_sample(faker_address::TOWNSHIP_PREFIX_ABBR_MS_MY))
    }

    pub fn townshipPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(address_sample(faker_address::TOWNSHIP_PREFIX_MS_MY))
    }

    pub fn townshipSuffix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ms_MY")?;
        Ok(address_sample(faker_address::TOWNSHIP_SUFFIX_MS_MY))
    }

    pub fn townState(&self) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        Ok(Self::malaysian_town_state())
    }

    /// Generates a Malaysian postcode from a FakerPHP state key, such as `perlis`.
    pub fn postcodeWithState(&self, state: &str) -> DatabaseResult<String> {
        self.require_address_locale("ms_MY")?;
        faker_address::MALAYSIAN_REGIONS
            .iter()
            .find(|region| region.key == state)
            .map(faker_address::MalaysianRegion::postcode)
            .ok_or(DatabaseError::InvalidQuery(
                "unknown Faker Malaysian postcode state",
            ))
    }

    fn malaysian_region() -> &'static faker_address::MalaysianRegion {
        let regions = faker_address::MALAYSIAN_REGIONS;
        &regions[fastrand::usize(..regions.len())]
    }

    fn malaysian_town_state() -> String {
        let region = Self::malaysian_region();
        format!(
            "{} {}, {}",
            region.postcode(),
            address_sample(region.towns),
            address_sample(region.names)
        )
    }

    fn malaysian_township(&self) -> String {
        self.malaysian_address_name(faker_address::TOWNSHIP_FORMATS_MS_MY)
    }

    fn malaysian_address_name(&self, formats: &[&str]) -> String {
        let formatted = self
            .bothify(Some(address_sample(formats)))
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
            name.push_str(address_sample(values));
            template = rest;
        }
        name.push_str(template);
        name
    }

    pub fn banglaStreetName(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("bn_BD")?;
        Ok(address_sample(faker_address::STREET_NAMES_BN_BD))
    }

    pub fn wardNumber(&self) -> DatabaseResult<String> {
        self.require_address_locale("ne_NP")?;
        Ok(self.nepali_ward_number())
    }

    fn bangla_street_number(&self) -> String {
        const DIGITS: [char; 10] = ['০', '১', '২', '৩', '৪', '৫', '৬', '৭', '৮', '৯'];
        self.numberBetween(1, 100)
            .to_string()
            .bytes()
            .map(|digit| DIGITS[usize::from(digit - b'0')])
            .collect()
    }

    fn nepali_ward_number(&self) -> String {
        self.numerify(Some(address_sample(faker_address::WARD_NUMBER_NE_NP)))
    }

    pub fn governorate(&self) -> DatabaseResult<&'static str> {
        match self.locale() {
            "ar_EG" => Ok(address_sample(faker_address::GOVERNORATES_AR_EG).0),
            "ar_SA" => Ok(address_sample(faker_address::GOVERNORATES_AR_SA)),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker governorate locale",
            )),
        }
    }

    pub fn subdivision(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ar_SA")?;
        Ok(address_sample(faker_address::SUBDIVISIONS_AR_SA))
    }

    pub fn buildingNamePrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(address_sample(faker_address::BUILDING_NAME_PREFIX_FA_IR))
    }

    pub fn buildingNumberPrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(address_sample(faker_address::BUILDING_NUMBER_PREFIX_FA_IR))
    }

    pub fn postcodePrefix(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("fa_IR")?;
        Ok(address_sample(faker_address::POSTCODE_PREFIX_FA_IR))
    }

    pub fn building(&self) -> DatabaseResult<String> {
        self.require_address_locale("fa_IR")?;
        Ok(self.persian_building())
    }

    fn persian_building(&self) -> String {
        let mut name = format!(
            "{} {}",
            address_sample(faker_address::BUILDING_NAME_PREFIX_FA_IR),
            self.firstName()
        );
        if self.boolean(None) {
            write!(
                name,
                " {} {}",
                address_sample(faker_address::BUILDING_NUMBER_PREFIX_FA_IR),
                self.buildingNumber()
            )
            .expect("writing to String cannot fail");
        }
        name
    }

    /// Samples a Georgian region name in the genitive form.
    pub fn regionGenitiveForm(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("ka_GE")?;
        Ok(address_sample(faker_address::REGION_GENITIVE_FORM_KA_GE))
    }

    pub fn regionSuffix(&self) -> DatabaseResult<&'static str> {
        match self.locale() {
            "hy_AM" => Ok(address_sample(faker_address::REGION_SUFFIX_HY_AM)),
            "ka_GE" => Ok(address_sample(faker_address::REGION_SUFFIX_KA_GE)),
            "lt_LT" => Ok(address_sample(faker_address::REGION_SUFFIX_LT_LT)),
            "lv_LV" => Ok(address_sample(faker_address::REGION_SUFFIX_LV_LV)),
            "ru_RU" | "uk_UA" => Ok("область"),
            "kk_KZ" => Ok("облысы"),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker region suffix locale",
            )),
        }
    }

    pub fn cityAndRegion(&self) -> DatabaseResult<String> {
        self.require_address_locale("uk_UA")?;
        Ok(Self::ukrainian_city_and_region())
    }

    fn ukrainian_city_and_region() -> String {
        let (city, region) = address_sample(faker_address::CITY_REGIONS_UK_UA);
        format!("{region} область, місто {city}")
    }

    pub fn streetSuffixShort(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "de_DE" => faker_address::STREET_SUFFIX_SHORT_DE_DE,
            "de_AT" => faker_address::STREET_SUFFIX_SHORT_DE_AT,
            "de_CH" => faker_address::STREET_SUFFIX_SHORT_DE_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker short street suffix locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    pub fn streetSuffixLong(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "de_DE" => faker_address::STREET_SUFFIX_LONG_DE_DE,
            "de_AT" => faker_address::STREET_SUFFIX_LONG_DE_AT,
            "de_CH" => faker_address::STREET_SUFFIX_LONG_DE_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker long street suffix locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    fn german_street_name(&self) -> String {
        let swiss = self.locale() == "de_CH";
        match fastrand::u8(0..if swiss { 3 } else { 2 }) {
            0 => format!(
                "{}{}",
                self.lastName(),
                self.streetSuffixShort()
                    .expect("fixed German street locale")
            ),
            1 if swiss => format!(
                "{}{}",
                self.city(),
                self.streetSuffixShort()
                    .expect("fixed German street locale")
            ),
            _ => format!(
                "{}-{}-{}",
                self.firstName(),
                self.lastName(),
                self.streetSuffixLong().expect("fixed German street locale")
            ),
        }
    }

    fn german_building_number(&self, formats: &[&str]) -> String {
        let format = address_sample(formats);
        if let Some(prefix) = format.strip_suffix("[abc]") {
            let mut number = self.numerify(Some(prefix));
            number.push(address_sample(&['a', 'b', 'c']));
            return number;
        }
        self.numerify(Some(format))
    }

    pub fn streetSuffixWord(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "nb_NO" => faker_address::STREET_SUFFIX_WORD_NB_NO,
            "sv_SE" => faker_address::STREET_SUFFIX_WORD_SV_SE,
            "da_DK" => faker_address::STREET_SUFFIX_WORD_DA_DK,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker street suffix word locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    pub fn buildingLevel(&self) -> DatabaseResult<String> {
        self.require_address_locale("da_DK")?;
        Ok(self.danish_building_level())
    }

    pub fn buildingSide(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("da_DK")?;
        Ok(address_sample(faker_address::BUILDING_SIDE_DA_DK))
    }

    pub fn kommune(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("da_DK")?;
        Ok(address_sample(faker_address::KOMMUNE_DA_DK))
    }

    fn danish_building_level(&self) -> String {
        self.numerify(Some(address_sample(faker_address::BUILDING_LEVEL_DA_DK)))
    }

    pub fn capital(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(address_sample(faker_address::CAPITALS_HU_HU))
    }

    pub fn bigCity(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(address_sample(faker_address::BIG_CITIES_HU_HU))
    }

    pub fn smallerCity(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("hu_HU")?;
        Ok(address_sample(faker_address::SMALLER_CITIES_HU_HU))
    }

    pub fn cityName(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
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
        Ok(address_sample(values))
    }

    /// Samples a county from the British Address provider.
    pub fn county(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "ro_RO" => faker_address::COUNTIES_RO_RO,
            "en_GB" => faker_address::COUNTY_EN_GB,
            "en_NG" => faker_address::COUNTY_EN_NG,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker county locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    pub fn province(&self) -> DatabaseResult<&'static str> {
        let provinces = match self.locale() {
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
        Ok(address_sample(provinces))
    }

    pub fn provinceAbbr(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "en_CA" => faker_address::PROVINCE_ABBR_EN_CA,
            "en_ZA" => faker_address::PROVINCE_ABBR_EN_ZA,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker province abbreviation locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    pub fn randomPostcodeLetter(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "en_CA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker postcode letter locale",
            ));
        }
        Ok(address_sample(faker_address::POSTCODE_LETTERS_EN_CA))
    }

    pub fn buildingLetter(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "en_AU" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker building letter locale",
            ));
        }
        Ok(address_sample(faker_address::BUILDING_LETTERS_EN_AU))
    }

    pub fn canton(&self) -> DatabaseResult<(&'static str, &'static str)> {
        let cantons = match self.locale() {
            "it_CH" => faker_address::CANTONS_IT_CH,
            "de_CH" => faker_address::CANTONS_DE_CH,
            "fr_CH" => faker_address::CANTONS_FR_CH,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker canton locale",
                ));
            }
        };
        Ok(address_sample(cantons))
    }

    pub fn cantonShort(&self) -> DatabaseResult<&'static str> {
        self.canton().map(|(code, _)| code)
    }

    pub fn cantonName(&self) -> DatabaseResult<&'static str> {
        self.canton().map(|(_, name)| name)
    }

    pub fn district(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
            "en_UG" => faker_address::DISTRICT_EN_UG,
            "ne_NP" => faker_address::DISTRICT_NE_NP,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker district locale",
                ));
            }
        };
        Ok(address_sample(values))
    }

    /// Samples a Brazilian region abbreviation.
    pub fn regionAbbr(&self) -> DatabaseResult<&'static str> {
        self.require_address_locale("pt_BR")?;
        Ok(address_sample(faker_address::REGION_ABBR_PT_BR))
    }

    pub fn region(&self) -> DatabaseResult<&'static str> {
        let values = match self.locale() {
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
            "uk_UA" => return Ok(address_sample(faker_address::CITY_REGIONS_UK_UA).1),
            _ => None,
        };
        if let Some(values) = values {
            return Ok(address_sample(values));
        }
        if !matches!(self.locale(), "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker region locale",
            ));
        }
        Ok(address_sample(faker_address::REGIONS_FR_FR))
    }

    /// Returns the French department code and name from the same catalog entry.
    pub fn department(&self) -> DatabaseResult<(&'static str, &'static str)> {
        if !matches!(self.locale(), "fr_FR" | "fr_BE" | "fr_CH" | "fr_CA") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker department locale",
            ));
        }
        Ok(address_sample(faker_address::DEPARTMENTS_FR_FR))
    }

    pub fn departmentName(&self) -> DatabaseResult<&'static str> {
        self.department().map(|(_, name)| name)
    }

    pub fn departmentNumber(&self) -> DatabaseResult<&'static str> {
        self.department().map(|(number, _)| number)
    }

    pub fn randomChineseNumber(&self) -> DatabaseResult<&'static str> {
        self.ensure_taiwan_address_locale()?;
        Ok(self.taiwan_chinese_digit())
    }

    pub fn randomNumber2(&self) -> DatabaseResult<u8> {
        self.ensure_taiwan_address_locale()?;
        Ok(fastrand::u8(1..=100))
    }

    pub fn randomNumber3(&self) -> DatabaseResult<u16> {
        self.ensure_taiwan_address_locale()?;
        Ok(fastrand::u16(1..=1000))
    }

    fn ensure_taiwan_address_locale(&self) -> DatabaseResult<()> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Taiwanese number locale",
            ));
        }
        Ok(())
    }

    fn taiwan_chinese_digit(&self) -> &'static str {
        address_sample(&["一", "二", "三", "四", "五", "六", "七", "八", "九"])
    }

    pub fn postcode(&self) -> String {
        match self.address_locale() {
            "hu_HU" => self.numerify(Some(address_sample(faker_address::POSTCODE_HU_HU))),
            "sr_RS" | "sr_Latn_RS" | "sr_Cyrl_RS" | "me_ME" => {
                self.numerify(Some(address_sample(faker_address::POSTCODE_SR_RS)))
            }
            "hy_AM" => self.numerify(Some(address_sample(faker_address::POSTCODE_HY_AM))),
            "ka_GE" => self.numerify(Some(address_sample(faker_address::POSTCODE_KA_GE))),
            "lt_LT" => self.numerify(Some(address_sample(faker_address::POSTCODE_LT_LT))),
            "lv_LV" => self.numerify(Some(address_sample(faker_address::POSTCODE_LV_LV))),
            "hr_HR" => self.numerify(Some(address_sample(faker_address::POSTCODE_HR_HR))),
            "sl_SI" => self.numerify(Some(address_sample(faker_address::POSTCODE_SL_SI))),
            "ro_RO" => self.numerify(Some(address_sample(faker_address::POSTCODE_RO_RO))),
            "ro_MD" => self.numerify(Some(address_sample(faker_address::POSTCODE_RO_MD))),
            "el_GR" => self.numerify(Some(address_sample(faker_address::POSTCODE_EL_GR))),
            "el_CY" => self.numerify(Some(address_sample(faker_address::POSTCODE_EL_CY))),
            "it_IT" => self.numerify(Some(address_sample(faker_address::POSTCODE_IT_IT))),
            "it_CH" => self.numerify(Some(address_sample(faker_address::POSTCODE_IT_CH))),
            "pt_BR" => self.numerify(Some(address_sample(faker_address::POSTCODE_PT_BR))),
            "pt_PT" => self.numerify(Some(address_sample(faker_address::POSTCODE_PT_PT))),
            "es_ES" => self.numerify(Some(address_sample(faker_address::POSTCODE_ES_ES))),
            "es_AR" => self.numerify(Some(address_sample(faker_address::POSTCODE_ES_AR))),
            "es_PE" => self.numerify(Some(address_sample(faker_address::POSTCODE_ES_PE))),
            "es_VE" => self.numerify(Some(address_sample(faker_address::POSTCODE_ES_VE))),
            "de_CH" | "en_PH" | "en_ZA" => self.numerify(Some("####")),
            "de_DE" => self.numerify(Some("#####")),
            "de_AT" => address_sample(faker_address::POSTCODE_DE_AT).into(),
            "nl_NL" => address_sample(faker_address::POSTCODE_NL_NL).into(),
            "nl_BE" => address_sample(faker_address::POSTCODE_NL_BE).into(),
            "nb_NO" => self.numerify(Some(address_sample(faker_address::POSTCODE_NB_NO))),
            "sv_SE" => self.numerify(Some(address_sample(faker_address::POSTCODE_SV_SE))),
            "da_DK" => self.numerify(Some(address_sample(faker_address::POSTCODE_DA_DK))),
            "fi_FI" => self.numerify(Some(address_sample(faker_address::POSTCODE_FI_FI))),
            "is_IS" => self.numerify(Some(address_sample(faker_address::POSTCODE_IS_IS))),
            "en_SG" => self.numerify(Some("######")),
            "en_IN" => self.numerify(Some(address_sample(faker_address::POSTCODE_EN_IN))),
            "en_UG" => self.numerify(Some(address_sample(&["#", "##", "###", "####", "#####"]))),
            "en_NG" => self.numerify(Some(address_sample(&["#####", "## ###"]))),
            "en_GB" => address_sample(faker_address::POSTCODE_EN_GB).into(),
            "en_CA" => {
                let pattern = address_sample(&["?#? #?#", "?#?-#?#", "?#?#?#"]);
                let mut postcode = String::with_capacity(pattern.len());
                for character in pattern.chars() {
                    match character {
                        '?' => {
                            postcode.push_str(address_sample(faker_address::POSTCODE_LETTERS_EN_CA))
                        }
                        '#' => postcode.push(char::from(b'0' + self.randomDigit())),
                        literal => postcode.push(literal),
                    }
                }
                postcode
            }
            "en_AU" => self.numerify(Some(address_sample(faker_address::POSTCODE_EN_AU))),
            "en_NZ" => self.numerify(Some("####")),
            "en_US" => {
                let format = if self.boolean(None) {
                    "#####"
                } else {
                    "#####-####"
                };
                self.numerify(Some(format))
            }
            "zh_CN" => format!("{:02}{}00", fastrand::u8(1..=85), fastrand::u8(10..=88)),
            "zh_TW" => self.numerify(Some(address_sample(&["###-##", "###"]))),
            "fr_BE" | "fr_CH" => self.numerify(Some("####")),
            "fr_CA" => self
                .bothify(Some(address_sample(&["?#? #?#", "?#?#?#"])))
                .to_ascii_uppercase(),
            "en_HK" | "fr_FR" | "ne_NP" | "th_TH" | "tr_TR" | "uk_UA" => {
                self.numerify(Some("#####"))
            }
            "ru_RU" => self.numerify(Some("######")),
            "cs_CZ" => self.numerify(Some(address_sample(faker_address::POSTCODE_CS_CZ))),
            "pl_PL" => self.numerify(Some(address_sample(faker_address::POSTCODE_PL_PL))),
            "sk_SK" => self.numerify(Some("### ##")),
            "kk_KZ" => self.numerify(Some("0#####")),
            "bn_BD" => self.numerify(Some(address_sample(faker_address::POSTCODE_BN_BD))),
            "fa_IR" => self.numerify(Some(address_sample(faker_address::POSTCODE_FA_IR))),
            "he_IL" => self.numerify(Some(address_sample(faker_address::POSTCODE_HE_IL))),
            "ar_EG" | "ar_JO" | "ar_SA" => {
                self.numerify(Some(address_sample(faker_address::POSTCODE_AR)))
            }
            "id_ID" => self.numerify(Some("%####")),
            "ms_MY" => Self::malaysian_region().postcode(),
            "ko_KR" => self.numerify(Some(address_sample(faker_address::POSTCODE_KO_KR))),
            "vi_VN" => self.numerify(Some(address_sample(faker_address::POSTCODE_VI_VN))),
            "ja_JP" => format!(
                "{}{}",
                fastrand::i64(faker_address::POSTCODE_PREFIX_JA_JP),
                fastrand::i64(faker_address::POSTCODE_SUFFIX_JA_JP)
            ),
            _ => generate!(self.address_locale(), faker::address::raw::PostCode,),
        }
    }

    pub fn buildingNumber(&self) -> String {
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
                    .bothify(Some(address_sample(faker_address::BUILDING_NUMBER_DA_DK)))
                    .to_ascii_uppercase();
            }
            "nb_NO" | "sv_SE" => {
                return self
                    .bothify(Some(address_sample(faker_address::BUILDING_NUMBER_NB_NO)))
                    .to_ascii_uppercase();
            }
            "nl_NL" => {
                return self.bothify(Some(address_sample(faker_address::BUILDING_NUMBER_NL_NL)));
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
            "id_ID" => return self.numberBetween(1, 999).to_string(),
            "ms_MY" => {
                return self
                    .bothify(Some(address_sample(faker_address::BUILDING_NUMBER_MS_MY)))
                    .to_ascii_uppercase();
            }
            "ja_JP" => return self.numberBetween(101, 110).to_string(),
            _ => return generate!(self.address_locale(), faker::address::raw::BuildingNumber,),
        };
        self.numerify(Some(formats[fastrand::usize(..formats.len())]))
    }

    pub fn secondaryAddress(&self) -> String {
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
                return self.bothify(Some(address_sample(
                    faker_address::SECONDARY_ADDRESS_FORMATS_EN_GB,
                )));
            }
            "ja_JP" => {
                let prefix = address_sample(&["ハイツ", "コーポ"]);
                return format!("{prefix}{}{}号", self.lastName(), self.buildingNumber());
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
                    fastrand::u8(1..=100),
                    if self.boolean(None) { "室" } else { "房" }
                );
            }
            _ => return generate!(self.address_locale(), faker::address::raw::SecondaryAddress,),
        };
        self.numerify(Some(formats[fastrand::usize(..formats.len())]))
    }

    pub fn streetAddress(&self) -> String {
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
                let mut street = format!("{} {}", self.streetName(), self.buildingNumber());
                let layout = fastrand::u8(0..3);
                if layout > 0 {
                    write!(street, ", {}", self.danish_building_level())
                        .expect("writing to String cannot fail");
                }
                if layout == 2 {
                    write!(
                        street,
                        " {}",
                        address_sample(faker_address::BUILDING_SIDE_DA_DK)
                    )
                    .expect("writing to String cannot fail");
                }
                return street;
            }
            "fi_FI" => {
                let mut street = format!("{} {}", self.streetName(), self.buildingNumber());
                if self.boolean(None) {
                    write!(street, " {}", self.secondaryAddress())
                        .expect("writing to String cannot fail");
                }
                return street;
            }
            "ru_RU" => {
                return format!(
                    "{} {}, {}",
                    address_sample(faker_address::STREET_PREFIX_RU_RU),
                    address_sample(faker_address::STREET_RU_RU),
                    self.buildingNumber(),
                );
            }
            "uk_UA" => {
                return format!(
                    "{} {}, {}",
                    address_sample(faker_address::STREET_PREFIX_UK_UA),
                    self.streetName(),
                    self.buildingNumber(),
                );
            }
            "kk_KZ" => {
                return format!(
                    "{} {}, {}",
                    address_sample(faker_address::STREET_KK_KZ),
                    self.streetSuffix(),
                    self.buildingNumber(),
                );
            }
            "fa_IR" => return format!("{} {}", self.streetName(), self.persian_building()),
            "he_IL" => {
                let mut street = format!("{} {} ", self.streetName(), self.buildingNumber());
                if self.boolean(None) {
                    street.push_str(&self.secondaryAddress());
                }
                return street;
            }
            "bn_BD" => return format!("{} {}", self.bangla_street_number(), self.streetName()),
            "ne_NP" => return self.streetName(),
            "id_ID" => {
                return format!(
                    "{} {} No. {}",
                    address_sample(faker_address::STREET_PREFIX_ID_ID),
                    self.streetName(),
                    self.buildingNumber(),
                );
            }
            "ms_MY" => {
                return format!(
                    "{}{}, {}",
                    address_sample(faker_address::BUILDING_PREFIX_MS_MY),
                    self.buildingNumber(),
                    self.streetName(),
                );
            }
            _ => {}
        }
        if self.address_locale() == "en_HK" {
            let number = self.buildingNumber();
            return match fastrand::u8(0..3) {
                0 => format!("{number} {}", self.streetName()),
                1 => format!(
                    "{number} {}",
                    Self::hong_kong_name(faker_address::VILLAGE_FORMATS_EN_HK)
                ),
                _ => format!(
                    "Block {number}, {}",
                    Self::hong_kong_name(faker_address::ESTATE_FORMATS_EN_HK)
                ),
            };
        }
        if self.address_locale() == "en_IN" {
            let number = self.buildingNumber();
            let locality = self.indian_locality();
            return if self.boolean(None) {
                format!("{number}, {locality},")
            } else {
                format!("{number}, {}, {locality}", self.indian_society_name())
            };
        }
        if self.address_locale() == "en_SG" {
            let street = self.streetName();
            return if self.boolean(None) {
                format!("Jalan {street}")
            } else {
                format!("{street} {}", self.streetSuffix())
            };
        }
        let street = self.streetName();
        match self.address_locale() {
            "ja_JP" => format!(
                "{street}{}{}-{}-{}",
                self.lastName(),
                fastrand::i64(faker_address::AREA_NUMBER_JA_JP),
                fastrand::i64(faker_address::AREA_NUMBER_JA_JP),
                fastrand::i64(faker_address::AREA_NUMBER_JA_JP),
            ),
            "en_GB" if fastrand::u8(0..3) == 0 => format!("{}\n{street}", self.secondaryAddress()),
            "en_GB" | "en_NZ" => format!("{} {street}", self.buildingNumber()),
            "en_AU" => {
                let number = self.buildingNumber();
                match fastrand::u8(0..3) {
                    0 => format!("{number} {street}"),
                    1 => format!(
                        "{number}{} {street}",
                        address_sample(faker_address::BUILDING_LETTERS_EN_AU)
                    ),
                    _ => format!("{} {number} {street}", self.secondaryAddress()),
                }
            }
            "en_US" | "en_CA" | "fr_CA" | "en_PH" | "en_UG" | "en_ZA" | "ar_EG" | "ar_JO"
            | "ar_SA" => {
                let mut address = format!("{} {street}", self.buildingNumber());
                if self.boolean(None) {
                    write!(address, " {}", self.secondaryAddress())
                        .expect("writing to String cannot fail");
                }
                address
            }
            "cs_CZ" if fastrand::u8(0..5) == 0 => street,
            "cs_CZ" | "de_AT" | "de_CH" | "de_DE" | "fr_BE" | "fr_CH" | "pl_PL" | "sk_SK"
            | "tr_TR" | "nl_NL" | "nl_BE" | "nb_NO" | "sv_SE" | "is_IS" => {
                format!("{street} {}", self.buildingNumber())
            }
            "fr_FR" if fastrand::u8(0..6) == 0 => street,
            "fr_FR" => format!("{}, {street}", self.buildingNumber()),
            "zh_TW" => {
                let mut address = street;
                let format = fastrand::u8(0..6);
                if format >= 2 {
                    write!(address, "{}巷", fastrand::u16(1..=1000))
                        .expect("writing to String cannot fail");
                }
                if format >= 4 {
                    write!(address, "{}弄", fastrand::u16(1..=1000))
                        .expect("writing to String cannot fail");
                }
                write!(address, "{}號", fastrand::u16(1..=1000))
                    .expect("writing to String cannot fail");
                if format % 2 == 1 {
                    write!(address, "{}樓", fastrand::u8(1..=100))
                        .expect("writing to String cannot fail");
                }
                address
            }
            _ => format!("{} {street}", self.buildingNumber()),
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
                    self.streetAddress(),
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
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            "de_AT" | "de_CH" | "de_DE" | "nl_NL" | "nb_NO" | "sv_SE" | "da_DK" | "is_IS" => {
                format!(
                    "{}\n{} {}",
                    self.streetAddress(),
                    self.postcode(),
                    self.city()
                )
            }
            "pl_PL" => format!(
                "{}, {} {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            "cs_CZ" => {
                let street = self.streetAddress();
                let postcode = self.postcode();
                let city = self.city();
                match fastrand::u8(0..7) {
                    0 => {
                        let region = address_sample(faker_address::REGION_CS_CZ);
                        format!("{street}\n{region}\n{postcode} {city}")
                    }
                    1 => format!("{street}\n{postcode} {city}\nČeská republika"),
                    _ => format!("{street}\n{postcode} {city}"),
                }
            }
            // FakerPHP uses a single-quoted template here, so its separators are literal.
            "sk_SK" => format!(
                r"{}\n {}\n {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            "ru_RU" => format!(
                "{}, {} область, город {}, {}",
                self.postcode(),
                address_sample(faker_address::REGION_RU_RU),
                self.city(),
                self.streetAddress(),
            ),
            "uk_UA" => format!(
                "{}, {}, {}",
                self.postcode(),
                Self::ukrainian_city_and_region(),
                self.streetAddress()
            ),
            "kk_KZ" => format!(
                "{}, {} облысы, {} қаласы, {}",
                self.postcode(),
                address_sample(faker_address::REGION_KK_KZ),
                self.city(),
                self.streetAddress(),
            ),
            "tr_TR" => format!(
                "{}\n{} {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            "fa_IR" => {
                let mut address = format!("{} {}", self.city(), self.streetAddress());
                if self.boolean(None) {
                    write!(
                        address,
                        " {} {}",
                        address_sample(faker_address::POSTCODE_PREFIX_FA_IR),
                        self.postcode()
                    )
                    .expect("writing to String cannot fail");
                }
                address
            }
            "he_IL" => format!(
                "{}\n{}, {}",
                self.streetAddress(),
                self.city(),
                self.postcode()
            ),
            "ar_EG" | "ar_JO" | "ar_SA" => format!("{}\n{}", self.streetAddress(), self.city()),
            "bn_BD" => format!("{}, {} {}", self.streetAddress(), self.city(), self.state()),
            "ne_NP" => format!(
                "{}-{}, {}, {} {}",
                self.city(),
                self.nepali_ward_number(),
                self.streetAddress(),
                address_sample(faker_address::DISTRICT_NE_NP),
                self.postcode(),
            ),
            "id_ID" => format!(
                "{}, {} {}, {}",
                self.streetAddress(),
                self.city(),
                self.postcode(),
                self.stateAbbr()
            ),
            "ms_MY" => format!(
                "{}, {}, {}",
                self.streetAddress(),
                self.malaysian_township(),
                Self::malaysian_town_state()
            ),
            "th_TH" => format!(
                "{}\n{}, {}",
                self.streetAddress(),
                self.city(),
                self.postcode()
            ),
            "vi_VN" => {
                let (street, region) = match fastrand::u8(0..3) {
                    0 => (
                        self.streetAddress(),
                        address_sample(faker_address::CITY_VI_VN),
                    ),
                    1 => (
                        self.streetAddress(),
                        address_sample(faker_address::PROVINCE_VI_VN),
                    ),
                    _ => (
                        format!(
                            "{}, {}",
                            self.buildingNumber(),
                            self.vietnamese_locality_name(faker_address::HAMLET_NAME_FORMATS_VI_VN)
                        ),
                        address_sample(faker_address::PROVINCE_VI_VN),
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
                        address_sample(faker_address::METROPOLITAN_CITY_KO_KR),
                        faker_address::BOROUGH_KO_KR,
                    )
                } else {
                    (
                        address_sample(faker_address::PROVINCE_KO_KR),
                        faker_address::CITY_KO_KR,
                    )
                };
                format!(
                    "{region} {} {} {}",
                    address_sample(locality),
                    self.streetName(),
                    self.buildingNumber(),
                )
            }
            "ja_JP" => {
                let mut address = format!(
                    "{}  {}{}{}{}",
                    self.postcode(),
                    address_sample(faker_address::PREFECTURE_JA_JP),
                    self.city(),
                    address_sample(faker_address::WARD_JA_JP),
                    self.streetAddress(),
                );
                if self.boolean(None) {
                    write!(address, " {}", self.secondaryAddress())
                        .expect("writing to String cannot fail");
                }
                address
            }
            "en_HK" => format!(
                "{}\n{}\n{}",
                self.streetAddress(),
                Self::hong_kong_name(faker_address::TOWN_FORMATS_EN_HK),
                self.city()
            ),
            "en_PH" => {
                let street = self.streetAddress();
                let postcode = self.postcode();
                let province = address_sample(faker_address::PROVINCE_EN_PH);
                if self.boolean(None) {
                    let barangay = address_sample(faker_address::BARANGAY_EN_PH);
                    format!(
                        "{street}, {barangay}, {} {postcode} {province}",
                        self.city()
                    )
                } else {
                    let municipality = address_sample(faker_address::MUNICIPALITY_EN_PH);
                    format!("{street}, {municipality} {postcode} {province}")
                }
            }
            "en_IN" => format!(
                "{} {} - {}",
                self.streetAddress(),
                self.city(),
                self.postcode()
            ),
            "en_SG" => {
                let street = self.streetAddress();
                let postcode = self.postcode();
                if self.boolean(None) {
                    let number =
                        self.numerify(Some(address_sample(faker_address::STREET_NUMBER_EN_SG)));
                    format!("{number} {street}\nSINGAPORE {postcode}")
                } else {
                    let block =
                        self.numerify(Some(address_sample(faker_address::BLOCK_NUMBER_EN_SG)));
                    let floor = address_sample(faker_address::FLOOR_NUMBER_EN_SG);
                    let apartment = address_sample(faker_address::APARTMENT_NUMBER_EN_SG);
                    format!("{block} {street}\n{floor} {apartment}\nSINGAPORE {postcode}")
                }
            }
            "en_ZA" => format!(
                "{}\n{}, {} {}",
                self.streetAddress(),
                self.city(),
                address_sample(faker_address::PROVINCE_ABBR_EN_ZA),
                self.postcode()
            ),
            "en_GB" => format!(
                "{}\n{}\n{}",
                self.streetAddress(),
                self.city(),
                self.postcode()
            ),
            "en_CA" => format!(
                "{}\n{}, {}  {}",
                self.streetAddress(),
                self.city(),
                address_sample(faker_address::PROVINCE_ABBR_EN_CA),
                self.postcode()
            ),
            "en_NZ" => format!(
                "{}, {}, {}, {}",
                self.streetAddress(),
                self.city(),
                address_sample(faker_address::REGION_EN_NZ),
                self.postcode()
            ),
            "en_US" | "en_AU" | "en_UG" => format!(
                "{}\n{}, {} {}",
                self.streetAddress(),
                self.city(),
                self.stateAbbr(),
                self.postcode()
            ),
            "zh_CN" => {
                let districts = DISTRICTS_ZH_CN;
                format!(
                    "{}{}",
                    self.city(),
                    districts[fastrand::usize(..districts.len())]
                )
            }
            "zh_TW" => format!(
                "{} {}{}",
                self.postcode(),
                self.city(),
                self.streetAddress()
            ),
            "fr_BE" => format!(
                "{}\n {} {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            "fr_CA" => format!(
                "{}, {}, {} {}",
                self.streetAddress(),
                self.city(),
                self.stateAbbr(),
                self.postcode()
            ),
            "fr_FR" | "fr_CH" => format!(
                "{}\n{} {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
            _ => format!(
                "{} {} {}",
                self.streetAddress(),
                self.postcode(),
                self.city()
            ),
        }
    }

    pub fn name(&self) -> String {
        self.nameWithGender(None)
    }

    pub fn firstName(&self) -> String {
        self.firstNameWithGender(None)
    }

    pub fn lastName(&self) -> String {
        if self.locale() == "nl_NL" {
            return company::dutch::dutch_surname().to_owned();
        }
        match self.locale() {
            "ru_RU" => return self.russian_surname(fastrand::bool()),
            "is_IS" => return company::nordic::icelandic_surname(),
            // The pinned Lithuanian no-argument surname template samples given names.
            "lt_LT" => return self.firstNameWithGender(None),
            "ms_MY" => return south_southeast_asian::malaysian_last_name().to_owned(),
            "pl_PL" | "cs_CZ" | "sk_SK" | "el_GR" | "bg_BG" | "id_ID" => {
                return self
                    .gendered_surname(fastrand::bool())
                    .expect("fixed gendered surname locale");
            }
            _ => {}
        }
        let surnames = faker_person::surnames(faker_person::provider_locale(self.locale()))
            .expect("fixed Faker surname catalog");
        surnames[fastrand::usize(..surnames.len())].to_owned()
    }

    pub fn middleName(&self) -> DatabaseResult<&'static str> {
        if matches!(self.locale(), "ru_RU" | "uk_UA") {
            return self.gendered_middle_name(fastrand::bool());
        }
        let names = match self.locale() {
            "da_DK" => company::nordic::DA_DK_MIDDLE,
            "is_IS" => company::nordic::IS_IS_MIDDLE,
            // The pinned Vietnamese no-argument middle-name template samples given names.
            "vi_VN" => {
                let catalog = faker_person::catalog(self.locale());
                if fastrand::bool() {
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
        Ok(names[fastrand::usize(..names.len())])
    }

    pub fn middleNameWithGender(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        match gender {
            Some("male") => self.middleNameMale(),
            Some("female") => self.middleNameFemale(),
            _ => self.middleName(),
        }
    }

    pub fn middleNameMale(&self) -> DatabaseResult<&'static str> {
        self.gendered_middle_name(true)
    }

    pub fn middleNameFemale(&self) -> DatabaseResult<&'static str> {
        self.gendered_middle_name(false)
    }

    fn gendered_middle_name(&self, male: bool) -> DatabaseResult<&'static str> {
        let names = match (self.locale(), male) {
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
        Ok(names[fastrand::usize(..names.len())])
    }

    fn russian_surname(&self, male: bool) -> String {
        let names = faker_person::RU_RU_SURNAMES;
        let name = names[fastrand::usize(..names.len())];
        if male {
            name.to_owned()
        } else {
            format!("{name}а")
        }
    }

    pub fn lastNameWithGender(&self, gender: Option<&str>) -> String {
        if matches!(
            self.locale(),
            "lt_LT" | "pl_PL" | "cs_CZ" | "sk_SK" | "el_GR" | "bg_BG" | "ru_RU" | "id_ID" | "en_UG"
        ) {
            match gender {
                Some("male") => return self.lastNameMale().expect("fixed male surname locale"),
                Some("female") => {
                    return self.lastNameFemale().expect("fixed female surname locale");
                }
                _ => {}
            }
        }
        self.lastName()
    }

    pub fn lastNameMale(&self) -> DatabaseResult<String> {
        self.gendered_surname(true)
    }

    pub fn lastNameFemale(&self) -> DatabaseResult<String> {
        self.gendered_surname(false)
    }

    fn gendered_surname(&self, male: bool) -> DatabaseResult<String> {
        let names = match (self.locale(), male) {
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
                return Ok(format!("{}{ending}", self.lastName()));
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker gendered surname locale",
                ));
            }
        };
        Ok(names[fastrand::usize(..names.len())].to_owned())
    }

    pub fn firstNameMaleNe(&self) -> DatabaseResult<&'static str> {
        self.hungarian_married_name(faker_person::HU_MARRIED_FIRST_NAMES)
    }

    pub fn lastNameFemaleMarried(&self) -> DatabaseResult<&'static str> {
        self.hungarian_married_name(faker_person::HU_MARRIED_SURNAMES)
    }

    fn hungarian_married_name(
        &self,
        names: &'static [&'static str],
    ) -> DatabaseResult<&'static str> {
        if self.locale() != "hu_HU" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker married name locale",
            ));
        }
        Ok(names[fastrand::usize(..names.len())])
    }

    pub fn firstNameMale(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.locale()).male)
    }

    pub fn firstNameFemale(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.locale()).female)
    }

    fn require_malaysian_person(&self) -> DatabaseResult<()> {
        if self.locale() != "ms_MY" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Malaysian Person locale",
            ));
        }
        Ok(())
    }

    pub fn myKadNumber(
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
        let year = self.numberBetween(0, 99);
        // The upstream month and day are sampled separately.
        let month = self.dateTimeInTimezone(None, None).month();
        let day = self.dateTimeInTimezone(None, None).day();
        let mut place = self.numberBetween(1, 55);
        if place >= 17 {
            place += 4;
        }
        let serial = self.numberBetween(0, 999);
        let digit = self.numberBetween(0, 9);
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
        dateOfBirth: Option<&str>,
        county: Option<&str>,
        isResident: bool,
    ) -> DatabaseResult<String> {
        if self.locale() != "ro_RO" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker CNP locale"));
        }
        romanian_serbian::cnp(gender, dateOfBirth, county, isResident)
    }

    pub fn firstNameWithGender(&self, gender: Option<&str>) -> String {
        match gender {
            Some("male") => self.firstNameMale(),
            Some("female") => self.firstNameFemale(),
            _ if self.locale() == "ms_MY" => south_southeast_asian::malaysian_first_name(),
            _ if self.boolean(None) => self.firstNameMale(),
            _ => self.firstNameFemale(),
        }
    }

    pub fn nameWithGender(&self, gender: Option<&str>) -> String {
        let catalog = faker_person::catalog(self.locale());
        let formats = match gender {
            Some("male") => catalog.male_formats,
            Some("female") => catalog.female_formats,
            _ => {
                let index =
                    fastrand::usize(..catalog.male_formats.len() + catalog.female_formats.len());
                if let Some(format) = catalog.male_formats.get(index) {
                    return self.render_person_name(format, catalog);
                }
                return self.render_person_name(
                    catalog.female_formats[index - catalog.male_formats.len()],
                    catalog,
                );
            }
        };
        let format = formats[fastrand::usize(..formats.len())];
        self.render_person_name(format, catalog)
    }

    /// Concatenates independent draws from a Taiwanese name pool.
    pub fn randomName(&self, pool: &[&str], count: usize) -> DatabaseResult<String> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker random name locale",
            ));
        }
        Ok((0..count)
            .map_while(|_| self.randomElement(pool).copied())
            .collect())
    }

    fn sample_given_name(&self, names: &[&str]) -> String {
        if self.locale() == "zh_TW" {
            return self
                .randomName(names, fastrand::usize(1..=2))
                .expect("fixed random name locale");
        }
        names[fastrand::usize(..names.len())].to_owned()
    }

    fn render_person_name(&self, mut format: &str, catalog: &faker_person::PersonNames) -> String {
        let mut name = String::new();
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, remaining) = token.split_once("}}").expect("fixed Faker name template");
            name.push_str(literal);
            match token {
                "firstNameMale" => name.push_str(&self.firstNameMale()),
                "firstNameFemale" => name.push_str(&self.firstNameFemale()),
                "middleNameFemale" => name.push_str(
                    self.middleNameFemale()
                        .expect("fixed gendered middle-name locale"),
                ),
                "middleNameMale" => name.push_str(
                    self.middleNameMale()
                        .expect("fixed gendered middle-name locale"),
                ),
                "middleName" => name.push_str(self.middleName().expect("fixed middle-name locale")),
                "lastNameMale" => {
                    name.push_str(&self.lastNameMale().expect("fixed male surname locale"))
                }
                "lastNameFemale" => {
                    name.push_str(&self.lastNameFemale().expect("fixed female surname locale"))
                }
                "firstNameMaleNe" => {
                    name.push_str(self.firstNameMaleNe().expect("fixed married name locale"))
                }
                "lastNameFemaleMarried" => name.push_str(
                    self.lastNameFemaleMarried()
                        .expect("fixed married name locale"),
                ),
                "lastName" => name.push_str(&self.lastName()),
                "titleMale" => name.push_str(self.titleMale()),
                "titleFemale" => name.push_str(self.titleFemale()),
                "title" => name.push_str(self.title(None)),
                "namePrefix" => name.push(self.namePrefix().expect("fixed name-prefix locale")),
                "prefix" => {
                    name.push_str(catalog.prefixes[fastrand::usize(..catalog.prefixes.len())])
                }
                "suffix" => {
                    name.push_str(catalog.suffixes[fastrand::usize(..catalog.suffixes.len())])
                }
                _ if self.locale() == "ms_MY" => {
                    name.push_str(&south_southeast_asian::malaysian_name_part(token));
                }
                _ => unreachable!("fixed Faker name template token"),
            }
            format = remaining;
        }
        name.push_str(format);
        name
    }

    /// Samples a Japanese full name in katakana.
    pub fn kanaName(&self, gender: Option<&str>) -> DatabaseResult<String> {
        Ok(format!(
            "{} {}",
            self.lastKanaName()?,
            self.firstKanaName(gender)?
        ))
    }

    pub fn firstKanaName(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        match gender {
            Some("male") => self.firstKanaNameMale(),
            Some("female") => self.firstKanaNameFemale(),
            _ if fastrand::bool() => self.firstKanaNameMale(),
            _ => self.firstKanaNameFemale(),
        }
    }

    pub fn firstKanaNameMale(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_MALE)
    }

    pub fn firstKanaNameFemale(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_FEMALE)
    }

    pub fn lastKanaName(&self) -> DatabaseResult<&'static str> {
        self.sample_kana(faker_person::JA_JP_KANA_SURNAMES)
    }

    fn sample_kana(&self, names: &'static [&'static str]) -> DatabaseResult<&'static str> {
        if self.locale() != "ja_JP" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker kana name locale",
            ));
        }
        Ok(names[fastrand::usize(..names.len())])
    }

    /// Samples a name prefix from the selected Person provider.
    pub fn prefix(&self) -> DatabaseResult<&'static str> {
        let catalog = match self.locale() {
            "ar_EG" | "ar_JO" | "ar_SA" | "fr_CH" | "fr_FR" | "nl_NL" => {
                faker_person::catalog(self.locale()).prefixes
            }
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker name prefix locale",
                ));
            }
        };
        Ok(catalog[fastrand::usize(..catalog.len())])
    }

    /// Samples a name suffix, including the upstream null and empty-string values.
    pub fn suffix(&self) -> DatabaseResult<Option<&'static str>> {
        let locale = faker_person::provider_locale(self.locale());
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
        Ok(Some(catalog[fastrand::usize(..catalog.len())]))
    }

    pub fn title(&self, gender: Option<&str>) -> &'static str {
        if self.locale() == "sl_SI" {
            let titles = faker_person::SLOVENIAN_TITLES;
            return titles[fastrand::usize(..titles.len())];
        }
        match gender {
            Some("male") => self.titleMale(),
            Some("female") => self.titleFemale(),
            _ if self.boolean(None) => self.titleMale(),
            _ => self.titleFemale(),
        }
    }

    pub fn titleMale(&self) -> &'static str {
        let titles: &[&str] = match self.locale() {
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
        titles[fastrand::usize(..titles.len())]
    }

    pub fn titleFemale(&self) -> &'static str {
        let titles: &[&str] = match self.locale() {
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
        titles[fastrand::usize(..titles.len())]
    }

    pub fn firstNameAscii(&self) -> DatabaseResult<&'static str> {
        username::ascii_given_name(self.locale()).ok_or(DatabaseError::InvalidQuery(
            "unsupported Faker ASCII name locale",
        ))
    }

    pub fn lastNameAscii(&self) -> DatabaseResult<&'static str> {
        username::ascii_surname(self.locale()).ok_or(DatabaseError::InvalidQuery(
            "unsupported Faker ASCII name locale",
        ))
    }

    pub fn userName(&self) -> String {
        let candidate = username::candidate(self);
        let candidate = internet_ascii(&candidate).replace("..", ".");
        if candidate.trim_matches(['.', '_']).is_empty() {
            return self.randomLetter().to_string();
        }
        candidate.trim_end_matches('.').to_owned()
    }

    pub fn safeEmailDomain(&self) -> &'static str {
        ["example.com", "example.org", "example.net"][fastrand::usize(0..3)]
    }

    pub fn freeEmailDomain(&self) -> &'static str {
        let domains = domains::free_email_domains(self.locale());
        domains[fastrand::usize(..domains.len())]
    }

    pub fn safeEmail(&self) -> String {
        format!("{}@{}", self.userName(), self.safeEmailDomain())
    }

    pub fn freeEmail(&self) -> String {
        format!("{}@{}", self.userName(), self.freeEmailDomain())
    }

    pub fn companyEmail(&self) -> DatabaseResult<String> {
        Ok(format!("{}@{}", self.userName(), self.domainName()?))
    }

    pub fn email(&self) -> DatabaseResult<String> {
        match self.locale() {
            "zh_CN" => Ok(self.freeEmail()),
            "ne_NP" => match fastrand::u8(0..6) {
                0..=1 => self.companyEmail(),
                2 => Ok(self.freeEmail()),
                _ => Ok(format!("{}.np", self.companyEmail()?)),
            },
            _ if self.boolean(None) => Ok(self.freeEmail()),
            _ => self.companyEmail(),
        }
    }

    pub fn password(
        &self,
        minLength: Option<usize>,
        maxLength: Option<usize>,
    ) -> DatabaseResult<String> {
        let min = minLength.unwrap_or(6);
        let max = maxLength.unwrap_or(20);
        let length = fastrand::usize(min.min(max)..=min.max(max));
        let mut result = String::new();
        result.try_reserve_exact(length).map_err(|_| {
            DatabaseError::InvalidQuery("Faker password length exceeds string capacity")
        })?;
        result.extend((0..length).map(|_| self.randomAscii()));
        Ok(result)
    }

    pub(super) fn de_phone_area_code(&self) -> String {
        phone_area_code(&DE_AREA_CODES, faker_phone::AREA_DE_DE)
    }

    pub(super) fn us_phone_area_code(&self) -> String {
        phone_area_code(&US_AREA_CODES, faker_phone::AREA_EN_US)
    }

    pub(super) fn us_phone_exchange_code(&self) -> String {
        let first = fastrand::u8(2..=9);
        let middle = self.randomDigit();
        let last = if middle == 1 {
            self.randomDigitNot(1)
        } else {
            self.randomDigit()
        };
        format!("{first}{middle}{last}")
    }

    pub fn e164PhoneNumber(&self) -> String {
        match self.locale() {
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
                self.numerify(Some(formats[fastrand::usize(..formats.len())]))
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
        let prefix = if fastrand::bool() { "978" } else { "979" };
        let mut code = String::with_capacity(13);
        code.push_str(prefix);
        code.push_str(&self.numerify(Some("#########")));
        ean_with_checksum(code)
    }

    pub fn creditCardType(&self) -> &'static str {
        let vendors = if self.locale() == "pt_BR" {
            CARD_VENDORS_PT_BR
        } else {
            CARD_VENDORS
        };
        vendors[fastrand::usize(..vendors.len())]
    }

    pub fn creditCardNumber(
        &self,
        cardType: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        let vendor = cardType.unwrap_or_else(|| self.creditCardType());
        let masks = if self.locale() == "pt_BR" {
            card_masks_pt_br(vendor)
        } else {
            card_masks(vendor)
        }
        .ok_or(DatabaseError::InvalidQuery(
            "unknown Faker credit card type",
        ))?;
        let mut number = self.numerify(Some(masks[fastrand::usize(..masks.len())]));
        number.push(luhn_check_digit(&number));
        if formatted {
            let separator = separator.unwrap_or("-");
            Ok([&number[..4], &number[4..8], &number[8..12], &number[12..]].join(separator))
        } else {
            Ok(number)
        }
    }

    pub fn creditCardExpirationDate(&self, valid: bool) -> DatabaseResult<DateTime<Tz>> {
        let start = if valid { "now" } else { "-36 months" };
        let zone = self
            .getDefaultTimezone()
            .unwrap_or(self.application_timezone);
        self.dateTimeBetweenExpressions(start, "+36 months")
            .map(|date| date.with_timezone(&zone))
    }

    pub fn creditCardExpirationDateString(
        &self,
        valid: bool,
        expirationDateFormat: Option<&str>,
    ) -> DatabaseResult<String> {
        let date = self.creditCardExpirationDate(valid)?;
        let mut output = String::new();
        write!(
            output,
            "{}",
            date.format(expirationDateFormat.unwrap_or("%m/%y"))
        )
        .map_err(|_| DatabaseError::InvalidQuery("invalid Faker date format"))?;
        Ok(output)
    }

    pub fn creditCardDetails(&self, valid: bool) -> DatabaseResult<CreditCardDetails> {
        let card_type = self.creditCardType();
        Ok(CreditCardDetails {
            r#type: card_type,
            number: self.creditCardNumber(Some(card_type), false, None)?,
            name: self.name(),
            expiration_date: self.creditCardExpirationDateString(valid, None)?,
        })
    }

    fn institution_sample(
        &self,
        locale: &str,
        catalog: &'static [&'static str],
    ) -> DatabaseResult<&'static str> {
        if self.locale() != locale {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker institution locale",
            ));
        }
        Ok(catalog[fastrand::usize(..catalog.len())])
    }

    /// Selects a bank from the locale's catalog.
    pub fn bank(&self) -> DatabaseResult<&'static str> {
        if self.locale() == "ms_MY" {
            return [Self::localBank, Self::foreignBank, Self::governmentBank]
                [fastrand::usize(..3)](self);
        }
        let catalog = faker_institutions::banks(self.locale())
            .ok_or(DatabaseError::InvalidQuery("unsupported Faker bank locale"))?;
        Ok(catalog[fastrand::usize(..catalog.len())])
    }

    pub fn microFinance(&self) -> DatabaseResult<String> {
        let name = self.institution_sample("ne_NP", faker_institutions::NE_NP_MICRO_FINANCES)?;
        Ok(format!("{name} Laghubitta Bittiya Sanstha"))
    }

    /// Generates a SWIFT code using the Malaysian or Nepali provider.
    pub fn swiftCode(&self) -> DatabaseResult<String> {
        match self.locale() {
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
    pub fn vat(&self, spacedNationalPrefix: Option<bool>) -> DatabaseResult<String> {
        let separator = if spacedNationalPrefix.unwrap_or(true) {
            " "
        } else {
            ""
        };
        let digits = |count| self.randomNumber(Some(count), true);
        match self.locale() {
            "zh_TW" => Ok(self.VAT()?.to_string()),
            "bg_BG" => Ok(format!(
                "BG{separator}{}{}",
                digits(5)?,
                digits(fastrand::u32(4..=5))?
            )),
            "de_AT" => Ok(format!("AT{separator}U{}", digits(8)?)),
            "es_ES" => {
                const PREFIXES: &[u8] = b"ABCDEFGHJNPQRSUVW";
                let prefix = char::from(PREFIXES[fastrand::usize(..PREFIXES.len())]);
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
            "en_GB" => self.vatWithType(None),
            "it_IT" => {
                let mut number = format!(
                    "{}{:03}",
                    self.numerify(Some("#######")),
                    self.numberBetween(1, 121)
                );
                number.push(luhn_check_digit(&number));
                Ok(format!("IT{number}"))
            }
            "nl_NL" => Ok(format!("NL{}B{}", digits(9)?, digits(2)?)),
            _ => Err(DatabaseError::InvalidQuery("unsupported Faker VAT locale")),
        }
    }

    /// Selects the UK provider's standard, branch, government or health VAT sample.
    pub fn vatWithType(&self, vatType: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "en_GB" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker VAT type locale",
            ));
        }
        match vatType {
            Some("gov") => Ok(format!("GBGD{:03}", self.numberBetween(0, 499))),
            Some("health") => Ok(format!("GBHA{}", self.numberBetween(500, 999))),
            _ => {
                let first = self.numberBetween(100, 999);
                let second = self.randomNumber(Some(4), true)?;
                let key = Self::calculateModulus97(&format!("{first}{second}"), true)?;
                let mut vat = format!("GB{first} {second} {key}");
                if vatType == Some("branch") {
                    write!(vat, " {}", self.randomNumber(Some(3), true)?)
                        .expect("writing to String cannot fail");
                }
                Ok(vat)
            }
        }
    }

    /// Computes the UK provider's modulus 97 or 9755 check digits.
    pub fn calculateModulus97(input: &str, use9755: bool) -> DatabaseResult<u8> {
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
        if self.locale() != "fr_FR" {
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

    pub fn vatId(&self) -> DatabaseResult<String> {
        if self.locale() != "it_IT" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker vatId locale",
            ));
        }
        self.vat(None)
    }

    pub fn btw(&self) -> DatabaseResult<String> {
        if self.locale() != "nl_NL" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker btw locale"));
        }
        self.vat(None)
    }

    /// Generates the bank account format provided by this locale.
    pub fn bankAccountNumber(
        &self,
        prefix: &str,
        countryCode: Option<&str>,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        if let Some((country, default_length)) = account_iban_defaults(self.locale()) {
            return self.iban(
                Some(countryCode.unwrap_or(country)),
                prefix,
                length.or(default_length),
            );
        }
        if !prefix.is_empty() || countryCode.is_some() || length.is_some() {
            return Err(DatabaseError::InvalidQuery(
                "Faker local bank account takes no arguments",
            ));
        }
        match self.locale() {
            "ar_EG" => self.iban(Some("EG"), "", Some(25)),
            "en_US" => {
                let length = 5 + (0..4).map(|_| fastrand::usize(0..=3)).sum::<usize>();
                Ok(self.numerify(Some(&"#".repeat(length))))
            }
            "ms_MY" => Ok(self.numerify(Some(&"#".repeat(fastrand::usize(10..=16))))),
            "ne_NP" => self.regexify(if fastrand::bool() {
                "[A-Z][1-9]{8,19}"
            } else {
                "00[1-9]{7,18}"
            }),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker bank account locale",
            )),
        }
    }

    /// Generates the routing-number sample provided by the US locale.
    pub fn bankRoutingNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "en_US" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker bank routing locale",
            ));
        }
        let bank_type = [0, 0, 0, 0, 20, 20, 60][fastrand::usize(..7)];
        let district = self.numberBetween(1, 12);
        let clearing_center = self.randomDigitNotNull();
        let state = self.randomDigit();
        let institution = self.numberBetween(1000, 9999);
        let routing = format!(
            "{:02}{clearing_center}{state}{institution}",
            district + bank_type
        );
        let checksum = Self::calculateRoutingNumberChecksum(&routing)?;
        Ok(format!("{routing}{checksum}"))
    }

    /// Computes the check digit from the first eight routing-number digits.
    pub fn calculateRoutingNumberChecksum(routing: &str) -> DatabaseResult<u8> {
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
        countryCode: Option<&str>,
        prefix: &str,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        let country = countryCode.map_or_else(
            || {
                IBAN_FORMATS[fastrand::usize(..IBAN_FORMATS.len())]
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
            let letter = class == b'a' || (class == b'c' && fastrand::bool());
            account.push(if letter {
                char::from(fastrand::u8(b'A'..=b'Z'))
            } else {
                char::from(fastrand::u8(b'0'..=b'9'))
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
    pub fn swiftBicNumber(&self) -> String {
        const ALPHANUMERIC: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let location_length = if fastrand::bool() { 2 } else { 5 };
        let mut code = String::with_capacity(6 + location_length);
        code.extend((0..6).map(|_| char::from(fastrand::u8(b'A'..=b'Z'))));
        code.extend(
            (0..location_length)
                .map(|_| char::from(ALPHANUMERIC[fastrand::usize(..ALPHANUMERIC.len())])),
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

    pub fn domainWord(&self) -> DatabaseResult<String> {
        let mut name = internet_ascii(&self.lastName());
        if name.trim_matches(['.', '_']).is_empty() {
            return Err(DatabaseError::InvalidQuery(
                "Faker surname cannot be transliterated to a domain word",
            ));
        }
        name.truncate(name.trim_end_matches('.').len());
        Ok(name)
    }

    pub fn tld(&self) -> &'static str {
        let suffixes = domains::tlds(self.locale());
        suffixes[fastrand::usize(..suffixes.len())]
    }

    pub fn domainName(&self) -> DatabaseResult<String> {
        let name = match username::ascii_surname(self.locale()) {
            Some(surname) => surname.to_owned(),
            None => self.domainWord()?,
        };
        Ok(format!("{name}.{}", self.tld()))
    }

    pub fn slug(&self, wordCount: Option<usize>, variableWordCount: bool) -> String {
        self.words(lorem_count(wordCount.unwrap_or(6), variableWordCount))
            .join("-")
    }

    pub fn url(&self) -> DatabaseResult<String> {
        if self.locale() == "ne_NP" {
            let format = match fastrand::u8(0..11) {
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

        let format = match fastrand::u8(0..10) {
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
        let mut url = format!("{prefix}{}{domain_suffix}/", self.domainName()?);
        if path {
            url.push_str(&self.slug(None, true));
            url.push_str(extension);
        }
        Ok(url)
    }

    pub fn ipv4(&self) -> String {
        let address = if self.boolean(None) {
            fastrand::u32(0x8000_0000..=0xffff_fffe)
        } else {
            fastrand::u32(0x0100_0000..=0x7fff_ffff)
        };
        Ipv4Addr::from(address).to_string()
    }

    pub fn ipv6(&self) -> String {
        let mut address = String::with_capacity(39);
        for index in 0..8 {
            if index != 0 {
                address.push(':');
            }
            write!(address, "{:x}", fastrand::u16(..)).expect("writing to String cannot fail");
        }
        address
    }

    pub fn localIpv4(&self) -> String {
        let (network, host_mask) = match fastrand::u8(0..3) {
            0 => (0x0a00_0000, 0x00ff_ffff),
            1 => (0xac10_0000, 0x000f_ffff),
            _ => (0xc0a8_0000, 0x0000_ffff),
        };
        Ipv4Addr::from(network | fastrand::u32(0..=host_mask)).to_string()
    }

    pub fn macAddress(&self) -> String {
        let mut address = String::with_capacity(17);
        for index in 0..6 {
            if index != 0 {
                address.push(':');
            }
            write!(address, "{:02X}", fastrand::u8(..)).expect("writing to String cannot fail");
        }
        address
    }

    /// Samples a MIME type from the fixed FakerPHP file catalog.
    pub fn mimeType(&self) -> &'static str {
        MIME_TYPES[fastrand::usize(..MIME_TYPES.len())].0
    }

    /// Samples a MIME group, then one of its extensions without the dot.
    pub fn fileExtension(&self) -> &'static str {
        let extensions = MIME_TYPES[fastrand::usize(..MIME_TYPES.len())].1;
        extensions[fastrand::usize(..extensions.len())]
    }

    /// Generates the version 3 sample UUID used by the FakerPHP file provider.
    pub fn uuid(&self) -> uuid::Uuid {
        let seed = format!(
            "{}#{}",
            self.numberBetween(0, i32::MAX.into()),
            self.numberBetween(0, i32::MAX.into())
        );
        let mut bytes: [u8; 16] = Md5::digest(seed).into();
        bytes[6] = (bytes[6] & 0x0f) | 0x30;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        uuid::Uuid::from_bytes(bytes)
    }

    /// Copies one readable file from a directory without recursing into subdirectories.
    pub fn file(
        &self,
        sourceDirectory: &Path,
        targetDirectory: &Path,
        fullPath: bool,
    ) -> io::Result<PathBuf> {
        let source = fs::canonicalize(sourceDirectory)?;
        let target = fs::canonicalize(targetDirectory)?;
        if !source.is_dir() || !target.is_dir() || source == target {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Faker file requires two different directories",
            ));
        }
        let mut candidates = fs::read_dir(&source)?.collect::<io::Result<Vec<_>>>()?;
        fastrand::shuffle(&mut candidates);
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
            let path = targetDirectory.join(&name);
            let mut output = match OpenOptions::new().write(true).create_new(true).open(&path) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                result => result?,
            };
            if let Err(error) = io::copy(&mut source_file, &mut output) {
                drop(output);
                let _ = fs::remove_file(&path);
                return Err(error);
            }
            return Ok(if fullPath { path } else { PathBuf::from(name) });
        }
    }

    pub fn md5(&self) -> String {
        let value = self.numberBetween(0, i64::from(i32::MAX)).to_string();
        digest_hex(&Md5::digest(value.as_bytes()))
    }

    pub fn sha1(&self) -> String {
        self.digest_sample(&digest::SHA1_FOR_LEGACY_USE_ONLY)
    }

    pub fn sha256(&self) -> String {
        self.digest_sample(&digest::SHA256)
    }

    fn digest_sample(&self, algorithm: &'static digest::Algorithm) -> String {
        let value = self.numberBetween(0, i64::from(i32::MAX)).to_string();
        digest_hex(digest::digest(algorithm, value.as_bytes()).as_ref())
    }

    pub fn randomElement<'a, T>(&self, values: &'a [T]) -> Option<&'a T> {
        self.randomKey(values).map(|index| &values[index])
    }

    /// Selects a positional key; pass an explicit slice of keys to sample a keyed collection.
    pub fn randomKey<T>(&self, values: &[T]) -> Option<usize> {
        (!values.is_empty()).then(|| fastrand::usize(..values.len()))
    }

    pub fn randomElements<'a, T>(
        &self,
        values: &'a [T],
        count: Option<usize>,
        allowDuplicates: bool,
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
            None => fastrand::usize(1..=size),
        };
        if allowDuplicates {
            return Ok((0..count)
                .map(|_| &values[fastrand::usize(..size)])
                .collect());
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
            let index = fastrand::usize(..remaining);
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

    pub fn shuffleArray<T: Clone>(&self, values: &[T]) -> Vec<T> {
        let mut shuffled = values.to_vec();
        fastrand::shuffle(&mut shuffled);
        shuffled
    }

    pub fn shuffleString(&self, value: &str) -> String {
        let mut characters: Vec<_> = value.chars().collect();
        fastrand::shuffle(&mut characters);
        characters.into_iter().collect()
    }

    pub fn randomLetter(&self) -> char {
        char::from(fastrand::u8(b'a'..=b'z'))
    }

    pub fn randomAscii(&self) -> char {
        char::from(fastrand::u8(b'!'..=b'~'))
    }

    pub fn numerify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("###")
            .chars()
            .map(|character| match character {
                '#' => char::from(b'0' + self.randomDigit()),
                '%' => char::from(b'0' + self.randomDigitNotNull()),
                literal => literal,
            })
            .collect()
    }

    pub fn lexify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("????")
            .chars()
            .map(|character| match character {
                '?' => self.randomLetter(),
                literal => literal,
            })
            .collect()
    }

    pub fn bothify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("## ??")
            .chars()
            .map(|character| match character {
                '#' => char::from(b'0' + self.randomDigit()),
                '%' => char::from(b'0' + self.randomDigitNotNull()),
                '?' => self.randomLetter(),
                '*' if fastrand::bool() => self.randomLetter(),
                '*' => char::from(b'0' + self.randomDigit()),
                literal => literal,
            })
            .collect()
    }

    pub fn regexify(&self, pattern: &str) -> DatabaseResult<String> {
        Ok(RegexPattern::parse(pattern)?.sample())
    }

    pub fn asciify(&self, pattern: Option<&str>) -> String {
        pattern
            .unwrap_or("****")
            .chars()
            .map(|character| match character {
                '*' => self.randomAscii(),
                literal => literal,
            })
            .collect()
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

    /// Resolves PHP-style date expressions in the application time zone and samples their range.
    pub fn dateTimeBetweenExpressions(
        &self,
        start: &str,
        end: &str,
    ) -> DatabaseResult<DateTime<Utc>> {
        let now = Utc::now();
        let start = self
            .timestamp_expression(start, now)
            .or_else(faker_timestamp)?;
        let end = self
            .resolve_timestamp_max(end, now)
            .or_else(faker_timestamp)?;
        let start = DateTime::from_timestamp(start, 0).ok_or(DATE_RANGE_ERROR)?;
        let end = DateTime::from_timestamp(end, 0).ok_or(DATE_RANGE_ERROR)?;
        self.dateTimeBetween(start, end)
    }

    /// Resolves a PHP-style maximum for any formatter that takes a UTC date bound.
    pub fn resolveDateMax(&self, max: &str) -> DatabaseResult<DateTime<Utc>> {
        self.resolve_date_max(max, Utc::now())
    }

    fn timestamp_expression(
        &self,
        expression: &str,
        now: DateTime<Utc>,
    ) -> Result<i64, calendar::CalendarError> {
        calendar::parse_timestamp_expression(expression, now, self.application_timezone)
    }

    fn resolve_date_max(&self, max: &str, now: DateTime<Utc>) -> DatabaseResult<DateTime<Utc>> {
        let timestamp = self
            .resolve_timestamp_max(max, now)
            .map_err(calendar_query_error)?;
        DateTime::from_timestamp(timestamp, 0).ok_or(DATE_RANGE_ERROR)
    }

    fn resolve_timestamp_max(
        &self,
        max: &str,
        now: DateTime<Utc>,
    ) -> Result<i64, calendar::CalendarError> {
        if max.is_empty() {
            return Ok(now.timestamp());
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
    pub fn dateTimeInInterval(
        &self,
        date: Option<DateTime<Utc>>,
        interval: Option<TimeDelta>,
    ) -> DatabaseResult<DateTime<Utc>> {
        let zone = self.application_timezone;
        let uses_local_calendar = date.is_none();
        let date = match date {
            Some(date) => date,
            None => {
                let now = Utc::now().with_timezone(&zone).naive_local();
                resolve_local(zone, months_ago(now, 30 * 12)?)?
            }
        };
        let other = match interval {
            Some(interval) => date.checked_add_signed(interval).ok_or(DATE_RANGE_ERROR)?,
            None if uses_local_calendar => {
                let local = date
                    .with_timezone(&zone)
                    .naive_local()
                    .checked_add_days(Days::new(5))
                    .ok_or(DATE_RANGE_ERROR)?;
                resolve_local(zone, local)?
            }
            None => date
                .checked_add_days(Days::new(5))
                .ok_or(DATE_RANGE_ERROR)?,
        };
        self.dateTimeBetween(date.min(other), date.max(other))
    }

    /// Samples a range using PHP-style date and relative interval expressions.
    pub fn dateTimeInIntervalExpressions(
        &self,
        date: Option<&str>,
        interval: Option<&str>,
    ) -> DatabaseResult<DateTime<Utc>> {
        let (start, end) = calendar::date_interval(
            match date {
                Some("") => "now",
                Some(date) => date,
                None => "-30 years",
            },
            interval.unwrap_or("+5 days"),
            Utc::now(),
            self.application_timezone,
        )
        .map_err(|_| DatabaseError::InvalidQuery("invalid Faker date or interval expression"))?;
        self.dateTimeBetween(start.min(end), start.max(end))
    }

    pub fn dateTimeThisCentury(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.date_time_since_months_ago(100 * 12, max)
    }

    pub fn dateTimeThisDecade(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.date_time_since_months_ago(10 * 12, max)
    }

    pub fn dateTimeThisYear(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        let now = Utc::now();
        let zone = self.application_timezone;
        let year = now.with_timezone(&zone).year();
        let start = NaiveDate::from_ymd_opt(year, 1, 1)
            .and_then(|date| date.and_hms_opt(0, 0, 0))
            .ok_or(DATE_RANGE_ERROR)?;
        let start = resolve_local(zone, start)?;
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
        let zone = self.application_timezone;
        let start = months_ago(now.with_timezone(&zone).naive_local(), months)?;
        self.dateTimeBetween(resolve_local(zone, start)?, max.unwrap_or(now))
    }

    /// Samples whole seconds between the Unix epoch and the supplied upper endpoint.
    pub fn unixTime(&self, max: Option<DateTime<Utc>>) -> i64 {
        let end = max.map_or_else(|| Utc::now().timestamp(), |date| date.timestamp());
        self.numberBetween(0, end)
    }

    /// Samples Unix seconds with a string endpoint, including dates outside Chrono's range.
    pub fn unixTimeExpression(&self, max: &str) -> DatabaseResult<i64> {
        let end = self
            .resolve_timestamp_max(max, Utc::now())
            .or_else(faker_timestamp)?;
        Ok(self.numberBetween(0, end))
    }

    /// Returns a sampled instant in UTC.
    pub fn dateTime(&self, max: Option<DateTime<Utc>>) -> DateTime<Utc> {
        DateTime::from_timestamp(self.unixTime(max), 0)
            .expect("sample lies between valid timestamps")
    }

    /// Returns a sampled date in the explicit time zone or the generator's default.
    pub fn dateTimeInTimezone(
        &self,
        max: Option<DateTime<Utc>>,
        timezone: Option<Tz>,
    ) -> DateTime<Tz> {
        self.zoned_date(self.dateTime(max), timezone)
    }

    fn zoned_date(&self, instant: DateTime<Utc>, timezone: Option<Tz>) -> DateTime<Tz> {
        let timezone = timezone
            .or_else(|| self.getDefaultTimezone())
            .unwrap_or(self.application_timezone);
        instant.with_timezone(&timezone)
    }

    pub fn date(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%Y-%m-%d"), max)
    }

    pub fn time(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%H:%M:%S"), max)
    }

    /// Renders a sampled date with PHP DateTime format tokens.
    pub fn dateWithPhpFormat(
        &self,
        format: &str,
        max: Option<DateTime<Utc>>,
    ) -> DatabaseResult<String> {
        let timezone = self
            .getDefaultTimezone()
            .unwrap_or(self.application_timezone);
        calendar::format_php_date(self.dateTime(max), timezone, format)
            .map_err(|_| DatabaseError::InvalidQuery("Faker PHP date format could not be rendered"))
    }

    /// Renders a sampled time with PHP DateTime format tokens.
    pub fn timeWithPhpFormat(
        &self,
        format: &str,
        max: Option<DateTime<Utc>>,
    ) -> DatabaseResult<String> {
        self.dateWithPhpFormat(format, max)
    }

    pub fn iso8601(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None)
            .format("%Y-%m-%dT%H:%M:%S%z")
            .to_string()
    }

    /// Samples from the upstream 64-bit AD lower bound to the supplied endpoint.
    pub fn dateTimeAD(&self, max: Option<DateTime<Utc>>) -> DateTime<Utc> {
        const FIRST_AD_TIMESTAMP: i64 = -62_135_597_361;
        let end = max.unwrap_or_else(Utc::now).timestamp();
        DateTime::from_timestamp(self.numberBetween(FIRST_AD_TIMESTAMP, end), 0)
            .expect("sample lies between valid timestamps")
    }

    /// Samples an AD date and returns it in the chosen output time zone.
    pub fn dateTimeADInTimezone(
        &self,
        max: Option<DateTime<Utc>>,
        timezone: Option<Tz>,
    ) -> DateTime<Tz> {
        self.zoned_date(self.dateTimeAD(max), timezone)
    }

    pub fn amPm(&self, max: Option<DateTime<Utc>>) -> String {
        let date = self.dateTimeInTimezone(max, None);
        let morning = date.hour() < 12;
        match (self.locale(), morning) {
            ("tr_TR", true) => "öö".into(),
            ("tr_TR", false) => "ös".into(),
            ("zh_CN" | "zh_TW", true) => "上午".into(),
            ("zh_CN" | "zh_TW", false) => "下午".into(),
            _ => date.format("%P").to_string(),
        }
    }

    pub fn dayOfMonth(&self, max: Option<DateTime<Utc>>) -> String {
        let pattern = if self.locale() == "cs_CZ" {
            "%-d"
        } else {
            "%d"
        };
        self.dateTimeInTimezone(max, None)
            .format(pattern)
            .to_string()
    }

    pub fn dayOfWeek(&self, max: Option<DateTime<Utc>>) -> String {
        let date = self.dateTimeInTimezone(max, None);
        let names: &[&str] = match self.locale() {
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
            _ => return date.format("%A").to_string(),
        };
        names[date.weekday().num_days_from_sunday() as usize].to_owned()
    }

    pub fn month(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%m").to_string()
    }

    pub fn monthName(&self, max: Option<DateTime<Utc>>) -> String {
        let date = self.dateTimeInTimezone(max, None);
        let names: &[&str] = match self.locale() {
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
            _ => return date.format("%B").to_string(),
        };
        names[date.month0() as usize].to_owned()
    }

    /// Samples the Czech genitive form of a month name.
    pub fn monthNameGenitive(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<&'static str> {
        if self.locale() != "cs_CZ" {
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
        Ok(NAMES[self.dateTimeInTimezone(max, None).month0() as usize])
    }

    /// Samples the Czech display date, drawing each of its three fields separately.
    pub fn formattedDate(&self) -> DatabaseResult<String> {
        if self.locale() != "cs_CZ" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker formatted date locale",
            ));
        }
        let day = self.dayOfMonth(None);
        let month = self.monthNameGenitive(None)?;
        Ok(format!("{day}. {month} {}", self.year(None)))
    }

    pub fn year(&self, max: Option<DateTime<Utc>>) -> String {
        let year = self.dateTimeInTimezone(max, None).format("%Y").to_string();
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
        CENTURIES[(0..CENTURIES.len()).fake::<usize>()]
    }

    /// Picks an IANA time zone, optionally restricted to an ISO country code.
    pub fn timezone(&self, countryCode: Option<&str>) -> Option<&'static str> {
        type ZoneLists = (Vec<&'static str>, HashMap<&'static str, Vec<&'static str>>);
        static ZONES: OnceLock<ZoneLists> = OnceLock::new();
        let (all, by_country) = ZONES.get_or_init(|| {
            let mut all = vec!["UTC"];
            let mut by_country: HashMap<&str, Vec<&str>> = HashMap::new();
            // Public-domain IANA tzdb 2025b zone.tab, matching chrono-tz 0.10.4.
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
        let candidates = match countryCode {
            None | Some("" | "0") => all,
            Some(code) => by_country.get(code)?,
        };
        candidates
            .get(fastrand::usize(0..candidates.len()))
            .copied()
    }

    fn format_date(&self, format: &str, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        use std::fmt::Write as _;
        let mut output = String::new();
        write!(
            output,
            "{}",
            self.dateTimeInTimezone(max, None).format(format)
        )
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

fn lorem_count(count: usize, variable: bool) -> usize {
    if !variable || count == 0 {
        return count;
    }
    let percentage = fastrand::u128(60..=140);
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

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn sentence(&self, words: Range<usize>) -> DatabaseResult<String> {
        self.generate("sentence", || self.0.sentence(words.clone()))
    }

    pub fn sentenceWithWordCount(&self, count: usize, variable: bool) -> DatabaseResult<String> {
        self.generate("sentence", || self.0.sentenceWithWordCount(count, variable))
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

    pub fn wordsAsText(&self, count: usize) -> DatabaseResult<String> {
        self.generate("wordsAsText", || self.0.wordsAsText(count))
    }

    pub fn sentences(&self, count: usize) -> DatabaseResult<Vec<String>> {
        self.generate_keyed(
            "sentences",
            || Ok(self.0.sentences(count)),
            |items| Ok(format!("{items:?}")),
        )
    }

    pub fn sentencesAsText(&self, count: usize) -> DatabaseResult<String> {
        self.generate("sentencesAsText", || self.0.sentencesAsText(count))
    }

    pub fn paragraphs(&self, count: usize) -> DatabaseResult<Vec<String>> {
        self.generate_keyed(
            "paragraphs",
            || Ok(self.0.paragraphs(count)),
            |items| Ok(format!("{items:?}")),
        )
    }

    pub fn paragraphsAsText(&self, count: usize) -> DatabaseResult<String> {
        self.generate("paragraphsAsText", || self.0.paragraphsAsText(count))
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

    pub fn boolean(&self, chanceOfGettingTrue: Option<i64>) -> DatabaseResult<bool> {
        self.generate("boolean", || self.0.boolean(chanceOfGettingTrue))
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

    pub fn localLatitude(&self) -> DatabaseResult<f64> {
        self.try_generate("localLatitude", || {
            self.0.localLatitude().map(|value| value + 0.0)
        })
    }

    pub fn localLongitude(&self) -> DatabaseResult<f64> {
        self.try_generate("localLongitude", || {
            self.0.localLongitude().map(|value| value + 0.0)
        })
    }

    pub fn localCoordinates(&self) -> DatabaseResult<Coordinates> {
        self.generate_keyed(
            "localCoordinates",
            || self.0.localCoordinates(),
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

    pub fn streetName(&self) -> DatabaseResult<String> {
        self.generate("streetName", || self.0.streetName())
    }

    pub fn cityPrefix(&self) -> DatabaseResult<String> {
        self.generate("cityPrefix", || self.0.cityPrefix())
    }

    pub fn citySuffix(&self) -> DatabaseResult<String> {
        self.generate("citySuffix", || self.0.citySuffix())
    }

    pub fn streetSuffix(&self) -> DatabaseResult<String> {
        self.generate("streetSuffix", || self.0.streetSuffix())
    }

    pub fn country(&self) -> DatabaseResult<String> {
        self.generate("country", || self.0.country())
    }

    pub fn state(&self) -> DatabaseResult<String> {
        self.generate("state", || self.0.state())
    }

    pub fn stateAbbr(&self) -> DatabaseResult<String> {
        self.generate("stateAbbr", || self.0.stateAbbr())
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

    pub fn streetPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetPrefix", || self.0.streetPrefix())
    }

    pub fn streetNumber(&self) -> DatabaseResult<String> {
        self.try_generate("streetNumber", || self.0.streetNumber())
    }

    pub fn blockNumber(&self) -> DatabaseResult<String> {
        self.try_generate("blockNumber", || self.0.blockNumber())
    }

    pub fn floorNumber(&self) -> DatabaseResult<&'static str> {
        self.try_generate("floorNumber", || self.0.floorNumber())
    }

    pub fn apartmentNumber(&self) -> DatabaseResult<&'static str> {
        self.try_generate("apartmentNumber", || self.0.apartmentNumber())
    }

    pub fn streetPlainName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetPlainName", || self.0.streetPlainName())
    }

    pub fn floor(&self) -> DatabaseResult<&'static str> {
        self.try_generate("floor", || self.0.floor())
    }

    pub fn blockSegment(&self) -> DatabaseResult<&'static str> {
        self.try_generate("blockSegment", || self.0.blockSegment())
    }

    pub fn block(&self) -> DatabaseResult<String> {
        self.try_generate("block", || self.0.block())
    }

    pub fn townName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townName", || self.0.townName())
    }

    pub fn locality(&self) -> DatabaseResult<String> {
        self.try_generate("locality", || self.0.locality())
    }

    pub fn localityName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("localityName", || self.0.localityName())
    }

    pub fn areaSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("areaSuffix", || self.0.areaSuffix())
    }

    pub fn societyName(&self) -> DatabaseResult<String> {
        self.try_generate("societyName", || self.0.societyName())
    }

    pub fn societySuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("societySuffix", || self.0.societySuffix())
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

    pub fn englishStreetName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("englishStreetName", || self.0.englishStreetName())
    }

    pub fn villageSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("villageSuffix", || self.0.villageSuffix())
    }

    pub fn estateSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("estateSuffix", || self.0.estateSuffix())
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

    pub fn areaNumber(&self) -> DatabaseResult<i64> {
        self.try_generate("areaNumber", || self.0.areaNumber())
    }

    pub fn metropolitanCity(&self) -> DatabaseResult<&'static str> {
        self.try_generate("metropolitanCity", || self.0.metropolitanCity())
    }

    pub fn borough(&self) -> DatabaseResult<&'static str> {
        self.try_generate("borough", || self.0.borough())
    }

    pub fn hamletName(&self) -> DatabaseResult<String> {
        self.try_generate("hamletName", || self.0.hamletName())
    }

    pub fn hamletPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("hamletPrefix", || self.0.hamletPrefix())
    }

    pub fn wardName(&self) -> DatabaseResult<String> {
        self.try_generate("wardName", || self.0.wardName())
    }

    pub fn wardPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("wardPrefix", || self.0.wardPrefix())
    }

    pub fn districtName(&self) -> DatabaseResult<String> {
        self.try_generate("districtName", || self.0.districtName())
    }

    pub fn districtPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("districtPrefix", || self.0.districtPrefix())
    }

    pub fn buildingPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingPrefix", || self.0.buildingPrefix())
    }

    pub fn township(&self) -> DatabaseResult<String> {
        self.try_generate("township", || self.0.township())
    }

    pub fn townshipPrefixAbbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipPrefixAbbr", || self.0.townshipPrefixAbbr())
    }

    pub fn townshipPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipPrefix", || self.0.townshipPrefix())
    }

    pub fn townshipSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("townshipSuffix", || self.0.townshipSuffix())
    }

    pub fn townState(&self) -> DatabaseResult<String> {
        self.try_generate("townState", || self.0.townState())
    }

    pub fn postcodeWithState(&self, state: &str) -> DatabaseResult<String> {
        self.try_generate("postcode", || self.0.postcodeWithState(state))
    }

    pub fn banglaStreetName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("banglaStreetName", || self.0.banglaStreetName())
    }

    pub fn wardNumber(&self) -> DatabaseResult<String> {
        self.try_generate("wardNumber", || self.0.wardNumber())
    }

    pub fn governorate(&self) -> DatabaseResult<&'static str> {
        self.try_generate("governorate", || self.0.governorate())
    }

    pub fn subdivision(&self) -> DatabaseResult<&'static str> {
        self.try_generate("subdivision", || self.0.subdivision())
    }

    pub fn buildingNamePrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingNamePrefix", || self.0.buildingNamePrefix())
    }

    pub fn buildingNumberPrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingNumberPrefix", || self.0.buildingNumberPrefix())
    }

    pub fn postcodePrefix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("postcodePrefix", || self.0.postcodePrefix())
    }

    pub fn building(&self) -> DatabaseResult<String> {
        self.try_generate("building", || self.0.building())
    }

    pub fn regionGenitiveForm(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionGenitiveForm", || self.0.regionGenitiveForm())
    }

    pub fn regionSuffix(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionSuffix", || self.0.regionSuffix())
    }

    pub fn cityAndRegion(&self) -> DatabaseResult<String> {
        self.try_generate("cityAndRegion", || self.0.cityAndRegion())
    }

    pub fn streetSuffixShort(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixShort", || self.0.streetSuffixShort())
    }

    pub fn streetSuffixLong(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixLong", || self.0.streetSuffixLong())
    }

    pub fn streetSuffixWord(&self) -> DatabaseResult<&'static str> {
        self.try_generate("streetSuffixWord", || self.0.streetSuffixWord())
    }

    pub fn buildingLevel(&self) -> DatabaseResult<String> {
        self.try_generate("buildingLevel", || self.0.buildingLevel())
    }

    pub fn buildingSide(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingSide", || self.0.buildingSide())
    }

    pub fn kommune(&self) -> DatabaseResult<&'static str> {
        self.try_generate("kommune", || self.0.kommune())
    }

    pub fn capital(&self) -> DatabaseResult<&'static str> {
        self.try_generate("capital", || self.0.capital())
    }

    pub fn bigCity(&self) -> DatabaseResult<&'static str> {
        self.try_generate("bigCity", || self.0.bigCity())
    }

    pub fn smallerCity(&self) -> DatabaseResult<&'static str> {
        self.try_generate("smallerCity", || self.0.smallerCity())
    }

    pub fn cityName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cityName", || self.0.cityName())
    }

    pub fn provinceAbbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("provinceAbbr", || self.0.provinceAbbr())
    }

    pub fn randomPostcodeLetter(&self) -> DatabaseResult<&'static str> {
        self.try_generate("randomPostcodeLetter", || self.0.randomPostcodeLetter())
    }

    pub fn buildingLetter(&self) -> DatabaseResult<&'static str> {
        self.try_generate("buildingLetter", || self.0.buildingLetter())
    }

    pub fn county(&self) -> DatabaseResult<&'static str> {
        self.try_generate("county", || self.0.county())
    }

    pub fn province(&self) -> DatabaseResult<&'static str> {
        self.try_generate("province", || self.0.province())
    }

    pub fn cantonShort(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cantonShort", || self.0.cantonShort())
    }

    pub fn cantonName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("cantonName", || self.0.cantonName())
    }

    pub fn canton(&self) -> DatabaseResult<(&'static str, &'static str)> {
        self.generate_keyed("canton", || self.0.canton(), |(code, _)| Ok((*code).into()))
    }

    pub fn district(&self) -> DatabaseResult<&'static str> {
        self.try_generate("district", || self.0.district())
    }

    pub fn regionAbbr(&self) -> DatabaseResult<&'static str> {
        self.try_generate("regionAbbr", || self.0.regionAbbr())
    }

    pub fn region(&self) -> DatabaseResult<&'static str> {
        self.try_generate("region", || self.0.region())
    }

    pub fn departmentName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("departmentName", || self.0.departmentName())
    }

    pub fn departmentNumber(&self) -> DatabaseResult<&'static str> {
        self.try_generate("departmentNumber", || self.0.departmentNumber())
    }

    pub fn randomChineseNumber(&self) -> DatabaseResult<&'static str> {
        self.try_generate("randomChineseNumber", || self.0.randomChineseNumber())
    }

    pub fn randomNumber2(&self) -> DatabaseResult<u8> {
        self.try_generate("randomNumber2", || self.0.randomNumber2())
    }

    pub fn randomNumber3(&self) -> DatabaseResult<u16> {
        self.try_generate("randomNumber3", || self.0.randomNumber3())
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

    pub fn buildingNumber(&self) -> DatabaseResult<String> {
        self.generate("buildingNumber", || self.0.buildingNumber())
    }

    pub fn secondaryAddress(&self) -> DatabaseResult<String> {
        self.generate("secondaryAddress", || self.0.secondaryAddress())
    }

    pub fn streetAddress(&self) -> DatabaseResult<String> {
        self.generate("streetAddress", || self.0.streetAddress())
    }

    pub fn address(&self) -> DatabaseResult<String> {
        self.generate("address", || self.0.address())
    }

    pub fn titleMale(&self) -> DatabaseResult<&'static str> {
        self.generate("titleMale", || self.0.titleMale())
    }

    pub fn titleFemale(&self) -> DatabaseResult<&'static str> {
        self.generate("titleFemale", || self.0.titleFemale())
    }

    pub fn name(&self) -> DatabaseResult<String> {
        self.generate("name", || self.0.name())
    }

    pub fn firstName(&self) -> DatabaseResult<String> {
        self.generate("firstName", || self.0.firstName())
    }

    pub fn lastName(&self) -> DatabaseResult<String> {
        self.generate("lastName", || self.0.lastName())
    }

    pub fn middleName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleName", || self.0.middleName())
    }

    pub fn middleNameWithGender(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.try_generate("middleName", || self.0.middleNameWithGender(gender))
    }

    pub fn middleNameMale(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleNameMale", || self.0.middleNameMale())
    }

    pub fn middleNameFemale(&self) -> DatabaseResult<&'static str> {
        self.try_generate("middleNameFemale", || self.0.middleNameFemale())
    }

    pub fn lastNameWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("lastName", || self.0.lastNameWithGender(gender))
    }

    pub fn lastNameMale(&self) -> DatabaseResult<String> {
        self.try_generate("lastNameMale", || self.0.lastNameMale())
    }

    pub fn lastNameFemale(&self) -> DatabaseResult<String> {
        self.try_generate("lastNameFemale", || self.0.lastNameFemale())
    }

    pub fn firstNameMaleNe(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstNameMaleNe", || self.0.firstNameMaleNe())
    }

    pub fn lastNameFemaleMarried(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastNameFemaleMarried", || self.0.lastNameFemaleMarried())
    }

    pub fn firstNameMale(&self) -> DatabaseResult<String> {
        self.generate("firstNameMale", || self.0.firstNameMale())
    }

    pub fn firstNameFemale(&self) -> DatabaseResult<String> {
        self.generate("firstNameFemale", || self.0.firstNameFemale())
    }

    pub fn myKadNumber(
        &self,
        gender: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("myKadNumber", || {
            self.0.myKadNumber(gender, formatted, separator)
        })
    }

    pub fn cnp(
        &self,
        gender: Option<&str>,
        dateOfBirth: Option<&str>,
        county: Option<&str>,
        isResident: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("cnp", || {
            self.0.cnp(gender, dateOfBirth, county, isResident)
        })
    }

    pub fn firstNameWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("firstName", || self.0.firstNameWithGender(gender))
    }

    pub fn nameWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("name", || self.0.nameWithGender(gender))
    }

    pub fn kanaName(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("kanaName", || self.0.kanaName(gender))
    }

    pub fn firstKanaName(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaName", || self.0.firstKanaName(gender))
    }

    pub fn firstKanaNameMale(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaNameMale", || self.0.firstKanaNameMale())
    }

    pub fn firstKanaNameFemale(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstKanaNameFemale", || self.0.firstKanaNameFemale())
    }

    pub fn lastKanaName(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastKanaName", || self.0.lastKanaName())
    }

    pub fn randomName(&self, pool: &[&str], count: usize) -> DatabaseResult<String> {
        self.try_generate("randomName", || self.0.randomName(pool, count))
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

    pub fn firstNameAscii(&self) -> DatabaseResult<&'static str> {
        self.try_generate("firstNameAscii", || self.0.firstNameAscii())
    }

    pub fn lastNameAscii(&self) -> DatabaseResult<&'static str> {
        self.try_generate("lastNameAscii", || self.0.lastNameAscii())
    }

    pub fn userName(&self) -> DatabaseResult<String> {
        self.generate("userName", || self.0.userName())
    }

    pub fn safeEmailDomain(&self) -> DatabaseResult<&'static str> {
        self.generate("safeEmailDomain", || self.0.safeEmailDomain())
    }

    pub fn freeEmailDomain(&self) -> DatabaseResult<&'static str> {
        self.generate("freeEmailDomain", || self.0.freeEmailDomain())
    }

    pub fn safeEmail(&self) -> DatabaseResult<String> {
        self.generate("safeEmail", || self.0.safeEmail())
    }

    pub fn freeEmail(&self) -> DatabaseResult<String> {
        self.generate("freeEmail", || self.0.freeEmail())
    }

    pub fn companyEmail(&self) -> DatabaseResult<String> {
        self.try_generate("companyEmail", || self.0.companyEmail())
    }

    pub fn email(&self) -> DatabaseResult<String> {
        self.try_generate("email", || self.0.email())
    }

    pub fn password(
        &self,
        minLength: Option<usize>,
        maxLength: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("password", || self.0.password(minLength, maxLength))
    }

    pub fn phoneNumber(&self) -> DatabaseResult<String> {
        self.generate("phoneNumber", || self.0.phoneNumber())
    }

    pub fn e164PhoneNumber(&self) -> DatabaseResult<String> {
        self.generate("e164PhoneNumber", || self.0.e164PhoneNumber())
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

    pub fn creditCardType(&self) -> DatabaseResult<&'static str> {
        self.generate("creditCardType", || self.0.creditCardType())
    }

    pub fn creditCardNumber(
        &self,
        cardType: Option<&str>,
        formatted: bool,
        separator: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("creditCardNumber", || {
            self.0.creditCardNumber(cardType, formatted, separator)
        })
    }

    pub fn creditCardExpirationDate(&self, valid: bool) -> DatabaseResult<DateTime<Tz>> {
        self.try_generate("creditCardExpirationDate", || {
            self.0.creditCardExpirationDate(valid)
        })
    }

    pub fn creditCardExpirationDateString(
        &self,
        valid: bool,
        expirationDateFormat: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("creditCardExpirationDateString", || {
            self.0
                .creditCardExpirationDateString(valid, expirationDateFormat)
        })
    }

    pub fn creditCardDetails(&self, valid: bool) -> DatabaseResult<CreditCardDetails> {
        self.generate_keyed(
            "creditCardDetails",
            || self.0.creditCardDetails(valid),
            faker_json_key,
        )
    }

    pub fn bank(&self) -> DatabaseResult<&'static str> {
        self.try_generate("bank", || self.0.bank())
    }

    pub fn microFinance(&self) -> DatabaseResult<String> {
        self.try_generate("microFinance", || self.0.microFinance())
    }

    pub fn swiftCode(&self) -> DatabaseResult<String> {
        self.try_generate("swiftCode", || self.0.swiftCode())
    }

    pub fn vat(&self, spacedNationalPrefix: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("vat", || self.0.vat(spacedNationalPrefix))
    }

    pub fn vatWithType(&self, vatType: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("vat", || self.0.vatWithType(vatType))
    }

    pub fn siren(&self, formatted: bool) -> DatabaseResult<String> {
        self.try_generate("siren", || self.0.siren(formatted))
    }

    pub fn vatId(&self) -> DatabaseResult<String> {
        self.try_generate("vatId", || self.0.vatId())
    }

    pub fn btw(&self) -> DatabaseResult<String> {
        self.try_generate("btw", || self.0.btw())
    }

    pub fn bankAccountNumber(
        &self,
        prefix: &str,
        countryCode: Option<&str>,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("bankAccountNumber", || {
            self.0.bankAccountNumber(prefix, countryCode, length)
        })
    }

    pub fn bankRoutingNumber(&self) -> DatabaseResult<String> {
        self.try_generate("bankRoutingNumber", || self.0.bankRoutingNumber())
    }

    pub fn iban(
        &self,
        countryCode: Option<&str>,
        prefix: &str,
        length: Option<usize>,
    ) -> DatabaseResult<String> {
        self.try_generate("iban", || self.0.iban(countryCode, prefix, length))
    }

    pub fn swiftBicNumber(&self) -> DatabaseResult<String> {
        self.generate("swiftBicNumber", || self.0.swiftBicNumber())
    }

    pub fn imei(&self) -> DatabaseResult<String> {
        self.generate("imei", || self.0.imei())
    }

    pub fn domainWord(&self) -> DatabaseResult<String> {
        self.try_generate("domainWord", || self.0.domainWord())
    }

    pub fn tld(&self) -> DatabaseResult<&'static str> {
        self.generate("tld", || self.0.tld())
    }

    pub fn domainName(&self) -> DatabaseResult<String> {
        self.try_generate("domainName", || self.0.domainName())
    }

    pub fn slug(
        &self,
        wordCount: Option<usize>,
        variableWordCount: bool,
    ) -> DatabaseResult<String> {
        self.generate("slug", || self.0.slug(wordCount, variableWordCount))
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

    pub fn localIpv4(&self) -> DatabaseResult<String> {
        self.generate("localIpv4", || self.0.localIpv4())
    }

    pub fn macAddress(&self) -> DatabaseResult<String> {
        self.generate("macAddress", || self.0.macAddress())
    }

    pub fn imageUrl(&self, options: &super::ImageOptions<'_>) -> DatabaseResult<String> {
        self.try_generate("imageUrl", || self.0.imageUrl(options))
    }

    pub fn mimeType(&self) -> DatabaseResult<&'static str> {
        self.generate("mimeType", || self.0.mimeType())
    }

    pub fn fileExtension(&self) -> DatabaseResult<&'static str> {
        self.generate("fileExtension", || self.0.fileExtension())
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

    pub fn randomElement<'a, T: serde::Serialize>(
        &self,
        values: &'a [T],
    ) -> DatabaseResult<Option<&'a T>> {
        self.generate_keyed(
            "randomElement",
            || Ok(self.0.randomElement(values)),
            faker_json_key,
        )
    }

    pub fn randomElements<'a, T: serde::Serialize>(
        &self,
        values: &'a [T],
        count: Option<usize>,
        allowDuplicates: bool,
    ) -> DatabaseResult<Vec<&'a T>> {
        self.generate_keyed(
            "randomElements",
            || self.0.randomElements(values, count, allowDuplicates),
            faker_json_key,
        )
    }

    pub fn shuffleArray<T: Clone + serde::Serialize>(
        &self,
        values: &[T],
    ) -> DatabaseResult<Vec<T>> {
        self.generate_keyed(
            "shuffleArray",
            || Ok(self.0.shuffleArray(values)),
            faker_json_key,
        )
    }

    pub fn randomKey<T>(&self, values: &[T]) -> DatabaseResult<Option<usize>> {
        self.generate_keyed(
            "randomKey",
            || Ok(self.0.randomKey(values)),
            |key| Ok(format!("{key:?}")),
        )
    }

    pub fn shuffleString(&self, value: &str) -> DatabaseResult<String> {
        self.generate("shuffleString", || self.0.shuffleString(value))
    }

    pub fn randomLetter(&self) -> DatabaseResult<char> {
        self.generate("randomLetter", || self.0.randomLetter())
    }

    pub fn randomAscii(&self) -> DatabaseResult<char> {
        self.generate("randomAscii", || self.0.randomAscii())
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
        self.generate("regexify", || pattern.sample())
    }

    pub fn asciify(&self, pattern: Option<&str>) -> DatabaseResult<String> {
        self.generate("asciify", || self.0.asciify(pattern))
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

    pub fn dateTimeBetweenExpressions(
        &self,
        start: &str,
        end: &str,
    ) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeBetween", || {
            self.0.dateTimeBetweenExpressions(start, end)
        })
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

    pub fn dateTimeInIntervalExpressions(
        &self,
        date: Option<&str>,
        interval: Option<&str>,
    ) -> DatabaseResult<DateTime<Utc>> {
        self.try_generate("dateTimeInInterval", || {
            self.0.dateTimeInIntervalExpressions(date, interval)
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

    pub fn unixTimeExpression(&self, max: &str) -> DatabaseResult<i64> {
        let end = self
            .0
            .resolve_timestamp_max(max, Utc::now())
            .or_else(faker_timestamp)?;
        self.generate("unixTime", || self.0.numberBetween(0, end))
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

    pub fn dateWithPhpFormat(
        &self,
        format: &str,
        max: Option<DateTime<Utc>>,
    ) -> DatabaseResult<String> {
        self.try_generate("date", || self.0.dateWithPhpFormat(format, max))
    }

    pub fn timeWithPhpFormat(
        &self,
        format: &str,
        max: Option<DateTime<Utc>>,
    ) -> DatabaseResult<String> {
        self.try_generate("time", || self.0.timeWithPhpFormat(format, max))
    }

    pub fn iso8601(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("iso8601", || self.0.iso8601(max))
    }

    pub fn dateTimeAD(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<DateTime<Utc>> {
        self.generate("dateTimeAD", || self.0.dateTimeAD(max))
    }

    pub fn dateTimeADInTimezone(
        &self,
        max: Option<DateTime<Utc>>,
        timezone: Option<Tz>,
    ) -> DatabaseResult<DateTime<Tz>> {
        self.generate_zoned("dateTimeAD", || {
            Ok(self.0.dateTimeADInTimezone(max, timezone))
        })
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

    pub fn monthNameGenitive(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<&'static str> {
        self.try_generate("monthNameGenitive", || self.0.monthNameGenitive(max))
    }

    pub fn formattedDate(&self) -> DatabaseResult<String> {
        self.try_generate("formattedDate", || self.0.formattedDate())
    }

    pub fn year(&self, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.generate("year", || self.0.year(max))
    }

    pub fn century(&self) -> DatabaseResult<&'static str> {
        self.generate("century", || self.0.century())
    }

    pub fn timezone(&self, countryCode: Option<&str>) -> DatabaseResult<Option<&'static str>> {
        // The empty key represents PHP's null result for an unknown country.
        let zone = self.generate("timezone", || {
            self.0.timezone(countryCode).unwrap_or_default()
        })?;
        Ok((!zone.is_empty()).then_some(zone))
    }

    pub fn dateTimeInTimezone(
        &self,
        max: Option<DateTime<Utc>>,
        timezone: Option<Tz>,
    ) -> DatabaseResult<DateTime<Tz>> {
        self.generate_zoned("dateTime", || Ok(self.0.dateTimeInTimezone(max, timezone)))
    }

    fn generate_zoned(
        &self,
        formatter: &'static str,
        value: impl FnMut() -> DatabaseResult<DateTime<Tz>>,
    ) -> DatabaseResult<DateTime<Tz>> {
        self.generate_keyed(formatter, value, |date| {
            Ok(if date.timezone() == UTC {
                date.with_timezone(&Utc).to_string()
            } else {
                // PHP serializes named DateTime results as a wall clock and a zone name.
                let local_seconds =
                    date.timestamp() + i64::from(date.offset().fix().local_minus_utc());
                format!(
                    "{local_seconds}:{} [{}]",
                    date.timestamp_subsec_nanos(),
                    date.timezone().name()
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
        mut value: impl FnMut() -> DatabaseResult<T>,
        key: impl Fn(&T) -> DatabaseResult<String>,
    ) -> DatabaseResult<T> {
        for attempt in 0..=self.1 {
            let value = value()?;
            if attempt == self.1 {
                break;
            }
            let key = key(&value)?;
            if self
                .0
                .unique
                .lock()
                .unwrap()
                .entry(formatter)
                .or_default()
                .insert(key)
            {
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
    let mut output = Str::transliterateWithUnknown(value, Some(""));
    output.retain(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.'));
    output.make_ascii_lowercase();
    output
}

static US_AREA_CODES: OnceLock<Vec<RegexPattern>> = OnceLock::new();
static DE_AREA_CODES: OnceLock<Vec<RegexPattern>> = OnceLock::new();

fn phone_area_code(cache: &OnceLock<Vec<RegexPattern>>, formats: &[&str]) -> String {
    let patterns = cache.get_or_init(|| {
        formats
            .iter()
            .map(|pattern| RegexPattern::parse(pattern).expect("fixed Faker area-code pattern"))
            .collect()
    });
    let digit = fastrand::u8(2..=9);
    format!("{digit}{}", patterns[usize::from(digit - 2)].sample())
}
