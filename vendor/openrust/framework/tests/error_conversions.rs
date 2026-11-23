use std::{error::Error as _, sync::Arc};

use openrust::{
    auth::AuthError,
    config::ConfigError,
    container::Container,
    contracts::container::{BindingResolutionError, Container as _},
    database::DatabaseError,
    foundation::{BootstrapError, configuration::Routing},
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Config, Route},
    testing::TestApp,
};
use serde_json::json;

// 使用实际公开容器入口，覆盖成功值、目标分类和原始解析错误。
macro_rules! binding_boundary {
    ($name:ident, $target:path, $variant:path) => {
        #[test]
        fn $name() {
            fn resolve(container: &Container) -> Result<Arc<String>, $target> {
                Ok(container.make::<String>()?)
            }
            let container = Container::default();
            let error = resolve(&container).unwrap_err();
            assert!(matches!(error, $variant(_)));
            assert!(error.source().unwrap().is::<BindingResolutionError>());
            let value = Arc::new("resolved".to_owned());
            container.instance(value.clone()).unwrap();
            assert!(Arc::ptr_eq(&resolve(&container).unwrap(), &value));
        }
    };
}

binding_boundary!(
    translation,
    openrust::translation::TranslationError,
    openrust::translation::TranslationError::Unavailable
);
binding_boundary!(
    view,
    openrust::view::ViewError,
    openrust::view::ViewError::Unavailable
);
binding_boundary!(database, DatabaseError, DatabaseError::Unavailable);
binding_boundary!(
    cache,
    openrust::cache::CacheError,
    openrust::cache::CacheError::Unavailable
);
binding_boundary!(
    queue,
    openrust::queue::QueueError,
    openrust::queue::QueueError::Unavailable
);
binding_boundary!(
    events,
    openrust::events::EventError,
    openrust::events::EventError::Unavailable
);
binding_boundary!(
    log,
    openrust::log::LogError,
    openrust::log::LogError::Unavailable
);
binding_boundary!(
    hashing,
    openrust::hashing::HashError,
    openrust::hashing::HashError::Unavailable
);
binding_boundary!(
    filesystem,
    openrust::filesystem::FilesystemError,
    openrust::filesystem::FilesystemError::Unavailable
);
binding_boundary!(
    redis,
    openrust::redis::RedisError,
    openrust::redis::RedisError::Unavailable
);
binding_boundary!(
    process,
    openrust::process::Error,
    openrust::process::Error::Unavailable
);
binding_boundary!(
    context,
    openrust::log::context::ContextError,
    openrust::log::context::ContextError::Unavailable
);
binding_boundary!(
    maintenance,
    openrust::foundation::maintenance::MaintenanceError,
    openrust::foundation::maintenance::MaintenanceError::Unavailable
);

fn strict_config() -> Result<String, HttpError> {
    Ok(Config::string("app.locale_form_field")?)
}

fn config_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let value = strict_config()?;
        Response::json(StatusCode::OK, value)
    })
}

