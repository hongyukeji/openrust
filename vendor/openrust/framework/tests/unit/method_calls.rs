use openrust::{
    container::{
        Callback, ConstructorParam, Container, MethodCall, ParameterKey, ParameterOverrides,
        ParameterValue, ServiceKey, StaticCall,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::{
    cell::Cell,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
struct Receiver;
struct Dependency(&'static str);
fn setup() -> Container {
    let c = Container::default();
    c.instance(Arc::new(Receiver)).unwrap();
    c.instance(Arc::new(Dependency("root"))).unwrap();
    c.when::<Receiver>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency("receiver"))))
        .unwrap();
    c
}
#[tokio::test]
async fn object_methods_establish_class_context_but_class_at_method_preserves_parent_context() {
    let c = setup();
    c.registerMethod::<Receiver, _, _>(
        "read",
        vec![ConstructorParam::class::<Dependency>("dep")],
        |receiver, args, c| {
            Box::pin(async move {
                let injected = args.resolved.service::<Dependency>(0)?.0;
                tokio::task::yield_now().await;
                Ok((receiver, injected, c.makeAsync::<Dependency>().await?.0))
            })
        },
    )
    .unwrap();
    let supplied = Arc::new(Receiver);
    let (found, injected, inside): (Arc<Receiver>, _, _) = c
        .call(
            MethodCall::on(supplied.clone(), "READ"),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&found, &supplied));
    assert_eq!((injected, inside), ("receiver", "receiver"));
    let (_, injected, _): (Arc<Receiver>, &str, &str) = c
        .call(
            MethodCall::<Receiver>::class("read"),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert_eq!(injected, "root");
    assert_eq!(c.makeAsync::<Dependency>().await.unwrap().0, "root");
    assert!(
        c.registerMethod::<Receiver, (), _>("READ", vec![], |_, _, _| Box::pin(async { Ok(()) }))
            .is_err()
    );
}
#[tokio::test]
async fn receiver_construction_precedes_binding_lookup_and_binding_bypasses_original_signature() {
    let c = Container::default();
    c.registerMethod::<Receiver, u32, _>(
        "run",
        vec![ConstructorParam::scalar::<String>("required")],
        |_, _, _| panic!("must skip original"),
    )
    .unwrap();
    c.bind::<Receiver, _>(|c| {
        c.bindMethod::<Receiver, u32, _>("run", |_, _| Box::pin(async { Ok(42) }))?;
        Ok(Arc::new(Receiver))
    })
    .unwrap();
    let mut input = ParameterOverrides::new();
    input.insert(ParameterKey::name("required"), ParameterValue::null());
    let result: u32 = c
        .call(MethodCall::<Receiver>::class("run"), input)
        .await
        .unwrap();
    assert_eq!(result, 42);
    assert!(c.hasMethodBinding::<Receiver>("run"));
    assert!(!c.hasMethodBinding::<Receiver>("RUN"));
    let error = c
        .call::<u32, _>(
            MethodCall::<Receiver>::class("RUN"),
            ParameterOverrides::new(),
        )
        .await;
    assert!(matches!(
        error,
        Err(BindingResolutionError::UnresolvableParameter { .. })
    ));
}
#[tokio::test]
async fn direct_binding_keeps_parent_context_while_normal_method_establishes_class_context() {
    let c = setup();
    c.registerMethod::<Receiver, &str, _>("read", vec![], |_, _, _| {
        Box::pin(async { Ok("unused") })
    })
    .unwrap();
    c.bindMethod::<Receiver, _, _>("read", |_, c| {
        Box::pin(async move { Ok(c.makeAsync::<Dependency>().await?.0) })
    })
    .unwrap();
    assert_eq!(
        c.callMethodBinding::<Receiver, &str>("read", Arc::new(Receiver))
            .await
            .unwrap(),
        "root"
    );
    let read: &str = c
        .call(
            MethodCall::on(Arc::new(Receiver), "read"),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert_eq!(read, "receiver");
}
#[tokio::test]
async fn default_class_argument_uses_bound_not_contextual_and_class_alias_key_is_not_normalized() {
    let c = Container::default();
    c.instance(Arc::new(Receiver)).unwrap();
    c.when::<Receiver>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency("context"))))
        .unwrap();
    c.registerMethod::<Receiver, &str, _>(
        "read",
        vec![
            ConstructorParam::class::<Dependency>("dep")
                .withDefault(|| Ok(ParameterValue::new(Arc::new(Dependency("default"))))),
        ],
        |_, args, _| Box::pin(async move { Ok(args.resolved.service::<Dependency>(0)?.0) }),
    )
    .unwrap();
    let read: &str = c
        .call(
            MethodCall::on(Arc::new(Receiver), "read"),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert_eq!(read, "default");
    c.instance(Arc::new(Dependency("root"))).unwrap();
    let read: &str = c
        .call(
            MethodCall::on(Arc::new(Receiver), "read"),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert_eq!(read, "context");
    let mut input = ParameterOverrides::new();
    input.insert(
        ParameterKey::class::<Dependency>(),
        ParameterValue::new(Arc::new(Dependency("explicit"))),
    );
    let read: &str = c
        .call(MethodCall::on(Arc::new(Receiver), "read"), input)
        .await
        .unwrap();
    assert_eq!(read, "explicit");
}
#[tokio::test]
async fn invokable_requires_declaration_before_construction_and_falsy_default_names_use_invoke() {
    let c = Container::default();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    c.bind::<Receiver, _>(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(Receiver))
    })
    .unwrap();
    c.bindMethod::<Receiver, u32, _>("__invoke", |_, _| Box::pin(async { Ok(7) }))
        .unwrap();
    assert!(matches!(
        c.call::<u32, _>(
            MethodCall::<Receiver>::invokable(),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::InvalidMethod { .. })
    ));
    assert_eq!(count.load(Ordering::SeqCst), 0);
    c.registerMethod::<Receiver, u32, _>("__invoke", vec![], |_, _, _| Box::pin(async { Ok(1) }))
        .unwrap();
    for target in [
        MethodCall::<Receiver>::invokable(),
        MethodCall::defaultMethod(""),
        MethodCall::defaultMethod("0"),
    ] {
        assert_eq!(
            c.call::<u32, _>(target, ParameterOverrides::new())
                .await
                .unwrap(),
            7
        );
    }
    assert_eq!(count.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn qualified_static_ignores_binding_array_consults_it_and_wrong_contracts_fail() {
    let c = Container::default();
    c.registerStaticMethod::<Receiver, _, _>("read", vec![], |_, _| {
        Box::pin(async { Ok(Cell::new(1u32)) })
    })
    .unwrap();
    c.bindStaticMethod::<Receiver, _, _>("read", |key, _| {
        Box::pin(async move {
            assert_eq!(key, ServiceKey::typed::<Receiver>());
            Ok(Cell::new(2u32))
        })
    })
    .unwrap();
    assert_eq!(
        c.call::<Cell<u32>, _>(
            StaticCall::<Receiver>::qualified("read"),
            ParameterOverrides::new()
        )
        .await
        .unwrap()
        .get(),
        1
    );
    assert_eq!(
        c.call::<Cell<u32>, _>(
            StaticCall::<Receiver>::array("read"),
            ParameterOverrides::new()
        )
        .await
        .unwrap()
        .get(),
        2
    );
    assert_eq!(
        c.callStaticMethodBinding::<Receiver, Cell<u32>>("read")
            .await
            .unwrap()
            .get(),
        2
    );
    assert!(matches!(
        c.call::<String, _>(
            StaticCall::<Receiver>::qualified("read"),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::MethodContractMismatch { .. })
    ));
    assert!(matches!(
        c.call::<Cell<u32>, _>(
            MethodCall::on(Arc::new(Receiver), "read"),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::MethodContractMismatch { .. })
    ));
    assert!(!c.resolved(ServiceKey::typed::<Receiver>()).unwrap());
    c.registerMethod::<Receiver, (), _>("instance", vec![], |_, _, _| Box::pin(async { Ok(()) }))
        .unwrap();
    assert!(matches!(
        c.call::<(), _>(
            StaticCall::<Receiver>::qualified("instance"),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::MethodContractMismatch { .. })
    ));
    assert!(matches!(
        c.callMethodBinding::<Receiver, ()>("absent", Arc::new(Receiver))
            .await,
        Err(BindingResolutionError::MethodBindingNotFound { .. })
    ));
}
#[tokio::test]
async fn object_can_call_static_declaration_and_retains_receiver_until_callback_finishes() {
    struct Owned(Arc<AtomicUsize>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Container::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let seen = drops.clone();
    c.registerStaticMethod::<Owned, (), _>("run", vec![], move |_, _| {
        let seen = seen.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            assert_eq!(seen.load(Ordering::SeqCst), 0);
            Ok(())
        })
    })
    .unwrap();
    c.call::<(), _>(
        MethodCall::on(Arc::new(Owned(drops.clone())), "run"),
        ParameterOverrides::new(),
    )
    .await
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn method_inputs_remain_owned_when_binding_skips_arguments_and_callback_discards_receiver() {
    struct Held(Arc<AtomicUsize>);
    impl Drop for Held {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let c = Container::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let seen = drops.clone();
    c.bindMethod::<Held, (), _>("run", move |_, _| {
        let seen = seen.clone();
        Box::pin(async move {
            tokio::task::yield_now().await;
            assert_eq!(seen.load(Ordering::SeqCst), 0);
            Ok(())
        })
    })
    .unwrap();
    let mut input = ParameterOverrides::new();
    input.insert(
        ParameterKey::name("unused"),
        ParameterValue::new(Held(drops.clone())),
    );
    c.call::<(), _>(MethodCall::on(Arc::new(Held(drops.clone())), "run"), input)
        .await
        .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 2);
    let seen = drops.clone();
    let signature = [];
    c.call(
        Callback::new(&signature, move |_| async move {
            tokio::task::yield_now().await;
            assert_eq!(seen.load(Ordering::SeqCst), 2);
            Ok(())
        }),
        {
            let mut input = ParameterOverrides::new();
            input.insert(
                ParameterKey::Index(0),
                ParameterValue::new(Held(drops.clone())),
            );
            input
        },
    )
    .await
    .unwrap();
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn recursive_calls_do_not_create_resolution_cycles_or_move_existing_class_to_stack_top() {
    struct Other;
    let c = setup();
    c.when::<Other>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency("other"))))
        .unwrap();
    c.registerMethod::<Receiver, &str, _>("leaf", vec![], |_, _, c| {
        Box::pin(async move { Ok(c.makeAsync::<Dependency>().await?.0) })
    })
    .unwrap();
    c.registerMethod::<Other, &str, _>("middle", vec![], |_, _, c| {
        Box::pin(async move {
            c.call(
                MethodCall::on(Arc::new(Receiver), "leaf"),
                ParameterOverrides::new(),
            )
            .await
        })
    })
    .unwrap();
    c.registerMethod::<Receiver, &str, _>("outer", vec![], |_, _, c| {
        Box::pin(async move {
            c.call(
                MethodCall::on(Arc::new(Other), "middle"),
                ParameterOverrides::new(),
            )
            .await
        })
    })
    .unwrap();
    assert_eq!(
        c.call::<&str, _>(
            MethodCall::on(Arc::new(Receiver), "outer"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        "other"
    );
}
#[tokio::test]
async fn concurrent_calls_restore_own_context_after_each_await_and_anonymous_call_does_not_inherit_foreign_context()
 {
    struct Other;
    let c = setup();
    c.when::<Other>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency("other"))))
        .unwrap();
    c.registerMethod::<Receiver, &str, _>("read", vec![], |_, _, c| {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(c.makeAsync::<Dependency>().await?.0)
        })
    })
    .unwrap();
    c.registerMethod::<Other, &str, _>("read", vec![], |_, _, c| {
        Box::pin(async move {
            tokio::task::yield_now().await;
            Ok(c.makeAsync::<Dependency>().await?.0)
        })
    })
    .unwrap();
    let root = c.call(
        Callback::new(&[], |_| async {
            tokio::task::yield_now().await;
            Ok(c.makeAsync::<Dependency>().await?.0)
        }),
        ParameterOverrides::new(),
    );
    let (a, b, root) = tokio::join!(
        c.call::<&str, _>(
            MethodCall::on(Arc::new(Receiver), "read"),
            ParameterOverrides::new()
        ),
        c.call::<&str, _>(
            MethodCall::on(Arc::new(Other), "read"),
            ParameterOverrides::new()
        ),
        root
    );
    assert_eq!(
        (a.unwrap(), b.unwrap(), root.unwrap()),
        ("receiver", "other", "root")
    );
}
#[tokio::test]
async fn binding_replacement_preserves_running_callback_and_allows_reentry() {
    let c = setup();
    let wait = Arc::new(tokio::sync::Notify::new());
    let entered = Arc::new(AtomicUsize::new(0));
    let gate = wait.clone();
    let seen = entered.clone();
    c.bindMethod::<Receiver, u32, _>("run", move |_, c| {
        let gate = gate.clone();
        let seen = seen.clone();
        Box::pin(async move {
            c.bindMethod::<Receiver, u32, _>("next", |_, _| Box::pin(async { Ok(3) }))?;
            seen.fetch_add(1, Ordering::SeqCst);
            gate.notified().await;
            Ok(1)
        })
    })
    .unwrap();
    let mut running = Box::pin(c.call::<u32, _>(
        MethodCall::<Receiver>::class("run"),
        ParameterOverrides::new(),
    ));
    std::future::poll_fn(|cx| {
        assert!(running.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(entered.load(Ordering::SeqCst), 1);
    c.bindMethod::<Receiver, u32, _>("run", |_, _| Box::pin(async { Ok(2) }))
        .unwrap();
    wait.notify_one();
    assert_eq!(running.await.unwrap(), 1);
    assert_eq!(
        c.callMethodBinding::<Receiver, u32>("run", Arc::new(Receiver))
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        c.callMethodBinding::<Receiver, u32>("next", Arc::new(Receiver))
            .await
            .unwrap(),
        3
    );
}
#[tokio::test]
async fn dropped_method_future_restores_consumer_during_cleanup_and_closed_scope_is_rejected() {
    struct Cleanup {
        c: std::sync::Weak<Container>,
        seen: Arc<Mutex<Vec<&'static str>>>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            self.seen
                .lock()
                .unwrap()
                .push(match self.c.upgrade().unwrap().make::<Dependency>() {
                    Ok(value) => value.0,
                    Err(BindingResolutionError::ScopeClosed) => "closed",
                    Err(error) => panic!("{error}"),
                });
        }
    }
    let c = Arc::new(setup());
    let seen = Arc::new(Mutex::new(vec![]));
    let saved = seen.clone();
    let captured = Arc::downgrade(&c);
    c.registerMethod::<Receiver, (), _>("wait", vec![], move |_, _, _| {
        let held = Cleanup {
            c: captured.clone(),
            seen: saved.clone(),
        };
        Box::pin(async move {
            let _held = held;
            std::future::pending().await
        })
    })
    .unwrap();
    let mut future = Box::pin(c.call::<(), _>(
        MethodCall::on(Arc::new(Receiver), "wait"),
        ParameterOverrides::new(),
    ));
    std::future::poll_fn(|cx| {
        assert!(future.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(future);
    assert_eq!(*seen.lock().unwrap(), ["receiver"]);
    assert_eq!(c.make::<Dependency>().unwrap().0, "root");
    let escaped = Mutex::new(None);
    c.scope(async {
        let mut f = Box::pin(c.call::<(), _>(
            MethodCall::on(Arc::new(Receiver), "wait"),
            ParameterOverrides::new(),
        ));
        std::future::poll_fn(|cx| {
            assert!(f.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        *escaped.lock().unwrap() = Some(f);
    })
    .await;
    let escaped = escaped.lock().unwrap().take().unwrap();
    assert!(matches!(
        escaped.await,
        Err(BindingResolutionError::ScopeClosed)
    ));
}
#[tokio::test]
async fn class_variadics_accept_computed_lists_but_raw_constructor_service_arrays_fail() {
    let c = setup();
    c.when::<Receiver>()
        .unwrap()
        .needs::<Dependency>()
        .giveServices(vec![ServiceKey::typed::<Dependency>()])
        .unwrap();
    c.registerMethod::<Receiver, usize, _>(
        "many",
        vec![ConstructorParam::class::<Dependency>("items").variadic()],
        |_, args, _| Box::pin(async move { Ok(args.resolved.services::<Dependency>(0)?.len()) }),
    )
    .unwrap();
    assert!(matches!(
        c.call::<usize, _>(
            MethodCall::on(Arc::new(Receiver), "many"),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    c.when::<Receiver>()
        .unwrap()
        .needs::<Dependency>()
        .give(|_| Ok(Arc::new(Dependency("single"))))
        .unwrap();
    assert_eq!(
        c.call::<usize, _>(
            MethodCall::on(Arc::new(Receiver), "many"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        1
    );
    let c = setup();
    c.when::<Receiver>()
        .unwrap()
        .needs::<Vec<Arc<Dependency>>>()
        .give(|_| {
            Ok(Arc::new(vec![
                Arc::new(Dependency("a")),
                Arc::new(Dependency("b")),
            ]))
        })
        .unwrap();
    c.registerMethod::<Receiver, usize, _>(
        "many",
        vec![ConstructorParam::class::<Dependency>("items").variadic()],
        |_, args, _| Box::pin(async move { Ok(args.resolved.services::<Dependency>(0)?.len()) }),
    )
    .unwrap();
    // The direct Dependency context takes precedence over the Vec adapter.
    assert_eq!(
        c.call::<usize, _>(
            MethodCall::on(Arc::new(Receiver), "many"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn computed_variadic_collections_are_spread_and_errors_allow_explicit_retry() {
    let c = Container::default();
    c.instance(Arc::new(Receiver)).unwrap();
    c.when::<Receiver>()
        .unwrap()
        .needs::<Vec<Arc<Dependency>>>()
        .give(|_| {
            Ok(Arc::new(vec![
                Arc::new(Dependency("a")),
                Arc::new(Dependency("b")),
            ]))
        })
        .unwrap();
    c.registerMethod::<Receiver, Vec<&str>, _>(
        "many",
        vec![ConstructorParam::class::<Dependency>("items").variadic()],
        |_, args, _| {
            Box::pin(async move {
                Ok(args
                    .resolved
                    .services::<Dependency>(0)?
                    .iter()
                    .map(|d| d.0)
                    .collect())
            })
        },
    )
    .unwrap();
    assert_eq!(
        c.call::<Vec<&str>, _>(
            MethodCall::on(Arc::new(Receiver), "many"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        ["a", "b"]
    );
    let n = Arc::new(AtomicUsize::new(0));
    let seen = n.clone();
    c.bindMethod::<Receiver, u32, _>("retry", move |_, _| {
        let n = seen.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            if n == 0 {
                Err(BindingResolutionError::FactoryFailed {
                    service: "retry",
                    source: std::io::Error::other("private failure").into(),
                })
            } else {
                Ok(9)
            }
        })
    })
    .unwrap();
    assert!(
        c.call::<u32, _>(
            MethodCall::<Receiver>::class("retry"),
            ParameterOverrides::new()
        )
        .await
        .is_err()
    );
    assert_eq!(
        c.call::<u32, _>(
            MethodCall::<Receiver>::class("retry"),
            ParameterOverrides::new()
        )
        .await
        .unwrap(),
        9
    );
}

#[tokio::test]
async fn panicking_method_leaves_no_class_context_and_binding_can_be_replaced() {
    let c = setup();
    c.registerMethod::<Receiver, (), _>("panic", vec![], |_, _, c| {
        Box::pin(async move {
            assert_eq!(c.makeAsync::<Dependency>().await?.0, "receiver");
            panic!("test method panic");
        })
    })
    .unwrap();
    let mut future = Box::pin(c.call::<(), _>(
        MethodCall::on(Arc::new(Receiver), "panic"),
        ParameterOverrides::new(),
    ));
    std::future::poll_fn(|cx| {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| future.as_mut().poll(cx)))
                .is_err()
        );
        Poll::Ready(())
    })
    .await;
    drop(future);
    assert_eq!(c.makeAsync::<Dependency>().await.unwrap().0, "root");
    c.bindMethod::<Receiver, (), _>("panic", |_, _| Box::pin(async { Ok(()) }))
        .unwrap();
    c.call::<(), _>(
        MethodCall::on(Arc::new(Receiver), "panic"),
        ParameterOverrides::new(),
    )
    .await
    .unwrap();
}
