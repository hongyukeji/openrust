//! 条件输入的 PHP 真值判定与显式 Rust 分支结果。

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;

use serde_json::{Map, Value};

/// 判断条件回调返回值的 PHP 真值。
///
/// 默认代表业务对象，始终为真；业务对象可用空实现保留该语义。
/// 标量与容器的内置实现按 PHP 值判断，不执行输入规范化或序列化。
pub trait InputTruthy {
    /// 返回值是否应作为条件回调结果，而不是回退到请求本身。
    fn is_truthy(&self) -> bool {
        true
    }
}

impl InputTruthy for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}

macro_rules! integer_truthy {
    ($($integer:ty),+ $(,)?) => {
        $(impl InputTruthy for $integer {
            fn is_truthy(&self) -> bool {
                *self != 0
            }
        })+
    };
}
integer_truthy!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

macro_rules! float_truthy {
    ($($float:ty),+ $(,)?) => {
        $(impl InputTruthy for $float {
            fn is_truthy(&self) -> bool {
                // PHP 只将正负零判假；NaN 和无穷值仍为真。
                *self != 0.0
            }
        })+
    };
}
float_truthy!(f32, f64);

impl InputTruthy for str {
    fn is_truthy(&self) -> bool {
        !self.is_empty() && self != "0"
    }
}
impl InputTruthy for String {
    fn is_truthy(&self) -> bool {
        self.as_str().is_truthy()
    }
}
impl<T> InputTruthy for Vec<T> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<T> InputTruthy for [T] {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<T, const N: usize> InputTruthy for [T; N] {
    fn is_truthy(&self) -> bool {
        N != 0
    }
}
impl<K, V> InputTruthy for BTreeMap<K, V> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<K, V, S> InputTruthy for HashMap<K, V, S> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl InputTruthy for Map<String, Value> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<T: InputTruthy> InputTruthy for Option<T> {
    fn is_truthy(&self) -> bool {
        self.as_ref().is_some_and(InputTruthy::is_truthy)
    }
}
impl InputTruthy for Value {
    fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => value.is_truthy(),
            Self::Number(value) => value.as_f64().is_some_and(|value| value.is_truthy()),
            Self::String(value) => value.is_truthy(),
            Self::Array(value) => value.is_truthy(),
            Self::Object(value) => value.is_truthy(),
        }
    }
}
impl InputTruthy for crate::support::DataValue {
    fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => value.is_truthy(),
            Self::Number(value) => value.as_f64().is_some_and(|value| value.is_truthy()),
            Self::String(value) => value.is_truthy(),
            Self::Array(value) => value.is_truthy(),
            Self::Object(_) => true,
        }
    }
}
impl InputTruthy for () {
    fn is_truthy(&self) -> bool {
        false
    }
}
impl<T: InputTruthy + ?Sized> InputTruthy for &T {
    fn is_truthy(&self) -> bool {
        (**self).is_truthy()
    }
}

// 两种响应封装都代表 PHP 对象，即使承载 false/null/空数组也不能按载荷判假。
impl<T> InputTruthy for crate::http::Json<T> {}
impl<B> InputTruthy for http::Response<B> {}

/// 将没有实现 `InputTruthy` 的业务值显式视为始终为真的对象。
///
/// 包装不要求序列化能力，也不读取被包装对象的内容。
pub struct Truthy<T>(T);

impl<T> Truthy<T> {
    /// 保留给定业务对象，不做隐式转换。
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// 消费包装并取回原业务对象。
    pub fn into_inner(self) -> T {
        self.0
    }
}
impl<T> AsRef<T> for Truthy<T> {
    fn as_ref(&self) -> &T {
        &self.0
    }
}
impl<T> InputTruthy for Truthy<T> {}

/// 条件输入的只读结果，显式区分继续请求、回调值与默认值。
///
/// Default 分支不再检查真值；false、0 或 None 仍是已返回的默认值。
#[must_use = "条件结果须显式处理继续请求或返回值"]
pub enum InputOutcome<'a, R, D = Infallible> {
    /// 主回调未提供有效返回值，后续仍操作同一请求。
    Request(&'a super::Request),
    /// 主回调的结果；此处不再次判定其真值。
    Value(R),
    /// 默认回调结果，保持原类型和值。
    Default(D),
}

impl<'a, R, D> InputOutcome<'a, R, D> {
    /// 借用继续分支中的请求；其他分支返回 None。
    pub fn request_ref(&self) -> Option<&super::Request> {
        match self {
            Self::Request(request) => Some(request),
            _ => None,
        }
    }

