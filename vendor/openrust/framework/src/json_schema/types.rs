use super::SchemaError;
use indexmap::IndexMap;
use serde_json::{Map, Value};

/// 数值保留整数与浮点输入；非有限浮点在序列化时明确失败。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SchemaNumber {
    Integer(i64),
    Unsigned(u64),
    Float(f64),
}
macro_rules! number_from {
    ($variant:ident, $($ty:ty),+) => { $(impl From<$ty> for SchemaNumber {
        fn from(value: $ty) -> Self { Self::$variant(value.into()) }
    })+ };
}
number_from!(Integer, i8, i16, i32, i64);
number_from!(Unsigned, u8, u16, u32, u64);
number_from!(Float, f32, f64);
impl SchemaNumber {
    fn value(self) -> Result<Value, SchemaError> {
        match self {
            Self::Integer(value) => Ok(value.into()),
            Self::Unsigned(value) => Ok(value.into()),
            Self::Float(value) => serde_json::Number::from_f64(value)
                .map(Value::Number)
                .ok_or_else(|| SchemaError::new("JSON Schema 数值必须是有限数值")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Kind {
    String,
    Integer,
    Number,
    Boolean,
    Object(IndexMap<String, Type>),
    Array(Option<Box<Type>>),
    Union(Vec<String>),
    AnyOf(Vec<Type>),
}

/// 可组合的模式类型；required 只在父对象的 required 数组中输出。
#[derive(Clone, Debug, PartialEq)]
pub struct Type {
    pub(super) kind: Kind,
    pub(super) attributes: Map<String, Value>,
    numeric: IndexMap<String, SchemaNumber>,
    pub(super) required: bool,
    pub(super) nullable: bool,
}

#[allow(non_snake_case)]
impl Type {
    pub(super) fn new(kind: Kind) -> Self {
        Self {
            kind,
            attributes: Map::new(),
            numeric: IndexMap::new(),
            required: false,
            nullable: false,
        }
    }
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }
    pub fn nullable(mut self, nullable: bool) -> Self {
        self.nullable = nullable;
        self
    }
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.attributes.insert("title".into(), value.into().into());
        self
    }
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.attributes
            .insert("description".into(), value.into().into());
        self
    }
    pub fn r#enum(mut self, values: impl IntoIterator<Item = Value>) -> Self {
        self.attributes
            .insert("enum".into(), values.into_iter().collect());
        self
    }
    pub fn to_array(&self) -> Result<Value, SchemaError> {
        let mut attributes = self.attributes.clone();
        for (key, number) in &self.numeric {
            attributes.insert(key.clone(), number.value()?);
        }
        match &self.kind {
            Kind::AnyOf(schemas) => {
                let mut schemas = schemas
                    .iter()
                    .map(Self::to_array)
                    .collect::<Result<Vec<_>, _>>()?;
                if self.nullable {
                    schemas.push(serde_json::json!({"type":"null"}));
                }
                attributes.insert("anyOf".into(), Value::Array(schemas));
                return Ok(Value::Object(attributes));
            }
            Kind::Object(properties) => {
                if !properties.is_empty() {
                    let required = properties
                        .iter()
                        .filter(|(_, property)| property.required)
                        .map(|(key, _)| Value::String(key.clone()))
                        .collect::<Vec<_>>();
                    if !required.is_empty() {
                        attributes.insert("required".into(), required.into());
                    }
                    let properties = properties
                        .iter()
                        .map(|(key, property)| Ok((key.clone(), property.to_array()?)))
                        .collect::<Result<Map<_, _>, SchemaError>>()?;
                    attributes.insert("properties".into(), properties.into());
                }
            }
            Kind::Array(Some(items)) => {
                attributes.insert("items".into(), items.to_array()?);
            }
            _ => {}
        }
        let mut names: Vec<String> = match &self.kind {
            Kind::String => vec!["string".into()],
            Kind::Integer => vec!["integer".into()],
            Kind::Number => vec!["number".into()],
            Kind::Boolean => vec!["boolean".into()],
            Kind::Object(_) => vec!["object".into()],
            Kind::Array(_) => vec!["array".into()],
            Kind::Union(names) => names.clone(),
            Kind::AnyOf(_) => unreachable!(),
        };
        if self.nullable {
            names.push("null".into());
        }
        let kind = if !self.nullable && !matches!(self.kind, Kind::Union(_)) {
            Value::String(names.remove(0))
        } else {
            names.into_iter().map(Value::String).collect()
        };
        attributes.insert("type".into(), kind);
        Ok(attributes.into())
    }
    pub fn toString(&self) -> Result<String, SchemaError> {
        serde_json::to_string_pretty(&self.to_array()?)
            .map_err(|error| SchemaError::new(error.to_string()))
    }
}
impl AsRef<Type> for Type {
    fn as_ref(&self) -> &Type {
        self
    }
}
impl serde::Serialize for Type {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_array()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}

macro_rules! schema_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name(pub(super) Type);
        impl From<$name> for Type {
            fn from(value: $name) -> Self {
                value.0
            }
        }
        impl AsRef<Type> for $name {
            fn as_ref(&self) -> &Type {
                &self.0
            }
        }
        #[allow(non_snake_case)]
        impl $name {
            pub fn required(mut self, required: bool) -> Self {
                self.0.required = required;
                self
            }
            pub fn nullable(mut self, nullable: bool) -> Self {
                self.0.nullable = nullable;
                self
            }
            pub fn title(mut self, value: impl Into<String>) -> Self {
                self.0 = self.0.title(value);
                self
            }
            pub fn description(mut self, value: impl Into<String>) -> Self {
                self.0 = self.0.description(value);
                self
            }
            pub fn r#enum(mut self, values: impl IntoIterator<Item = Value>) -> Self {
                self.0 = self.0.r#enum(values);
                self
            }
            pub fn to_array(&self) -> Result<Value, SchemaError> {
                self.0.to_array()
            }
            pub fn toString(&self) -> Result<String, SchemaError> {
                self.0.toString()
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                self.0.serialize(serializer)
            }
        }
    };
}
schema_type!(StringType);
schema_type!(IntegerType);
schema_type!(NumberType);
schema_type!(BooleanType);
schema_type!(ObjectType);
schema_type!(ArrayType);
schema_type!(UnionType);
schema_type!(AnyOfType);