#[tokio::test]
async fn config_facade_question_mark_preserves_failure_type_reporting_and_redaction() {
    let unavailable = strict_config().unwrap_err();
    let config = unavailable
        .source()
        .unwrap()
        .downcast_ref::<ConfigError>()
        .unwrap();
    assert!(matches!(config, ConfigError::Unavailable(_)));
    assert!(config.source().unwrap().is::<BindingResolutionError>());

    let reported = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for field in [
        json!("locale"),
        json!(null),
        json!({"secret": "private-token"}),
    ] {
        let valid = field.is_string();
        type ConfigValues = std::collections::BTreeMap<String, serde_json::Value>;
        type Loader = fn(
            &std::path::Path,
            &openrust::config::Environment,
        ) -> Result<ConfigValues, ConfigError>;
        let load: Loader = match field {
            serde_json::Value::String(_) => |_, _| {
                Ok([(
                    "app".into(),
                    json!({"debug": false, "locale_form_field": "locale"}),
                )]
                .into())
            },
            serde_json::Value::Null => |_, _| {
                Ok([(
                    "app".into(),
                    json!({"debug": false, "locale_form_field": null}),
                )]
                .into())
            },
            _ => |_, _| {
                Ok([(
                    "app".into(),
                    json!({"debug": false, "locale_form_field": {"secret": "private-token"}}),
                )]
                .into())
            },
        };
        let counter = reported.clone();
        let mut app = TestApp::create(move |builder| {
            builder
                .with_config(load)
                .with_exceptions(move |exceptions| {
                    exceptions.report::<ConfigError>(move |error| {
                        assert!(matches!(error, ConfigError::InvalidType { key, .. } if key == "app.locale_form_field"));
                        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        openrust::foundation::configuration::ReportDecision::Stop
                    });
                })
                .with_routing(Routing::default().web(|| Route::get("/config", config_route)))
        }).unwrap();
        let response = app.get("/config").await.unwrap();
        if valid {
            response.assertOk().assertContent("\"locale\"");
        } else {
            response.assertStatus(StatusCode::INTERNAL_SERVER_ERROR);
            response
                .assertDontSee("private-token", false)
                .assertDontSee("locale_form_field", false);
        }
    }
    assert_eq!(reported.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[test]
fn bootstrap_and_routing_keep_original_variants_and_sources() {
    fn resolve(container: &Container) -> Result<(), BootstrapError> {
        container.make::<String>()?;
        Ok(())
    }
    let error = resolve(&Container::default()).unwrap_err();
    assert!(matches!(error, BootstrapError::ContainerFailed { .. }));
    assert!(error.source().unwrap().is::<BindingResolutionError>());
    let config = BootstrapError::from(ConfigError::InvalidKey {
        key: "app.secret".into(),
    });
    assert!(matches!(config, BootstrapError::ConfigurationFailed { .. }));
    assert!(config.source().unwrap().is::<ConfigError>());
    let routing = BootstrapError::from(openrust::routing::RoutingError::InvalidUri {
        uri: "invalid".into(),
    });
    assert!(matches!(routing, BootstrapError::RoutingFailed { .. }));
    assert!(
        routing
            .source()
            .unwrap()
            .is::<openrust::routing::RoutingError>()
    );
    fn register(container: &Container) -> Result<(), openrust::routing::RoutingError> {
        container.make::<String>()?;
        Ok(())
    }
    let routing = register(&Container::default()).unwrap_err();
    assert!(matches!(
        routing,
        openrust::routing::RoutingError::ContainerFailed { .. }
    ));
    assert!(routing.source().unwrap().is::<BindingResolutionError>());
}

#[test]
fn auth_keeps_backend_sources_and_does_not_unwrap_explicit_backend_http() {
    fn user_operation() -> Result<(), AuthError> {
        Err::<(), _>(DatabaseError::InvalidQuery("query-context"))?;
        Ok(())
    }
    let error = user_operation().unwrap_err();
    assert!(matches!(error, AuthError::Backend(_)));
    assert!(error.source().unwrap().is::<DatabaseError>());
    let http = HttpError::from(error);
    assert!(matches!(http, HttpError::Internal { .. }));
    assert!(
        http.source()
            .unwrap()
            .source()
            .unwrap()
            .is::<DatabaseError>()
    );

    // 既有 Http 转换保留429，强制 Backend 包装仍为500，二者不能机械互换。
    let throttled = AuthError::Throttled(5);
    assert!(matches!(
        HttpError::from(throttled),
        HttpError::Throttled { .. }
    ));
    let wrapped = AuthError::Backend(Box::new(HttpError::NotFound));
    assert!(matches!(
        HttpError::from(wrapped),
        HttpError::Internal { .. }
    ));
}

#[test]
fn json_question_mark_keeps_serialization_classification() {
    fn collection() -> Result<(), openrust::support::CollectionError> {
        serde_json::from_str::<serde_json::Value>("invalid")?;
        Ok(())
    }
    let error = collection().unwrap_err();
    assert!(matches!(
        error,
        openrust::support::CollectionError::Serialization(_)
    ));
    assert!(error.source().unwrap().is::<serde_json::Error>());
    let error = Response::json(StatusCode::OK, BadJson).err().unwrap();
    assert!(matches!(error, HttpError::Internal { .. }));
    assert!(error.source().unwrap().is::<serde_json::Error>());
}

struct BadJson;
impl serde::Serialize for BadJson {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("private-json-token"))
    }
}

