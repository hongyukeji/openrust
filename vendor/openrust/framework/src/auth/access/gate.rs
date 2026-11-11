use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, RwLock},
};

use crate::{
    auth::{AuthFuture, AuthResult},
    contracts::auth::{Authenticatable, access::Gate as GateContract},
};

use super::{Argument, AuthorizationError, Policy, PolicyTarget, Response};

type Decision = Option<Response>;
type Ability = dyn for<'a> Fn(
        Option<&'a dyn Authenticatable>,
        &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Decision>>
    + Send
    + Sync;
type Before = dyn for<'a> Fn(
        Option<&'a dyn Authenticatable>,
        &'a str,
        &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Decision>>
    + Send
    + Sync;
type After = dyn for<'a> Fn(
        Option<&'a dyn Authenticatable>,
        &'a str,
        Option<&'a Response>,
        &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Decision>>
    + Send
    + Sync;

trait RegisteredPolicy: Send + Sync {
    fn has_ability(&self, ability: &str) -> bool;
    fn evaluate<'a>(
        &'a self,
        user: Option<&'a dyn Authenticatable>,
        ability: &'a str,
        args: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Decision>>;
}

struct TypedPolicy<T, P>(P, std::marker::PhantomData<fn() -> T>);

impl<T: Send + Sync + 'static, P: Policy<T>> RegisteredPolicy for TypedPolicy<T, P> {
    fn has_ability(&self, ability: &str) -> bool {
        self.0.has_ability(ability)
    }

    fn evaluate<'a>(
        &'a self,
        user: Option<&'a dyn Authenticatable>,
        ability: &'a str,
        args: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Decision>> {
        Box::pin(async move {
            if user.is_none() && !self.0.allows_guests(ability) {
                return Ok(None);
            }
            if let Some(result) = self.0.before(user, ability, args).await? {
                return Ok(Some(result));
            }
            let (resource, rest) = args.split_first().expect("typed policy has a target");
            self.0
                .evaluate(ability, user, resource.downcast_ref::<T>(), rest)
                .await
        })
    }
}

#[derive(Default)]
struct Definitions {
    abilities: HashMap<String, Arc<Ability>>,
    policies: HashMap<TypeId, Arc<dyn RegisteredPolicy>>,
    before: Vec<Arc<Before>>,
    after: Vec<Arc<After>>,
    default_denial: Option<Response>,
}

/// Application-owned ability, policy, and interception registrations.
#[derive(Default)]
pub struct GateRegistry(RwLock<Definitions>);

#[allow(non_snake_case, reason = "Matches Laravel's Gate registration API")]
impl GateRegistry {
    pub fn define(
        &self,
        ability: impl Into<String>,
        callback: impl for<'a> Fn(
            &'a dyn Authenticatable,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Response>>
        + Send
        + Sync
        + 'static,
    ) {
        let callback = Arc::new(callback);
        self.register(
            ability,
            Arc::new(move |user, args| {
                let callback = callback.clone();
                Box::pin(async move {
                    match user {
                        Some(user) => callback(user, args).await.map(Some),
                        None => Ok(None),
                    }
                })
            }),
        );
    }

    pub fn defineGuest(
        &self,
        ability: impl Into<String>,
        callback: impl for<'a> Fn(
            Option<&'a dyn Authenticatable>,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Response>>
        + Send
        + Sync
        + 'static,
    ) {
        let callback = Arc::new(callback);
        self.register(
            ability,
            Arc::new(move |user, args| {
                let callback = callback.clone();
                Box::pin(async move { callback(user, args).await.map(Some) })
            }),
        );
    }

    fn register(&self, ability: impl Into<String>, callback: Arc<Ability>) {
        self.0
            .write()
            .expect("gate registry lock")
            .abilities
            .insert(ability.into(), callback);
    }

    pub fn policy<T: Send + Sync + 'static>(&self, policy: impl Policy<T> + 'static) {
        self.0.write().expect("gate registry lock").policies.insert(
            TypeId::of::<T>(),
            Arc::new(TypedPolicy(policy, std::marker::PhantomData::<fn() -> T>)),
        );
    }

    pub fn before(
        &self,
        callback: impl for<'a> Fn(
            Option<&'a dyn Authenticatable>,
            &'a str,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Decision>>
        + Send
        + Sync
        + 'static,
    ) {
        self.0
            .write()
            .expect("gate registry lock")
            .before
            .push(Arc::new(callback));
    }

    pub fn after(
        &self,
        callback: impl for<'a> Fn(
            Option<&'a dyn Authenticatable>,
            &'a str,
            Option<&'a Response>,
            &'a [&'a Argument],
        ) -> AuthFuture<'a, AuthResult<Decision>>
        + Send
        + Sync
        + 'static,
    ) {
        self.0
            .write()
            .expect("gate registry lock")
            .after
            .push(Arc::new(callback));
    }

    pub fn defaultDenialResponse(&self, response: Response) {
        let mut definitions = self.0.write().expect("gate registry lock");
        definitions.default_denial = response.denied().then_some(response);
    }

    pub fn forUser(self: &Arc<Self>, user: Option<Arc<dyn Authenticatable>>) -> Gate {
        Gate {
            registry: self.clone(),
            user,
        }
    }
}

