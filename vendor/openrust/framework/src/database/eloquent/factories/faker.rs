use super::{
    faker_address::DISTRICTS_ZH_CN, faker_miscellaneous, faker_person, faker_phone,
    regex_pattern::RegexPattern,
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Write as _,
    net::Ipv4Addr,
    ops::Range,
    sync::{Mutex, OnceLock},
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
    TimeZone, Utc,
};
use chrono_tz::{GapInfo, Tz, UTC};
use md5::{Digest as _, Md5};

/// A sampled latitude and longitude in signed decimal degrees.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
pub struct Coordinates {
    pub latitude: f64,
    pub longitude: f64,
}

/// Generates localized sample data for tests and seeders.
pub struct Faker {
    locale: String,
    application_timezone: Tz,
    default_timezone: Option<Tz>,
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
    city => faker::address::raw::CityName,
    streetName => faker::address::raw::StreetName,
    cityPrefix => faker::address::raw::CityPrefix,
    citySuffix => faker::address::raw::CitySuffix,
    streetSuffix => faker::address::raw::StreetSuffix,
    country => faker::address::raw::CountryName,
    state => faker::address::raw::StateName,
    stateAbbr => faker::address::raw::StateAbbr,
    company => faker::company::raw::CompanyName,
    word => faker::lorem::raw::Word,
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

/// Creates a generator using an explicit locale or the scoped application configuration.
pub fn fake(locale: Option<&str>) -> DatabaseResult<Faker> {
    Faker::configured(Connection::model_container(None).as_deref(), locale)
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn new(locale: &str) -> DatabaseResult<Self> {
        let locale = canonical_locale(locale)
            .map_err(|_| DatabaseError::InvalidConfiguration("app.faker_locale"))?;
        Ok(Self {
            locale,
            application_timezone: UTC,
            default_timezone: None,
            unique: Mutex::default(),
        })
    }

    pub(super) fn for_connection(connection: &Connection) -> DatabaseResult<Self> {
        Self::configured(
            Connection::model_container(Some(connection)).as_deref(),
            None,
        )
    }

    fn configured(container: Option<&Container>, locale: Option<&str>) -> DatabaseResult<Self> {
        let Some(container) = container else {
            return Self::new(locale.unwrap_or("en_US"));
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
        let mut faker = Self::new(&locale)?;
        faker.application_timezone = match config.get("app.timezone") {
            None => UTC,
            Some(serde_json::Value::String(zone)) => zone
                .parse()
                .map_err(|_| DatabaseError::InvalidConfiguration("app.timezone"))?,
            _ => return Err(DatabaseError::InvalidConfiguration("app.timezone")),
        };
        Ok(faker)
    }

    /// Overrides the time zone for this generator. Pass None to restore the application default.
    pub fn setDefaultTimezone(&mut self, timezone: Option<Tz>) {
        self.default_timezone = timezone;
    }

    pub fn getDefaultTimezone(&self) -> Option<Tz> {
        self.default_timezone
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

    pub fn postcode(&self) -> String {
        match self.locale() {
            "en_US" => {
                let format = if self.boolean(None) {
                    "#####"
                } else {
                    "#####-####"
                };
                self.numerify(Some(format))
            }
            "zh_CN" => format!("{:02}{}00", fastrand::u8(1..=85), fastrand::u8(10..=88)),
            _ => generate!(self.locale.as_str(), faker::address::raw::PostCode,),
        }
    }

    pub fn buildingNumber(&self) -> String {
        let formats: &[&str] = match self.locale() {
            "en_US" => &["%####", "%###", "%##"],
            "fr_FR" => &["%", "%#", "%#", "%#", "%##"],
            _ => return generate!(self.locale.as_str(), faker::address::raw::BuildingNumber,),
        };
        self.numerify(Some(formats[fastrand::usize(..formats.len())]))
    }

    pub fn secondaryAddress(&self) -> String {
        let formats: &[&str] = match self.locale() {
            "en_US" => &["Apt. ###", "Suite ###"],
            "fr_FR" => &[
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
            _ => return generate!(self.locale.as_str(), faker::address::raw::SecondaryAddress,),
        };
        self.numerify(Some(formats[fastrand::usize(..formats.len())]))
    }

    pub fn streetAddress(&self) -> String {
        let street = self.streetName();
        match self.locale() {
            "en_US" => {
                let mut address = format!("{} {street}", self.buildingNumber());
                if self.boolean(None) {
                    write!(address, " {}", self.secondaryAddress())
                        .expect("writing to String cannot fail");
                }
                address
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
        match self.locale() {
            "en_US" => format!(
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
            "fr_FR" => format!(
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

    pub fn firstNameMale(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.locale()).male)
    }

    pub fn firstNameFemale(&self) -> String {
        self.sample_given_name(faker_person::catalog(self.locale()).female)
    }

    pub fn firstNameWithGender(&self, gender: Option<&str>) -> String {
        match gender {
            Some("male") => self.firstNameMale(),
            Some("female") => self.firstNameFemale(),
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

    fn sample_given_name(&self, names: &[&str]) -> String {
        let first = names[fastrand::usize(..names.len())];
        if self.locale() == "zh_TW" && self.boolean(None) {
            return format!("{first}{}", names[fastrand::usize(..names.len())]);
        }
        first.to_owned()
    }

    fn render_person_name(&self, mut format: &str, catalog: &faker_person::PersonNames) -> String {
        let mut name = String::new();
        while let Some((literal, token)) = format.split_once("{{") {
            let (token, remaining) = token.split_once("}}").expect("fixed Faker name template");
            name.push_str(literal);
            match token {
                "firstNameMale" => name.push_str(&self.firstNameMale()),
                "firstNameFemale" => name.push_str(&self.firstNameFemale()),
                "lastName" => name.push_str(&self.lastName()),
                "titleMale" => name.push_str(self.titleMale()),
                "titleFemale" => name.push_str(self.titleFemale()),
                "title" => name.push_str(self.title(None)),
                "prefix" => {
                    name.push_str(catalog.prefixes[fastrand::usize(..catalog.prefixes.len())])
                }
                "suffix" => {
                    name.push_str(catalog.suffixes[fastrand::usize(..catalog.suffixes.len())])
                }
                _ => unreachable!("fixed Faker name template token"),
            }
            format = remaining;
        }
        name.push_str(format);
        name
    }

    pub fn title(&self, gender: Option<&str>) -> &'static str {
        match gender {
            Some("male") => self.titleMale(),
            Some("female") => self.titleFemale(),
            _ if self.boolean(None) => self.titleMale(),
            _ => self.titleFemale(),
        }
    }

    pub fn titleMale(&self) -> &'static str {
        let titles: &[&str] = match self.locale() {
            "nl_NL" => faker_person::NL_NL_TITLES,
            "tr_TR" => faker_person::TR_TR_TITLES,
            "ar_SA" => &["السيد", "الأستاذ", "الدكتور", "المهندس"],
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
            "nl_NL" => faker_person::NL_NL_TITLES,
            "tr_TR" => faker_person::TR_TR_TITLES,
            "ar_SA" => &["السيدة", "الآنسة", "الدكتورة", "المهندسة"],
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

    pub fn userName(&self) -> String {
        let first = || {
            if self.locale() == "zh_CN" {
                self.word()
            } else {
                self.firstName()
            }
        };
        let last = || {
            if self.locale() == "zh_CN" {
                self.word()
            } else {
                self.lastName()
            }
        };
        let candidate = match fastrand::u8(0..4) {
            0 => format!("{}.{}", last(), first()),
            1 if self.locale() == "zh_CN" => format!("{}_{}", first(), last()),
            1 => format!("{}.{}", first(), last()),
            2 => format!("{}{:02}", first(), fastrand::u8(0..100)),
            _ => format!("{}{}", self.randomLetter(), last()),
        };
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
        let domains: &[&str] = match self.locale() {
            "de_DE" => &[
                "web.de",
                "gmail.com",
                "hotmail.de",
                "yahoo.de",
                "googlemail.com",
                "aol.de",
                "gmx.de",
                "freenet.de",
                "posteo.de",
                "mail.de",
                "live.de",
                "t-online.de",
            ],
            "fr_FR" => &[
                "gmail.com",
                "hotmail.fr",
                "yahoo.fr",
                "laposte.net",
                "free.fr",
                "sfr.fr",
                "orange.fr",
                "club-internet.fr",
                "dbmail.com",
                "live.com",
                "noos.fr",
                "tele2.fr",
                "wanadoo.fr",
            ],
            "it_IT" => &[
                "gmail.com",
                "yahoo.com",
                "hotmail.com",
                "email.it",
                "libero.it",
                "yahoo.it",
            ],
            "ja_JP" => &[
                "gmail.com",
                "yahoo.co.jp",
                "hotmail.co.jp",
                "mail.goo.ne.jp",
            ],
            "nl_NL" => &["gmail.com", "hotmail.nl", "live.nl", "yahoo.nl"],
            "pt_BR" => &[
                "gmail.com",
                "yahoo.com",
                "hotmail.com",
                "uol.com.br",
                "terra.com.br",
                "ig.com.br",
                "r7.com",
            ],
            "pt_PT" => &[
                "gmail.com",
                "yahoo.com",
                "hotmail.com",
                "sapo.pt",
                "clix.pt",
                "mail.pt",
            ],
            "tr_TR" => &[
                "gmail.com",
                "hotmail.com",
                "yahoo.com",
                "yandex.com.tr",
                "mynet.com",
                "turk.net",
                "superposta.com",
            ],
            "zh_CN" => &[
                "gmail.com",
                "yahoo.com",
                "hotmail.com",
                "126.com",
                "163.com",
                "qq.com",
                "sohu.com",
                "sina.com",
            ],
            _ => &["gmail.com", "yahoo.com", "hotmail.com"],
        };
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
        if self.locale() == "zh_CN" || self.boolean(None) {
            Ok(self.freeEmail())
        } else {
            self.companyEmail()
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

    pub fn phoneNumber(&self) -> String {
        match self.locale() {
            "zh_CN" => {
                let operators = faker_phone::CHINA_OPERATORS;
                let prefix = operators[fastrand::usize(..operators.len())];
                format!(
                    "{prefix}{}",
                    self.numerify(Some(&"###########"[prefix.len()..]))
                )
            }
            "zh_TW" => {
                let formats = faker_phone::TAIWAN;
                self.numerify(Some(formats[fastrand::usize(..formats.len())]))
            }
            _ => generate!(self.locale.as_str(), faker::phone_number::raw::PhoneNumber,),
        }
    }

    pub fn e164PhoneNumber(&self) -> String {
        match self.locale() {
            "en_US" => {
                let area = phone_area_code(&US_AREA_CODES, faker_phone::AREA_EN_US);
                let first = fastrand::u8(2..=9);
                let middle = self.randomDigit();
                let last = if middle == 1 {
                    self.randomDigitNot(1)
                } else {
                    self.randomDigit()
                };
                let subscriber = self.numerify(Some("####"));
                format!("+1{area}{first}{middle}{last}{subscriber}")
            }
            "de_DE" => {
                let area = phone_area_code(&DE_AREA_CODES, faker_phone::AREA_DE_DE);
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
        let suffixes: &[&str] = match self.locale() {
            "ar_SA" => &["biz", "com", "info", "sa", "net", "org"],
            "de_DE" => &["com", "com", "com", "net", "org", "de", "de", "de"],
            "fa_IR" => &[
                "biz", "com", "info", "ac.ir", "sch.ir", "co.ir", "ir", "net", "org",
            ],
            "fr_FR" => &["com", "com", "com", "net", "org", "fr", "fr", "fr"],
            "it_IT" => &["com", "com", "com", "net", "org", "it", "it", "it"],
            "ja_JP" => &[
                "com", "com", "com", "biz", "info", "net", "org", "jp", "jp", "jp",
            ],
            "nl_NL" => &["com", "com", "com", "net", "org", "nl", "nl", "nl"],
            "pt_BR" => &[
                "com", "com", "com.br", "com.br", "net", "net.br", "br", "org",
            ],
            "pt_PT" => &["com", "com", "pt", "pt", "net", "org", "eu"],
            "tr_TR" => &[
                "com", "com", "com", "com", "com.tr", "com.tr", "info", "net", "org", "org.tr",
                "edu", "edu.tr", "edu.tr",
            ],
            "zh_CN" => &[
                "com", "com", "com", "com", "com", "com", "biz", "info", "net", "org", "cn",
                "com.cn", "edu.cn", "net.cn", "biz.cn", "gov.cn", "org.cn",
            ],
            _ => &[
                "com", "com", "com", "com", "com", "com", "biz", "info", "net", "org",
            ],
        };
        suffixes[fastrand::usize(..suffixes.len())]
    }

    pub fn domainName(&self) -> DatabaseResult<String> {
        Ok(format!("{}.{}", self.domainWord()?, self.tld()))
    }

    pub fn slug(&self, wordCount: Option<usize>, variableWordCount: bool) -> String {
        self.words(lorem_count(wordCount.unwrap_or(6), variableWordCount))
            .join("-")
    }

    pub fn url(&self) -> DatabaseResult<String> {
        let (prefix, path, extension) = match fastrand::u8(0..10) {
            0 => ("http://www.", false, ""),
            1 => ("http://", false, ""),
            2 | 3 => ("http://www.", true, ""),
            4 => ("https://www.", true, ""),
            5 => ("http://www.", true, ".html"),
            6 | 7 => ("http://", true, ""),
            8 => ("http://", true, ".html"),
            _ => ("https://", true, ".html"),
        };
        let mut url = format!("{prefix}{}/", self.domainName()?);
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
        let timezone = timezone
            .or(self.default_timezone)
            .unwrap_or(self.application_timezone);
        self.dateTime(max).with_timezone(&timezone)
    }

    pub fn date(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%Y-%m-%d"), max)
    }

    pub fn time(&self, format: Option<&str>, max: Option<DateTime<Utc>>) -> DatabaseResult<String> {
        self.format_date(format.unwrap_or("%H:%M:%S"), max)
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

    pub fn amPm(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%P").to_string()
    }

    pub fn dayOfMonth(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%d").to_string()
    }

    pub fn dayOfWeek(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%A").to_string()
    }

    pub fn month(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%m").to_string()
    }

    pub fn monthName(&self, max: Option<DateTime<Utc>>) -> String {
        self.dateTimeInTimezone(max, None).format("%B").to_string()
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
pub struct UniqueFaker<'a>(&'a Faker);

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

    pub fn firstNameMale(&self) -> DatabaseResult<String> {
        self.generate("firstNameMale", || self.0.firstNameMale())
    }

    pub fn firstNameFemale(&self) -> DatabaseResult<String> {
        self.generate("firstNameFemale", || self.0.firstNameFemale())
    }

    pub fn firstNameWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("firstName", || self.0.firstNameWithGender(gender))
    }

    pub fn nameWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.generate("name", || self.0.nameWithGender(gender))
    }

    pub fn title(&self, gender: Option<&str>) -> DatabaseResult<&'static str> {
        self.generate("title", || self.0.title(gender))
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
        self.generate("dateTimeInTimezone", || {
            self.0.dateTimeInTimezone(max, timezone)
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
        for _ in 0..10_000 {
            let value = value()?;
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
