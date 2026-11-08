//! 日志上下文的键语义、属性选值与作用域隔离。

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use figment::{Figment, providers::Serialized};
use serde_json::{Map, Value, json};

use super::{ContextServiceProvider, Repository};
use crate::container::{
    ClassAttribute, ClassDescriptor, ConstructorParam, ParameterValue, ServiceKey,
    attributes::Context,
};
use crate::foundation::{ProviderBuilder, ProviderRuntime};
use crate::logging::{LogLevel, LogManager, LogRecord, LogServiceProvider, LogSink, Logger};
use crate::{BoxFuture, Result};

#[test]
fn numeric_keys_are_reindexed_after_removal_without_moving_other_fields() {
    let repo = Repository::default();
    for (key, value) in [
        ("9", "first"),
        ("note", "a"),
        ("8", "second"),
        ("tail", "b"),
        ("-3", "third"),
    ] {
        repo.add(key, json!(value));
    }
    assert_eq!(repo.get("0"), Some(json!("first")));
    assert_eq!(repo.get("9"), None);
    repo.forget("0");
    assert_eq!(
        repo.all().keys().map(String::as_str).collect::<Vec<_>>(),
        ["note", "1", "tail", "2"]
    );
    assert_eq!(repo.get("0"), None);

    repo.add("08", json!("literal"));
    repo.add("1", json!("fourth"));
    let ordinary = repo.all();
    assert_eq!(
        ordinary.keys().map(String::as_str).collect::<Vec<_>>(),
        ["note", "0", "tail", "1", "08", "2"]
    );
    assert_eq!(ordinary["0"], "second");
    assert_eq!(ordinary["1"], "third");
    assert_eq!(ordinary["2"], "fourth");
    assert_eq!(ordinary["08"], "literal");

    repo.add_hidden("7", json!("hidden-first"));
    repo.add_hidden("8", json!("hidden-second"));
    repo.forget_hidden("0");
    repo.add_hidden("state", json!(true));
    assert_eq!(repo.get_hidden("0"), Some(json!("hidden-second")));
    assert_eq!(repo.get_hidden("1"), None);
    assert_eq!(repo.get("0"), Some(json!("second")));
}

#[test]
fn repository_preserves_literal_and_falsy_values_in_independent_domains() {
    let repo = Repository::default();
    repo.add("op.value", json!(false));
    repo.add_hidden("op.value", json!(""));
    repo.add("zero", json!(0));
    repo.add("unset", Value::Null);
    repo.add_hidden("private-null", Value::Null);

    assert_eq!(repo.get("op"), None);
    assert_eq!(repo.get("op.value"), Some(json!(false)));
    assert_eq!(repo.get_hidden("op.value"), Some(json!("")));
    assert_eq!(repo.get("zero"), Some(json!(0)));
    assert_eq!(repo.get_hidden("zero"), None);
    assert_eq!(repo.get("unset"), None);
    assert_eq!(repo.get_hidden("private-null"), None);
    assert!(repo.has("unset") && repo.has_hidden("private-null"));
    assert_eq!(repo.all().get("unset"), Some(&Value::Null));
    assert!(!repo.all().contains_key("private-null"));
    repo.forget_hidden("op.value");
    assert_eq!(repo.get("op.value"), Some(json!(false)));
    repo.flush();
    assert!(repo.all().is_empty() && repo.all_hidden().is_empty());
}

async fn start(descriptors: impl IntoIterator<Item = ClassDescriptor>) -> Result<ProviderRuntime> {
    let mut builder = ProviderBuilder::new();
    builder
        .provider(ContextServiceProvider)
        .provider(LogServiceProvider)
        .class_descriptors(descriptors);
    builder
        .start(Figment::from(Serialized::defaults(json!({
            "app": {"env": "testing"},
            "logging": {"channels": {"capture": {"driver": "test-capture"}}}
        }))))
        .await
}

async fn shutdown(runtime: &ProviderRuntime) -> Result<()> {
    runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await
}

struct Reading {
    public: Arc<String>,
    hidden: Arc<String>,
    count: u32,
    constructor_default: u32,
}

fn descriptor(calls: Arc<AtomicUsize>) -> ClassDescriptor {
    ClassDescriptor::concrete::<Reading>(
        "test::ContextReading",
        vec![
            ConstructorParam::scalar::<String>("public").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(
                        Context::<String>::new("op.label")
                            .with_default("public-default".to_owned()),
                    )
                }),
            ),
            ConstructorParam::scalar::<String>("hidden").with_attribute(
                ClassAttribute::contextual(|| {
                    Ok(Context::<String>::new("op.label")
                        .hidden()
                        .with_default("hidden-default".to_owned()))
                }),
            ),
            ConstructorParam::scalar::<u32>("count").with_attribute(ClassAttribute::contextual(
                move || {
                    let calls = Arc::clone(&calls);
                    Ok(Context::<u32>::new("count").with_default_using(move || {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Ok(42)
                    }))
                },
            )),
            ConstructorParam::scalar::<u32>("constructor_default")
                .with_default(|| Ok(ParameterValue::new(9_u32)))
                .with_attribute(ClassAttribute::contextual(|| {
                    Ok(Context::<u32>::new("missing"))
                })),
        ],
        |args| {
            Ok(Arc::new(Reading {
                public: args.value(0)?.downcast::<String>()?,
                hidden: args.value(1)?.downcast::<String>()?,
                count: *args.value(2)?.downcast::<u32>()?,
                constructor_default: *args.value(3)?.downcast::<u32>()?,
            }))
        },
    )
}

