//! 本人头像上传：真实解码、持久化预留与用户事务，旧文件交给原生Worker清理。
use super::{
    UserResource, account, client,
    confirmation::{self, Purpose},
    credentials,
    resource_cleanup::{
        CleanupKind, CleanupTicket, ResourceCleanup, StoredResource, UploadReservation,
    },
    updates,
};
use crate::{
    Fortify, TwoFactorAuthenticatable,
    resolver::{Resolver, internal},
};
use openrust::{
    auth::{AuthError, AuthResult},
    contracts::{
        auth::{Authenticatable, Guard as _},
        config::Repository,
    },
    database::{DatabaseManager, Value},
    filesystem::{FilesystemManager, Visibility, WriteOptions},
    foundation::Application,
    http::{ApiResponse, HttpError, HttpFuture, Request, Response, StatusCode, UploadedFile, Uri},
    image::{Error as ImageError, Limits},
    queue::QueueManager,
    routing::{RouteGroup, RoutingError},
    sanctum::{HasApiTokens, Sanctum},
    support::{
        ServiceProvider, Str,
        facades::{
            App, Auth, Config, Facade as _, Image, Lang, Log, Redirect, Route, Storage, Url,
            Validator,
        },
    },
    validation::{Data, RuleSet},
};
use serde_json::json;
use std::{sync::Arc, time::Duration};
const PATH: &str = "profile_photo_path";
const DISK: &str = "profile_photo_disk";
pub(super) fn enabled(config: &dyn Repository) -> Result<bool, openrust::config::ConfigError> {
    super::configuration::boolean(config, "user.avatar.enabled", true)
}
fn limits(config: &dyn Repository) -> Result<(usize, u32), HttpError> {
    let integer = |key: &str, default: u64| match config.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or_else(|| internal(std::io::Error::other("头像限额必须为正整数"))),
    };
    let size = usize::try_from(integer("user.avatar.max_kilobytes", 2048)?).map_err(internal)?;
    size.checked_mul(1024)
        .ok_or_else(|| internal(std::io::Error::other("头像字节限额溢出")))?;
    let dimension = u32::try_from(integer("user.avatar.max_dimension", 4096)?).map_err(internal)?;
    Ok((size, dimension))
}
pub(super) fn view_limits(config: &dyn Repository) -> Result<(usize, u32), HttpError> {
    limits(config)
}
fn text(user: &dyn Authenticatable, key: &str) -> AuthResult<Option<String>> {
    match user.get_auth_attribute(key)? {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Text(v)) if !v.is_empty() => Ok(Some(v)),
        _ => Err(AuthError::Configuration(
            "头像路径或磁盘属性必须为非空字符串或null",
        )),
    }
}
pub(super) fn stored(user: &dyn Authenticatable) -> AuthResult<Option<StoredResource>> {
    let Some(path) = text(user, PATH)? else {
        return Ok(None);
    };
    let disk = text(user, DISK)?.ok_or(AuthError::Configuration(
        "已有头像缺少存储磁盘，请先迁移历史数据",
    ))?;
    let resource = StoredResource { disk, path };
    resource.validate()?;
    Ok(Some(resource))
}
pub(super) fn url(user: &dyn Authenticatable) -> Result<Option<String>, HttpError> {
    if App::get_facade_application()?.is_some()
        && !Config::boolean_with_default("user.avatar.enabled", true)?
    {
        return Ok(None);
    }
    let Some(file) = stored(user)? else {
        return Ok(None);
    };
    Ok(Some(
        Storage::disk(Some(&file.disk))
            .map_err(internal)?
            .url(&file.path)
            .map_err(internal)?,
    ))
}
async fn upload(request: &mut Request, config: &dyn Repository) -> Result<UploadedFile, HttpError> {
    let (size, dimension) = limits(config)?;
    let declaration = format!("required|file|mimes:jpg,jpeg,png,webp|max:{size}");
    let rules = RuleSet::parse([("photo", declaration.as_str())])?;
    let mut validator = Validator::make(Data::from_request(request).await?, rules)?;
    validator.set_attribute_names(
        [(
            "photo".into(),
            Lang::get("user.profile_photo", &Default::default(), None).map_err(internal)?,
        )]
        .into(),
    );
    validator.validate_request(request).await?;
    let file = request
        .file("photo")
        .await?
        .ok_or_else(|| client::failed("photo", "user.photo_invalid"))?;
    if !matches!(file.extension(), Some("jpg" | "jpeg" | "png" | "webp")) {
        return Err(client::failed("photo", "user.photo_invalid"));
    }
    Image::from_upload(&file)
        .map_err(internal)?
        .with_limits(Limits {
            max_input_bytes: size * 1024,
            max_dimension: dimension,
            max_pixels: u64::from(dimension) * u64::from(dimension),
        })
        .dimensions()
        .await
        .map_err(|error| {
            client::failed(
                "photo",
                match error {
                    ImageError::DimensionsTooLarge => "user.photo_dimensions_invalid",
                    ImageError::InputTooLarge => "user.photo_size_invalid",
                    _ => "user.photo_invalid",
                },
            )
        })?;
    Ok(file)
}
pub(super) fn register<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    app: &Application,
) -> openrust::support::ProviderResult {
    super::StarterKitServiceProvider.load_routes_from(app, routes::<M>)?;
    Ok(())
}
fn same_origin(config: &dyn Repository, url: &str) -> bool {
    if url.starts_with('/') && !url.starts_with("//") {
        return true;
    }
    let Ok(uri) = url.parse::<Uri>() else {
        return false;
    };
    let Some(base) = config
        .get("app.url")
        .and_then(|v| v.as_str().map(str::to_owned))
        .and_then(|v| v.parse::<Uri>().ok())
    else {
        return false;
    };
    let port = match uri.scheme_str() {
        Some("http") => 80,
        Some("https") => 443,
        _ => return false,
    };
    uri.scheme() == base.scheme()
        && uri
            .host()
            .zip(base.host())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        && uri.port_u16().unwrap_or(port) == base.port_u16().unwrap_or(port)
}
fn routes<M: HasApiTokens + TwoFactorAuthenticatable + Clone>() -> Result<(), RoutingError> {
    let fortify = Resolver::<Fortify>::get_facade_root()
        .map_err(|source| RoutingError::ContainerFailed { source })?;
    if !enabled(fortify.config.as_ref()).map_err(|_| RoutingError::InvalidAction)? {
        return Ok(());
    }
    let disk = fortify.string("user.avatar.disk", "public");
    let settings = fortify
        .config
        .get("filesystems.disks")
        .and_then(|v| v.get(&disk).cloned());
    if let Some(value) = settings
        && value.get("driver").and_then(serde_json::Value::as_str) == Some("local")
        && value.get("visibility").and_then(serde_json::Value::as_str) == Some("public")
        && value
            .get("url")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|url| same_origin(fortify.config.as_ref(), url))
    {
        Route::static_disk(&disk)?;
    }
    if !fortify.truthy("fortify.views") {
        return Ok(());
    }
    let group = RouteGroup::new()
        .middleware_name("web")
        .middleware_name("auth.session")
        .middleware_name(format!(
            "auth:{}",
            fortify.guard().as_deref().unwrap_or("web")
        ));
    Route::group(group, |router| {
        router
            .post("/user/profile-photo", web_store::<M>)?
            .middleware_name("throttle:10,1")
            .name("user.photo.store");
        router
            .delete("/user/profile-photo", web_destroy::<M>)?
            .middleware_name("throttle:10,1")
            .name("user.photo.destroy");
        Ok(())
    })
}
pub(super) fn store<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    change::<M>(request, false, false)
}
pub(super) fn destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    change::<M>(request, false, true)
}
fn web_store<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    change::<M>(request, true, false)
}
fn web_destroy<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    change::<M>(request, true, true)
}
async fn compensate(cleanup: &ResourceCleanup, reservation: &UploadReservation) {
    if let Err(error) = cleanup.cancel_upload(reservation).await {
        let _ = Log::warning(
            "头像临时文件等待后台回收",
            &[
                ("operation".into(), json!(reservation.id())),
                ("reason".into(), json!(error.to_string())),
            ]
            .into_iter()
            .collect(),
        );
    }
}
fn change<M: HasApiTokens + TwoFactorAuthenticatable + Clone>(
    request: &mut Request,
    web: bool,
    remove: bool,
) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let fortify = Resolver::<Fortify>::get_facade_root().map_err(internal)?;
        if !enabled(fortify.config.as_ref())? {
            return Err(HttpError::NotFound);
        }
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
        let guard = updates::cookie_guard(request, &user, actor.is_none(), &fortify).await?;
        let file = if remove {
            None
        } else {
            Some(upload(request, fortify.config.as_ref()).await?)
        };
        let proof = if web {
            confirmation::claim_session(request, &user, &fortify).await?
        } else {
            confirmation::claim(request, &user, Purpose::Avatar, &fortify).await?
        };
        let cleanup = App::make::<ResourceCleanup>().await?;
        let queue = App::make::<QueueManager>().await?;
        let sanctum = App::make::<Sanctum>().await?;
        let database = App::make::<DatabaseManager>().await?;
        let mut reservation = None;
        if let Some(file) = file {
            let disk_name = fortify.string("user.avatar.disk", "public");
            let settings = fortify
                .config
                .get("filesystems.disks")
                .and_then(|v| v.get(&disk_name).cloned());
            if settings
                .as_ref()
                .and_then(|v| v.get("visibility"))
                .and_then(serde_json::Value::as_str)
                != Some("public")
            {
                return Err(internal(std::io::Error::other(
                    "默认头像处理器需要明确配置的公开磁盘；私有头像请替换应用处理器与资源",
                )));
            }
            let disk = App::make::<FilesystemManager>()
                .await?
                .disk(Some(&disk_name))
                .map_err(internal)?;
            let extension = match file.extension() {
                Some("jpg" | "jpeg") => "jpg",
                Some("png") => "png",
                Some("webp") => "webp",
                _ => return Err(client::failed("photo", "user.photo_invalid")),
            };
            let resource = StoredResource {
                disk: disk_name,
                path: format!("profile-photos/{}.{extension}", Str::random(64)),
            };
            let reserve_queue = queue.clone();
            let preparation = cleanup.clone();
            let policy = fortify.clone();
            let authorization = proof.clone();
            let checked_actor = actor.clone();
            let checked_sanctum = sanctum.clone();
            let checked_db = database.clone();
            let selected = resource.clone();
            let prepared = credentials::with_locked(&mut user, move |locked, tx| {
                Box::pin(async move {
                    authorization
                        .validate_in_transaction(locked, &policy, tx)
                        .await?;
                    account::check_actor(
                        locked,
                        tx,
                        &checked_sanctum,
                        &checked_db,
                        checked_actor,
                        Some("account:update"),
                        &[],
                    )
                    .await?;
                    preparation
                        .reserve_upload(locked, tx, &reserve_queue, selected)
                        .await
                })
            })
            .await?;
            let result = tokio::time::timeout(
                Duration::from_secs(120),
                disk.put_with_options(
                    &resource.path,
                    file.get(),
                    WriteOptions {
                        visibility: Some(Visibility::Public),
                    },
                ),
            )
            .await;
            let written = match result {
                Ok(Ok(true)) => Ok(()),
                Ok(Ok(false)) => Err(internal(std::io::Error::other("头像文件未写入"))),
                Ok(Err(error)) => Err(internal(error)),
                Err(error) => Err(internal(error)),
            };
            if let Err(error) = written {
                compensate(&cleanup, &prepared).await;
                return Err(error);
            }
            reservation = Some(Arc::new(prepared));
        }
        let retained = reservation.clone();
        let selected = reservation.as_ref().map(|r| r.resource().clone());
        let policy = fortify.clone();
        let checked = cleanup.clone();
        let authorization = proof.clone();
        let actor = actor.clone();
        let result = credentials::with_locked(&mut user, move |locked, tx| {
            Box::pin(async move {
                authorization
                    .validate_in_transaction(locked, &policy, tx)
                    .await?;
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
                let old = stored(locked)?;
                if let Some(upload) = retained.as_ref() {
                    checked.retain_upload(locked, tx, upload).await?;
                }
                locked.set_attribute(
                    PATH,
                    selected
                        .as_ref()
                        .map(|r| Value::Text(r.path.clone()))
                        .unwrap_or(Value::Null),
                );
                locked.set_attribute(
                    DISK,
                    selected
                        .as_ref()
                        .map(|r| Value::Text(r.disk.clone()))
                        .unwrap_or(Value::Null),
                );
                if !locked.save_in_transaction(tx).await? {
                    return Err(AuthError::Configuration("头像模型拒绝保存"));
                }
                locked.refresh_for_update(tx).await?;
                let actual = stored(locked)?;
                if actual.as_ref().map(|r| (&r.disk, &r.path))
                    != selected.as_ref().map(|r| (&r.disk, &r.path))
                {
                    return Err(AuthError::Configuration(
                        "头像模型事件修改了受保护的存储引用",
                    ));
                }
                let ticket = if let Some(old) = old {
                    Some(
                        checked
                            .enqueue(locked, tx, &queue, CleanupKind::Avatar, vec![old])
                            .await?,
                    )
                } else {
                    None
                };
                authorization
                    .validate_in_transaction(locked, &policy, tx)
                    .await?;
                Ok(ticket)
            })
        })
        .await;
        let ticket = match result {
            Ok(ticket) => ticket,
            Err(error) => {
                if let Some(upload) = reservation.as_ref() {
                    compensate(&cleanup, upload).await;
                }
                return Err(error.into());
            }
        };
        // 头像不改变凭据：只刷新本请求缓存，不轮换会话或清除近期确认。
        if let Some(name) = guard.as_deref() {
            Auth::session_guard(request, Some(name))?.set_user(Box::new(user.clone()));
            Auth::guard(request, Some("sanctum"))?.set_user(Box::new(user.clone()));
        }
        if web && !request.expects_json() {
            return Ok(client::no_store(
                Redirect::route("user.profile", &[], Default::default())?
                    .with(
                        request,
                        "status",
                        if remove {
                            "user.photo_removed"
                        } else {
                            "user.photo_updated"
                        }
                        .into(),
                    )?
                    .into(),
            ));
        }
        let resource = UserResource::for_request(&user, request).await?;
        let status = if ticket.is_some() {
            StatusCode::ACCEPTED
        } else {
            StatusCode::OK
        };
        let mut response = ApiResponse::success(resource)
            .with_status(status)
            .with_message_key(
                if remove {
                    "user.photo_removed"
                } else {
                    "user.photo_updated"
                },
                Default::default(),
            );
        if let Some(ticket) = ticket {
            response = response.with_field("meta.cleanup", ticket_data(ticket)?);
        }
        Ok(client::no_store(response.into_response()?).header(
            "referrer-policy".parse().expect("静态头名称"),
            "no-referrer".parse().expect("静态头值"),
        ))
    })
}
pub(super) fn ticket_data(ticket: CleanupTicket) -> Result<serde_json::Value, HttpError> {
    let url = Url::route(
        "user.resource-cleanups.show",
        &[openrust::routing::RouteParameter::named(
            "operation",
            ticket.id.clone(),
        )],
        true,
    )
    .map_err(internal)?;
    Ok(
        json!({"id":ticket.id,"receipt":ticket.receipt.as_str(),"expires_at":ticket.expires_at,"status_url":url,"status":"pending"}),
    )
}