    /// 消费继续分支，保留原请求的借用寿命。
    pub fn into_request(self) -> Option<&'a super::Request> {
        match self {
            Self::Request(request) => Some(request),
            _ => None,
        }
    }

    /// 消费主回调分支；Some 内的 false、0 或 None 不会被丢弃。
    pub fn into_value(self) -> Option<R> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }

    /// 消费默认回调分支；Some(None) 与不是 Default 分支不同。
    pub fn into_default(self) -> Option<D> {
        match self {
            Self::Default(value) => Some(value),
            _ => None,
        }
    }

    /// 仅继续请求分支调用闭包；返回值及默认值保持短路，错误原样传播。
    pub fn and_then<F>(self, next: F) -> crate::Result<Self>
    where
        F: FnOnce(&'a super::Request) -> crate::Result<Self>,
    {
        match self {
            Self::Request(request) => next(request),
            other => Ok(other),
        }
    }
}

/// 条件输入的可变结果，持有同一请求的独占借用。
///
/// 分支规则与 `InputOutcome` 相同，不通过内部可变性隐藏请求修改。
#[must_use = "条件结果须显式处理继续请求或返回值"]
pub enum InputOutcomeMut<'a, R, D = Infallible> {
    /// 继续操作同一请求，并保留独占借用。
    Request(&'a mut super::Request),
    /// 主回调结果；不在此处重复真值判定。
    Value(R),
    /// 默认回调结果，false、0 或 None 均保留。
    Default(D),
}

impl<'a, R, D> InputOutcomeMut<'a, R, D> {
    /// 临时只读借用继续分支的请求，不转移独占借用。
    pub fn request_ref(&self) -> Option<&super::Request> {
        match self {
            Self::Request(request) => Some(request),
            _ => None,
        }
    }

    /// 消费继续分支，并取回原寿命的独占请求借用。
    pub fn into_request(self) -> Option<&'a mut super::Request> {
        match self {
            Self::Request(request) => Some(request),
            _ => None,
        }
    }

    /// 消费主回调分支；保留 Some 内的原始返回值。
    pub fn into_value(self) -> Option<R> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }

    /// 消费默认回调分支，不根据默认值真值决定是否继续。
    pub fn into_default(self) -> Option<D> {
        match self {
            Self::Default(value) => Some(value),
            _ => None,
        }
    }

    /// 仅继续请求分支调用闭包，并将同一独占借用交给下一步。
    pub fn and_then<F>(self, next: F) -> crate::Result<Self>
    where
        F: FnOnce(&'a mut super::Request) -> crate::Result<Self>,
    {
        match self {
            Self::Request(request) => next(request),
            other => Ok(other),
        }
    }
}

// 共享 support 类型不反向依赖 HTTP；真值适配由持有 trait 的本层提供。
impl InputTruthy for crate::support::Stringable {}
impl<T> InputTruthy for crate::support::Collection<T> {}
impl InputTruthy for crate::support::Fluent {}
impl InputTruthy for crate::support::InputNumber {
    fn is_truthy(&self) -> bool {
        match self {
            Self::Integer(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
        }
    }
}

impl InputTruthy for crate::support::InputDate {}

impl InputTruthy for crate::support::InputInterval {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    /// 检出将主回调返回的 0、负零或 NaN 错当作统一 Rust 布尔值的实现。
    #[test]
    fn scalar_truthiness_uses_php_zero_and_string_zero_rules() {
        assert!(!false.is_truthy());
        assert!(true.is_truthy());
        macro_rules! integer_cases {
            ($($integer:ty),+ $(,)?) => {$({
                assert!(!<$integer>::from(0_u8).is_truthy());
                assert!(<$integer>::from(1_u8).is_truthy());
            })+};
        }
        integer_cases!(i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);
        assert!(!0_i8.is_truthy());
        assert!((-1_i8).is_truthy());
        assert!(!0.0_f32.is_truthy());
        assert!(!(-0.0_f32).is_truthy());
        assert!(f32::NAN.is_truthy());
        assert!(f32::INFINITY.is_truthy());
        assert!(!0.0_f64.is_truthy());
        assert!(!(-0.0_f64).is_truthy());
        assert!(f64::NAN.is_truthy());
        assert!(f64::NEG_INFINITY.is_truthy());
        assert!(!"".is_truthy());
        assert!(!"0".is_truthy());
        assert!(!String::from("0").is_truthy());
        for text in ["00", "0.0", "false", " ", "\0"] {
            assert!(text.is_truthy());
        }
        assert!(!().is_truthy());
    }

