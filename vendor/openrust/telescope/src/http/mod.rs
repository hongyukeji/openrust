use crate::{EntryType, EntryUpdate, Error, Telescope, facades, storage::EntryQueryOptions};
use openrust::{
    http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
    routing::{RouteGroup, RoutingError},
    support::facades::{Config, Facade, Route},
};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;
pub mod middleware;
pub type Authorize =
    for<'a> fn(&'a mut Request) -> HttpFuture<'a, std::result::Result<bool, HttpError>>;
const TYPES: &[(&str, EntryType, &str)] = &[
    ("batches", EntryType::Batch, "BatchWatcher"),
    ("cache", EntryType::Cache, "CacheWatcher"),
    (
        "client-requests",
        EntryType::ClientRequest,
        "ClientRequestWatcher",
    ),
    ("commands", EntryType::Command, "CommandWatcher"),
    ("dumps", EntryType::Dump, "DumpWatcher"),
    ("events", EntryType::Event, "EventWatcher"),
    ("exceptions", EntryType::Exception, "ExceptionWatcher"),
    ("gates", EntryType::Gate, "GateWatcher"),
    ("jobs", EntryType::Job, "JobWatcher"),
    ("logs", EntryType::Log, "LogWatcher"),
    ("mail", EntryType::Mail, "MailWatcher"),
    ("models", EntryType::Model, "ModelWatcher"),
    (
        "notifications",
        EntryType::Notification,
        "NotificationWatcher",
    ),
    ("queries", EntryType::Query, "QueryWatcher"),
    ("redis", EntryType::Redis, "RedisWatcher"),
    ("requests", EntryType::Request, "RequestWatcher"),
    ("schedule", EntryType::ScheduledTask, "ScheduleWatcher"),
    ("views", EntryType::View, "ViewWatcher"),
];
pub(crate) fn register() -> Result<(), RoutingError> {
    let config = Config::getFacadeRoot().map_err(|_| RoutingError::NoRegistrationScope)?;
    let path = config
        .get("telescope.path")
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or("telescope".into());
    if path.is_empty()
        || !path
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || b"/-_".contains(&value))
    {
        return Err(RoutingError::InvalidUri { uri: path });
    }
    let mut group = RouteGroup::new().prefix(format!("/{}", path.trim_matches('/')));
    if let Some(domain) = config
        .get("telescope.domain")
        .and_then(|value| value.as_str().map(str::to_owned))
    {
        group = group.domain(domain);
    }
    let middleware = config
        .get("telescope.middleware")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    for name in middleware.iter().filter_map(Value::as_str) {
        group = if name == "openrust_telescope::http::middleware::Authorize" {
            group.middleware(Arc::new(middleware::Authorize))
        } else {
            group.middleware_name(name)
        };
    }
    Route::group(group, |router| {
        for (path, _, _) in TYPES {
            router.r#match(
                &[openrust::http::Method::POST],
                &format!("/telescope-api/{path}"),
                api,
            )?;
            if *path != "dumps" {
                router.get(&format!("/telescope-api/{path}/{{telescopeEntryId}}"), api)?;
            }
        }
        router.r#match(
            &[openrust::http::Method::PUT],
            "/telescope-api/exceptions/{telescopeEntryId}",
            api,
        )?;
        router.get("/telescope-api/monitored-tags", api)?;
        router.r#match(
            &[openrust::http::Method::POST],
            "/telescope-api/monitored-tags",
            api,
        )?;
        router.r#match(
            &[openrust::http::Method::POST],
            "/telescope-api/monitored-tags/delete",
            api,
        )?;
        router.r#match(
            &[openrust::http::Method::POST],
            "/telescope-api/toggle-recording",
            api,
        )?;
        router.r#match(
            &[openrust::http::Method::DELETE],
            "/telescope-api/entries",
            api,
        )?;
        for action in ["preview", "download"] {
            router.get(
                &format!("/telescope-api/mail/{{telescopeEntryId}}/{action}"),
                mail,
            )?;
        }
        router
            .get("/{view?}", page)?
            .r#where("view", ".*")?
            .name("telescope");
        Ok(())
    })
}
fn failure(error: Error) -> HttpError {
    match error {
        Error::NotFound => HttpError::NotFound,
        source => HttpError::Internal {
            source: Box::new(source),
        },
    }
}
fn telescope() -> Result<Arc<Telescope>, HttpError> {
    facades::Telescope::getFacadeRoot().map_err(|source| HttpError::Internal {
        source: Box::new(source),
    })
}
fn page(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        use openrust::contracts::session::Session as _;
        let nonce = Uuid::now_v7().simple().to_string();
        let csrf = request
            .session()
            .ok()
            .and_then(|session| session.token())
            .unwrap_or_default()
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let html = include_str!("dashboard.html")
            .replace("__NONCE__", &nonce)
            .replace("__CSRF__", &csrf)
            .replace(
                "__BASE__",
                &serde_json::to_string(&format!(
                    "/{}",
                    Config::get("telescope.path")
                        .ok()
                        .flatten()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or("telescope".into())
                        .trim_matches('/')
                ))
                .map_err(|_| HttpError::InvalidRequest)?,
            );
        Ok(Response::new(StatusCode::OK, Body::from(html))
            .header("content-type".parse().unwrap(), "text/html; charset=utf-8".parse().unwrap())
            .header("cache-control".parse().unwrap(), "no-store".parse().unwrap())
            .header("content-security-policy".parse().unwrap(),format!("default-src 'none'; style-src 'nonce-{nonce}'; script-src 'nonce-{nonce}'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'").parse().map_err(|_|HttpError::InvalidRequest)?))
    })
}

