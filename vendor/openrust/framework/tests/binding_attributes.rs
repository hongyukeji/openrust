use openrust::{
    container::{Bind, BindWhen, ClassDescriptor, Container, EnvironmentQuery, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc, Barrier,
    atomic::{AtomicBool, Ordering},
};

trait Reporter: Send + Sync {
    fn label(&self) -> &'static str;
}
struct Local;
struct Fallback;
impl Reporter for Local {
    fn label(&self) -> &'static str {
        "local"
    }
}
impl Reporter for Fallback {
    fn label(&self) -> &'static str {
        "fallback"
    }
}
fn register(container: &Container) {
    container
        .registerClass(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Local))).unwrap())
        .unwrap();
    container
        .registerClass(ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Fallback))).unwrap())
        .unwrap();
}
fn local() -> Bind {
    Bind::new::<dyn Reporter, Local>(|value| value)
}
fn fallback() -> Bind {
    Bind::new::<dyn Reporter, Fallback>(|value| value)
}

#[tokio::test]
async fn environment_rules_use_order_and_fallback_and_are_rechecked_after_flush() {
    let container = Container::default();
    register(&container);
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            fallback().into(),
            local().forEnvironments(["local"]).into(),
        ])
        .unwrap();
    assert!(matches!(
        container.make::<dyn Reporter>(),
        Err(BindingResolutionError::NotBound { .. })
    ));
    container.resolveEnvironmentUsing(Some(Arc::new(|query| {
        assert_eq!(query, EnvironmentQuery::Names(&["local"]));
        Ok(true.into())
    })));
    assert_eq!(container.make::<dyn Reporter>().unwrap().label(), "local");
    assert!(container.bound::<dyn Reporter>());
    assert_eq!(
        container.makeAsync::<dyn Reporter>().await.unwrap().label(),
        "local"
    );
    container.resolveEnvironmentUsing(Some(Arc::new(|_| Ok(false.into()))));
    assert_eq!(
        container.makeAsync::<dyn Reporter>().await.unwrap().label(),
        "local"
    );

    container.flush();
    assert_eq!(
        container.makeAsync::<dyn Reporter>().await.unwrap().label(),
        "fallback"
    );
}

#[test]
fn conditional_misses_retry_and_explicit_bindings_keep_precedence() {
    let container = Container::default();
    register(&container);
    let enabled = Arc::new(AtomicBool::new(false));
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(local(), {
                let enabled = enabled.clone();
                move |_| Ok(enabled.load(Ordering::Acquire))
            })
            .into(),
        ])
        .unwrap();
    assert!(container.make::<dyn Reporter>().is_err());
    enabled.store(true, Ordering::Release);
    assert_eq!(container.make::<dyn Reporter>().unwrap().label(), "local");

    container.flush();
    container
        .bind::<dyn Reporter, _>(|_| Ok(Arc::new(Fallback)))
        .unwrap();
    assert_eq!(
        container.make::<dyn Reporter>().unwrap().label(),
        "fallback"
    );
}

#[test]
fn a_concurrent_explicit_binding_wins_over_a_running_attribute_condition() {
    let container = Arc::new(Container::default());
    register(&container);
    let ready = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(local(), {
                let (ready, release) = (ready.clone(), release.clone());
                move |_| {
                    ready.wait();
                    release.wait();
                    Ok(true)
                }
            })
            .into(),
        ])
        .unwrap();
    let worker = std::thread::spawn({
        let container = container.clone();
        move || container.make::<dyn Reporter>().unwrap().label()
    });
    ready.wait();
    container
        .bind::<dyn Reporter, _>(|_| Ok(Arc::new(Fallback)))
        .unwrap();
    release.wait();
    assert_eq!(worker.join().unwrap(), "fallback");
}

#[tokio::test]
async fn async_binding_builds_async_concretes_and_retains_both_factory_modes() {
    let container = Container::default();
    container
        .registerClass(
            ClassDescriptor::concreteAsync(vec![], |_| Box::pin(async { Ok(Arc::new(Local)) }))
                .unwrap(),
        )
        .unwrap();
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(local(), |_| Ok(true)).into(),
        ])
        .unwrap();
    assert!(matches!(
        container.make::<dyn Reporter>(),
        Err(BindingResolutionError::AsyncResolutionRequired { .. })
    ));
    assert_eq!(
        container.makeAsync::<dyn Reporter>().await.unwrap().label(),
        "local"
    );
    assert!(
        container
            .resolved(ServiceKey::typed::<dyn Reporter>())
            .unwrap()
    );
    let bindings = container.getBindings();
    let binding = &bindings
        .get(&ServiceKey::typed::<dyn Reporter>())
        .unwrap()
        .concrete;
    assert_eq!(
        binding
            .callAsync::<dyn Reporter>(&container, Default::default())
            .await
            .unwrap()
            .label(),
        "local"
    );
}

#[test]
fn a_matching_condition_can_replace_its_own_provisional_binding() {
    let container = Container::default();
    register(&container);
    container
        .registerBindingAttributes::<dyn Reporter>(vec![
            BindWhen::new(local(), |container| {
                container.bind::<dyn Reporter, _>(|_| Ok(Arc::new(Fallback)))?;
                Ok(true)
            })
            .into(),
        ])
        .unwrap();
    assert_eq!(container.make::<dyn Reporter>().unwrap().label(), "local");
}
