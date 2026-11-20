use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use jiff::{Timestamp, tz::TimeZone};
use regex::Regex;
use serde_json::Value as Json;

use super::Faker;
use crate::database::{DatabaseError, DatabaseResult, Value};

#[path = "faker_named_identity.rs"]
mod named_identity;
#[path = "faker_named_phone.rs"]
mod named_phone;
#[path = "faker_named_regional.rs"]
mod named_regional;
#[path = "faker_named_text.rs"]
mod named_text;

type Formatter = dyn Fn(&Faker, &[Value]) -> DatabaseResult<Value> + Send + Sync;
type Builtin = fn(&Faker, &[Value]) -> DatabaseResult<Value>;
pub(super) type Formatters = HashMap<String, Arc<Formatter>>;

pub(super) type Extensions = HashMap<String, Arc<Formatters>>;

const BARCODE: &str = r"Faker\Extension\BarcodeExtension";
const BLOOD: &str = r"Faker\Extension\BloodExtension";
const COLOR: &str = r"Faker\Extension\ColorExtension";
const DATE_TIME: &str = r"Faker\Extension\DateTimeExtension";
const VERSION: &str = r"Faker\Extension\VersionExtension";
const FILE: &str = r"Faker\Extension\FileExtension";
const NUMBER: &str = r"Faker\Extension\NumberExtension";
const UUID: &str = r"Faker\Extension\UuidExtension";
const BUILTIN_EXTENSIONS: &[&str] = &[
    BARCODE, BLOOD, COLOR, DATE_TIME, FILE, NUMBER, UUID, VERSION,
];

/// A resolved extension bound to its generator.
pub struct FakerExtension<'a> {
    faker: &'a Faker,
    methods: ExtensionMethods,
}

enum ExtensionMethods {
    Builtin(&'static str),
    Custom(Arc<Formatters>),
}

impl FakerExtension<'_> {
    fn method(&self, name: &str) -> Option<Resolved> {
        let name = name.to_ascii_lowercase();
        match &self.methods {
            ExtensionMethods::Builtin(id) => extension_method(id, &name).map(Resolved::Builtin),
            ExtensionMethods::Custom(methods) => methods.get(&name).cloned().map(Resolved::Custom),
        }
    }

    /// Invokes a method of this extension with positional arguments.
    pub fn format(&self, method: &str, arguments: &[Value]) -> DatabaseResult<Value> {
        let formatter = self
            .method(method)
            .ok_or_else(|| DatabaseError::UnknownFakerFormatter(method.to_owned()))?;
        formatter.call(self.faker, arguments)
    }
}

/// A set of named formatters that can be added to a Faker generator.
#[derive(Default)]
pub struct FakerProvider {
    formatters: Formatters,
}

impl FakerProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a formatter. A later entry of the same name replaces the previous one.
    pub fn formatter(
        mut self,
        name: &str,
        callback: impl Fn(&Faker, &[Value]) -> DatabaseResult<Value> + Send + Sync + 'static,
    ) -> Self {
        self.formatters
            .insert(name.to_ascii_lowercase(), Arc::new(callback));
        self
    }
}

enum Resolved {
    Builtin(Builtin),
    Custom(Arc<Formatter>),
}

impl Resolved {
    fn call(&self, faker: &Faker, arguments: &[Value]) -> DatabaseResult<Value> {
        match self {
            Self::Builtin(callback) => callback(faker, arguments),
            Self::Custom(callback) => callback(faker, arguments),
        }
    }
}

#[allow(non_snake_case, reason = "Matches Faker generator methods")]
impl Faker {
    /// Installs every formatter on this generator. The latest provider wins.
    pub fn addProvider(&self, provider: FakerProvider) {
        let replaced = {
            let mut formatters = self.formatters.write().unwrap();
            provider
                .formatters
                .into_iter()
                .filter_map(|(name, callback)| formatters.insert(name, callback))
                .collect::<Vec<_>>()
        };
        // Captured values may have destructors that use this generator.
        drop(replaced);
    }

    /// Registers an extension under a case-sensitive container identifier.
    pub fn addExtension(&self, id: &str, provider: FakerProvider) {
        let replaced = self
            .extensions
            .write()
            .unwrap()
            .insert(id.to_owned(), Arc::new(provider.formatters));
        drop(replaced);
    }