fn mail(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let telescope = telescope()?;
        let id = request
            .route("telescopeEntryId")
            .ok_or(HttpError::NotFound)?;
        let entry = telescope.repository.find(id).await.map_err(failure)?;
        if entry.r#type != EntryType::Mail {
            return Err(HttpError::NotFound);
        }
        let preview = request.path().ends_with("/preview");
        let content = entry
            .content
            .get(if preview { "html" } else { "raw" })
            .and_then(Value::as_str)
            .unwrap_or_default();
        let mut response = Response::new(StatusCode::OK, Body::from(content.to_owned()))
            .header(
                "cache-control".parse().unwrap(),
                "no-store".parse().unwrap(),
            )
            .header(
                "x-content-type-options".parse().unwrap(),
                "nosniff".parse().unwrap(),
            );
        response = if preview {
            response
                .header(
                    "content-type".parse().unwrap(),
                    "text/html; charset=UTF-8".parse().unwrap(),
                )
                .header(
                    "content-security-policy".parse().unwrap(),
                    "sandbox allow-popups allow-popups-to-escape-sandbox"
                        .parse()
                        .unwrap(),
                )
        } else {
            response
                .header(
                    "content-type".parse().unwrap(),
                    "message/rfc822".parse().unwrap(),
                )
                .header(
                    "content-disposition".parse().unwrap(),
                    format!("attachment; filename=\"mail-{}.eml\"", entry.id)
                        .parse()
                        .map_err(|_| HttpError::InvalidRequest)?,
                )
        };
        Ok(response)
    })
}

