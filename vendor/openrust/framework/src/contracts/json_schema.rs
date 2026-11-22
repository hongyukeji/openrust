use crate::json_schema::{SchemaError, types::*};

/// 类型化模式工厂契约；集合与回调由调用方在编译期组织。
#[allow(non_snake_case)]
pub trait JsonSchema {
    fn object<K: Into<String>>(
        &self,
        properties: impl IntoIterator<Item = (K, Type)>,
    ) -> ObjectType;
    fn array(&self) -> ArrayType;
    fn string(&self) -> StringType;
    fn integer(&self) -> IntegerType;
    fn number(&self) -> NumberType;
    fn boolean(&self) -> BooleanType;
    fn union<S: Into<String>>(
        &self,
        types: impl IntoIterator<Item = S>,
    ) -> Result<UnionType, SchemaError>;
    fn anyOf(&self, schemas: impl IntoIterator<Item = Type>) -> AnyOfType;
}