    /// Resolves an extension without invoking any of its methods.
    pub fn ext(&self, id: &str) -> DatabaseResult<FakerExtension<'_>> {
        let custom = self.extensions.read().unwrap().get(id).cloned();
        let methods = if let Some(methods) = custom {
            ExtensionMethods::Custom(methods)
        } else if let Some(&builtin) = BUILTIN_EXTENSIONS.iter().find(|&&builtin| builtin == id) {
            ExtensionMethods::Builtin(builtin)
        } else {
            return Err(DatabaseError::UnknownFakerExtension(id.to_owned()));
        };
        Ok(FakerExtension {
            faker: self,
            methods,
        })
    }

    /// Resolves a formatter by name. The returned callback retains the resolved provider.
    pub fn getFormatter<'a>(
        &'a self,
        name: &str,
    ) -> DatabaseResult<impl Fn(&[Value]) -> DatabaseResult<Value> + 'a + use<'a>> {
        let key = name.to_ascii_lowercase();
        let formatter =
            if let Some((id, method)) = qualified(name).or_else(|| generator_extension(&key)) {
                self.ext(id)?
                    .method(method)
                    .ok_or_else(|| DatabaseError::UnknownFakerFormatter(name.to_owned()))?
            } else if key == "parse" {
                Resolved::Builtin(parse_formatter)
            } else if let Some(callback) = self.formatters.read().unwrap().get(&key).cloned() {
                Resolved::Custom(callback)
            } else if let Some(builtin) = defaults(&key) {
                Resolved::Builtin(builtin)
            } else {
                return Err(DatabaseError::UnknownFakerFormatter(name.to_owned()));
            };

        Ok(move |arguments: &[Value]| formatter.call(self, arguments))
    }

    pub fn format(&self, name: &str, arguments: &[Value]) -> DatabaseResult<Value> {
        self.getFormatter(name)?(arguments)
    }

    /// Invokes a named formatter, awaiting I/O when the formatter downloads an image.
    pub async fn formatAsync(&self, name: &str, arguments: &[Value]) -> DatabaseResult<Value> {
        match self.getFormatter(name) {
            Ok(formatter) => formatter(arguments),
            Err(DatabaseError::UnknownFakerFormatter(_)) if name.eq_ignore_ascii_case("image") => {
                named_identity::image(self, arguments).await
            }
            Err(error) => Err(error),
        }
    }

    /// Replaces named formatter tokens with their scalar results.
    pub fn parse(&self, template: &str) -> DatabaseResult<String> {
        static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"\{\{\s?(\w+|[\w\\]+->\w+?)\s?\}\}").expect("valid Faker token pattern")
        });

        let mut rendered = String::with_capacity(template.len());
        let mut offset = 0;
        for captures in TOKENS.captures_iter(template) {
            let token = captures.get(0).expect("complete Faker token");
            rendered.push_str(&template[offset..token.start()]);
            match self.format(&captures[1], &[])? {
                Value::Text(text) => rendered.push_str(&text),
                Value::Integer(value) => rendered.push_str(&value.to_string()),
                Value::Unsigned(value) => rendered.push_str(&value.to_string()),
                Value::Real(value) => rendered.push_str(&value.to_string()),
                Value::Bool(true) => rendered.push('1'),
                Value::Bool(false) | Value::Null => {}
                _ => {
                    return Err(DatabaseError::InvalidQuery(
                        "Faker template requires a scalar",
                    ));
                }
            }
            offset = token.end();
        }
        rendered.push_str(&template[offset..]);
        Ok(rendered)
    }
}

fn qualified(name: &str) -> Option<(&str, &str)> {
    let (id, method) = name.split_once("->")?;
    (!id.is_empty()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'\\')
        && !method.is_empty()
        && method.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    .then_some((id, method))
}

fn generator_extension(name: &str) -> Option<(&'static str, &str)> {
    Some(match name {
        "mimetype" | "filepath" => (FILE, name),
        "semver" => (VERSION, name),
        "fileextension" => (FILE, "extension"),
        "bloodtype" | "bloodrh" | "bloodgroup" => (BLOOD, name),
        "ean13" | "ean8" | "isbn10" | "isbn13" => (BARCODE, name),
        "randomdigit" | "randomdigitnotzero" | "numberbetween" | "randomdigitnot"
        | "randomnumber" | "randomfloat" => (NUMBER, name),
        _ => return None,
    })
}