/// Evaluates abilities for one resolved identity.
pub struct Gate {
    registry: Arc<GateRegistry>,
    user: Option<Arc<dyn Authenticatable>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's Gate evaluation API")]
impl Gate {
    pub fn has(&self, ability: &str) -> bool {
        self.registry
            .0
            .read()
            .expect("gate registry lock")
            .abilities
            .contains_key(ability)
    }

    pub async fn inspect(&self, ability: &str, args: &[&Argument]) -> AuthResult<Response> {
        let (before, after, policy, defined, default_denial) = {
            let definitions = self.registry.0.read().expect("gate registry lock");
            let policy = args.first().and_then(|value| {
                let id = value
                    .downcast_ref::<PolicyTarget>()
                    .map_or_else(|| (*value).type_id(), |target| target.0);
                definitions.policies.get(&id).cloned()
            });
            (
                definitions.before.clone(),
                definitions.after.clone(),
                policy,
                definitions.abilities.get(ability).cloned(),
                definitions.default_denial.clone(),
            )
        };
        let user = self.user.as_deref();
        let mut result = None;
        for callback in before {
            result = callback(user, ability, args).await?;
            if result.is_some() {
                break;
            }
        }
        if result.is_none() {
            if let Some(policy) = policy.filter(|policy| policy.has_ability(ability)) {
                result = policy.evaluate(user, ability, args).await?;
            } else if let Some(defined) = defined {
                result = defined(user, args).await?;
            }
        }
        for callback in after {
            let evaluated = callback(user, ability, result.as_ref(), args).await?;
            if result.is_none() {
                result = evaluated;
            }
        }
        Ok(result
            .or(default_denial)
            .unwrap_or_else(|| Response::from(false)))
    }

    pub async fn allows(&self, ability: &str, args: &[&Argument]) -> AuthResult<bool> {
        self.inspect(ability, args)
            .await
            .map(|response| response.allowed())
    }

    pub async fn denies(&self, ability: &str, args: &[&Argument]) -> AuthResult<bool> {
        self.inspect(ability, args)
            .await
            .map(|response| response.denied())
    }

    pub async fn check(&self, abilities: &[&str], args: &[&Argument]) -> AuthResult<bool> {
        for ability in abilities {
            if self.denies(ability, args).await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub async fn any(&self, abilities: &[&str], args: &[&Argument]) -> AuthResult<bool> {
        for ability in abilities {
            if self.allows(ability, args).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn none(&self, abilities: &[&str], args: &[&Argument]) -> AuthResult<bool> {
        Ok(!self.any(abilities, args).await?)
    }

    pub async fn authorize(
        &self,
        ability: &str,
        args: &[&Argument],
    ) -> Result<Response, AuthorizationError> {
        decision(
            self.inspect(ability, args)
                .await
                .map_err(AuthorizationError::Evaluation)?,
        )
    }

    pub fn allowIf(
        &self,
        condition: impl FnOnce(&dyn Authenticatable) -> bool,
    ) -> Result<Response, AuthorizationError> {
        decision(self.user.as_deref().is_some_and(condition).into())
    }

    pub fn denyIf(
        &self,
        condition: impl FnOnce(&dyn Authenticatable) -> bool,
    ) -> Result<Response, AuthorizationError> {
        self.allowIf(|user| !condition(user))
    }
}

fn decision(response: Response) -> Result<Response, AuthorizationError> {
    if response.allowed() {
        Ok(response)
    } else {
        Err(AuthorizationError::Denied(response))
    }
}

impl GateContract for Gate {
    fn has(&self, ability: &str) -> bool {
        Gate::has(self, ability)
    }
    fn inspect<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<Response>> {
        Box::pin(Gate::inspect(self, ability, arguments))
    }
    fn allows<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, AuthResult<bool>> {
        Box::pin(Gate::allows(self, ability, arguments))
    }
    fn authorize<'a>(
        &'a self,
        ability: &'a str,
        arguments: &'a [&'a Argument],
    ) -> AuthFuture<'a, Result<Response, AuthorizationError>> {
        Box::pin(Gate::authorize(self, ability, arguments))
    }
}
