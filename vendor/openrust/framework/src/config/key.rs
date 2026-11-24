use super::Value;
/// 批量读取中的键与缺键默认值；惰性默认值不要求静态或线程安全。
pub struct ConfigKey<'a> {
    pub(crate) name: String,
    pub(crate) default: KeyDefault<'a>,
}
pub(crate) enum KeyDefault<'a> {
    Value(Value),
    Lazy(Box<dyn FnOnce() -> Value + 'a>),
}
impl KeyDefault<'_> {
    pub(crate) fn resolve(self) -> Value {
        match self {
            Self::Value(value) => value,
            Self::Lazy(default) => default(),
        }
    }
}
impl<'a> ConfigKey<'a> {
    pub fn lazy(name: impl Into<String>, default: impl FnOnce() -> Value + 'a) -> Self {
        Self {
            name: name.into(),
            default: KeyDefault::Lazy(Box::new(default)),
        }
    }
}
impl From<&str> for ConfigKey<'_> {
    fn from(name: &str) -> Self {
        Self {
            name: name.into(),
            default: KeyDefault::Value(Value::Null),
        }
    }
}
impl From<String> for ConfigKey<'_> {
    fn from(name: String) -> Self {
        Self {
            name,
            default: KeyDefault::Value(Value::Null),
        }
    }
}
impl From<(&str, Value)> for ConfigKey<'_> {
    fn from((name, default): (&str, Value)) -> Self {
        Self {
            name: name.into(),
            default: KeyDefault::Value(default),
        }
    }
}
impl From<(String, Value)> for ConfigKey<'_> {
    fn from((name, default): (String, Value)) -> Self {
        Self {
            name,
            default: KeyDefault::Value(default),
        }
    }
}

impl From<&String> for ConfigKey<'_> {
    fn from(name: &String) -> Self {
        Self::from(name.as_str())
    }
}
