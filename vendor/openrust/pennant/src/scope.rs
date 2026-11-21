use crate::{Error, Result};

/// Rust scopes retain their original value for feature resolvers.
#[derive(Clone, Debug, PartialEq)]
pub enum Scope {
    Null,
    String(String),
    Number(serde_json::Number),
    Model {
        class: String,
        key: String,
        morph: Option<String>,
    },
    Custom(String),
    Global,
}

pub trait FeatureScopeSerializeable {
    fn featureScopeSerialize(&self) -> String;
}

impl Scope {
    pub fn serialize(&self, useMorphMap: bool) -> String {
        match self {
            Self::Null => "__laravel_null".into(),
            Self::Global => "__laravel_global".into(),
            Self::String(value) | Self::Custom(value) => value.clone(),
            Self::Number(value) => {
                if value.is_f64() {
                    openrust::support::php_float_string(value.as_f64().expect("float"))
                } else {
                    value.to_string()
                }
            }
            Self::Model { class, key, morph } => format!(
                "{}|{key}",
                if useMorphMap {
                    morph.as_ref().unwrap_or(class)
                } else {
                    class
                }
            ),
        }
    }

    pub fn fromSerializable(scope: &impl FeatureScopeSerializeable) -> Self {
        Self::Custom(scope.featureScopeSerialize())
    }

    pub fn fromModel<M: openrust::database::eloquent::Model>(model: &M) -> Result<Self> {
        let key = model.getKey().ok_or(Error::InvalidScope)?;
        Ok(Self::Model {
            class: std::any::type_name::<M>().into(),
            key: match key {
                openrust::database::Value::Text(value) => value.clone(),
                openrust::database::Value::Integer(value) => value.to_string(),
                openrust::database::Value::Unsigned(value) => value.to_string(),
                _ => return Err(Error::InvalidScope),
            },
            morph: Some(model.getMorphClass()?.into()),
        })
    }
}
impl From<&str> for Scope {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl From<String> for Scope {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}
impl From<i64> for Scope {
    fn from(value: i64) -> Self {
        Self::Number(value.into())
    }
}
impl From<u64> for Scope {
    fn from(value: u64) -> Self {
        Self::Number(value.into())
    }
}
impl From<()> for Scope {
    fn from(_: ()) -> Self {
        Self::Null
    }
}
