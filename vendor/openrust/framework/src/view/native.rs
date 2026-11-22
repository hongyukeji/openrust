//! 构建后的原生函数与当前渲染局部数据之间的显式边界。

use super::{ViewData, ViewError, ViewResult, ViewValue};

pub type NativeResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type Function = fn(&mut NativeContext<'_>) -> NativeResult;

/// 仅由应用构建产物提供，不保存任何请求或任务状态。
pub struct NativeBlock {
    source: &'static str,
    path: &'static str,
    line: usize,
    function: Function,
}
impl NativeBlock {
    #[doc(hidden)]
    pub const fn new(
        source: &'static str,
        path: &'static str,
        line: usize,
        function: Function,
    ) -> Self {
        Self {
            source,
            path,
            line,
            function,
        }
    }
}

/// 顺序及原始源码全部匹配后才能执行，摘要碰撞不会选择另一份函数。
pub struct NativeTemplate {
    blocks: &'static [NativeBlock],
}
impl NativeTemplate {
    #[doc(hidden)]
    pub const fn new(blocks: &'static [NativeBlock]) -> Self {
        Self { blocks }
    }
    pub(super) fn matches(&self, sources: &[String]) -> bool {
        self.blocks.len() == sources.len()
            && self.blocks.iter().zip(sources).all(|(a, b)| a.source == b)
    }
    pub(super) fn execute(&self, source: &str, data: &mut ViewData) -> Option<ViewResult<()>> {
        let (index, block) = self
            .blocks
            .iter()
            .enumerate()
            .find(|(_, b)| b.source == source)?;
        Some(
            (block.function)(&mut NativeContext { data }).map_err(|source| {
                ViewError::RenderFailed(
                    std::io::Error::other(format!(
                        "{}:{} 原生 Rust 块 {} 执行失败：{source}",
                        block.path,
                        block.line,
                        index + 1
                    ))
                    .into(),
                )
            }),
        )
    }
}

/// 当前指令块的局部数据。读取返回拥有的值，写回不会修改调用方或全局共享数据。
pub struct NativeContext<'a> {
    data: &'a mut ViewData,
}
impl NativeContext<'_> {
    pub fn get<T: FromViewValue>(&self, key: &str) -> ViewResult<T> {
        let value = self
            .data
            .get(key)
            .ok_or_else(|| error(&format!("原生 Rust 块缺少视图变量 {key:?}")))?;
        T::from_view_value(value).ok_or_else(|| {
            error(&format!(
                "原生 Rust 块视图变量 {key:?} 的类型不匹配：期望 {}",
                std::any::type_name::<T>()
            ))
        })
    }
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<ViewValue>) {
        self.data.insert(key.into(), value.into());
    }
}

/// 严格转换动态视图值；不进行字符串／数字强制转换或 JSON 往返。
pub trait FromViewValue: Sized {
    fn from_view_value(value: &ViewValue) -> Option<Self>;
}
macro_rules! scalar {
    ($ty:ty,$method:ident) => {
        impl FromViewValue for $ty {
            fn from_view_value(value: &ViewValue) -> Option<Self> {
                value.$method()
            }
        }
    };
}
scalar!(bool, as_bool);
scalar!(i64, as_i64);
scalar!(u64, as_u64);
scalar!(f64, as_f64);
impl FromViewValue for String {
    fn from_view_value(value: &ViewValue) -> Option<Self> {
        value.as_str().map(str::to_owned)
    }
}
impl FromViewValue for ViewValue {
    fn from_view_value(value: &ViewValue) -> Option<Self> {
        Some(value.clone())
    }
}
impl FromViewValue for ViewData {
    fn from_view_value(value: &ViewValue) -> Option<Self> {
        value.as_object().cloned()
    }
}
impl<T: FromViewValue> FromViewValue for Vec<T> {
    fn from_view_value(value: &ViewValue) -> Option<Self> {
        value.as_array()?.iter().map(T::from_view_value).collect()
    }
}
impl<T: FromViewValue> FromViewValue for Option<T> {
    fn from_view_value(value: &ViewValue) -> Option<Self> {
        if value.is_null() {
            Some(None)
        } else {
            T::from_view_value(value).map(Some)
        }
    }
}

fn error(message: &str) -> ViewError {
    ViewError::RenderFailed(std::io::Error::other(message.to_owned()).into())
}
