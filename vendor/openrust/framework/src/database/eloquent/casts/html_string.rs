use crate::{
    database::{DatabaseResult, Value, eloquent::Attributes},
    support::HtmlString,
};

use super::{AttributeCast, CastWrite, CastableObject, ValueObjectCast, primitive::scalar_string};

/// Reads an attribute as HTML and stores and serializes it as plain text.
pub struct AsHtmlString;

impl AsHtmlString {
    pub const fn cast() -> AttributeCast {
        ValueObjectCast::with_assignment::<HtmlString>(|value| match value {
            Value::Null | Value::Text(_) => Ok(value),
            other => scalar_string(&other).map(Value::Text),
        })
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Eloquent naming")]
impl CastableObject for HtmlString {
    fn fromAttributes(name: &str, raw: &Attributes) -> DatabaseResult<Option<Self>> {
        match raw.get(name) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => scalar_string(value).map(Self::new).map(Some),
        }
    }

    fn toAttributes(value: Option<&Self>, _: &str, _: &Attributes) -> DatabaseResult<CastWrite> {
        Ok(CastWrite::Value(value.map_or(Value::Null, |html| {
            Value::Text(html.toHtml().to_owned())
        })))
    }

    fn serialize(&self) -> DatabaseResult<Value> {
        Ok(Value::Text(self.toHtml().to_owned()))
    }
}