fn api(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let telescope = telescope()?;
        telescope.flushEntries();
        telescope.stopRecording();
        let repository = telescope.repository.clone();
        let path = request
            .path()
            .rsplit_once("/telescope-api/")
            .map(|(_, path)| path.to_owned())
            .ok_or(HttpError::NotFound)?;
        let value = match (request.method().as_str(), path.as_str()) {
            ("GET", "monitored-tags") => {
                json!({"tags":repository.monitoring().await.map_err(failure)?})
            }
            ("POST", "monitored-tags" | "monitored-tags/delete") => {
                let tag = request
                    .input(Some("tag"))
                    .await?
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .ok_or(HttpError::InvalidRequest)?;
                if path.ends_with("/delete") {
                    repository.stopMonitoring(&[tag]).await.map_err(failure)?;
                } else {
                    repository.monitor(&[tag]).await.map_err(failure)?;
                }
                return Ok(Response::empty(StatusCode::OK));
            }
            ("POST", "toggle-recording") => {
                if paused().await? {
                    openrust::cache::Cache::forget("telescope:pause-recording")
                        .await
                        .map_err(|source| HttpError::Internal {
                            source: Box::new(source),
                        })?;
                } else {
                    openrust::cache::Cache::put(
                        "telescope:pause-recording",
                        &openrust::cache::Value::Json(json!(true)),
                        Some(openrust::cache::Ttl::Seconds(30 * 24 * 60 * 60)),
                    )
                    .await
                    .map_err(|source| HttpError::Internal {
                        source: Box::new(source),
                    })?;
                }
                return Ok(Response::empty(StatusCode::OK));
            }
            ("DELETE", "entries") => {
                repository.clear().await.map_err(failure)?;
                return Ok(Response::empty(StatusCode::OK));
            }
            (method, _) => {
                let section = path.split('/').next().unwrap_or_default();
                let (_, kind, watcher) = TYPES
                    .iter()
                    .find(|(name, _, _)| *name == section)
                    .ok_or(HttpError::NotFound)?;
                if method == "POST" {
                    if *kind == EntryType::Dump {
                        openrust::cache::Cache::put(
                            "telescope:dump-watcher",
                            &openrust::cache::Value::Json(json!(true)),
                            Some(openrust::cache::Ttl::Seconds(4)),
                        )
                        .await
                        .map_err(|source| HttpError::Internal {
                            source: Box::new(source),
                        })?;
                    }
                    let options = EntryQueryOptions::fromRequest(request).await?;
                    let mut result = json!({"entries":repository.get(Some(*kind), options).await.map_err(failure)?,"status":status(watcher).await?});
                    if *kind == EntryType::Dump {
                        result["dump"] = "true".into();
                    }
                    result
                } else {
                    let id = request
                        .route("telescopeEntryId")
                        .ok_or(HttpError::NotFound)?
                        .to_owned();
                    let mut entry = repository.find(&id).await.map_err(failure)?;
                    if method == "PUT"
                        && request.input(Some("resolved_at")).await? == Some(json!("now"))
                    {
                        let now = jiff::Timestamp::now()
                            .strftime("%Y-%m-%d %H:%M:%S")
                            .to_string();
                        repository
                            .update(&[EntryUpdate::make(
                                entry.id,
                                entry.r#type,
                                json!({"resolved_at":now}).as_object().unwrap().clone(),
                            )])
                            .await
                            .map_err(failure)?;
                        entry = repository.find(&id).await.map_err(failure)?;
                    }
                    let batch = repository
                        .get(
                            None,
                            EntryQueryOptions::forBatchId(Some(entry.batchId)).limit(u64::MAX),
                        )
                        .await
                        .map_err(failure)?;
                    json!({"entry":entry,"batch":batch})
                }
            }
        };
        Ok(Response::json(StatusCode::OK, value)?.header(
            "cache-control".parse().unwrap(),
            "no-store".parse().unwrap(),
        ))
    })
}
async fn paused() -> Result<bool, HttpError> {
    Ok(matches!(
        openrust::cache::Cache::get("telescope:pause-recording")
            .await
            .map_err(|source| HttpError::Internal {
                source: Box::new(source)
            })?,
        Some(openrust::cache::Value::Json(Value::Bool(true)))
    ))
}
async fn status(watcher: &str) -> Result<&'static str, HttpError> {
    let config = Config::getFacadeRoot().map_err(|source| HttpError::Internal {
        source: Box::new(source),
    })?;
    if config.get("telescope.enabled") != Some(json!(true)) {
        return Ok("disabled");
    }
    if paused().await? {
        return Ok("paused");
    }
    let enabled = match config.get(&format!("telescope.watchers.{watcher}")) {
        Some(Value::Bool(value)) => value,
        Some(Value::Object(value)) => value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        _ => false,
    };
    Ok(if enabled { "enabled" } else { "off" })
}
impl EntryQueryOptions {
    pub async fn fromRequest(request: &mut Request) -> Result<Self, HttpError> {
        let values = request.input(None).await?.unwrap_or(json!({}));
        let string = |key| -> Result<Option<String>, HttpError> {
            match values.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value)) => Ok(Some(value.clone())),
                _ => Err(HttpError::InvalidRequest),
            }
        };
        let batch = string("batch_id")?
            .map(|value| Uuid::parse_str(&value))
            .transpose()
            .map_err(|_| HttpError::InvalidRequest)?;
        let uuids = values
            .get("uuids")
            .filter(|v| !v.is_null())
            .map(|values| {
                values
                    .as_array()
                    .ok_or(HttpError::InvalidRequest)?
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .ok_or(HttpError::InvalidRequest)
                            .and_then(|value| {
                                Uuid::parse_str(value).map_err(|_| HttpError::InvalidRequest)
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let number = |key: &str| -> Result<Option<i64>, HttpError> {
            values
                .get(key)
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str()?.parse().ok())
                        .ok_or(HttpError::InvalidRequest)
                })
                .transpose()
        };
        let take = number("take")?.unwrap_or(50);
        Ok(Self::default()
            .batchId(batch)
            .tag(string("tag")?)
            .familyHash(string("family_hash")?)
            .uuids(uuids)
            .beforeSequence(number("before")?)
            .limit(if take < 0 { u64::MAX } else { take as u64 }))
    }
}
