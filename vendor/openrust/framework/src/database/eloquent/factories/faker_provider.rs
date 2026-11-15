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

    /// Resolves a formatter by name. The returned callback retains the resolved provider.
    pub fn getFormatter<'a>(
        &'a self,
        name: &str,
    ) -> DatabaseResult<impl Fn(&[Value]) -> DatabaseResult<Value> + 'a + use<'a>> {
        let key = name.to_ascii_lowercase();
        let formatter = if let Some(builtin) = core(&key) {
            Resolved::Builtin(builtin)
        } else if let Some(callback) = self.formatters.read().unwrap().get(&key).cloned() {
            Resolved::Custom(callback)
        } else if let Some(builtin) = defaults(&key) {
            Resolved::Builtin(builtin)
        } else {
            return Err(DatabaseError::UnknownFakerFormatter(name.to_owned()));
        };

        Ok(move |arguments: &[Value]| match &formatter {
            Resolved::Builtin(callback) => callback(self, arguments),
            Resolved::Custom(callback) => callback(self, arguments),
        })
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

fn core(name: &str) -> Option<Builtin> {
    Some(match name {
        "mimetype" => |faker, _| Ok(faker.mimeType().into()),
        "fileextension" => |faker, _| Ok(faker.fileExtension().into()),
        "bloodtype" => |faker, _| Ok(faker.bloodType().into()),
        "bloodrh" => |faker, _| Ok(faker.bloodRh().into()),
        "parse" => |faker, args| match args {
            [Value::Text(template)] => faker.parse(template).map(Value::from),
            _ => Err(DatabaseError::InvalidQuery(
                "Faker parse expects a template",
            )),
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
        "randomdigit" => |faker, _| Ok(i64::from(faker.randomDigit()).into()),
        "safeemail" => |faker, _| Ok(faker.safeEmail().into()),
        "username" => |faker, _| Ok(faker.userName().into()),
        "uuid" => |faker, _| Ok(faker.uuid().to_string().into()),
        _ => return None,
    })
}
