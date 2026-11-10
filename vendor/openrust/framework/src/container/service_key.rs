//! Process-local typed and named service identities.

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
        formatter.write_str(self.name())
    }
}

impl From<&str> for ServiceKey {
    fn from(name: &str) -> Self {
        Self::named(name)
    }
}

impl From<String> for ServiceKey {
    fn from(name: String) -> Self {
        Self::named(name)
    }
}
