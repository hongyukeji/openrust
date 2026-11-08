//! 一次装配的编译类身份；不登记绑定、不查别名，也不保存服务实例。

use super::ServiceKey;
use crate::core::{Error, Result};
use std::{any::TypeId, collections::HashMap, sync::Arc};

/// 已编译描述符提供的显式类符号及Rust类型身份。
pub(super) struct ClassIdentity {
    symbol: Arc<str>,
    canonical: ServiceKey,
}

impl ClassIdentity {
    /// 创建显式声明；不从Rust诊断名称推断类符号。
    pub(super) fn of<T: ?Sized + 'static>(symbol: impl Into<Arc<str>>) -> Self {
        Self {
            symbol: symbol.into(),
            canonical: ServiceKey::typed::<T>(),
        }
    }
}

/// Container装配前冻结的类身份投影；与绑定目录职责分离。
#[derive(Default)]
pub(super) struct ClassIdentities {
    by_symbol: HashMap<Arc<str>, ServiceKey>,
    by_type: HashMap<TypeId, Arc<str>>,
}

impl ClassIdentities {
    /// 校验整批身份后发布；同符号或同类型的任何重复均拒绝。
    pub(super) fn from_entries(entries: impl IntoIterator<Item = ClassIdentity>) -> Result<Self> {
        let mut identities = Self::default();
        for entry in entries {
            let Some(id) = entry.canonical.type_id() else {
                return Err(invalid_class_identity());
            };
            if identities.by_symbol.contains_key(&entry.symbol)
                || identities.by_type.contains_key(&id)
            {
                return Err(invalid_class_identity());
            }
            identities.by_type.insert(id, Arc::clone(&entry.symbol));
            identities.by_symbol.insert(entry.symbol, entry.canonical);
        }
        Ok(identities)
    }

    /// 先规范显式类符号，再由调用方法决定是否跟随alias。
    pub(super) fn canonical_key(&self, input: &ServiceKey) -> ServiceKey {
        self.symbol_key(input).unwrap_or(input).clone()
    }

    pub(super) fn canonical_key_owned(&self, input: ServiceKey) -> ServiceKey {
        self.symbol_key(&input).cloned().unwrap_or(input)
    }

    fn symbol_key(&self, input: &ServiceKey) -> Option<&ServiceKey> {
        if input.type_id().is_none() {
            self.by_symbol.get(input.name())
        } else {
            None
        }
    }

    /// 获取已声明类的原始符号；普通服务名不伪装成类声明。
    #[cfg(test)]
    pub(super) fn symbol(&self, input: &ServiceKey) -> Option<&str> {
        let canonical = self.canonical_key(input);
        canonical
            .type_id()
            .and_then(|id| self.by_type.get(&id))
            .map(AsRef::as_ref)
    }
}

fn invalid_class_identity() -> Error {
    Error::custom_with_status(
        http::StatusCode::INTERNAL_SERVER_ERROR,
        "container_class_identity_duplicate",
        "编译类符号或Rust类型的声明重复或无效",
    )
}

#[cfg(test)]
mod tests {
    use super::{ClassIdentities, ClassIdentity};
    use crate::{Result, container::ServiceKey};

    struct First;
    struct Second;

    #[test]
    fn explicit_symbol_and_type_share_identity_without_guessing_diagnostic_names() -> Result<()> {
        let directory = ClassIdentities::from_entries([ClassIdentity::of::<First>("Acme\\First")])?;
        let typed = ServiceKey::typed::<First>();
        assert_eq!(
            directory.canonical_key(&ServiceKey::named("Acme\\First")),
            typed
        );
        assert_eq!(directory.canonical_key(&typed), typed);
        for name in ["acme\\First", "", std::any::type_name::<First>()] {
            let key = ServiceKey::named(name);
            assert_eq!(directory.canonical_key(&key), key);
            assert_ne!(directory.canonical_key(&key), typed);
            assert_eq!(directory.symbol(&key), None);
        }
        assert_eq!(directory.symbol(&typed), Some("Acme\\First"));
        assert_eq!(
            directory.symbol(&ServiceKey::named("Acme\\First")),
            Some("Acme\\First")
        );
        assert_eq!(directory.symbol(&ServiceKey::typed::<Second>()), None);
        Ok(())
    }

    #[test]
    fn duplicate_symbol_type_and_identical_declaration_are_rejected() {
        for declarations in [
            [
                ClassIdentity::of::<First>("Same"),
                ClassIdentity::of::<Second>("Same"),
            ],
            [
                ClassIdentity::of::<First>("One"),
                ClassIdentity::of::<First>("Two"),
            ],
            [
                ClassIdentity::of::<First>("Same"),
                ClassIdentity::of::<First>("Same"),
            ],
        ] {
            let result = ClassIdentities::from_entries(declarations);
            assert!(
                matches!(result, Err(ref error) if error.code() == "container_class_identity_duplicate" && error.status() == http::StatusCode::INTERNAL_SERVER_ERROR)
            );
        }
    }

    #[test]
    fn typed_diagnostic_name_never_redirects_to_another_declared_class() -> Result<()> {
        let second_name = std::any::type_name::<Second>();
        let directory = ClassIdentities::from_entries([
            ClassIdentity::of::<First>(second_name),
            ClassIdentity::of::<Second>("ActualSecond"),
        ])?;
        assert_eq!(
            directory.canonical_key(&ServiceKey::named(second_name)),
            ServiceKey::typed::<First>()
        );
        assert_eq!(
            directory.canonical_key(&ServiceKey::typed::<Second>()),
            ServiceKey::typed::<Second>()
        );
        assert_eq!(
            directory.symbol(&ServiceKey::typed::<First>()),
            Some(second_name)
        );
        assert_eq!(
            directory.symbol(&ServiceKey::typed::<Second>()),
            Some("ActualSecond")
        );
        Ok(())
    }

    #[test]
    fn frozen_directory_owns_symbols_and_keeps_unbound_names_unchanged() -> Result<()> {
        let symbol = String::from("Owned");
        let directory =
            ClassIdentities::from_entries([ClassIdentity::of::<First>(symbol.as_str())])?;
        drop(symbol);
        assert_eq!(
            directory.symbol(&ServiceKey::typed::<First>()),
            Some("Owned")
        );
        let unbound = ServiceKey::named("Other");
        assert_eq!(directory.canonical_key(&unbound), unbound);
        let empty = ClassIdentities::default();
        assert_eq!(
            empty.canonical_key(&ServiceKey::named("Owned")),
            ServiceKey::named("Owned")
        );
        Ok(())
    }
}