fn parse_formatter(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    match args {
        [Value::Text(template)] => faker.parse(template).map(Value::from),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker parse expects a template",
        )),
    }
}

fn extension_method(id: &str, name: &str) -> Option<Builtin> {
    Some(match (id, name) {
        (BARCODE, "ean13") => |faker, _| Ok(faker.ean13().into()),
        (BARCODE, "ean8") => |faker, _| Ok(faker.ean8().into()),
        (BARCODE, "isbn10") => |faker, _| Ok(faker.isbn10().into()),
        (BARCODE, "isbn13") => |faker, _| Ok(faker.isbn13().into()),
        (BLOOD, "bloodtype") => |faker, _| Ok(faker.bloodType().into()),
        (BLOOD, "bloodrh") => |faker, _| Ok(faker.bloodRh().into()),
        (BLOOD, "bloodgroup") => |faker, _| Ok(faker.bloodGroup().into()),
        (COLOR, name) => return color_formatter(name, true),
        (DATE_TIME, "date") => |faker, args| php_date(faker, args, "Y-m-d"),
        (DATE_TIME, "time") => |faker, args| php_date(faker, args, "H:i:s"),
        (DATE_TIME, "unixtime") => |faker, args| {
            let [until] = arguments(args)?;
            match until {
                Some(Value::Text(expression)) => {
                    faker.unixTimeExpression(expression).map(Value::from)
                }
                _ => Ok(faker.unixTime(date_max(faker, until)?).into()),
            }
        },
        (DATE_TIME, "datetime") => |faker, args| date_time(faker, args, false),
        (DATE_TIME, "datetimead") => |faker, args| date_time(faker, args, true),
        (DATE_TIME, "datetimebetween") => date_between,
        (DATE_TIME, "datetimeininterval") => date_interval,
        (DATE_TIME, "datetimethisweek") => {
            |faker, args| date_period(faker, args, "monday this week", "sunday this week")
        }
        (DATE_TIME, "datetimethismonth") => |faker, args| {
            date_period(
                faker,
                args,
                "first day of this month",
                "last day of this month",
            )
        },
        (DATE_TIME, "datetimethisyear") => {
            |faker, args| date_period(faker, args, "first day of january", "last day of december")
        }
        (DATE_TIME, "datetimethisdecade") => |faker, args| date_calendar_period(faker, args, 10),
        (DATE_TIME, "datetimethiscentury") => |faker, args| date_calendar_period(faker, args, 100),
        (DATE_TIME, "iso8601") => |faker, args| date_component(faker, args, "Y-m-d\\TH:i:sO"),
        (DATE_TIME, "ampm") => |faker, args| date_component(faker, args, "a"),
        (DATE_TIME, "dayofmonth") => |faker, args| date_component(faker, args, "d"),
        (DATE_TIME, "dayofweek") => |faker, args| date_component(faker, args, "l"),
        (DATE_TIME, "month") => |faker, args| date_component(faker, args, "m"),
        (DATE_TIME, "monthname") => |faker, args| date_component(faker, args, "F"),
        (DATE_TIME, "year") => |faker, args| date_component(faker, args, "Y"),
        (DATE_TIME, "century") => |faker, _| Ok(faker.century().into()),
        (DATE_TIME, "timezone") => |faker, args| {
            let [country] = arguments(args)?;
            faker
                .timezone(text(country)?.filter(|country| !country.is_empty()))
                .map(Value::from)
                .ok_or(DatabaseError::InvalidQuery(
                    "Faker time zone country has no matches",
                ))
        },
        (VERSION, "semver") => |faker, args| {
            let [pre_release, build] = arguments(args)?;
            faker
                .semver(flag(pre_release)?, flag(build)?)
                .map(Value::from)
        },
        (FILE, "filepath") => |faker, args| {
            let [] = arguments(args)?;
            let file = faker
                .create_temporary_file()
                .map_err(DatabaseError::FakerIo)?;
            let path = file
                .path()
                .to_str()
                .ok_or(DatabaseError::InvalidQuery("Faker file path is not UTF-8"))?
                .to_owned();
            file.keep()
                .map_err(|error| DatabaseError::FakerIo(error.error))?;
            Ok(path.into())
        },
        (FILE, "mimetype") => |faker, _| Ok(faker.mimeType().into()),
        (FILE, "extension") => |faker, _| Ok(faker.fileExtension().into()),
        (UUID, "uuid3") => |faker, _| Ok(faker.uuid().to_string().into()),
        (NUMBER, "randomdigit") => |faker, _| Ok(i64::from(faker.randomDigit()).into()),
        (NUMBER, "randomdigitnotzero") => {
            |faker, _| Ok(i64::from(faker.randomDigitNotNull()).into())
        }
        (NUMBER, "numberbetween") => |faker, args| {
            let [first, second] = arguments(args)?;
            Ok(faker
                .numberBetween(
                    integer(first)?.unwrap_or(0),
                    integer(second)?.unwrap_or(if second.is_none() {
                        i64::from(i32::MAX)
                    } else {
                        0
                    }),
                )
                .into())
        },
        (NUMBER, "randomdigitnot") => |faker, args| {
            let [Some(except)] = arguments(args)? else {
                return Err(DatabaseError::InvalidQuery(
                    "Faker randomDigitNot requires an integer",
                ));
            };
            Ok(i64::from(faker.randomDigitNot(integer(Some(except))?.unwrap_or(0))).into())
        },
        (NUMBER, "randomnumber") => |faker, args| {
            let [digits, strict] = arguments(args)?;
            let digits = integer(digits)?
                .map(u32::try_from)
                .transpose()
                .map_err(|_| {
                    DatabaseError::InvalidQuery("Faker random number exceeds digit limit")
                })?;
            let strict = match strict {
                None | Some(Value::Null) => false,
                Some(Value::Bool(value)) => *value,
                _ => {
                    return Err(DatabaseError::InvalidQuery(
                        "Faker strict requires a boolean",
                    ));
                }
            };
            faker.randomNumber(digits, strict).map(Value::from)
        },
        (NUMBER, "randomfloat") => |faker, args| {
            let [decimals, min, max] = arguments(args)?;
            let decimals = integer(decimals)?
                .map(i32::try_from)
                .transpose()
                .map_err(|_| {
                    DatabaseError::InvalidQuery("Faker decimal precision is out of range")
                })?;
            faker
                .randomFloat(decimals, real(min)?.unwrap_or(0.0), real(max)?)
                .map(Value::Real)
        },
        _ => return None,
    })
}

