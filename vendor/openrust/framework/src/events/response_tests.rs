//! 检出选择顺序、到达时类型检查、原值响应与已观察身份冲突的回归。
use super::{Event, Events, EventsServiceProvider};
use crate::{
    Result,
    support::{DataObject, DataValue},
};
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

struct A(u8);
impl Event for A {
    const NAME: &'static str = "Tests\\Response\\A";
}
struct B(u8);
impl Event for B {
    const NAME: &'static str = "Tests\\Response\\B";
}
struct Duplicate;
impl Event for Duplicate {
    const NAME: &'static str = A::NAME;
}
struct Empty;
impl Event for Empty {
    const NAME: &'static str = "";
}
struct Pattern;
impl Event for Pattern {
    const NAME: &'static str = "Invalid*Class";
}
async fn build(provider: EventsServiceProvider) -> Result<Events> {
    let mut builder = crate::foundation::ProviderBuilder::new();
    builder.provider(provider);
    let runtime = builder.start(figment::Figment::new()).await?;
    Ok((*runtime.container().resolve::<Events>().await?).clone())
}
fn record(log: &Mutex<Vec<String>>, entry: String) {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(entry);
}
fn values(log: &Mutex<Vec<String>>) -> Vec<String> {
    log.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

#[tokio::test]
async fn canonical_and_string_borrow_same_object_exact_before_heterogeneous_wildcard() -> Result<()>
{
    let calls = Arc::new(Mutex::new(Vec::new()));
    let a = A(7);
    let address = std::ptr::from_ref(&a).addr();
    let mut provider = EventsServiceProvider::new();
    let seen = Arc::clone(&calls);
    provider.listen_named_reply("*", move |payload, context| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            let value = if let Some(a) = payload.downcast_ref::<A>() {
                a.0
            } else {
                payload
                    .downcast_ref::<B>()
                    .ok_or(crate::Error::Unauthorized)?
                    .0
            };
            record(&seen, format!("wild:{}:{value}", context.event_name()));
            Ok(DataValue::String("wild".into()))
        })
    });
    let seen = Arc::clone(&calls);
    provider.listen::<A, _>(move |event, context| {
        assert_eq!(std::ptr::from_ref(event).addr(), address);
        assert_eq!(context.event_name(), A::NAME);
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            record(&seen, "typed".into());
            Ok(())
        })
    });
    let seen = Arc::clone(&calls);
    provider.listen_named_reply(A::NAME, move |payload, _| {
        assert_eq!(
            payload
                .downcast_ref::<A>()
                .map(|event| std::ptr::from_ref(event).addr()),
            Some(address)
        );
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            record(&seen, "string".into());
            Ok(DataValue::Number(0.into()))
        })
    });
    let events = build(provider).await?;
    for response in [
        events.dispatch(&a).await?,
        events.dispatch_named(A::NAME, &a).await?,
    ] {
        assert!(
            matches!(response.as_deref(),Some([DataValue::Null,DataValue::Number(n),DataValue::String(s)]) if n.as_i64()==Some(0) && s=="wild")
        );
    }
    events.dispatch(&B(9)).await?;
    assert_eq!(
        values(&calls),
        [
            "typed",
            "string",
            "wild:Tests\\Response\\A:7",
            "typed",
            "string",
            "wild:Tests\\Response\\A:7",
            "wild:Tests\\Response\\B:9"
        ]
    );
    Ok(())
}

#[tokio::test]
async fn wildcard_patterns_keep_first_group_order_not_flat_registration_order() -> Result<()> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut provider = EventsServiceProvider::new();
    for (pattern, label) in [("x*", "a1"), ("*", "b1"), ("x*", "a2"), ("xyz", "exact")] {
        let calls = Arc::clone(&calls);
        provider.listen_named_reply(pattern, move |_, _| {
            let calls = Arc::clone(&calls);
            Box::pin(async move {
                record(&calls, label.into());
                Ok(DataValue::Null)
            })
        });
    }
    build(provider).await?.dispatch_named("xyz", &A(0)).await?;
    assert_eq!(values(&calls), ["exact", "a1", "a2", "b1"]);
    Ok(())
}