#[tokio::test]
async fn view_async_failure_retains_phase_and_io_chain() {
    fn propagate(source: Result<(), openrust::view::ViewError>) -> Result<(), HttpError> {
        source?;
        Ok(())
    }
    let error = propagate(Err(openrust::view::ViewError::ReadFailed(
        std::io::Error::other("view-path"),
    )))
    .unwrap_err();
    let source = error
        .source()
        .unwrap()
        .downcast_ref::<openrust::view::ViewError>()
        .unwrap();
    assert!(matches!(source, openrust::view::ViewError::ReadFailed(_)));
    assert_eq!(source.source().unwrap().to_string(), "view-path");
    let worker = tokio::spawn(async { std::future::pending::<()>().await });
    worker.abort();
    let error = HttpError::from(worker.await.unwrap_err());
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<tokio::task::JoinError>()
            .unwrap()
            .is_cancelled()
    );
}

#[test]
fn concrete_http_conversions_keep_the_original_source() {
    fn check<E: std::error::Error + Send + Sync + 'static>(source: E)
    where
        HttpError: From<E>,
    {
        fn propagate<E>(result: Result<usize, E>) -> Result<usize, HttpError>
        where
            HttpError: From<E>,
        {
            Ok(result?)
        }
        assert_eq!(propagate::<E>(Ok(7)).unwrap(), 7);
        let error = propagate(Err(source)).unwrap_err();
        assert!(matches!(error, HttpError::Internal { .. }));
        assert!(error.source().unwrap().is::<E>());
    }
    check(ConfigError::InvalidKey {
        key: "private-key".into(),
    });
    check(Container::default().make::<String>().unwrap_err());
    check(openrust::translation::TranslationError::InvalidLocale);
    check(openrust::view::ViewError::RenderFailed(Box::new(
        std::io::Error::other("view-source"),
    )));
    check(DatabaseError::InvalidQuery("operation"));
    check(serde_json::from_str::<serde_json::Value>("invalid").unwrap_err());
    check(openrust::log::LogError::WriteFailed(Box::new(
        std::io::Error::other("log-source"),
    )));
    check(openrust::redis::RedisError::InvalidConfiguration);
    check(http::HeaderValue::from_str("invalid\nheader").unwrap_err());
    check(std::io::Error::other("io-source"));
    check(openrust::mail::MailError::Composition {
        source: Box::new(std::io::Error::other("mail-source")),
    });
    check(openrust::notifications::NotificationError::Delivery {
        source: Box::new(std::io::Error::other("notification-source")),
    });
    check(openrust::hashing::HashError::HashingFailed);
}

#[test]
fn concrete_auth_conversions_keep_backend_classification() {
    fn check<E: std::error::Error + Send + Sync + 'static>(source: E)
    where
        AuthError: From<E>,
    {
        fn propagate<E>(result: Result<usize, E>) -> Result<usize, AuthError>
        where
            AuthError: From<E>,
        {
            Ok(result?)
        }
        assert_eq!(propagate::<E>(Ok(7)).unwrap(), 7);
        let error = propagate(Err(source)).unwrap_err();
        assert!(matches!(error, AuthError::Backend(_)));
        assert!(error.source().unwrap().is::<E>());
    }
    check(Container::default().make::<String>().unwrap_err());
    check(DatabaseError::InvalidQuery("auth-query"));
    check(openrust::cache::CacheError::LockTimeout);
    check(openrust::hashing::HashError::HashingFailed);
    check(openrust::session::SessionError::Storage);
    check(ConfigError::InvalidKey {
        key: "auth.key".into(),
    });
    check(openrust::events::EventError::InvalidRegistration);
    check(openrust::cookie::CookieError);
    check(serde_json::from_str::<serde_json::Value>("invalid").unwrap_err());
    check(openrust::routing::UrlError::InvalidParameter);
}

binding_boundary!(
    image,
    openrust::image::Error,
    openrust::image::Error::Unavailable
);
binding_boundary!(
    concurrency,
    openrust::concurrency::Error,
    openrust::concurrency::Error::Unavailable
);

#[test]
fn concurrency_exposes_io_encoding_and_process_sources() {
    fn read() -> openrust::concurrency::Result<()> {
        Err::<(), _>(std::io::Error::other("operation-context"))?;
        Ok(())
    }
    let error = read().unwrap_err();
    assert!(matches!(error, openrust::concurrency::Error::Io(_)));
    assert_eq!(error.source().unwrap().to_string(), "operation-context");
    let json = serde_json::from_str::<serde_json::Value>("invalid").unwrap_err();
    let error = openrust::concurrency::Error::from(json);
    assert!(matches!(error, openrust::concurrency::Error::Encoding(_)));
    assert!(error.source().unwrap().is::<serde_json::Error>());
    let error = openrust::concurrency::Error::from(openrust::process::Error::from(
        std::io::Error::other("process-context"),
    ));
    assert!(matches!(error, openrust::concurrency::Error::Process(_)));
    assert!(error.source().unwrap().is::<openrust::process::Error>());
    assert_eq!(
        error.source().unwrap().source().unwrap().to_string(),
        "process-context"
    );
}

