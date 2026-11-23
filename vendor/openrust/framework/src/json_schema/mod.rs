//! 类型化 JSON Schema 构造与受支持子集的反序列化。
mod deserializer;
pub mod types;
pub use deserializer::Deserializer;
pub use types::Type;

/// 模式不能用公开类型表示，或输入超出安全展开边界。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemaError(String);
impl SchemaError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for SchemaError {}

pub struct JsonSchema;
#[derive(Clone, Copy, Debug, Default)]
pub struct JsonSchemaTypeFactory;
pub struct Serializer;
impl Serializer {
    pub fn serialize(schema: impl AsRef<Type>) -> Result<serde_json::Value, SchemaError> {
        schema.as_ref().to_array()
    }
}

#[allow(non_snake_case)]
impl JsonSchema {
    pub fn fromArray(schema: serde_json::Value) -> Result<Type, SchemaError> {
        Deserializer::deserialize(schema)
    }
    pub fn string() -> types::StringType {
        types::StringType::new()
    }
    pub fn integer() -> types::IntegerType {
        types::IntegerType::new()
    }
    pub fn number() -> types::NumberType {
        types::NumberType::new()
    }
    pub fn boolean() -> types::BooleanType {
        types::BooleanType::new()
    }
    pub fn array() -> types::ArrayType {
        types::ArrayType::new()
    }
    pub fn object<K: Into<String>>(
        properties: impl IntoIterator<Item = (K, Type)>,
    ) -> types::ObjectType {
        types::ObjectType::new(properties)
    }
    pub fn union<S: Into<String>>(
        types: impl IntoIterator<Item = S>,
    ) -> Result<types::UnionType, SchemaError> {
        types::UnionType::new(types)
    }
    pub fn any_of(schemas: impl IntoIterator<Item = Type>) -> types::AnyOfType {
        types::AnyOfType::new(schemas)
    }
}
#[allow(non_snake_case)]
impl JsonSchemaTypeFactory {
    pub fn fromArray(&self, schema: serde_json::Value) -> Result<Type, SchemaError> {
        JsonSchema::fromArray(schema)
    }
    pub fn string(&self) -> types::StringType {
        JsonSchema::string()
    }
    pub fn integer(&self) -> types::IntegerType {
        JsonSchema::integer()
    }
    pub fn number(&self) -> types::NumberType {
        JsonSchema::number()
    }
    pub fn boolean(&self) -> types::BooleanType {
        JsonSchema::boolean()
    }
    pub fn array(&self) -> types::ArrayType {
        JsonSchema::array()
    }
    pub fn object<K: Into<String>>(
        &self,
        properties: impl IntoIterator<Item = (K, Type)>,
    ) -> types::ObjectType {
        JsonSchema::object(properties)
    }
    pub fn union<S: Into<String>>(
        &self,
        types: impl IntoIterator<Item = S>,
    ) -> Result<types::UnionType, SchemaError> {
        JsonSchema::union(types)
    }
    pub fn any_of(&self, schemas: impl IntoIterator<Item = Type>) -> types::AnyOfType {
        JsonSchema::any_of(schemas)
    }
}

impl crate::contracts::json_schema::JsonSchema for JsonSchemaTypeFactory {
    fn object<K: Into<String>>(
        &self,
        properties: impl IntoIterator<Item = (K, Type)>,
    ) -> types::ObjectType {
        JsonSchema::object(properties)
    }
    fn array(&self) -> types::ArrayType {
        JsonSchema::array()
    }
    fn string(&self) -> types::StringType {
        JsonSchema::string()
    }
    fn integer(&self) -> types::IntegerType {
        JsonSchema::integer()
    }
    fn number(&self) -> types::NumberType {
        JsonSchema::number()
    }
    fn boolean(&self) -> types::BooleanType {
        JsonSchema::boolean()
    }
    fn union<S: Into<String>>(
        &self,
        types: impl IntoIterator<Item = S>,
    ) -> Result<types::UnionType, SchemaError> {
        JsonSchema::union(types)
    }
    #[allow(non_snake_case)]
    fn any_of(&self, schemas: impl IntoIterator<Item = Type>) -> types::AnyOfType {
        JsonSchema::any_of(schemas)
    }
}
