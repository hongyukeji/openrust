/// 首项为规则名，其余项目保留各参数边界。
pub trait ParameterizedRule: Send + Sync {
    fn to_array(&self) -> Vec<String>;
}
