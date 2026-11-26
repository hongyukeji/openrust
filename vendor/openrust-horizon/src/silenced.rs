use std::{any::type_name, collections::HashSet, sync::RwLock};

use openrust::{
    container::ServiceKey, contracts::container::BindingResolutionError, support::facades::Facade,
};

/// 标记可通过 `Horizon::silence` 注册的应用任务类型。
pub trait Silenced: 'static {}

#[derive(Default)]
pub(crate) struct SilencedTypes(RwLock<HashSet<&'static str>>);

struct Silencing;

impl Facade for Silencing {
    type Service = SilencedTypes;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<SilencedTypes>()
    }
}

impl SilencedTypes {
    pub(crate) fn contains(subject: &str) -> Result<bool, BindingResolutionError> {
        Ok(Silencing::get_facade_root()?
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(subject))
    }
}

impl crate::Horizon {
    /// 在当前应用注册一个实现 `Silenced` 的具体类型。
    pub fn silence<T: Silenced>() -> Result<(), BindingResolutionError> {
        Silencing::get_facade_root()?
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(type_name::<T>());
        Ok(())
    }
}