#[tokio::test]
async fn type_error_only_when_listener_is_reached_and_is_server_error() -> Result<()> {
    let mut provider = EventsServiceProvider::new();
    provider.listen_named_reply(A::NAME, |_, _| {
        Box::pin(async { Ok(DataValue::Bool(false)) })
    });
    provider.listen::<A, _>(|_, _| Box::pin(async { Err(crate::Error::Unauthorized) }));
    let events = build(provider).await?;
    assert!(matches!(
        events.dispatch_named(A::NAME, &B(0)).await?.as_deref(),
        Some([])
    ));
    assert!(matches!(
        events.until_named(A::NAME, &B(0)).await?,
        DataValue::Bool(false)
    ));
    let mut provider = EventsServiceProvider::new();
    provider.listen::<A, _>(|_, _| Box::pin(async { Ok(()) }));
    let events = build(provider).await?;
    let error = events
        .dispatch_named(A::NAME, &B(0))
        .await
        .err()
        .ok_or(crate::Error::Unauthorized)?;
    assert_eq!(error.code(), "event_type_mismatch");
    assert_eq!(
        error.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    Ok(())
}

#[tokio::test]
async fn until_zero_stops_before_wrong_type_but_null_reaches_it() -> Result<()> {
    for (reply, should_error) in [
        (DataValue::Number(0.into()), false),
        (DataValue::Null, true),
    ] {
        let mut provider = EventsServiceProvider::new();
        provider.listen_named_reply(A::NAME, move |_, _| {
            let reply = reply.clone();
            Box::pin(async move { Ok(reply) })
        });
        provider.listen::<A, _>(|_, _| Box::pin(async { Ok(()) }));
        let outcome = build(provider).await?.until_named(A::NAME, &B(1)).await;
        if should_error {
            assert_eq!(
                outcome.err().map(|e| e.code().to_owned()).as_deref(),
                Some("event_type_mismatch")
            );
        } else {
            assert!(matches!(outcome?,DataValue::Number(n) if n.as_i64()==Some(0)));
        }
    }
    Ok(())
}

#[tokio::test]
async fn absent_empty_and_star_string_names_are_ordinary_strings() -> Result<()> {
    let events = build(EventsServiceProvider::new()).await?;
    assert!(matches!(events.dispatch(&A(0)).await?.as_deref(), Some([])));
    assert!(matches!(events.until(&A(0)).await?, DataValue::Null));
    assert!(matches!(
        events.dispatch_named("", &1u8).await?.as_deref(),
        Some([])
    ));
    assert!(matches!(
        events.dispatch_named("*", &1u8).await?.as_deref(),
        Some([])
    ));
    for error in [
        events.dispatch(&Empty).await.err(),
        events.dispatch(&Pattern).await.err(),
    ] {
        let error = error.ok_or(crate::Error::Unauthorized)?;
        assert_eq!(error.code(), "event_name_invalid");
        assert_eq!(
            error.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
    Ok(())
}

#[tokio::test]
async fn provider_rejects_conflicting_and_invalid_declared_types() -> Result<()> {
    for invalid in [false, true] {
        let mut provider = EventsServiceProvider::new();
        provider.listen::<A, _>(|_, _| Box::pin(async { Ok(()) }));
        if invalid {
            provider.listen::<Empty, _>(|_, _| Box::pin(async { Ok(()) }));
        } else {
            provider.listen::<Duplicate, _>(|_, _| Box::pin(async { Ok(()) }));
        }
        let error = build(provider)
            .await
            .err()
            .ok_or(crate::Error::Unauthorized)?;
        assert_eq!(
            error.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
        let mut cause = &error;
        while let crate::Error::ServiceInitialization { source, .. } = cause {
            cause = source;
        }
        assert_eq!(
            cause.code(),
            if invalid {
                "event_name_invalid"
            } else {
                "event_name_conflict"
            }
        );
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_observed_typed_identity_is_atomic_and_shared_by_clones() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&calls);
    let mut provider = EventsServiceProvider::new();
    provider.listen_named_reply("*", move |_, _| {
        let seen = Arc::clone(&seen);
        Box::pin(async move {
            seen.fetch_add(1, Ordering::SeqCst);
            Ok(DataValue::Null)
        })
    });
    let events = build(provider).await?;
    // A string payload may use the name without claiming the canonical identity.
    events.dispatch_named(A::NAME, &B(1)).await?;
    calls.store(0, Ordering::SeqCst);
    // 同步barrier阻住首条worker，迫使另一条OS执行线也到达，再竞争首次声明。
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let first = events.clone();
    let gate = Arc::clone(&barrier);
    let a = tokio::spawn(async move {
        let thread = std::thread::current().id();
        gate.wait();
        (thread, first.dispatch(&A(1)).await)
    });
    let b = tokio::spawn(async move {
        let thread = std::thread::current().id();
        barrier.wait();
        (thread, events.dispatch(&Duplicate).await)
    });
    let (a_thread, a) = a.await.map_err(crate::Error::internal)?;
    let (b_thread, b) = b.await.map_err(crate::Error::internal)?;
    assert_ne!(a_thread, b_thread);
    assert_ne!(a.is_ok(), b.is_ok());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let error = if let Err(error) = a {
        error
    } else {
        b.err().ok_or(crate::Error::Unauthorized)?
    };
    assert_eq!(error.code(), "event_name_conflict");
    assert_eq!(
        error.status(),
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    );
    Ok(())
}

struct Object;
impl DataObject for Object {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
#[tokio::test]
async fn response_collection_preserves_null_empty_and_object_identity_without_projection()
-> Result<()> {
    let object: Arc<dyn DataObject> = Arc::new(Object);
    let mut provider = EventsServiceProvider::new();
    for reply in [
        DataValue::Null,
        DataValue::Number(0.into()),
        DataValue::Array(Vec::new()),
        DataValue::Object(Arc::clone(&object)),
        DataValue::Bool(false),
    ] {
        provider.listen_reply::<A, _>(move |_, _| {
            let reply = reply.clone();
            Box::pin(async move { Ok(reply) })
        });
    }
    provider.listen::<A, _>(|_, _| Box::pin(async { Err(crate::Error::Unauthorized) }));
    let events = build(provider).await?;
    let responses = events
        .dispatch(&A(0))
        .await?
        .ok_or(crate::Error::Unauthorized)?;
    assert_eq!(responses.len(), 4);
    assert!(matches!(responses[0], DataValue::Null));
    assert!(matches!(&responses[1],DataValue::Number(n) if n.as_i64()==Some(0)));
    assert!(matches!(&responses[2],DataValue::Array(a) if a.is_empty()));
    let DataValue::Object(actual) = &responses[3] else {
        return Err(crate::Error::Unauthorized);
    };
    assert!(Arc::ptr_eq(actual, &object));
    assert!(actual.json_projection().is_none());
    events.dispatch_direct(&A(0)).await?;
    Ok(())
}

#[tokio::test]
async fn until_returns_each_non_null_value_and_errors_remain_errors() -> Result<()> {
    let object: Arc<dyn DataObject> = Arc::new(Object);
    for reply in [
        DataValue::Bool(false),
        DataValue::Bool(true),
        DataValue::Number(0.into()),
        DataValue::String("0".into()),
        DataValue::Array(Vec::new()),
        DataValue::Object(Arc::clone(&object)),
    ] {
        let mut provider = EventsServiceProvider::new();
        provider.listen::<A, _>(|_, _| Box::pin(async { Ok(()) }));
        let expected = reply.clone();
        provider.listen_reply::<A, _>(move |_, _| {
            let reply = reply.clone();
            Box::pin(async move { Ok(reply) })
        });
        provider.listen::<A, _>(|_, _| Box::pin(async { Err(crate::Error::Unauthorized) }));
        let actual = build(provider).await?.until(&A(0)).await?;
        match (expected, actual) {
            (DataValue::Bool(a), DataValue::Bool(b)) => assert_eq!(a, b),
            (DataValue::Number(a), DataValue::Number(b)) => assert_eq!(a, b),
            (DataValue::String(a), DataValue::String(b)) => assert_eq!(a, b),
            (DataValue::Array(a), DataValue::Array(b)) => assert_eq!(a.len(), b.len()),
            (DataValue::Object(a), DataValue::Object(b)) => assert!(Arc::ptr_eq(&a, &b)),
            _ => return Err(crate::Error::Unauthorized),
        }
    }
    let mut provider = EventsServiceProvider::new();
    provider.listen::<A, _>(|_, _| Box::pin(async { Err(crate::Error::Unauthorized) }));
    assert!(matches!(
        build(provider).await?.until(&A(0)).await,
        Err(crate::Error::Unauthorized)
    ));
    Ok(())
}

#[tokio::test]
async fn propagation_stop_keeps_previous_responses_without_veto() -> Result<()> {
    let mut provider = EventsServiceProvider::new();
    provider.listen_named_reply("phase", |_, _| {
        Box::pin(async { Ok(DataValue::Number(3.into())) })
    });
    provider.listen_named::<A, _>("phase", |_, _| {
        Box::pin(async { Ok(super::EventPropagation::Stop) })
    });
    provider.listen_named_reply("*", |_, _| {
        Box::pin(async { Err(crate::Error::Unauthorized) })
    });
    let events = build(provider).await?;
    assert!(
        matches!(events.dispatch_named("phase",&A(0)).await?.as_deref(),Some([DataValue::Number(n)]) if n.as_i64()==Some(3))
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nested_dispatch_releases_identity_lock_before_calling_user_code() -> Result<()> {
    let dispatcher = Arc::new(std::sync::OnceLock::<std::sync::Weak<Events>>::new());
    let nested = Arc::clone(&dispatcher);
    let mut provider = EventsServiceProvider::new();
    provider.listen_reply::<A, _>(move |_, _| {
        let nested = Arc::clone(&nested);
        Box::pin(async move {
            let current = nested
                .get()
                .and_then(std::sync::Weak::upgrade)
                .ok_or(crate::Error::Unauthorized)?;
            // 不阻塞地检出“带身份锁调用callback”变异，先失败而非同步重锁死整个executor。
            {
                let unlocked = current.observed.try_lock();
                assert!(unlocked.is_ok());
            }
            current.dispatch(&B(2)).await?;
            Ok(DataValue::Bool(true))
        })
    });
    let events = Arc::new(build(provider).await?);
    let weak = Arc::downgrade(&events);
    assert!(dispatcher.set(weak.clone()).is_ok());
    let response = events.until(&A(1)).await?;
    assert!(matches!(response, DataValue::Bool(true)));
    drop(events);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn propagation_stop_until_without_response_is_null() -> Result<()> {
    let mut provider = EventsServiceProvider::new();
    provider.listen_named::<A, _>("phase", |_, _| {
        Box::pin(async { Ok(super::EventPropagation::Stop) })
    });
    provider.listen_named_reply("*", |_, _| {
        Box::pin(async { Err(crate::Error::Unauthorized) })
    });
    assert!(matches!(
        build(provider).await?.until_named("phase", &A(0)).await?,
        DataValue::Null
    ));
    Ok(())
}
