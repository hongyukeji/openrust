use std::sync::Arc;

use crate::{
    auth::{Auth, AuthError, AuthFuture, AuthResult},
    container::ServiceKey,
    contracts::auth::Authenticatable,
    http::Request,
    support::facades::Facade,
};

use super::{
    Argument, AuthorizationError, Gate as ScopedGate, GateRegistry, Policy, PolicyHandle, Response,
};

/// Registers authorization rules and resolves the current request's Gate.
pub struct Gate;

impl Facade for Gate {
    type Service = GateRegistry;

    fn get_facade_accessor() -> ServiceKey {
        "gate".into()
    }
}

impl Gate {
    pub fn registry() -> AuthResult<Arc<GateRegistry>> {
        Self::get_facade_root().map_err(AuthError::backend)
    }

    pub fn define(
        ability: impl Into<String>,
        callback: impl for<'a> Fn(
            &'a dyn Authenticatable,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Response>>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        Self::registry()?.define(ability, callback);
        Ok(())
    }

    pub fn define_guest(
        ability: impl Into<String>,
        callback: impl for<'a> Fn(
            Option<&'a dyn Authenticatable>,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Response>>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        Self::registry()?.define_guest(ability, callback);
        Ok(())
    }

    pub fn policy<T: Send + Sync + 'static>(policy: impl Policy<T> + 'static) -> AuthResult<()> {
        Self::registry()?.policy(policy);
        Ok(())
    }

    /// 登记可按约定名称发现的已编译策略，构造仍由应用容器负责。
    pub fn discover_policy<T, P>() -> AuthResult<()>
    where
        T: Send + Sync + 'static,
        P: Policy<T> + 'static,
    {
        Self::registry()?.discover_policy::<T, P>();
        Ok(())
    }

    pub fn get_policy_for(target: &Argument) -> AuthResult<Option<PolicyHandle>> {
        Self::registry()?.get_policy_for(target)
    }

    pub fn resolve_policy<P: Send + Sync + 'static>() -> AuthResult<Arc<P>> {
        Self::registry()?.resolve_policy::<P>()
    }

    pub fn guess_policy_names_using(
        callback: impl Fn(&str) -> Vec<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        Self::registry()?.guess_policy_names_using(callback);
        Ok(())
    }

    pub fn policies() -> AuthResult<std::collections::HashMap<std::any::TypeId, &'static str>> {
        Ok(Self::registry()?.policies())
    }

    pub fn for_user(user: Option<Arc<dyn Authenticatable>>) -> AuthResult<ScopedGate> {
        Ok(Self::registry()?.for_user(user))
    }

    pub async fn for_request(request: &mut Request) -> AuthResult<ScopedGate> {
        Self::for_user(Auth::user(request).await?)
    }

    pub async fn inspect(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<Response> {
        Self::for_request(request)
            .await?
            .inspect(ability, arguments)
            .await
    }

    pub async fn allows(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<bool> {
        Self::for_request(request)
            .await?
            .allows(ability, arguments)
            .await
    }

    pub async fn denies(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<bool> {
        Self::for_request(request)
            .await?
            .denies(ability, arguments)
            .await
    }

    pub async fn authorize(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> Result<Response, AuthorizationError> {
        Self::for_request(request)
            .await
            .map_err(AuthorizationError::Evaluation)?
            .authorize(ability, arguments)
            .await
    }
}
