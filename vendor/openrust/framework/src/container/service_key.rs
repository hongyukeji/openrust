//! typed与命名服务共用的进程内键；诊断名不参与typed身份。

#![forbid(unsafe_code)]

use std::{
    any::{TypeId, type_name},
    fmt,
    hash::Hash,
    sync::Arc,
};

/// 同一绑定目录中的服务身份。
///
/// typed键使用本进程的[`TypeId`]，命名键使用精确字符串；两者互不等同。
/// 命名键不包含调用方期望返回的Rust类型，允许空字符串且区分大小写。
/// 本类型不提供持久化或序列化；诊断名称不能用作跨进程类型标识。
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ServiceKey {
    kind: Kind,
}

#[derive(Clone, Debug)]
enum Kind {
    Typed { id: TypeId, name: &'static str },
    Named(Arc<str>),
}

impl ServiceKey {
    /// 创建typed服务键；支持具体类型及`dyn Trait`等非定长类型。
    #[must_use]
    pub fn typed<T: ?Sized + 'static>() -> Self {
        Self::from_type_id(TypeId::of::<T>(), type_name::<T>())
    }

    /// 创建精确命名键，不修剪、规范化或拒绝空字符串。
    #[must_use]
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self {
            kind: Kind::Named(name.into()),
        }
    }

    /// 返回typed键的进程内类型身份；命名键返回`None`。
    #[must_use]
    pub fn type_id(&self) -> Option<TypeId> {
        match &self.kind {
            Kind::Typed { id, .. } => Some(*id),
            Kind::Named(_) => None,
        }
    }

    /// 返回typed诊断名或原始命名字符串，供错误链借用。
    ///
    /// 返回值借用本键，调用方不能假定命名键具有静态生命周期。
    #[must_use]
    pub fn name(&self) -> &str {
        match &self.kind {
            Kind::Typed { name, .. } => name,
            Kind::Named(name) => name,
        }
    }

    pub(super) fn identity(&self) -> Arc<str> {
        match &self.kind {
            Kind::Typed { name, .. } => Arc::from(*name),
            Kind::Named(name) => Arc::clone(name),
        }
    }

    pub(super) fn from_type_id(id: TypeId, name: &'static str) -> Self {
        Self {
            kind: Kind::Typed { id, name },
        }
    }
}

impl PartialEq for Kind {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Typed { id: left, .. }, Self::Typed { id: right, .. }) => left == right,
            (Self::Named(left), Self::Named(right)) => left == right,
            _ => false,
        }
    }
}

impl Eq for Kind {}

impl std::hash::Hash for Kind {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Typed { id, .. } => id.hash(state),
            Self::Named(name) => name.hash(state),
        }
    }
}

impl fmt::Display for ServiceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            Kind::Typed { name, .. } => formatter.write_str(name),
            Kind::Named(name) => fmt::Display::fmt(
                &crate::core::error::ServiceIdentity::from_name(Arc::clone(name)),
                formatter,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ServiceKey;
    use std::{any::TypeId, collections::HashMap};

    trait ExampleService {}

    #[test]
    fn typed_dyn_identity_survives_clone_and_legacy_diagnostic_changes() {
        let key = ServiceKey::typed::<dyn ExampleService>();
        let legacy = ServiceKey::from_type_id(TypeId::of::<dyn ExampleService>(), "旧诊断名称");
        assert_eq!(key.type_id(), Some(TypeId::of::<dyn ExampleService>()));
        assert_eq!(key, key.clone());
        assert_eq!(key, legacy);
        let mut directory = HashMap::new();
        directory.insert(key.clone(), 1);
        assert_eq!(directory.insert(legacy, 2), Some(1));
        assert_eq!(directory.get(&key), Some(&2));
        assert_ne!(key, ServiceKey::typed::<u64>());
    }

    #[test]
    fn named_keys_preserve_empty_case_and_whitespace_without_return_type_identity() {
        let name = String::from("mail.transport");
        let key = ServiceKey::named(name.clone());
        let mut directory = HashMap::new();
        directory.insert(key.clone(), "factory");
        assert_eq!(directory.get(&ServiceKey::named(name)), Some(&"factory"));
        assert_eq!(key.type_id(), None);
        assert_eq!(key.name(), "mail.transport");
        assert_eq!(key.to_string(), "mail.transport");
        assert_ne!(key, ServiceKey::named("Mail.transport"));
        assert_ne!(key, ServiceKey::named("mail.transport "));
        assert_eq!(ServiceKey::named("").name(), "");
    }

    #[test]
    fn typed_key_and_identically_spelled_named_key_are_separate_registry_entries() {
        let typed = ServiceKey::typed::<dyn ExampleService>();
        let named = ServiceKey::named(typed.name().to_owned());
        let directory = HashMap::from([(typed.clone(), "typed"), (named.clone(), "named")]);
        assert_ne!(typed, named);
        assert_eq!(directory.len(), 2);
        assert_eq!(directory.get(&typed), Some(&"typed"));
        assert_eq!(directory.get(&named), Some(&"named"));
    }

    #[test]
    fn named_display_bounds_and_escapes_without_changing_exact_identity() {
        let raw = format!("service\n{}", "x".repeat(256));
        let key = ServiceKey::named(raw.clone());
        let visible = key.to_string();
        assert_eq!(key.name(), raw);
        assert!(visible.contains("\\n"));
        assert!(!visible.contains('\n'));
        assert!(visible.len() <= 128);
        let typed = ServiceKey::typed::<u64>();
        assert_eq!(typed.to_string(), typed.name());
    }
}
