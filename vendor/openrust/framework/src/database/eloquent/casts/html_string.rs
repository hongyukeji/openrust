use crate::{
    database::{DatabaseResult, Value, eloquent::Attributes},
    support::HtmlString,
};

use super::{AttributeCast, CastWrite, CastableObject, ValueObjectCast, primitive::scalar_string};

/// Reads an attribute as HTML and stores and serializes it as plain text.
pub struct AsHtmlString;

impl AsHtmlString {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<HtmlString>(|value, _| match value {
            Value::Null | Value::Text(_) => Ok(value),
            other => scalar_string(&other).map(Value::Text),
        })
    }
}

impl CastableObject for HtmlString {
    fn from_attributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match raw.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => scalar_string(value).map(Self::new).map(Some),
        }
    }

    fn to_attributes(value: Option<&Self>, _: &str, _: &Attributes) -> DatabaseResult<CastWrite> {
        Ok(CastWrite::Value(value.map_or(Value::Null, |html| {
            Value::Text(html.to_html().to_owned())
        })))
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Text(self.to_html().to_owned()))
    }
}
