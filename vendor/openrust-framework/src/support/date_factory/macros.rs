//! 日期宏沿用已有门面的类型擦除调用方式，注册状态归所属工厂。
use super::{DateError, DateFacade, DateFactory};
use std::{
    any::{Any, type_name},
    collections::{BTreeMap, HashMap},
    sync::{Arc, PoisonError, RwLock},
};
type Arguments = Box<dyn Any>;
type NamedCallback = dyn Fn(&DateFactory, Arguments, &'static str) -> Result<Arguments, DateMacroError>
    + Send
    + Sync;
type GenericCallback = dyn Fn(&DateFactory, &str, &dyn Any, &'static str) -> Result<Arguments, DateMacroError>
    + Send
    + Sync;
#[derive(Debug)]
pub enum DateMacroError {
    UnknownMethod(String),
    InvalidGeneratedDate,
    TypeMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    Date(DateError),
    Callback(Box<dyn std::error::Error + Send + Sync>),
}
impl std::fmt::Display for DateMacroError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidGeneratedDate => f.write_str("日期通用宏的返回值不能创建日期"),
            Self::UnknownMethod(name) => write!(f, "未知日期方法：{name}"),
            Self::TypeMismatch { expected, actual } => {
                write!(f, "日期宏类型不匹配，需要{expected}，实际为{actual}")
            }
            Self::Date(error) => write!(f, "{error}"),
            Self::Callback(error) => write!(f, "日期宏执行失败：{error}"),
        }
    }
}
impl std::error::Error for DateMacroError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Date(error) => Some(error),
            Self::Callback(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
impl From<DateError> for DateMacroError {
    fn from(error: DateError) -> Self {
        Self::Date(error)
    }
}
#[derive(Clone)]
pub struct DateMacro {
    callback: Arc<NamedCallback>,
    output: &'static str,
}
impl DateMacro {
    pub fn new<A: Any, T: Any>(
        callback: impl Fn(&DateFactory, A) -> Result<T, DateMacroError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            output: type_name::<T>(),
            callback: Arc::new(move |factory, args, actual| {
                let args = args
                    .downcast::<A>()
                    .map_err(|_| DateMacroError::TypeMismatch {
                        expected: type_name::<A>(),
                        actual,
                    })?;
                callback(factory, *args).map(|value| Box::new(value) as Arguments)
            }),
        }
    }
    pub fn call<T: Any, A: Any>(
        &self,
        factory: &DateFactory,
        args: A,
    ) -> Result<T, DateMacroError> {
        unpack(
            (self.callback)(factory, Box::new(args), type_name::<A>())?,
            self.output,
        )
    }
}
#[derive(Clone)]
pub struct DateGenericMacro {
    callback: Arc<GenericCallback>,
}
impl DateGenericMacro {
    pub fn new<A: Any, T: Any>(
        callback: impl Fn(&DateFactory, &str, &A) -> Result<T, DateMacroError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            callback: Arc::new(move |factory, name, args, actual| {
                let args = args
                    .downcast_ref::<A>()
                    .ok_or(DateMacroError::TypeMismatch {
                        expected: type_name::<A>(),
                        actual,
                    })?;
                callback(factory, name, args).map(|value| Box::new(value) as Arguments)
            }),
        }
    }
}
fn unpack<T: Any>(value: Arguments, actual: &'static str) -> Result<T, DateMacroError> {
    value
        .downcast::<T>()
        .map(|value| *value)
        .map_err(|_| DateMacroError::TypeMismatch {
            expected: type_name::<T>(),
            actual,
        })
}
#[derive(Default)]
struct Registry {
    named: HashMap<String, DateMacro>,
    generic: BTreeMap<i64, Vec<DateGenericMacro>>,
}
#[derive(Default)]
pub(super) struct DateMacros(RwLock<Registry>);
impl DateFactory {
    pub fn r#macro(&self, name: impl Into<String>, callback: impl Into<Option<DateMacro>>) {
        let mut registry = self
            .macros
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        let name = name.into();
        let retired = if let Some(callback) = callback.into() {
            registry.named.insert(name, callback)
        } else {
            registry.named.remove(&name)
        };
        drop(registry);
        drop(retired);
    }
    pub fn mixin<K: Into<String>>(&self, macros: impl IntoIterator<Item = (K, DateMacro)>) {
        for (name, callback) in macros {
            self.r#macro(name, callback);
        }
    }
    pub fn mixin_using<K, I, F>(&self, provider: F) -> Result<(), DateMacroError>
    where
        K: Into<String>,
        I: IntoIterator<Item = (K, DateMacro)>,
        F: FnOnce(&Self) -> Result<I, DateMacroError>,
    {
        self.mixin(provider(self)?);
        Ok(())
    }
    pub fn has_macro(&self, name: &str) -> bool {
        self.get_macro(name).is_some()
    }
    pub fn get_macro(&self, name: &str) -> Option<DateMacro> {
        self.macros
            .0
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .named
            .get(name)
            .cloned()
    }
    pub fn generic_macro(&self, callback: DateGenericMacro, priority: i64) {
        self.macros
            .0
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .generic
            .entry(priority)
            .or_default()
            .push(callback);
    }
    pub fn reset_macros(&self) {
        let retired = std::mem::take(
            &mut *self
                .macros
                .0
                .write()
                .unwrap_or_else(PoisonError::into_inner),
        );
        drop(retired);
    }
    /// 对应未知静态方法的动态宏分派；已声明方法仍使用其正式具名入口。
    pub fn macro_call<T: Any, A: Any>(&self, name: &str, args: A) -> Result<T, DateMacroError> {
        let (named, generic) = {
            let registry = self.macros.0.read().unwrap_or_else(PoisonError::into_inner);
            (
                registry.named.get(name).cloned(),
                registry
                    .generic
                    .iter()
                    .rev()
                    .flat_map(|(_, items)| items.iter().cloned())
                    .collect::<Vec<_>>(),
            )
        };
        if let Some(named) = named {
            return named.call(self, args);
        }
        for callback in generic {
            match (callback.callback)(self, name, &args, type_name::<A>()) {
                Ok(value) => {
                    // 默认日期工厂对未声明的通用宏结果继续执行instance，不能返回任意标量。
                    let value = generated_date(value)?;
                    return unpack(Box::new(value), type_name::<super::Date>());
                }
                Err(DateMacroError::UnknownMethod(_)) => {}
                Err(error) => return Err(error),
            }
        }
        if self.is_strict_mode_enabled() {
            Err(DateMacroError::UnknownMethod(name.into()))
        } else {
            Err(DateMacroError::InvalidGeneratedDate)
        }
    }
}
fn generated_date(value: Arguments) -> Result<super::Date, DateMacroError> {
    let value = match value.downcast::<super::Date>() {
        Ok(value) => return Ok(*value),
        Err(value) => value,
    };
    let value = match value.downcast::<jiff::Zoned>() {
        Ok(value) => {
            return Ok(super::Date::from_utc(
                value.timestamp(),
                value.time_zone().clone(),
            ));
        }
        Err(value) => value,
    };
    let value = match value.downcast::<Option<super::Date>>() {
        Ok(value) => return value.ok_or(DateMacroError::InvalidGeneratedDate),
        Err(value) => value,
    };
    if let Ok(Some(value)) = value.downcast::<Option<jiff::Zoned>>().map(|value| *value) {
        return Ok(super::Date::from_utc(
            value.timestamp(),
            value.time_zone().clone(),
        ));
    }
    Err(DateMacroError::InvalidGeneratedDate)
}
impl DateFacade {
    pub fn r#macro(
        name: impl Into<String>,
        callback: impl Into<Option<DateMacro>>,
    ) -> Result<(), DateMacroError> {
        Self::factory()?.r#macro(name, callback);
        Ok(())
    }
    pub fn mixin<K: Into<String>>(
        macros: impl IntoIterator<Item = (K, DateMacro)>,
    ) -> Result<(), DateMacroError> {
        Self::factory()?.mixin(macros);
        Ok(())
    }
    pub fn mixin_using<K, I, F>(provider: F) -> Result<(), DateMacroError>
    where
        K: Into<String>,
        I: IntoIterator<Item = (K, DateMacro)>,
        F: FnOnce(&DateFactory) -> Result<I, DateMacroError>,
    {
        Self::factory()?.mixin_using(provider)
    }
    pub fn has_macro(name: &str) -> Result<bool, DateMacroError> {
        Ok(Self::factory()?.has_macro(name))
    }
    pub fn get_macro(name: &str) -> Result<Option<DateMacro>, DateMacroError> {
        Ok(Self::factory()?.get_macro(name))
    }
    pub fn generic_macro(callback: DateGenericMacro, priority: i64) -> Result<(), DateMacroError> {
        Self::factory()?.generic_macro(callback, priority);
        Ok(())
    }
    pub fn reset_macros() -> Result<(), DateMacroError> {
        Self::factory()?.reset_macros();
        Ok(())
    }
    pub fn macro_call<T: Any, A: Any>(name: &str, args: A) -> Result<T, DateMacroError> {
        Self::factory()?.macro_call(name, args)
    }
}
