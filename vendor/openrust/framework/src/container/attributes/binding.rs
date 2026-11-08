//! 编译期绑定声明的环境选择元数据。

use super::{Arc, BoxFuture, Resolver, Result, ServiceKey};
use crate::config::Environment;
use std::future::Future;

pub(in crate::container) enum BindingDeclaration {
    Environment(Bind),
    Conditional(BindWhen),
}

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

type Condition = dyn Fn(Resolver) -> BoxFuture<'static, Result<bool>> + Send + Sync;

pub(in crate::container) struct BindingCondition(pub Box<Condition>);

/// 按声明顺序求值的条件绑定。条件为真时登记指定的具体实现。
///
/// 条件通过当前的 [`Resolver`] 读取配置、检查绑定或异步解析依赖。
/// 条件全部未命中时，下一次独立解析会再次求值。
#[derive(Clone)]
pub struct BindWhen {
    concrete: ServiceKey,
    condition: Arc<BindingCondition>,
}

impl BindWhen {
    /// 声明具体实现及其条件。请通过传入的解析器解析条件所需的服务。
    #[must_use]
    pub fn new<F, Fut>(concrete: ServiceKey, condition: F) -> Self
    where
        F: Fn(Resolver) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<bool>> + Send + 'static,
    {
        Self {
            concrete,
            condition: Arc::new(BindingCondition(Box::new(move |resolver| {
                Box::pin(condition(resolver))
            }))),
        }
    }

    /// 借用条件命中时所选实现的服务键。
    #[must_use]
    pub const fn concrete(&self) -> &ServiceKey {
        &self.concrete
    }

    pub(in crate::container) fn condition(&self) -> Arc<BindingCondition> {
        Arc::clone(&self.condition)
    }
}