#[test]
fn collection_path_sources_stay_structured() {
    let error = openrust::support::Arr::string(&json!({"name": 1}), "name").unwrap_err();
    let collection = openrust::support::CollectionError::from(error);
    assert!(matches!(
        collection,
        openrust::support::CollectionError::Source(_)
    ));
    assert!(
        collection
            .source()
            .unwrap()
            .is::<openrust::support::ArrValueError>()
    );
}

#[tokio::test]
async fn file_cache_retains_operation_and_original_io_error() {
    use openrust::contracts::cache::Store as _;
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("blocked");
    std::fs::write(&blocked, "file").unwrap();
    let store = openrust::cache::FileStore::new(blocked, root.path().join("locks"));
    let error = store.get("key").await.unwrap_err();
    assert!(matches!(error, openrust::cache::CacheError::Backend { .. }));
    let io = error
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert!(io.to_string().starts_with("create cache directory:"));
    let original = io
        .source()
        .unwrap()
        .downcast_ref::<std::io::Error>()
        .unwrap();
    assert_eq!(io.kind(), original.kind());

    let healthy = openrust::cache::FileStore::new(
        root.path().join("cache"),
        root.path().join("healthy-locks"),
    );
    healthy
        .forever("key", &openrust::cache::Value::from("value"))
        .await
        .unwrap();
    assert_eq!(
        healthy.get("key").await.unwrap(),
        Some(openrust::cache::Value::from("value"))
    );
}

#[test]
fn cache_backend_conversions_preserve_domains_and_log_keeps_io() {
    let database = DatabaseError::InvalidQuery("cache-operation");
    let error = openrust::cache::CacheError::from(database);
    assert!(matches!(error, openrust::cache::CacheError::Backend { .. }));
    assert!(error.source().unwrap().is::<DatabaseError>());
    let error =
        openrust::cache::CacheError::from(openrust::redis::RedisError::InvalidConfiguration);
    assert!(matches!(error, openrust::cache::CacheError::Backend { .. }));
    assert!(error.source().unwrap().is::<openrust::redis::RedisError>());
    let error = openrust::log::LogError::from(std::io::Error::other("log-write"));
    assert!(matches!(error, openrust::log::LogError::WriteFailed(_)));
    assert!(error.source().unwrap().is::<std::io::Error>());
}

#[test]
fn collection_preserves_direct_data_path_errors() {
    let mut value = json!({});
    let path = std::iter::repeat_n("a", 1025).collect::<Vec<_>>().join(".");
    let source = openrust::support::data_set(&mut value, &path, json!(1)).unwrap_err();
    let error = openrust::support::CollectionError::from(source);
    assert!(matches!(
        error,
        openrust::support::CollectionError::Source(_)
    ));
    assert!(
        error
            .source()
            .unwrap()
            .is::<openrust::support::DataPathError>()
    );
}

#[tokio::test]
async fn image_path_failure_keeps_io_domain_and_http_source() {
    let root = tempfile::tempdir().unwrap();
    let image = openrust::image::ImageManager::new(Default::default())
        .fromPath(root.path().join("missing.png"));
    let source = image.dimensions().await.unwrap_err();
    assert!(matches!(source, openrust::image::Error::Io(_)));
    assert_eq!(
        source
            .source()
            .unwrap()
            .downcast_ref::<std::io::Error>()
            .unwrap()
            .kind(),
        std::io::ErrorKind::NotFound
    );
    let error = HttpError::from(source);
    assert!(matches!(error, HttpError::Internal { .. }));
    assert!(error.source().unwrap().is::<openrust::image::Error>());
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<std::io::Error>()
    );
}

#[test]
fn collection_keeps_nested_array_path_source() {
    let mut value = json!({});
    let path = std::iter::repeat_n("a", 1025).collect::<Vec<_>>().join(".");
    let source = openrust::support::data_set(&mut value, &path, json!(1)).unwrap_err();
    let error =
        openrust::support::CollectionError::from(openrust::support::ArrValueError::from(source));
    assert!(
        error
            .source()
            .unwrap()
            .is::<openrust::support::ArrValueError>()
    );
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .is::<openrust::support::DataPathError>()
    );
}