fn defaults(name: &str) -> Option<Builtin> {
    Some(match name {
        "address" => |faker, _| Ok(faker.address().into()),
        "city" => |faker, _| Ok(faker.city().into()),
        "company" => |faker, _| Ok(faker.company().into()),
        "companyen" => |faker, _| faker.companyEn().map(Value::from),
        "email" => |faker, _| faker.email().map(Value::from),
        "freeemail" => |faker, _| Ok(faker.freeEmail().into()),
        "phonenumber" => |faker, _| Ok(faker.phoneNumber().into()),
        "safeemail" => |faker, _| Ok(faker.safeEmail().into()),
        "username" => |faker, _| Ok(faker.userName().into()),
        "uuid" => |faker, _| Ok(faker.uuid().to_string().into()),
        "randomdigitnotnull" => |faker, _| Ok(i64::from(faker.randomDigitNotNull()).into()),
        "boolean" => |faker, args| {
            let [chance] = arguments(args)?;
            Ok(faker
                .boolean(integer(chance)?.or_else(|| chance.map(|_| 0)))
                .into())
        },
        "locale" => |faker, _| Ok(faker.randomLocale().into()),
        "languagecode" => |faker, _| Ok(faker.languageCode().into()),
        "countrycode" => |faker, _| Ok(faker.countryCode().into()),
        "countryisoalpha3" => |faker, _| Ok(faker.countryISOAlpha3().into()),
        "currencycode" => |faker, _| Ok(faker.currencyCode().into()),
        "emoji" => |faker, _| Ok(faker.emoji().into()),
        "tolower" => |_, args| {
            let [input] = arguments(args)?;
            Ok(text(input)?.unwrap_or_default().to_lowercase().into())
        },
        "toupper" => |_, args| {
            let [input] = arguments(args)?;
            Ok(text(input)?.unwrap_or_default().to_uppercase().into())
        },
        "biasednumberbetween" => |faker, args| {
            let [min, max, curve] = arguments(args)?;
            if !matches!(text(curve)?, None | Some("sqrt")) {
                return Err(DatabaseError::InvalidQuery(
                    "Faker named bias must be sqrt; use biasedNumberBetween for custom curves",
                ));
            }
            faker
                .biasedNumberBetween(integer(min)?, integer(max)?, None)
                .map(Value::from)
        },
        "latitude" => |faker, args| coordinate(faker, args, 90.0),
        "longitude" => |faker, args| coordinate(faker, args, 180.0),
        "randomelement" => |faker, args| {
            let [input] = arguments(args)?;
            let values = collection_values(input, default_elements())?;
            Ok(faker
                .randomElement(&values)
                .map_or(Value::Null, |value| json_value(value)))
        },
        "randomelements" => |faker, args| {
            let [input, count, duplicates] = arguments(args)?;
            let values = collection_values(input, default_elements())?;
            let count = match count {
                None => Some(1),
                Some(value) => integer(Some(value))?
                    .map(|count| {
                        usize::try_from(count.max(0)).map_err(|_| {
                            DatabaseError::InvalidQuery("Faker sample count is out of range")
                        })
                    })
                    .transpose()?,
            };
            let selected = faker.randomElements(&values, count, flag(duplicates)?)?;
            Ok(Value::Json(Json::Array(
                selected.into_iter().map(|value| (*value).clone()).collect(),
            )))
        },
        "randomkey" => random_key,
        "shuffle" => |faker, args| {
            let [input] = arguments(args)?;
            match input {
                None => Ok("".into()),
                Some(Value::Text(text)) => Ok(faker.shuffleString(text).into()),
                input => shuffle_collection(faker, input),
            }
        },
        "shufflearray" => |faker, args| {
            let [input] = arguments(args)?;
            shuffle_collection(faker, input)
        },
        "shufflestring" => |faker, args| {
            let [input, encoding] = arguments(args)?;
            if let Some(encoding) = text(encoding)?
                && !encoding.eq_ignore_ascii_case("UTF-8")
                && !encoding.eq_ignore_ascii_case("UTF8")
            {
                return Err(DatabaseError::InvalidQuery(
                    "Faker shuffleString requires UTF-8 text",
                ));
            }
            Ok(faker.shuffleString(text(input)?.unwrap_or("")).into())
        },
        "passthrough" => |_, args| {
            let [Some(value)] = arguments(args)? else {
                return Err(DatabaseError::InvalidQuery(
                    "Faker passthrough requires a value",
                ));
            };
            Ok(value.clone())
        },
        "randomletter" => |faker, _| Ok(faker.randomLetter().to_string().into()),
        "randomascii" => |faker, _| Ok(faker.randomAscii().to_string().into()),
        "numerify" => |faker, args| nullable_pattern(faker, args, Faker::numerify),
        "lexify" => |faker, args| nullable_pattern(faker, args, Faker::lexify),
        "bothify" => |faker, args| nullable_pattern(faker, args, Faker::bothify),
        "asciify" => |faker, args| Ok(faker.asciify(pattern(args)?).into()),
        "regexify" => |faker, args| {
            faker
                .regexify(pattern(args)?.unwrap_or(""))
                .map(Value::from)
        },
        _ => {
            return named_text::formatter(name)
                .or_else(|| named_regional::formatter(name))
                .or_else(|| named_identity::formatter(name))
                .or_else(|| named_phone::formatter(name))
                .or_else(|| legacy_date_formatter(name))
                .or_else(|| color_formatter(name, false));
        }
    })
}

