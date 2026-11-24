use crate::{http::RequestValue, support::ArrayKey, validation::ValidationResult};

/// 数组投影的原生返回类型由实现者声明。
pub trait Arrayable {
    type Array;
    fn to_array(&self) -> Self::Array;
}

/// 显式表达动态数组访问，不依赖运行期属性反射。
pub trait ArrayAccess {
    fn offset_exists(&self, key: &str) -> bool;
    fn offset_get(&self, key: &str) -> Option<RequestValue>;
    fn offset_set(&mut self, key: Option<ArrayKey>, value: RequestValue) -> ValidationResult<()>;
    fn offset_unset(&mut self, key: &str);
}

pub trait ValidatedData:
    Arrayable<Array = serde_json::Value> + ArrayAccess + IntoIterator<Item = (ArrayKey, RequestValue)>
{
}
