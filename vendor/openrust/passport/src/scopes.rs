//! Passport scope 注册、纯过滤规则与实际客户端模型的 scope 派发。

use crate::{ClientModel, PhpArray};
use serde::{Deserialize, Serialize};

/// 向授权界面与 JSON API 暴露的 scope 标识符和说明。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    /// 应用注册的 scope 标识符。
    pub id: String,
    /// 授权界面展示的说明。
    pub description: String,
}

/// 保留注册顺序的 scope 仓储；不查询或认证客户端。
///
/// 纯规则可判断客户端限制快照；授权编排须向模型派发入口传入实际选中的客户端。
/// 本仓储不查询或认证客户端，不替代活动状态及事务末检。
/// 限制数组保留原始值，成员判断委托框架的 PHP 弱比较互操作桥。
#[derive(Clone, Debug, Default)]
pub struct ScopeRepository {
    definitions: Vec<Scope>,
    inherited: bool,
}

impl ScopeRepository {
    /// 建立 scope 注册表；重复标识符取最后说明并保留首次插入位置。
    #[must_use]
    pub fn new(scopes: impl IntoIterator<Item = Scope>) -> Self {
        let mut repository = Self::default();
        repository.tokens_can(scopes);
        repository
    }

    /// 替换整个注册表；不改变已经配置的 scope 继承策略。
    pub fn tokens_can(&mut self, scopes: impl IntoIterator<Item = Scope>) {
        self.definitions.clear();
        for scope in scopes {
            if let Some(existing) = self
                .definitions
                .iter_mut()
                .find(|existing| existing.id == scope.id)
            {
                *existing = scope;
            } else {
                self.definitions.push(scope);
            }
        }
    }

    /// 检查已注册标识符；特殊 `*` 无需显式注册也返回 true。
    #[must_use]
    pub fn has_scope(&self, scope: &str) -> bool {
        scope == "*" || self.definitions.iter().any(|defined| defined.id == scope)
    }

    /// 按注册顺序返回标识符；不为特殊 `*` 自动增加注册项。
    #[must_use]
    pub fn scope_ids(&self) -> Vec<String> {
        self.definitions
            .iter()
            .map(|scope| scope.id.clone())
            .collect()
    }

    /// 按注册顺序返回带说明的 scope 快照。
    #[must_use]
    pub fn scopes(&self) -> Vec<Scope> {
        self.definitions.clone()
    }

    /// 按输入顺序返回确实注册的项，保留重复输入；未知项被忽略。
    #[must_use]
    pub fn scopes_for(&self, scopes: &[String]) -> Vec<Scope> {
        scopes
            .iter()
            .filter_map(|id| self.definitions.iter().find(|scope| scope.id == *id))
            .cloned()
            .collect()
    }

    /// 显式开启或关闭冒号前缀继承；默认关闭。
    #[must_use]
    pub fn with_inherited_scopes(mut self, inherited: bool) -> Self {
        self.inherited = inherited;
        self
    }

    /// 检查当前活动客户端的 scope 限制；缺失或 null 不限制，显式空列表全部拒绝。
    ///
    /// 客户端的 `*` 仍作为普通标识符匹配，不授予任意其他 scope。
    #[must_use]
    pub fn client_has_scope(&self, restrictions: Option<&PhpArray>, scope: &str) -> bool {
        restrictions.is_none_or(|restrictions| self.scope_exists_in(scope, restrictions))
    }

    /// 检查令牌已授予的 scope；空列表拒绝，持有 `*` 则允许任意 scope。
    #[must_use]
    pub fn token_can(&self, token_scopes: &PhpArray, scope: &str) -> bool {
        !token_scopes.is_empty()
            && (token_scopes.contains_loose("*") || self.scope_exists_in(scope, token_scopes))
    }

    /// 按 grant、全局注册表和显式限制快照纯过滤，保留顺序及重复项。
    ///
    /// 仅 `password`、`personal_access`、`client_credentials` grant 保留 `*`。
    /// 本方法不派发客户端模型覆写；需要实际模型策略时使用 `finalize_model_scopes`。
    /// 不替代客户端认证或活动状态的数据库校验。
    #[must_use]
    pub fn finalize_scopes(
        &self,
        scopes: &[String],
        grant_type: &str,
        client_restrictions: Option<&PhpArray>,
    ) -> Vec<String> {
        let allow_wildcard = matches!(
            grant_type,
            "password" | "personal_access" | "client_credentials"
        );
        scopes
            .iter()
            .filter(|scope| allow_wildcard || scope.as_str() != "*")
            .filter(|scope| self.has_scope(scope))
            .filter(|scope| self.client_has_scope(client_restrictions, scope))
            .cloned()
            .collect()
    }

    /// 先复用 grant/全局注册过滤，再逐项派发实际客户端模型的 scope 覆写。
    ///
    /// 保留请求项顺序和重复；未知项及当前 grant 不允许的 `*` 不触发模型回调。
    /// `None` 仅跳过客户端过滤，不证明客户端活动或已认证；授权编排仍须自行保留
    /// 同一 Factory 所选客户端及安全快照，并在存储提交前完成活动状态和权威末检。
    /// 本方法不承担上游 finalize 时重新查询活动客户端的时点；完整查询接线另行验收。
    /// 也不证明依赖外部可变状态的模型策略在提交时仍未变化。
    ///
    /// # Errors
    /// 客户端模型 `has_scope` 的错误原样返回，并停止后续回调。
    pub fn finalize_model_scopes(
        &self,
        scopes: &[String],
        grant_type: &str,
        client: Option<&dyn ClientModel>,
    ) -> openrust::Result<Vec<String>> {
        let mut finalized = Vec::new();
        for scope in self.finalize_scopes(scopes, grant_type, None) {
            if let Some(client) = client
                && !client.has_scope(self, &scope)?
            {
                continue;
            }
            finalized.push(scope);
        }
        Ok(finalized)
    }

    fn scope_exists_in(&self, scope: &str, haystack: &PhpArray) -> bool {
        haystack.contains_loose(scope)
            || (self.inherited
                && scope
                    .match_indices(':')
                    .any(|(index, _)| haystack.contains_loose(&scope[..index])))
    }
}

#[cfg(test)]
#[path = "scopes_tests.rs"]
mod tests;