fn color_formatter(name: &str, core: bool) -> Option<Builtin> {
    Some(match name {
        "safecolorname" if core => |faker, _| Ok(faker.sample_color_name("en_US", true).into()),
        "colorname" if core => |faker, _| Ok(faker.sample_color_name("en_US", false).into()),
        "hexcolor" => |faker, _| Ok(faker.hexColor().into()),
        "safehexcolor" => |faker, _| Ok(faker.safeHexColor().into()),
        "rgbcolor" => |faker, _| Ok(faker.rgbColor().into()),
        "rgbcsscolor" => |faker, _| Ok(faker.rgbCssColor().into()),
        "rgbacsscolor" => |faker, _| Ok(faker.rgbaCssColor().into()),
        "safecolorname" => |faker, _| Ok(faker.safeColorName().into()),
        "colorname" => |faker, _| Ok(faker.colorName().into()),
        "hslcolor" => |faker, _| Ok(faker.hslColor().into()),
        "rgbcolorasarray" => |faker, _| Ok(Value::Json(serde_json::json!(faker.rgbColorAsArray()))),
        "hslcolorasarray" => |faker, _| Ok(Value::Json(serde_json::json!(faker.hslColorAsArray()))),
        _ => return None,
    })
}

fn legacy_date_formatter(name: &str) -> Option<Builtin> {
    Some(match name {
        "ampm" => |faker, args| legacy_date_component(faker, args, Faker::amPm),
        "dayofmonth" => |faker, args| legacy_date_component(faker, args, Faker::dayOfMonth),
        "dayofweek" => |faker, args| legacy_date_component(faker, args, Faker::dayOfWeek),
        "monthname" => |faker, args| legacy_date_component(faker, args, Faker::monthName),
        "monthnamegenitive" => |faker, args| {
            let [until] = arguments(args)?;
            faker
                .monthNameGenitive(date_max(faker, until)?)
                .map(Value::from)
        },
        "formatteddate" => |faker, args| {
            let [] = arguments(args)?;
            if faker.locale() != "cs_CZ" {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker formatted date locale",
                ));
            }
            faker
                .parse("{{dayOfMonth}}. {{monthNameGenitive}} {{year}}")
                .map(Value::from)
        },
        "getdefaulttimezone" => |faker, args| {
            let [] = arguments(args)?;
            Ok(faker.getDefaultTimezone().map_or(Value::Null, |zone| {
                zone.iana_name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| zone.to_offset(Timestamp::UNIX_EPOCH).to_string())
                    .into()
            }))
        },
        "setdefaulttimezone" => |faker, args| {
            let [zone] = arguments(args)?;
            faker.setDefaultTimezone(date_zone(zone)?);
            Ok(Value::Null)
        },
        "datetimethismonth" => {
            |faker, args| legacy_date_period(faker, args, Faker::dateTimeThisMonth)
        }
        "datetimethisyear" => {
            |faker, args| legacy_date_period(faker, args, Faker::dateTimeThisYear)
        }
        "datetimethisdecade" => {
            |faker, args| legacy_date_period(faker, args, Faker::dateTimeThisDecade)
        }
        "datetimethiscentury" => {
            |faker, args| legacy_date_period(faker, args, Faker::dateTimeThisCentury)
        }
        "date" | "time" | "unixtime" | "datetime" | "datetimead" | "datetimebetween"
        | "datetimeininterval" | "iso8601" | "month" | "year" | "century" | "timezone" => {
            return extension_method(DATE_TIME, name);
        }
        _ => return None,
    })
}

