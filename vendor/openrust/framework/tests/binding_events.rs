use openrust::{
    class_projection,
    container::{Bind, BindWhen, ClassDescriptor, ConstructorParam, Container, ServiceKey},
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{Arc, Mutex};

trait View: Send + Sync {
    fn number(&self) -> u32;
}
struct Item(u32);
impl View for Item {
    fn number(&self) -> u32 {
        self.0
    }
}
struct Replacement;
impl View for Replacement {
    fn number(&self) -> u32 {
        99
    }
}

fn descriptor(parameters: Vec<ConstructorParam>) -> ClassDescriptor {
    ClassDescriptor::concrete(parameters, |args| {
        let number = if args.is_empty() {
            7
        } else {
            *args.service::<u32>(0)?
        };
        Ok(Arc::new(Item(number)))
    })
    .unwrap()
    .withProjection(class_projection!(Item => dyn View))
}

fn declared(container: &Container, bind: Bind) {
    container
        .registerBindingAttributes::<dyn View>(vec![BindWhen::new(bind, |_| Ok(true)).into()])
        .unwrap();
}

#[tokio::test]
async fn direct_class_events_match_declared_interfaces_and_expose_the_same_arc() {
    let container = Container::default();
    container.registerClass(descriptor(vec![])).unwrap();
    let item = ServiceKey::typed::<Item>();
    let view = ServiceKey::typed::<dyn View>();
    container.alias(item.clone(), "item").unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    container
        .beforeResolving(Some(view.clone()), {
            let observed = observed.clone();
            let item = item.clone();
            move |event, _| {
                assert_eq!(event.key(), &item);
                assert!(event.value::<dyn View>().is_none());
                observed.lock().unwrap().push("before");
                Ok(())
            }
        })
        .unwrap();
    let output = Arc::new(Mutex::new(None));
    container
        .resolving(Some(view.clone()), {
            let observed = observed.clone();
            let output = output.clone();
            move |event, _| {
                let concrete: Arc<dyn View> = event.value::<Item>().unwrap();
                let projected = event.value::<dyn View>().unwrap();
                assert!(Arc::ptr_eq(&concrete, &projected));
                *output.lock().unwrap() = Some(projected);
                observed.lock().unwrap().push("resolving");
                Ok(())
            }
        })
        .unwrap();
    container
        .afterResolving(Some(view), {
            let observed = observed.clone();
            move |_, _| {
                observed.lock().unwrap().push("after");
                Ok(())
            }
        })
        .unwrap();
    let view = container.getAsync::<dyn View>("item").await.unwrap();
    assert!(Arc::ptr_eq(&view, output.lock().unwrap().as_ref().unwrap()));
    assert_eq!(*observed.lock().unwrap(), ["before", "resolving", "after"]);
}

#[tokio::test]
async fn binding_forwarding_suppresses_only_the_inner_concrete_events() {
    for (bind, asynchronous) in [
        (Bind::implementation::<dyn View, Item>(), true),
        (Bind::new::<dyn View, Item>(|value| value), false),
    ] {
        let container = Container::default();
        container
            .registerClass(descriptor(vec![ConstructorParam::class::<u32>("number")]))
            .unwrap();
        declared(&container, bind);
        let abstract_key = ServiceKey::typed::<dyn View>();
        let item = ServiceKey::typed::<Item>();
        let dependency = ServiceKey::typed::<u32>();
        let observed = Arc::new(Mutex::new(Vec::new()));

        for (label, phase) in [("before", 0), ("resolving", 1), ("after", 2)] {
            let callback = {
                let (observed, abstract_key, item, dependency) = (
                    observed.clone(),
                    abstract_key.clone(),
                    item.clone(),
                    dependency.clone(),
                );
                move |event: &openrust::container::ResolutionEvent, _: &Container| {
                    assert_ne!(event.key(), &item);
                    if event.key() == &abstract_key || event.key() == &dependency {
                        observed.lock().unwrap().push((event.key().clone(), label));
                    }
                    Ok(())
                }
            };
            match phase {
                0 => container.beforeResolving(None, callback),
                1 => container.resolving(None, callback),
                _ => container.afterResolving(None, callback),
            }
            .unwrap();
        }
        container
            .resolving(Some(item), {
                let abstract_key = abstract_key.clone();
                move |event, _| {
                    assert_eq!(event.key(), &abstract_key);
                    let value: Arc<dyn View> = event.value::<Item>().unwrap();
                    assert!(Arc::ptr_eq(&value, &event.value::<dyn View>().unwrap()));
                    Ok(())
                }
            })
            .unwrap();

        assert!(matches!(
            container.makeAsync::<dyn View>().await,
            Err(BindingResolutionError::NotBound { .. })
        ));
        observed.lock().unwrap().clear();
        container.bind::<u32, _>(|_| Ok(Arc::new(13))).unwrap();
        let value = if asynchronous {
            container.makeAsync::<dyn View>().await
        } else {
            container.make::<dyn View>()
        }
        .unwrap();
        assert_eq!(value.number(), 13);
        assert_eq!(
            *observed.lock().unwrap(),
            [
                (abstract_key.clone(), "before"),
                (dependency.clone(), "before"),
                (dependency.clone(), "resolving"),
                (dependency, "after"),
                (abstract_key.clone(), "resolving"),
                (abstract_key, "after"),
            ]
        );
    }
}

#[tokio::test]
async fn replacing_forwarded_object_discards_its_old_concrete_view() {
    let container = Container::default();
    container.registerClass(descriptor(vec![])).unwrap();
    declared(&container, Bind::implementation::<dyn View, Item>());
    let view = ServiceKey::typed::<dyn View>();
    container
        .extend::<dyn View, _>(view.clone(), |_, _| Ok(Arc::new(Replacement)))
        .unwrap();
    container
        .resolving(Some(ServiceKey::typed::<Item>()), |_, _| {
            panic!("old concrete callback must not fire for a replaced object");
        })
        .unwrap();
    container
        .afterResolving(Some(view), |event, _| {
            assert!(event.value::<Item>().is_none());
            assert_eq!(event.value::<dyn View>().unwrap().number(), 99);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        container.makeAsync::<dyn View>().await.unwrap().number(),
        99
    );
}
