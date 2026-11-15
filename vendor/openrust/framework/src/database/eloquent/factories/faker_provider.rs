use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use regex::Regex;

use super::Faker;
use crate::database::{DatabaseError, DatabaseResult, Value};

type Formatter = dyn Fn(&Faker, &[Value]) -> DatabaseResult<Value> + Send + Sync;
type Builtin = fn(&Faker, &[Value]) -> DatabaseResult<Value>;
pub(super) type Formatters = HashMap<String, Arc<Formatter>>;

pub(super) type Extensions = HashMap<String, Arc<Formatters>>;

const BARCODE: &str = r"Faker\Extension\BarcodeExtension";
const BLOOD: &str = r"Faker\Extension\BloodExtension";
const COLOR: &str = r"Faker\Extension\ColorExtension";
const FILE: &str = r"Faker\Extension\FileExtension";
const NUMBER: &str = r"Faker\Extension\NumberExtension";
const UUID: &str = r"Faker\Extension\UuidExtension";
const BUILTIN_EXTENSIONS: &[&str] = &[BARCODE, BLOOD, COLOR, FILE, NUMBER, UUID];

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
        "mimetype" => (FILE, name),
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
        (COLOR, "hexcolor") => |faker, _| Ok(faker.hexColor().into()),
        (COLOR, "safehexcolor") => |faker, _| Ok(faker.safeHexColor().into()),
        (COLOR, "rgbcolor") => |faker, _| Ok(faker.rgbColor().into()),
        (COLOR, "rgbcsscolor") => |faker, _| Ok(faker.rgbCssColor().into()),
        (COLOR, "rgbacsscolor") => |faker, _| Ok(faker.rgbaCssColor().into()),
        (COLOR, "safecolorname") => |faker, _| Ok(faker.safeColorName().into()),
        (COLOR, "colorname") => |faker, _| Ok(faker.colorName().into()),
        (COLOR, "hslcolor") => |faker, _| Ok(faker.hslColor().into()),
        (COLOR, "rgbcolorasarray") => {
            |faker, _| Ok(Value::Json(serde_json::json!(faker.rgbColorAsArray())))
        }
        (COLOR, "hslcolorasarray") => {
            |faker, _| Ok(Value::Json(serde_json::json!(faker.hslColorAsArray())))
        }
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
        "firstname" => |faker, _| Ok(faker.firstName().into()),
        "freeemail" => |faker, _| Ok(faker.freeEmail().into()),
        "lastname" => |faker, _| Ok(faker.lastName().into()),
        "name" => |faker, _| Ok(faker.name().into()),
        "phonenumber" => |faker, _| Ok(faker.phoneNumber().into()),
        "postcode" => |faker, _| Ok(faker.postcode().into()),
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
        "latitude" => |faker, args| coordinate(faker, args, 90.0),
        "longitude" => |faker, args| coordinate(faker, args, 180.0),
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
        _ => return None,
    })
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