fn legacy_date_component(
    faker: &Faker,
    args: &[Value],
    sample: fn(&Faker, Option<Timestamp>) -> String,
) -> DatabaseResult<Value> {
    let [until] = arguments(args)?;
    Ok(sample(faker, date_max(faker, until)?).into())
}

fn legacy_date_period(
    faker: &Faker,
    args: &[Value],
    sample: impl FnOnce(&Faker, Option<Timestamp>) -> DatabaseResult<Timestamp>,
) -> DatabaseResult<Value> {
    let [until, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let instant = sample(faker, date_max(faker, until)?)?;
    Ok(rfc3339_seconds(&faker.zoned_date(instant, zone)).into())
}

fn arguments<const N: usize>(args: &[Value]) -> DatabaseResult<[Option<&Value>; N]> {
    if args.len() > N {
        return Err(DatabaseError::InvalidQuery(
            "too many Faker formatter arguments",
        ));
    }
    Ok(std::array::from_fn(|index| args.get(index)))
}

fn integer(value: Option<&Value>) -> DatabaseResult<Option<i64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Integer(value)) => Ok(Some(*value)),
        Some(Value::Unsigned(value)) => i64::try_from(*value)
            .map(Some)
            .map_err(|_| DatabaseError::InvalidQuery("Faker integer argument is out of range")),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects an integer",
        )),
    }
}

