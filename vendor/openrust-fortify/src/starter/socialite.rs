//! 显式启用的第三方账户接线；原生Socialite负责state、PKCE和供应商协议。
#[cfg(feature = "socialite")]
mod identity;
use crate::Fortify;
use openrust::{
    auth::AuthResult,
    database::{Transaction, eloquent::Model},
};
use serde::{Deserialize, Serialize};

pub(super) fn configured(
    config: &dyn openrust::contracts::config::Repository,
) -> AuthResult<Vec<String>> {
    let value = config
        .get("user.socialite.providers")
        .unwrap_or(serde_json::json!([]));
    let names = value
        .as_array()
        .ok_or(openrust::auth::AuthError::Configuration(
            "user.socialite.providers必须是提供方名称数组",
        ))?;
    let mut result = Vec::new();
    for name in names {
        let name = name
            .as_str()
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 64
                    && name.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'
                    })
            })
            .ok_or(openrust::auth::AuthError::Configuration(
                "第三方登录提供方名称无效",
            ))?;
        if result.iter().any(|existing| existing == name) {
            return Err(openrust::auth::AuthError::Configuration(
                "第三方登录提供方重复",
            ));
        }
        result.push(name.to_owned());
    }
    Ok(result)
}

#[cfg(feature = "socialite")]
pub(super) fn providers(fortify: &Fortify) -> AuthResult<Vec<String>> {
    configured(fortify.config.as_ref())
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Proof {
    id: String,
    generation: String,
    provider: String,
}
impl Proof {
    #[cfg(feature = "socialite")]
    fn from_identity(identity: &identity::Identity) -> AuthResult<Self> {
        Ok(Self {
            id: identity.id()?,
            generation: identity.generation.clone(),
            provider: identity.provider.clone(),
        })
    }
    pub async fn current<M: Model + crate::TwoFactorAuthenticatable>(
        &self,
        user: &M,
        fortify: &Fortify,
        transaction: &mut Transaction,
    ) -> AuthResult<bool> {
        #[cfg(feature = "socialite")]
        {
            if !providers(fortify)?.contains(&self.provider) {
                return Ok(false);
            }
            identity::current(
                user,
                transaction,
                &self.id,
                &self.generation,
                &self.provider,
            )
            .await
        }
        #[cfg(not(feature = "socialite"))]
        {
            let _ = (user, fortify, transaction);
            Ok(false)
        }
    }
}

#[cfg(feature = "socialite")]
mod api;
#[cfg(feature = "socialite")]
mod flow;
#[cfg(feature = "socialite")]
mod registration;
#[cfg(feature = "socialite")]
mod web;
#[cfg(feature = "socialite")]
pub use registration::{CreatesUsersFromProvider, ProviderProfile};
#[derive(Clone)]
pub(super) struct Deadline(pub i64);

#[cfg(feature = "socialite")]
pub(super) fn register<M>(
    app: &openrust::foundation::Application,
) -> openrust::support::ProviderResult
where
    M: openrust::sanctum::HasApiTokens + crate::TwoFactorAuthenticatable + Clone,
{
    use openrust::support::ServiceProvider as _;
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)
}
#[cfg(feature = "socialite")]
fn routes<M>() -> Result<(), openrust::routing::RoutingError>
where
    M: openrust::sanctum::HasApiTokens + crate::TwoFactorAuthenticatable + Clone,
{
    use openrust::{
        routing::{RouteGroup, RoutingError},
        support::facades::{Config, Facade as _, Route},
    };
    use std::sync::Arc;
    let fortify = crate::resolver::Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if providers(&fortify)
        .map_err(|_| RoutingError::InvalidAction)?
        .is_empty()
    {
        return Ok(());
    }
    let mut browser = RouteGroup::new()
        .prefix(fortify.string("fortify.prefix", ""))
        .middleware_name("web")
        .middleware_name("sensitive")
        .middleware_name("cache.headers:no_store;private")
        .middleware_name("throttle:10,1,user-socialite-web");
    if let Some(domain) = fortify
        .config
        .get("fortify.domain")
        .and_then(|v| v.as_str().map(str::to_owned))
    {
        browser = browser.domain(domain);
    }
    Route::group(browser, |router| {
        if fortify.truthy("fortify.views") {
            router
                .get("/auth/{provider}/redirect", web::login::<M>)?
                .name("user.providers.redirect")
                .middleware_name(format!(
                    "guest:{}",
                    fortify.guard().as_deref().unwrap_or("web")
                ));
        }
        router
            .get("/auth/{provider}/callback", web::callback::<M>)?
            .name("user.providers.callback");
        router
            .get("/auth/{provider}/handoff", api::handoff::<M>)?
            .name("user.providers.handoff");
        Ok(())
    })?;
    if fortify.truthy("fortify.views") {
        Route::group(
            RouteGroup::new()
                .middleware_name("web")
                .middleware_name("sensitive")
                .middleware_name("auth.session")
                .middleware_name(format!(
                    "auth:{}",
                    fortify.guard().as_deref().unwrap_or("web")
                )),
            |router| {
                router
                    .get("/user/connected-accounts", web::index::<M>)?
                    .name("user.providers.page");
                router
                    .post("/user/connected-accounts/confirm", web::confirm::<M>)?
                    .name("user.providers.confirm");
                router
                    .post("/user/connected-accounts/{provider}", web::start_link::<M>)?
                    .name("user.providers.link");
                router
                    .delete("/user/connected-accounts/{provider}", web::destroy::<M>)?
                    .name("user.providers.destroy");
                Ok(())
            },
        )?;
    }
    let prefix = Config::string_with_default("api.prefix", "api")
        .map_err(|_| RoutingError::InvalidAction)?;
    Route::group(
        RouteGroup::new()
            .prefix(prefix)
            .middleware_name("api")
            .middleware_name("sensitive")
            .middleware(Arc::new(super::client_middleware::NoStore))
            .middleware(Arc::new(super::client_middleware::BodyCredentials))
            .middleware_name("throttle:10,1,user-socialite-api"),
        |router| {
            router
                .post("/auth/providers/exchange", api::exchange::<M>)?
                .name("user.providers.exchange");
            router
                .post("/auth/providers/{provider}", api::begin::<M>)?
                .name("user.providers.begin");
            router
                .get("/user/connected-accounts", api::index::<M>)?
                .middleware_name("auth:sanctum")
                .middleware(Arc::new(super::client_middleware::UserModel::<M>::new()))
                .name("user.providers.index");
            for (path, handler, name) in [
                (
                    "/user/connected-accounts/exchange",
                    api::preview::<M>
                        as for<'a> fn(
                            &'a mut openrust::http::Request,
                        ) -> openrust::http::HttpFuture<
                            'a,
                            Result<openrust::http::Response, openrust::http::HttpError>,
                        >,
                    "user.providers.preview",
                ),
                (
                    "/user/connected-accounts/confirm",
                    api::confirm::<M>,
                    "user.providers.api-confirm",
                ),
                (
                    "/user/connected-accounts/{provider}",
                    api::begin_link::<M>
                        as for<'a> fn(
                            &'a mut openrust::http::Request,
                        ) -> openrust::http::HttpFuture<
                            'a,
                            Result<openrust::http::Response, openrust::http::HttpError>,
                        >,
                    "user.providers.begin-link",
                ),
            ] {
                router
                    .post(path, handler)?
                    .middleware_name("auth:sanctum")
                    .middleware(Arc::new(super::client_middleware::UserModel::<M>::new()))
                    .name(name);
            }
            router
                .delete("/user/connected-accounts/{provider}", api::destroy::<M>)?
                .middleware_name("auth:sanctum")
                .middleware(Arc::new(super::client_middleware::UserModel::<M>::new()))
                .name("user.providers.api-destroy");
            Ok(())
        },
    )
}

