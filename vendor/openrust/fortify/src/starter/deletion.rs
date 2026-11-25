//! 本人注销：应用Action、同库凭据撤销与持久化外部资源清理。
use super::{
    account, avatar, client,
    confirmation::{self, Purpose},
    credentials, devices,
    resource_cleanup::{CleanupKind, ResourceCleanup},
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{AuthError, AuthFuture, AuthResult},
    contracts::{
        auth::{Authenticatable, Guard as _},
        container::Container as _,
        foundation::Application as _,
        session::Session as _,
    },
    database::{DatabaseError, DatabaseManager, Transaction, eloquent::Model},
    foundation::Application,
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, ResponseOutput, StatusCode},
    queue::QueueManager,
    routing::{RouteGroup, RoutingError},
    sanctum::{HasApiTokens, Sanctum},
    support::{
        ProviderResult, ServiceProvider,
        facades::{App, Auth, Facade as _, Redirect, Route, View},
    },
};
use std::sync::Arc;

/*
 * 应用只定义业务依赖和用户删除策略；凭据撤销与资源任务由套件统一装配。
 * 所有数据库操作使用传入事务，失败须返回错误，外部I/O留给提交后的清理任务。
 */
pub trait DeletesUsers<M: Model>: Send + Sync {
    fn preflight<'a>(
        &'a self,
        _user: &'a M,
        _transaction: &'a mut Transaction,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async { Ok(()) })
    }
    fn delete<'a>(
        &'a self,
        user: &'a mut M,
        transaction: &'a mut Transaction,
    ) -> AuthFuture<'a, AuthResult<()>>;
}

pub struct DeleteUser;
impl<M: Model> DeletesUsers<M> for DeleteUser {
    fn delete<'a>(
        &'a self,
        user: &'a mut M,
        transaction: &'a mut Transaction,
    ) -> AuthFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            if !user.delete_in_transaction(transaction).await? {
                return Err(DatabaseError::InvalidQuery("用户模型取消注销").into());
            }
            Ok(())
        })
    }
}
struct Selected<M: Model>(Arc<dyn DeletesUsers<M>>);
pub fn delete_users_using<M: Model>(
    app: &Application,
    action: Arc<dyn DeletesUsers<M>>,
) -> ProviderResult {
    app.container().instance(Arc::new(Selected::<M>(action)))?;
    Ok(())
}
async fn action<M: Model>() -> Result<Arc<dyn DeletesUsers<M>>, HttpError> {
    let app = App::get_facade_application()?.ok_or(HttpError::Unauthenticated)?;
    if app
        .container()
        .has(openrust::container::ServiceKey::typed::<Selected<M>>())
    {
        return Ok(app.make::<Selected<M>>().await?.0.clone());
    }
    Ok(Arc::new(DeleteUser))
}
pub(super) fn register<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    app: &Application,
) -> ProviderResult {
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)?;
    Ok(())
}
fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>() -> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if !fortify.truthy("fortify.views") {
        return Ok(());
    }
    Route::group(
        RouteGroup::new()
            .middleware_name("web")
            .middleware_name("auth.session")
            .middleware_name(format!(
                "auth:{}",
                fortify.guard().as_deref().unwrap_or("web")
            )),
        |router| {
            router
                .delete("/user", web_destroy::<M>)?
                .middleware_name("throttle:6,1")
                .name("current-user.destroy");
            Ok(())
        },
    )
}
pub(super) fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    remove::<M>(request, false)
}
fn web_destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    remove::<M>(request, true)
}