fn real(value: Option<&Value>) -> DatabaseResult<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Real(value)) => Ok(Some(*value)),
        Some(Value::Integer(value)) => Ok(Some(*value as f64)),
        Some(Value::Unsigned(value)) => Ok(Some(*value as f64)),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects a number",
        )),
    }
}

fn pattern(args: &[Value]) -> DatabaseResult<Option<&str>> {
    match arguments(args)? {
        [None] => Ok(None),
        [Some(Value::Null)] => Ok(Some("")),
        [Some(Value::Text(value))] => Ok(Some(value)),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects a text pattern",
        )),
    }
}

fn nullable_pattern(
    faker: &Faker,
    args: &[Value],
    generate: impl FnOnce(&Faker, Option<&str>) -> String,
) -> DatabaseResult<Value> {
    let pattern = pattern(args)?;
    if matches!(args, [Value::Null]) {
        return Ok(Value::Null);
    }
    Ok(generate(faker, pattern).into())
}

fn coordinate(faker: &Faker, args: &[Value], bound: f64) -> DatabaseResult<Value> {
    let [min, max] = arguments(args)?;
    let min = real(min)?.unwrap_or(if min.is_none() { -bound } else { 0.0 });
    let max = if max.is_none() {
        Some(bound)
    } else {
        real(max)?
    };
    faker.randomFloat(Some(6), min, max).map(Value::Real)
}

fn flag(value: Option<&Value>) -> DatabaseResult<bool> {
    match value {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects a boolean",
        )),
    }
}

fn text(value: Option<&Value>) -> DatabaseResult<Option<&str>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(value)) => Ok(Some(value)),
        _ => Err(DatabaseError::InvalidQuery("Faker formatter expects text")),
    }
}

fn date_max(faker: &Faker, value: Option<&Value>) -> DatabaseResult<Option<Timestamp>> {
    date_max_at(faker, value, Timestamp::now())
}

fn date_max_at(
    faker: &Faker,
    value: Option<&Value>,
    now: Timestamp,
) -> DatabaseResult<Option<Timestamp>> {
    let timestamp = match value {
        Some(Value::Text(expression)) => return faker.resolve_date_max(expression, now).map(Some),
        value => integer(value)?,
    };
    timestamp
        .map(|timestamp| {
            Timestamp::new(timestamp, 0)
                .map_err(|_| DatabaseError::InvalidQuery("Faker timestamp is out of range"))
        })
        .transpose()
}

fn php_date(faker: &Faker, args: &[Value], default: &str) -> DatabaseResult<Value> {
    let [format, until] = arguments(args)?;
    faker
        .dateWithPhpFormat(text(format)?.unwrap_or(default), date_max(faker, until)?)
        .map(Value::from)
}

fn date_component(faker: &Faker, args: &[Value], format: &str) -> DatabaseResult<Value> {
    let [until] = arguments(args)?;
    faker
        .dateWithPhpFormat(format, date_max(faker, until)?)
        .map(Value::from)
}

