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

    fn getFacadeAccessor() -> ServiceKey {
        "gate".into()
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's Gate facade")]
impl Gate {
    pub fn registry() -> AuthResult<Arc<GateRegistry>> {
        Self::getFacadeRoot().map_err(AuthError::backend)
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

    pub fn defineGuest(
        ability: impl Into<String>,
        callback: impl for<'a> Fn(
            Option<&'a dyn Authenticatable>,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Response>>
        + Send
        + Sync
        + 'static,
    ) -> AuthResult<()> {
        Self::registry()?.defineGuest(ability, callback);
        Ok(())
    }

    pub fn policy<T: Send + Sync + 'static>(policy: impl Policy<T> + 'static) -> AuthResult<()> {
        Self::registry()?.policy(policy);
        Ok(())
    }

    /// 登记可按约定名称发现的已编译策略，构造仍由应用容器负责。
    pub fn discoverPolicy<T, P>() -> AuthResult<()>
    where
        T: Send + Sync + 'static,
        P: Policy<T> + 'static,
    {
        Self::registry()?.discoverPolicy::<T, P>();
        Ok(())
    }

    pub fn getPolicyFor(target: &Argument) -> AuthResult<Option<PolicyHandle>> {
        Self::registry()?.getPolicyFor(target)
    }

    pub fn resolvePolicy<P: Send + Sync + 'static>() -> AuthResult<Arc<P>> {
        Self::registry()?.resolvePolicy::<P>()
    }

    pub fn guessPolicyNamesUsing(
        callback: impl Fn(&str) -> Vec<String> + Send + Sync + 'static,
    ) -> AuthResult<()> {
        Self::registry()?.guessPolicyNamesUsing(callback);
        Ok(())
    }

    pub fn policies() -> AuthResult<std::collections::HashMap<std::any::TypeId, &'static str>> {
        Ok(Self::registry()?.policies())
    }

    pub fn forUser(user: Option<Arc<dyn Authenticatable>>) -> AuthResult<ScopedGate> {
        Ok(Self::registry()?.forUser(user))
    }

    pub async fn forRequest(request: &mut Request) -> AuthResult<ScopedGate> {
        Self::forUser(Auth::user(request).await?)
    }

    pub async fn inspect(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<Response> {
        Self::forRequest(request)
            .await?
            .inspect(ability, arguments)
            .await
    }

    pub async fn allows(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<bool> {
        Self::forRequest(request)
            .await?
            .allows(ability, arguments)
            .await
    }

    pub async fn denies(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> AuthResult<bool> {
        Self::forRequest(request)
            .await?
            .denies(ability, arguments)
            .await
    }

    pub async fn authorize(
        request: &mut Request,
        ability: &str,
        arguments: &[&Argument],
    ) -> Result<Response, AuthorizationError> {
        Self::forRequest(request)
            .await
            .map_err(AuthorizationError::Evaluation)?
            .authorize(ability, arguments)
            .await
    }
}