#[tokio::test]
async fn context_attribute_selects_domain_and_only_evaluates_lazy_defaults_when_absent()
-> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = start([descriptor(Arc::clone(&calls))]).await?;
    let first = runtime.container().scope()?;
    let second = runtime.container().scope()?;
    let repo = first.resolve::<Repository>().await?;
    repo.add("op.label", json!("public"));
    repo.add_hidden("op.label", json!("hidden"));
    repo.add("count", json!(0));

    let found = first.resolve::<Reading>().await?;
    assert_eq!(found.public.as_str(), "public");
    assert_eq!(found.hidden.as_str(), "hidden");
    assert_eq!((found.count, found.constructor_default), (0, 9));
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    repo.add("count", Value::Null);
    assert_eq!(first.resolve::<Reading>().await?.count, 42);
    let absent = second.resolve::<Reading>().await?;
    assert_eq!(absent.public.as_str(), "public-default");
    assert_eq!(absent.hidden.as_str(), "hidden-default");
    assert_eq!(absent.count, 42);
    assert_eq!(calls.load(Ordering::Relaxed), 2);

    repo.add("count", json!("sensitive-mismatch-probe"));
    assert!(matches!(first.resolve::<Reading>().await,
        Err(error) if error.code() == "container_contextual_context_type_mismatch"
            && !format!("{error} {error:?}").contains("sensitive-mismatch-probe")));
    assert_eq!(calls.load(Ordering::Relaxed), 2);
    first.close();
    second.close();
    shutdown(&runtime).await
}

#[derive(Debug, PartialEq)]
struct Fields {
    context: Map<String, Value>,
    extra: Map<String, Value>,
}

#[derive(Default)]
struct Capture(Mutex<Vec<Fields>>);

impl LogSink for Capture {
    fn write<'a>(&'a self, record: &'a LogRecord) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            self.0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Fields {
                    context: record.context().clone(),
                    extra: record.extra().clone(),
                });
            Ok(())
        })
    }
}

async fn capture_logger(runtime: &ProviderRuntime) -> Result<(Arc<Logger>, Arc<Capture>)> {
    let manager = runtime.container().resolve::<LogManager>().await?;
    let captured = Arc::new(Capture::default());
    manager.extend("test-capture", {
        let captured = Arc::clone(&captured);
        move |_| Ok(Arc::clone(&captured) as Arc<dyn LogSink>)
    });
    Ok((manager.channel(Some("capture")).await?, captured))
}

#[tokio::test]
async fn cached_logger_captures_public_extra_from_each_entry_and_its_own_root() -> Result<()> {
    let runtime = start([]).await?;
    let (logger, captured) = capture_logger(&runtime).await?;
    let first = Arc::new(runtime.container().scope()?);
    let second = Arc::new(runtime.container().scope()?);
    let repo = first.resolve::<Repository>().await?;
    repo.add("label", json!("first"));
    repo.add_hidden("secret", json!("private"));
    runtime
        .container()
        .resolve::<Repository>()
        .await?
        .add("label", json!("root"));

    let local = Map::from_iter([("label".to_owned(), json!("call"))]);
    first
        .run(logger.log(LogLevel::Info, "scoped", local.clone()))
        .await?;
    second
        .run(logger.log(LogLevel::Info, "empty-scope", Map::new()))
        .await?;
    logger.log(LogLevel::Info, "root", Map::new()).await?;
    assert_eq!(
        *captured.0.lock().unwrap_or_else(PoisonError::into_inner),
        [
            Fields {
                context: local,
                extra: Map::from_iter([("label".to_owned(), json!("first"))])
            },
            Fields {
                context: Map::new(),
                extra: Map::new()
            },
            Fields {
                context: Map::new(),
                extra: Map::from_iter([("label".to_owned(), json!("root"))])
            },
        ]
    );
    first.close();
    second.close();
    shutdown(&runtime).await
}

#[tokio::test]
async fn repository_factory_can_log_before_publishing_its_fields() -> Result<()> {
    let runtime = start([]).await?;
    let (logger, captured) = capture_logger(&runtime).await?;
    let container = runtime.container();
    let key = ServiceKey::named("test.reentrant-context");
    container
        .scoped_key::<Repository, _>(key.clone(), {
            let logger = Arc::clone(&logger);
            move |_| {
                let logger = Arc::clone(&logger);
                Box::pin(async move {
                    let repository = Arc::new(Repository::default());
                    repository.add("phase", json!("ready"));
                    logger.log(LogLevel::Info, "building", Map::new()).await?;
                    Ok(repository)
                })
            }
        })
        .await?;
    container.alias(key, ServiceKey::typed::<Repository>())?;

    tokio::time::timeout(Duration::from_secs(2), container.resolve::<Repository>())
        .await
        .map_err(|_| {
            crate::Error::custom(
                "test_context_factory_waits_for_itself",
                "上下文工厂内的日志未完成",
            )
        })??;
    logger.log(LogLevel::Info, "published", Map::new()).await?;

    assert_eq!(
        *captured.0.lock().unwrap_or_else(PoisonError::into_inner),
        [
            Fields {
                context: Map::new(),
                extra: Map::new()
            },
            Fields {
                context: Map::new(),
                extra: Map::from_iter([("phase".to_owned(), json!("ready"))])
            },
        ]
    );
    shutdown(&runtime).await
}