fn date_time(faker: &Faker, args: &[Value], ad: bool) -> DatabaseResult<Value> {
    let [until, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let until = date_max(faker, until)?;
    let date = if ad {
        faker.dateTimeADInTimezone(until, zone)
    } else {
        faker.dateTimeInTimezone(until, zone)
    };
    Ok(rfc3339_seconds(&date).into())
}

fn date_zone(value: Option<&Value>) -> DatabaseResult<Option<TimeZone>> {
    text(value)?
        .map(crate::support::Date::timezone)
        .transpose()
        .map_err(|_| DatabaseError::InvalidQuery("invalid Faker time zone"))
}

fn date_bound(
    faker: &Faker,
    value: Option<&Value>,
    default: &str,
    now: Timestamp,
) -> DatabaseResult<Timestamp> {
    match value {
        None => faker.resolve_date_max(default, now),
        Some(value) => Ok(date_max_at(faker, Some(value), now)?.unwrap_or(now)),
    }
}

fn date_between(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [from, until, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let now = Timestamp::now();
    let start = date_bound(faker, from, "-30 years", now)?;
    let end = date_bound(faker, until, "now", now)?;
    let date = faker.dateTimeBetween(start, end)?;
    Ok(rfc3339_seconds(&faker.zoned_date(date, zone)).into())
}

fn date_interval(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [from, interval, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let timestamp;
    let from = match from {
        None | Some(Value::Null) => None,
        Some(Value::Text(expression)) => Some(expression.as_str()),
        value => {
            let seconds = integer(value)?.expect("a non-null integer bound");
            timestamp = format!("@{seconds}");
            Some(timestamp.as_str())
        }
    };
    let date = faker.dateTimeInIntervalExpressions(from, text(interval)?)?;
    Ok(rfc3339_seconds(&faker.zoned_date(date, zone)).into())
}

fn date_period(
    faker: &Faker,
    args: &[Value],
    start: &str,
    default_end: &str,
) -> DatabaseResult<Value> {
    let [until, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let now = Timestamp::now();
    let start = faker.resolve_date_max(start, now)?;
    let end = date_bound(faker, until, default_end, now)?;
    let date = faker.dateTimeBetween(start, end)?;
    Ok(rfc3339_seconds(&faker.zoned_date(date, zone)).into())
}

fn date_calendar_period(faker: &Faker, args: &[Value], years: i32) -> DatabaseResult<Value> {
    let [until, zone] = arguments(args)?;
    let zone = date_zone(zone)?;
    let now = Timestamp::now();
    let start = faker.calendar_period_start(now, years)?;
    let end = date_bound(faker, until, "now", now)?;
    let date = faker.dateTimeBetween(start, end)?;
    Ok(rfc3339_seconds(&faker.zoned_date(date, zone)).into())
}

fn default_elements() -> &'static [Json] {
    static ELEMENTS: LazyLock<[Json; 3]> = LazyLock::new(|| ["a", "b", "c"].map(Json::from));
    &*ELEMENTS
}

fn collection_values<'a>(
    input: Option<&'a Value>,
    default: &'a [Json],
) -> DatabaseResult<Vec<&'a Json>> {
    match input {
        None => Ok(default.iter().collect()),
        Some(Value::Json(Json::Array(values))) => Ok(values.iter().collect()),
        Some(Value::Json(Json::Object(values))) => Ok(values.values().collect()),
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects a JSON array or object",
        )),
    }
}

fn json_value(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(value) => Value::Bool(*value),
        Json::String(value) => Value::Text(value.clone()),
        Json::Number(value) => {
            if let Some(value) = value.as_i64() {
                Value::Integer(value)
            } else if let Some(value) = value.as_u64() {
                Value::Unsigned(value)
            } else {
                Value::Real(value.as_f64().expect("finite JSON number"))
            }
        }
        value => Value::Json(value.clone()),
    }
}

fn random_key(faker: &Faker, args: &[Value]) -> DatabaseResult<Value> {
    let [input] = arguments(args)?;
    match input {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(Value::Json(Json::Array(values))) => Ok(faker
            .randomKey(values)
            .map_or(Value::Null, |key| Value::Integer(key as i64))),
        Some(Value::Json(Json::Object(values))) => {
            let keys: Vec<_> = values.keys().collect();
            Ok(faker
                .randomElement(&keys)
                .map_or(Value::Null, |key| Value::from(key.as_str())))
        }
        _ => Err(DatabaseError::InvalidQuery(
            "Faker formatter expects a JSON array or object",
        )),
    }
}

fn shuffle_collection(faker: &Faker, input: Option<&Value>) -> DatabaseResult<Value> {
    let values = collection_values(input, &[])?;
    let values = faker.shuffleArray(&values).into_iter().cloned().collect();
    Ok(Value::Json(Json::Array(values)))
}

pub(super) fn rfc3339_seconds(date: &jiff::Zoned) -> String {
    jiff::fmt::temporal::DateTimePrinter::new()
        .precision(Some(0))
        .timestamp_with_offset_to_string(&date.timestamp(), date.offset())
}

#[cfg(test)]
#[path = "../../../../tests/unit/faker_timestamps.rs"]
mod time_tests;
