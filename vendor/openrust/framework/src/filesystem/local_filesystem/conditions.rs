use super::{FileError, FileResult, Filesystem};
use std::ops::Deref;

/// 保留传入值的类型，同时显式表达条件真值；业务对象可自行实现。
pub trait ConditionValue {
    fn is_truthy(&self) -> bool;
}
impl ConditionValue for bool {
    fn is_truthy(&self) -> bool {
        *self
    }
}
impl ConditionValue for str {
    fn is_truthy(&self) -> bool {
        !self.is_empty() && self != "0"
    }
}
impl ConditionValue for String {
    fn is_truthy(&self) -> bool {
        self.as_str().is_truthy()
    }
}
impl<T: ConditionValue + ?Sized> ConditionValue for &T {
    fn is_truthy(&self) -> bool {
        T::is_truthy(self)
    }
}
impl<T: ConditionValue> ConditionValue for Option<T> {
    fn is_truthy(&self) -> bool {
        self.as_ref().is_some_and(ConditionValue::is_truthy)
    }
}
impl<T> ConditionValue for Vec<T> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<T> ConditionValue for [T] {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<T, const N: usize> ConditionValue for [T; N] {
    fn is_truthy(&self) -> bool {
        N != 0
    }
}
impl<K, V, S> ConditionValue for std::collections::HashMap<K, V, S> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
impl<K, V> ConditionValue for std::collections::BTreeMap<K, V> {
    fn is_truthy(&self) -> bool {
        !self.is_empty()
    }
}
macro_rules! numeric_conditions {($($kind:ty),+)=>{$(impl ConditionValue for $kind {fn is_truthy(&self)->bool {*self != Self::default()}})+}}
numeric_conditions!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);
impl ConditionValue for serde_json::Value {
    fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::String(value) => value.is_truthy(),
            Self::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
            Self::Array(value) => !value.is_empty(),
            Self::Object(_) => true,
        }
    }
}

/// 回调返回的值与原服务分别保留，None不会伪造一个默认返回值。
#[derive(Debug)]
pub enum ConditionalResult<S, T> {
    Target(S),
    Value(T),
}
impl<S, T> ConditionalResult<S, T> {
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Value(value) => Some(value),
            Self::Target(_) => None,
        }
    }
    pub fn into_target(self) -> Option<S> {
        match self {
            Self::Target(target) => Some(target),
            Self::Value(_) => None,
        }
    }
    pub(in crate::filesystem) fn map_target<R>(
        self,
        map: impl FnOnce(S) -> R,
    ) -> ConditionalResult<R, T> {
        match self {
            Self::Target(target) => ConditionalResult::Target(map(target)),
            Self::Value(value) => ConditionalResult::Value(value),
        }
    }
}

/// 条件为假时不执行操作；捕获入口先计算一次条件，再选择实际调用。
pub struct HigherOrderWhenProxy<S> {
    target: S,
    condition: Option<bool>,
    negate_capture: bool,
}
impl<S: Deref<Target = Filesystem>> HigherOrderWhenProxy<S> {
    pub(crate) fn new(target: S, condition: Option<bool>, negate_capture: bool) -> Self {
        Self {
            target,
            condition,
            negate_capture,
        }
    }
    pub fn condition(mut self, value: impl ConditionValue) -> Self {
        self.condition = Some(value.is_truthy());
        self
    }
    pub fn capture<V: ConditionValue>(mut self, test: impl FnOnce(&Filesystem) -> V) -> Self {
        self.condition = Some(test(&self.target).is_truthy() ^ self.negate_capture);
        self
    }
    pub fn try_capture<V: ConditionValue>(
        mut self,
        test: impl FnOnce(&Filesystem) -> FileResult<V>,
    ) -> FileResult<Self> {
        self.condition = Some(test(&self.target)?.is_truthy() ^ self.negate_capture);
        Ok(self)
    }
    pub fn call<T>(
        self,
        callback: impl FnOnce(&Filesystem) -> FileResult<T>,
    ) -> FileResult<ConditionalResult<S, T>> {
        match self.condition {
            Some(true) => callback(&self.target).map(ConditionalResult::Value),
            Some(false) => Ok(ConditionalResult::Target(self.target)),
            None => Err(FileError::MissingCondition),
        }
    }
}
impl Filesystem {
    pub fn when<V: ConditionValue, T>(
        &self,
        value: V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        if value.is_truthy() {
            Ok(match callback(self, value)? {
                Some(value) => ConditionalResult::Value(value),
                None => ConditionalResult::Target(self),
            })
        } else {
            Ok(ConditionalResult::Target(self))
        }
    }
    pub fn unless<V: ConditionValue, T>(
        &self,
        value: V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        if !value.is_truthy() {
            Ok(match callback(self, value)? {
                Some(value) => ConditionalResult::Value(value),
                None => ConditionalResult::Target(self),
            })
        } else {
            Ok(ConditionalResult::Target(self))
        }
    }
    pub fn when_else<V: ConditionValue, T>(
        &self,
        value: V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
        fallback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        let result = if value.is_truthy() {
            callback(self, value)?
        } else {
            fallback(self, value)?
        };
        Ok(match result {
            Some(value) => ConditionalResult::Value(value),
            None => ConditionalResult::Target(self),
        })
    }
    pub fn unless_else<V: ConditionValue, T>(
        &self,
        value: V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
        fallback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        let result = if !value.is_truthy() {
            callback(self, value)?
        } else {
            fallback(self, value)?
        };
        Ok(match result {
            Some(value) => ConditionalResult::Value(value),
            None => ConditionalResult::Target(self),
        })
    }
    pub fn when_test<V: ConditionValue, T>(
        &self,
        test: impl FnOnce(&Self) -> V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        self.when(test(self), callback)
    }
    pub fn unless_test<V: ConditionValue, T>(
        &self,
        test: impl FnOnce(&Self) -> V,
        callback: impl FnOnce(&Self, V) -> FileResult<Option<T>>,
    ) -> FileResult<ConditionalResult<&Self, T>> {
        self.unless(test(self), callback)
    }
    pub fn when_condition(&self, value: impl ConditionValue) -> HigherOrderWhenProxy<&Self> {
        HigherOrderWhenProxy::new(self, Some(value.is_truthy()), false)
    }
    pub fn unless_condition(&self, value: impl ConditionValue) -> HigherOrderWhenProxy<&Self> {
        HigherOrderWhenProxy::new(self, Some(!value.is_truthy()), false)
    }
    pub fn when_capture(&self) -> HigherOrderWhenProxy<&Self> {
        HigherOrderWhenProxy::new(self, None, false)
    }
    pub fn unless_capture(&self) -> HigherOrderWhenProxy<&Self> {
        HigherOrderWhenProxy::new(self, None, true)
    }
}