    /// 检出容器按元素真值判断、Option 丢失内值语义或引用未代理的问题。
    #[test]
    fn containers_options_and_references_keep_value_semantics() {
        assert!(!Vec::<bool>::new().is_truthy());
        assert!(vec![false].is_truthy());
        assert!(![false; 0].is_truthy());
        assert!([false; 1].is_truthy());
        let empty_slice: &[u8] = &[];
        assert!(!empty_slice.is_truthy());
        assert!(!BTreeMap::<u8, bool>::new().is_truthy());
        assert!(BTreeMap::from([(0, false)]).is_truthy());
        assert!(!HashMap::<u8, bool>::new().is_truthy());
        assert!(HashMap::from([(0, false)]).is_truthy());
        assert!(!Map::<String, Value>::new().is_truthy());
        assert!(!Option::<bool>::None.is_truthy());
        assert!(!Some(false).is_truthy());
        assert!(!Some("0").is_truthy());
        assert!(Some("00").is_truthy());
        let reference = &&String::from("0");
        assert!(!reference.is_truthy());
    }

    /// 检出动态值把 null 或空关联数组错当对象，或忽略字符串 0 的情况。
    #[test]
    fn json_values_follow_php_array_and_scalar_truthiness() {
        for value in [
            Value::Null,
            json!(false),
            json!(0),
            json!(-0.0),
            json!(""),
            json!("0"),
            json!([]),
            json!({}),
        ] {
            assert!(!value.is_truthy(), "{value:?}");
        }
        for value in [
            json!(true),
            json!(-1),
            json!(0.5),
            json!("00"),
            json!([false]),
            json!({"a":null}),
        ] {
            assert!(value.is_truthy(), "{value:?}");
        }
    }

    /// 检出对象被误按 Serialize 内容或被包装空容器判假。
    #[test]
    fn business_objects_and_response_wrappers_remain_truthy_without_serde() {
        struct Business;
        impl InputTruthy for Business {}
        struct Opaque;
        let value = Truthy::new(Opaque);
        assert!(Business.is_truthy());
        assert!(value.is_truthy());
        let _: &Opaque = value.as_ref();
        let _: Opaque = value.into_inner();
        let empty = Truthy::new(Vec::<u8>::new());
        assert!(empty.is_truthy());
        assert_eq!(empty.into_inner(), [] as [u8; 0]);
        assert!(crate::http::Json(false).is_truthy());
        assert!(http::Response::new(()).is_truthy());
    }

    /// 检出 getter 重判 false/None，或已返回分支仍调用下一闭包的问题。
    #[test]
    fn readonly_outcome_preserves_values_defaults_and_short_circuits() -> crate::Result<()> {
        let called = Cell::new(false);
        let result = InputOutcome::<'_, bool>::Value(false).and_then(|_| {
            called.set(true);
            Ok(InputOutcome::Value(true))
        })?;
        assert!(!called.get());
        assert!(result.request_ref().is_none());
        assert_eq!(result.into_value(), Some(false));
        let default = InputOutcome::<'_, (), Option<u8>>::Default(None).and_then(|_| {
            called.set(true);
            Ok(InputOutcome::Value(()))
        })?;
        assert!(!called.get());
        assert_eq!(default.into_default(), Some(None));
        assert_eq!(
            InputOutcome::<'_, (), bool>::Default(false).into_default(),
            Some(false)
        );
        assert_eq!(InputOutcome::<'_, i32>::Value(0).into_value(), Some(0));
        assert!(
            InputOutcome::<'_, bool>::Value(false)
                .into_request()
                .is_none()
        );
        Ok(())
    }

    /// 检出可变结果分支失去 false/None 或偷偷继续访问请求的情况。
    #[test]
    fn mutable_outcome_preserves_returned_branches_without_request_construction()
    -> crate::Result<()> {
        let called = Cell::new(false);
        let result = InputOutcomeMut::<'_, bool>::Value(false).and_then(|_| {
            called.set(true);
            Ok(InputOutcomeMut::Value(true))
        })?;
        assert!(!called.get());
        assert!(result.request_ref().is_none());
        assert_eq!(result.into_value(), Some(false));
        let default = InputOutcomeMut::<'_, (), Option<u8>>::Default(None).and_then(|_| {
            called.set(true);
            Ok(InputOutcomeMut::Value(()))
        })?;
        assert!(!called.get());
        assert_eq!(default.into_default(), Some(None));
        assert_eq!(
            InputOutcomeMut::<'_, (), bool>::Default(false).into_default(),
            Some(false)
        );
        assert_eq!(InputOutcomeMut::<'_, i32>::Value(0).into_value(), Some(0));
        assert!(
            InputOutcomeMut::<'_, bool>::Value(false)
                .into_request()
                .is_none()
        );
        Ok(())
    }
}
