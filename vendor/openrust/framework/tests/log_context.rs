use openrust::{
    container::{
        AttributeParameter, AttributeValue, ClassAttribute, attributes::Context as ContextAttribute,
    },
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    log::{
        LogContext,
        context::{ContextError, Repository},
    },
};
use serde_json::json;

fn fields(key: &str, value: serde_json::Value) -> LogContext {
    LogContext::from_iter([(key.to_owned(), value)])
}

#[test]
fn manages_null_values_stacks_counters_and_hidden_fields() {
    let context = Repository::default();
    context.add("empty", json!(null));
    assert!(context.has("empty"));
    assert!(context.get("empty").is_none());
    assert!(!context.addIf("empty", json!(9)));
    assert!(
        context
            .remember("empty", || panic!("stored null is present"))
            .is_none()
    );
    assert_eq!(
        context.rememberHidden("cached", || {
            assert!(context.missingHidden("cached"));
            json!("ready")
        }),
        Some(json!("ready"))
    );
    context.addHidden("empty", json!("secret"));
    assert_eq!(context.getHidden("empty"), Some(json!("secret")));
    assert_eq!(context.only(&["empty"]), fields("empty", json!(null)));
    assert_eq!(
        context.exceptHidden(&["empty"]),
        fields("cached", json!("ready"))
    );
    assert_eq!(context.pullHidden("empty"), Some(json!("secret")));
    assert!(context.missingHidden("empty"));
    context.push("stack", [json!(1), json!("1")]).unwrap();
    assert!(context.stackContains("stack", &json!(1)));
    assert!(!context.stackContains("stack", &json!(true)));
    assert_eq!(context.pop("stack").unwrap(), json!("1"));
    assert!(matches!(
        context.push("empty", [json!(1)]),
        Err(ContextError::InvalidStack)
    ));
    assert!(matches!(
        context.pop("absent"),
        Err(ContextError::EmptyStack)
    ));
    assert_eq!(context.increment("attempt", 3).unwrap(), 3);
    assert_eq!(context.decrement("attempt", 2).unwrap(), 1);
    context.add("attempt", json!(i64::MAX));
    assert!(matches!(
        context.increment("attempt", 1),
        Err(ContextError::InvalidCounter)
    ));
    assert_eq!(context.get("attempt"), Some(json!(i64::MAX)));
    context.flush();
    assert!(context.isEmpty());
}

#[tokio::test]
async fn sibling_overlays_restore_outer_fields_after_error_cancellation_and_panic() {
    let context = Repository::default();
    context.add("value", json!("base"));
    context.addHidden("token", json!("base-secret"));
    let barrier = tokio::sync::Barrier::new(2);
    let left = context.scope(
        async {
            context.addHidden("token", json!("left-secret"));
            barrier.wait().await;
            assert_eq!(context.get("value"), Some(json!("left")));
            assert_eq!(context.getHidden("token"), Some(json!("left-secret")));
            Err::<(), _>("failed")
        },
        fields("value", json!("left")),
        LogContext::new(),
    );
    let right = context.scope(
        async {
            barrier.wait().await;
            assert_eq!(context.get("value"), Some(json!("right")));
            assert_eq!(context.getHidden("token"), Some(json!("base-secret")));
        },
        fields("value", json!("right")),
        LogContext::new(),
    );
    assert_eq!(tokio::join!(left, right).0, Err("failed"));
    let pending = context.scope(
        async {
            context.flush();
            std::future::pending::<()>().await
        },
        LogContext::new(),
        LogContext::new(),
    );
    let mut pending = Box::pin(pending);
    std::future::poll_fn(|cx| {
        use std::future::Future as _;
        assert!(pending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(pending);
    let panic = std::panic::catch_unwind(|| {
        context.scopeSync(
            || {
                context.flush();
                panic!("scope test panic");
            },
            LogContext::new(),
            LogContext::new(),
        );
    });
    assert!(panic.is_err());
    assert_eq!(context.get("value"), Some(json!("base")));
    assert_eq!(context.getHidden("token"), Some(json!("base-secret")));
}

#[tokio::test]
async fn application_tasks_use_distinct_repository_instances() {
    let mut app = Application::configure(std::env::temp_dir())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let container = app.container();
    assert!(container.make::<Repository>().is_err());
    let first = container.scope(async {
        let context = container.make::<Repository>().unwrap();
        context.add("task", json!(1));
        context.addHidden("task", json!("private-task"));
        let public = ClassAttribute::contextual(|| Ok(ContextAttribute::<u32>::new("task")));
        let hidden =
            ClassAttribute::contextual(|| Ok(ContextAttribute::<String>::new("task").hidden()));
        let missing = ClassAttribute::contextual(|| {
            Ok(ContextAttribute::<u32>::new("missing").withDefaultUsing(|| Ok(3)))
        });
        let parameter = AttributeParameter {
            name: "task",
            variadic: false,
        };
        let AttributeValue::One(value) =
            container.resolveFromAttribute(&public, parameter).unwrap()
        else {
            panic!("expected one value")
        };
        assert_eq!(*value.downcast::<u32>().unwrap(), 1);
        let AttributeValue::One(value) = container
            .resolveFromAttributeAsync(&hidden, parameter)
            .await
            .unwrap()
        else {
            panic!("expected one value")
        };
        assert_eq!(value.downcast::<String>().unwrap().as_str(), "private-task");
        let AttributeValue::One(value) =
            container.resolveFromAttribute(&missing, parameter).unwrap()
        else {
            panic!("expected one value")
        };
        assert_eq!(*value.downcast::<u32>().unwrap(), 3);
        let invalid =
            ClassAttribute::contextual(|| Ok(ContextAttribute::<u32>::new("task").hidden()));
        assert!(
            !container
                .resolveFromAttribute(&invalid, parameter)
                .unwrap_err()
                .to_string()
                .contains("private-task")
        );
        tokio::task::yield_now().await;
        assert_eq!(context.get("task"), Some(json!(1)));
        context
    });
    let second = container.scope(async {
        let context = container.make::<Repository>().unwrap();
        assert!(context.isEmpty());
        context.addHidden("task", json!(2));
        tokio::task::yield_now().await;
        assert!(context.missing("task"));
        context
    });
    let (first, second) = tokio::join!(first, second);
    assert!(!std::sync::Arc::ptr_eq(&first, &second));
    container
        .scope(async {
            assert!(container.make::<Repository>().unwrap().isEmpty());
        })
        .await;
    assert!(container.make::<Repository>().is_err());
}