impl StringType {
    pub fn new() -> Self {
        Self(Type::new(Kind::String))
    }
    pub fn min(mut self, value: i64) -> Self {
        self.0.attributes.insert("minLength".into(), value.into());
        self
    }
    pub fn max(mut self, value: i64) -> Self {
        self.0.attributes.insert("maxLength".into(), value.into());
        self
    }
    pub fn pattern(mut self, value: impl Into<String>) -> Self {
        self.0
            .attributes
            .insert("pattern".into(), value.into().into());
        self
    }
    pub fn format(mut self, value: impl Into<String>) -> Self {
        self.0
            .attributes
            .insert("format".into(), value.into().into());
        self
    }
    pub fn default(mut self, value: impl Into<String>) -> Self {
        self.0
            .attributes
            .insert("default".into(), value.into().into());
        self
    }
}
macro_rules! numeric_type {
    ($name:ident, $kind:ident, $value:ty) => {
        #[allow(non_snake_case)]
        impl $name {
            pub fn new() -> Self {
                Self(Type::new(Kind::$kind))
            }
            pub fn min(mut self, value: $value) -> Self {
                self.0.numeric.insert("minimum".into(), value.into());
                self
            }
            pub fn max(mut self, value: $value) -> Self {
                self.0.numeric.insert("maximum".into(), value.into());
                self
            }
            pub fn multipleOf(mut self, value: $value) -> Self {
                self.0.numeric.insert("multipleOf".into(), value.into());
                self
            }
            pub fn default(mut self, value: $value) -> Self {
                self.0.numeric.insert("default".into(), value.into());
                self
            }
        }
    };
}
numeric_type!(IntegerType, Integer, i64);
numeric_type!(NumberType, Number, impl Into<SchemaNumber>);
impl BooleanType {
    pub fn new() -> Self {
        Self(Type::new(Kind::Boolean))
    }
    pub fn default(mut self, value: bool) -> Self {
        self.0.attributes.insert("default".into(), value.into());
        self
    }
}
impl ObjectType {
    pub fn new<K: Into<String>>(properties: impl IntoIterator<Item = (K, Type)>) -> Self {
        Self(Type::new(Kind::Object(
            properties
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )))
    }
    #[allow(non_snake_case)]
    pub fn withoutAdditionalProperties(mut self) -> Self {
        self.0
            .attributes
            .insert("additionalProperties".into(), false.into());
        self
    }
    pub fn default(mut self, value: Map<String, Value>) -> Self {
        self.0.attributes.insert("default".into(), value.into());
        self
    }
}
impl ArrayType {
    pub fn new() -> Self {
        Self(Type::new(Kind::Array(None)))
    }
    pub fn items(mut self, value: impl Into<Type>) -> Self {
        self.0.kind = Kind::Array(Some(Box::new(value.into())));
        self
    }
    pub fn min(mut self, value: i64) -> Self {
        self.0.attributes.insert("minItems".into(), value.into());
        self
    }
    pub fn max(mut self, value: i64) -> Self {
        self.0.attributes.insert("maxItems".into(), value.into());
        self
    }
    pub fn unique(mut self, unique: bool) -> Self {
        if unique {
            self.0.attributes.insert("uniqueItems".into(), true.into());
        } else {
            self.0.attributes.remove("uniqueItems");
        }
        self
    }
    pub fn default(mut self, value: Vec<Value>) -> Self {
        self.0.attributes.insert("default".into(), value.into());
        self
    }
}
impl UnionType {
    pub const SUPPORTED: [&str; 6] = ["string", "integer", "number", "boolean", "object", "array"];
    pub fn new<S: Into<String>>(types: impl IntoIterator<Item = S>) -> Result<Self, SchemaError> {
        let mut names = Vec::new();
        let mut nullable = false;
        for name in types {
            let name = name.into();
            if name == "null" {
                nullable = true;
                continue;
            }
            if !Self::SUPPORTED.contains(&name.as_str()) {
                return Err(SchemaError::new(format!(
                    "不支持的 JSON Schema 联合类型：{name}"
                )));
            }
            if !names.contains(&name) {
                names.push(name);
            }
        }
        Ok(Self(Type::new(Kind::Union(names)).nullable(nullable)))
    }
    pub fn types(&self) -> &[String] {
        match &self.0.kind {
            Kind::Union(names) => names,
            _ => unreachable!(),
        }
    }
}
impl AnyOfType {
    pub fn new(schemas: impl IntoIterator<Item = Type>) -> Self {
        Self(Type::new(Kind::AnyOf(schemas.into_iter().collect())))
    }
    pub fn schemas(&self) -> &[Type] {
        match &self.0.kind {
            Kind::AnyOf(schemas) => schemas,
            _ => unreachable!(),
        }
    }
}
macro_rules! default_type {
    ($($name:ident),+) => { $(impl Default for $name { fn default() -> Self { Self::new() } })+ };
}
default_type!(StringType, IntegerType, NumberType, BooleanType, ArrayType);