#[cfg(feature = "socialite")]
pub(super) async fn ensure_other_primary<
    M: openrust::sanctum::HasApiTokens + crate::TwoFactorAuthenticatable,
>(
    user: &M,
    transaction: &mut Transaction,
    removing: &str,
    fortify: &Fortify,
) -> AuthResult<()> {
    if user
        .get_auth_password()
        .is_some_and(|password| !password.is_empty())
    {
        return Ok(());
    }
    let others = transaction
        .table(identity::TABLE)
        .r#where(
            "owner_hash",
            "=",
            super::challenges::owner_key(user.get_morph_class()?, &identity::user_key(user)?)?,
        )
        .r#where("provider", "!=", removing)
        .get()
        .await?;
    let enabled = providers(fortify)?;
    if others.iter().any(|row|matches!(row.get("provider"),Some(openrust::database::Value::Text(p)) if enabled.contains(p))){return Ok(());}
    if crate::Features::new(fortify.config.clone()).enabled(crate::Features::passkeys())? {
        let service = openrust::support::facades::App::make::<crate::passkeys::Passkeys>()
            .await
            .map_err(|e| openrust::auth::AuthError::Backend(Box::new(e)))?;
        if !service
            .for_user_in_transaction(user, transaction)
            .await?
            .is_empty()
        {
            return Ok(());
        }
    }
    Err(super::client::failed("provider", "user.last_primary_required").into())
}

/// 仅在应用没有替换创建动作时装配生成的默认实现。
#[cfg(feature = "socialite")]
pub fn create_users_using(
    app: &openrust::foundation::Application,
    action: std::sync::Arc<dyn CreatesUsersFromProvider>,
) -> openrust::support::ProviderResult {
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    if !app
        .container()
        .has(openrust::container::ServiceKey::typed::<
            dyn CreatesUsersFromProvider,
        >())
    {
        app.container().instance(action)?;
    }
    Ok(())
}
#[cfg(feature = "socialite")]
pub(super) async fn purge<M: openrust::sanctum::HasApiTokens>(
    user: &M,
    transaction: &mut Transaction,
) -> AuthResult<()> {
    if transaction
        .get_schema_builder()
        .has_table(identity::TABLE)
        .await?
    {
        identity::purge(user, transaction).await?;
    }
    Ok(())
}

#[cfg(feature = "socialite")]
pub(super) async fn has_enabled_identity<M: openrust::sanctum::HasApiTokens>(
    user: &M,
    transaction: &mut Transaction,
    fortify: &Fortify,
) -> AuthResult<bool> {
    let enabled = providers(fortify)?;
    if enabled.is_empty() {
        return Ok(false);
    }
    let rows = transaction
        .table(identity::TABLE)
        .r#where(
            "owner_hash",
            "=",
            super::challenges::owner_key(user.get_morph_class()?, &identity::user_key(user)?)?,
        )
        .get()
        .await?;
    for row in rows {
        let record = identity::Identity::from_row(&row)?;
        if record.belongs_to(user)? && enabled.contains(&record.provider) {
            return Ok(true);
        }
    }
    Ok(false)
}
