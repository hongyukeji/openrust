//! 编译期绑定声明的环境选择元数据。

use super::ServiceKey;
use crate::config::Environment;

/// 将声明它的服务关联到一个具体实现。
///
/// 服务可以声明多个 `Bind`。容器按声明顺序选择环境匹配项，无匹配时使用首个回退声明。
#[derive(Clone)]
pub struct Bind {
    concrete: ServiceKey,
    environments: Vec<String>,
    fallback: bool,
}

impl Bind {
    /// 声明用于无环境匹配时的回退实现。
    #[must_use]
    pub fn new(concrete: ServiceKey) -> Self {
        Self::for_environments(concrete, ["*"])
    }

    /// 声明匹配任一所给环境模式的实现。空字符串和 `"0"` 会被过滤。
    ///
    /// 仅当过滤后唯一的 `"*"` 位于原始列表的首位时，才是回退声明。
    /// 过滤后没有环境时，容器在使用声明时报告错误。
    #[must_use]
    pub fn for_environments(
        concrete: ServiceKey,
        environments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let mut patterns = environments.into_iter().map(Into::<String>::into);
        let first = patterns.next();
        let starts_with_wildcard = first.as_deref() == Some("*");
        let environments = first
            .into_iter()
            .chain(patterns)
            .filter(|pattern| !matches!(pattern.as_str(), "" | "0"))
            .collect::<Vec<_>>();
        Self {
            concrete,
            fallback: starts_with_wildcard && environments.len() == 1,
            environments,
        }
    }

    /// 借用具体实现的服务键。
    #[must_use]
    pub const fn concrete(&self) -> &ServiceKey {
        &self.concrete
    }

    /// 借用过滤后的环境模式，保持原来的相对顺序。
    #[must_use]
    pub fn environments(&self) -> &[String] {
        &self.environments
    }

    pub(in crate::container) const fn is_fallback(&self) -> bool {
        self.fallback
    }

    pub(in crate::container) fn matches(&self, environment: &Environment) -> bool {
        environment.matches(self.environments.iter().map(String::as_str))
    }
}