async fn matching_guards<M: Model + Authenticatable>(
    request: &mut Request,
    user: &M,
    fortify: &Fortify,
) -> Result<Vec<String>, HttpError> {
    let guards = fortify
        .config
        .get("auth.guards")
        .ok_or_else(|| internal(std::io::Error::other("缺少会话Guard配置")))?;
    let guards = guards
        .as_object()
        .ok_or_else(|| internal(std::io::Error::other("会话Guard配置必须为对象")))?;
    let mut selected = Vec::new();
    for (name, settings) in guards {
        if settings.get("driver").and_then(serde_json::Value::as_str) != Some("session") {
            continue;
        }
        let mut guard = Auth::session_guard(request, Some(name))?;
        let Some(current) = guard.user().await? else {
            continue;
        };
        if let Some(model) = (current as &dyn std::any::Any).downcast_ref::<M>()
            && model.get_key() == user.get_key()
            && model.get_table() == user.get_table()
            && model.get_connection()?.get_name() == user.get_connection()?.get_name()
        {
            selected.push(name.clone());
        }
    }
    Ok(selected)
}
fn remove<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    web: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        let mut user = if web {
            super::web::user::<M>(request, &fortify).await?
        } else {
            account::user::<M>(request).await?
        };
        let actor = if web {
            None
        } else {
            account::actor(request, &user, Some("account:update"))?
        };
        let proof = if web {
            confirmation::claim_session(request, &user, &fortify)?
        } else {
            confirmation::claim(request, &user, Purpose::Delete, &fortify).await?
        };
        let guards = if actor.is_none() {
            matching_guards(request, &user, &fortify).await?
        } else {
            Vec::new()
        };
        let selected = action::<M>().await?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let tokens = sanctum.personal_access_tokens(&database, "")?;
        if !tokens.uses_connection(&user.get_connection()?) {
            return Err(internal(AuthError::Configuration(
                "注销令牌必须使用用户连接",
            )));
        }
        let cleanup = App::make::<ResourceCleanup>().await?;
        let queue = App::make::<QueueManager>().await?;
        let app = App::get_facade_application()?.ok_or(HttpError::Unauthenticated)?;
        let passkeys = if app
            .container()
            .has(openrust::container::ServiceKey::typed::<
                openrust_passkeys::Passkeys,
            >()) {
            Some(app.make::<openrust_passkeys::Passkeys>().await?)
        } else {
            None
        };
        let policy = fortify.clone();
        let ticket = credentials::with_locked(&mut user, move |locked, tx| {
            Box::pin(async move {
                proof.validate_in_transaction(locked, &policy, tx).await?;
                account::check_actor(
                    locked,
                    tx,
                    &sanctum,
                    &database,
                    actor,
                    Some("account:update"),
                    &[],
                )
                .await?;
                selected.preflight(locked, tx).await?;
                let resources = avatar::stored(locked)?.into_iter().collect::<Vec<_>>();
                let before = locked.clone();
                credentials::rotate_locked(locked, tx, None).await?;
                tokens
                    .purge_owned_in_transaction(
                        tx,
                        locked.get_morph_class()?,
                        openrust::auth::AuthIdentifier::try_from(
                            locked.get_key().ok_or(AuthError::InvalidIdentity)?,
                        )?,
                    )
                    .await?;
                if let Some(passkeys) = passkeys {
                    passkeys.purge_user_in_transaction(tx, locked).await?;
                }
                if let Some(devices) = devices::policy()? {
                    devices.revoke_others(locked, tx, None).await?;
                }
                let ticket = if resources.is_empty() {
                    None
                } else {
                    Some(
                        cleanup
                            .enqueue(locked, tx, &queue, CleanupKind::AccountDeletion, resources)
                            .await?,
                    )
                };
                let table = locked.get_table().to_owned();
                let key_name = locked.get_key_name().to_owned();
                let key = locked
                    .get_key()
                    .cloned()
                    .ok_or(AuthError::InvalidIdentity)?;
                tx.table(super::challenges::TABLE)
                    .r#where(
                        "owner_hash",
                        "=",
                        super::challenges::owner_key(
                            locked.get_morph_class()?,
                            &openrust::auth::AuthIdentifier::try_from(
                                locked.get_key().ok_or(AuthError::InvalidIdentity)?,
                            )?,
                        )?,
                    )
                    .delete()
                    .await?;
                selected.delete(locked, tx).await?;
                if let Some(row) = tx
                    .table(&table)
                    .r#where(&key_name, "=", key)
                    .first()
                    .await?
                    && locked.get_deleted_at_column().is_none_or(|column| {
                        row.get(column)
                            .is_none_or(|value| *value == openrust::database::Value::Null)
                    })
                {
                    return Err(
                        DatabaseError::InvalidQuery("注销Action未删除用户，须回滚事务").into(),
                    );
                }
                proof.validate(&before, &policy)?;
                Ok(ticket)
            })
        })
        .await?;
        if !guards.is_empty() {
            for name in guards {
                Auth::session_guard(request, Some(&name))?
                    .logout_current_device()
                    .await?;
            }
            super::web::forget(request)?;
            let session = request.session_mut().map_err(internal)?;
            session.migrate(true).await?;
            session.regenerate_token().map_err(internal)?;
        }
        Auth::forget_guards(request)?;
        if let Some(ticket) = ticket {
            let data = avatar::ticket_data(ticket)?;
            if web && !request.expects_json() {
                let view = View::make("user.deleted", [("cleanup".into(), data.into())].into())?;
                return Response::view(StatusCode::ACCEPTED, view, Default::default(), request)
                    .await
                    .map(|response| {
                        client::no_store(response).header(
                            "referrer-policy".parse().expect("静态头名称"),
                            "no-referrer".parse().expect("静态头值"),
                        )
                    });
            }
            return Ok(client::no_store(
                ApiResponse::empty()
                    .with_status(StatusCode::ACCEPTED)
                    .with_field("meta.cleanup", data)
                    .into_response()?,
            ));
        }
        if web && !request.expects_json() {
            return Ok(client::no_store(
                Redirect::route("login", &[], Default::default())?.into_response()?,
            ));
        }
        Ok(client::no_store(Response::empty(StatusCode::NO_CONTENT)))
    })
}
