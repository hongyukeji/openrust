// 许可见 LICENSE.symfony。
use indexmap::IndexMap;
use std::{
    any::Any,
    sync::{Arc, Weak},
};

/// 可按名称注册的控制台助手。实例可记录自己最后加入的集合。
pub trait HelperInterface: Any + Send + Sync {
    fn get_name(&self) -> &str;
    fn set_helper_set(&self, helper_set: Option<&HelperSet>);
    fn get_helper_set(&self) -> Option<HelperSet>;
}

type Entries = IndexMap<String, Arc<dyn HelperInterface>>;
type SharedEntries = parking_lot::RwLock<Entries>;

/// 助手实例的集合关联。集合拥有助手，反向引用使用弱引用回收生命周期。
#[derive(Default)]
pub(super) struct HelperMembership(parking_lot::RwLock<Weak<SharedEntries>>);

impl HelperMembership {
    pub(super) const fn new() -> Self {
        Self(parking_lot::RwLock::new(Weak::new()))
    }

    pub fn set_helper_set(&self, helper_set: Option<&HelperSet>) {
        *self.0.write() = helper_set.map_or_else(Weak::new, |set| Arc::downgrade(&set.0));
    }

    pub fn get_helper_set(&self) -> Option<HelperSet> {
        self.0.read().upgrade().map(HelperSet)
    }
}

impl Clone for HelperMembership {
    fn clone(&self) -> Self {
        Self(parking_lot::RwLock::new(self.0.read().clone()))
    }
}

impl std::fmt::Debug for HelperMembership {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HelperMembership")
            .field("attached", &(self.0.read().strong_count() > 0))
            .finish()
    }
}

/// 所属控制台应用可共享的助手集合。克隆句柄保留集合身份。
#[derive(Clone, Default)]
pub struct HelperSet(Arc<SharedEntries>);

impl HelperSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_helpers(
        helpers: impl IntoIterator<Item = (Option<String>, Arc<dyn HelperInterface>)>,
    ) -> Self {
        let set = Self::new();
        for (alias, helper) in helpers {
            set.set(helper, alias.as_deref());
        }
        set
    }

    pub fn set(&self, helper: Arc<dyn HelperInterface>, alias: Option<&str>) {
        let name = helper.get_name().to_owned();
        let (previous, previous_alias) = {
            let mut entries = self.0.write();
            let previous = entries.insert(name, Arc::clone(&helper));
            let previous_alias =
                alias.and_then(|name| entries.insert(name.to_owned(), Arc::clone(&helper)));
            (previous, previous_alias)
        };
        // 用户实现和被替换实例的析构都在锁外执行。
        drop((previous, previous_alias));
        helper.set_helper_set(Some(self));
    }

    pub fn has(&self, name: &str) -> bool {
        self.0.read().contains_key(name)
    }

    pub fn get(&self, name: &str) -> Result<Arc<dyn HelperInterface>, HelperNotFound> {
        self.0
            .read()
            .get(name)
            .cloned()
            .ok_or_else(|| HelperNotFound(name.to_owned()))
    }

    pub fn get_iterator(&self) -> indexmap::map::IntoIter<String, Arc<dyn HelperInterface>> {
        self.0.read().clone().into_iter()
    }

    pub fn same_set(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl IntoIterator for &HelperSet {
    type Item = (String, Arc<dyn HelperInterface>);
    type IntoIter = indexmap::map::IntoIter<String, Arc<dyn HelperInterface>>;

    fn into_iter(self) -> Self::IntoIter {
        self.get_iterator()
    }
}

pub(super) fn resolve_helper(
    helpers: Option<&HelperSet>,
    name: &str,
) -> Result<Arc<dyn HelperInterface>, super::ConsoleError> {
    helpers
        .ok_or_else(|| super::ConsoleError::new("当前命令没有关联的控制台助手集合"))?
        .get(name)
        .map_err(super::ConsoleError::from_source)
}

/// 所请求名称在助手集合中不存在。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperNotFound(String);

impl std::fmt::Display for HelperNotFound {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "控制台助手未定义：{}", self.0)
    }
}

impl std::error::Error for HelperNotFound {}
