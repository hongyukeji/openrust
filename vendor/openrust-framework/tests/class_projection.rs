use openrust::{
    class_projection,
    container::{
        Bind, BindWhen, ClassDescriptor, ClassProjection, ConstructorParam, Container, Lifetime,
        ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

trait View: Send + Sync {
    fn number(&self) -> u32;
}
struct Item(u32);
impl View for Item {
    fn number(&self) -> u32 {
        self.0
    }
}
struct Other;
impl View for Other {
    fn number(&self) -> u32 {
        99
    }
}

#[tokio::test]
async fn projections_keep_the_selected_key_and_share_concrete_cache_identity() {
    let container = Container::default();
    let calls = Arc::new(AtomicUsize::new(0));
    container
        .register_class(
            ClassDescriptor::concrete(vec![], {
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(Arc::new(Item(7)))
                }
            })
            .unwrap()
            .with_projection(class_projection!(Item => dyn View))
            .with_lifetime(Lifetime::Singleton),
        )
        .unwrap();
    container
        .alias(ServiceKey::typed::<Item>(), "item")
        .unwrap();

    assert!(!container.bound::<dyn View>());
    assert!(matches!(
        container.make_async::<dyn View>().await,
        Err(BindingResolutionError::NotBound { .. })
    ));
    assert!(matches!(
        container.get::<Other>("item"),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 0);

    let (view, concrete) = tokio::join!(
        container.get_async::<dyn View>("item"),
        container.make_async::<Item>()
    );
    let concrete: Arc<dyn View> = concrete.unwrap();
    let view = view.unwrap();
    assert_eq!(view.number(), 7);
    assert!(Arc::ptr_eq(&view, &concrete));
    assert!(Arc::ptr_eq(
        &view,
        &container.get::<dyn View>("item").unwrap()
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(container.get_bindings().is_empty());
    assert!(container.resolved("item").unwrap());

    // The output type of the selected replacement controls which views are available.
    container
        .bind::<Item, _>(|_| Ok(Arc::new(Item(21))))
        .unwrap();
    assert_eq!(
        container
            .get_async::<dyn View>("item")
            .await
            .unwrap()
            .number(),
        21
    );
    container
        .offset_set(ServiceKey::typed::<Item>(), Arc::new(Other))
        .unwrap();
    assert!(matches!(
        container.get::<dyn View>("item"),
        Err(BindingResolutionError::TypeMismatch { .. })
    ));
}

struct Consumer(Arc<dyn View>);

#[tokio::test]
async fn interface_binding_reuses_the_descriptor_and_constructor_injection() {
    let container = Container::default();
    container
        .register_class(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(11))))
                .unwrap()
                .with_projection(class_projection!(Item => dyn View))
                .with_lifetime(Lifetime::Scoped),
        )
        .unwrap();
    container
        .register_class(
            ClassDescriptor::concrete(vec![ConstructorParam::class::<dyn View>("view")], |args| {
                Ok(Arc::new(Consumer(args.service(0)?)))
            })
            .unwrap(),
        )
        .unwrap();
    container
        .register_binding_attributes::<dyn View>(vec![
            BindWhen::new(Bind::implementation::<dyn View, Item>(), |_| Ok(true)).into(),
        ])
        .unwrap();

    let first = container
        .scope(async {
            let consumer = container.make_async::<Consumer>().await.unwrap();
            let direct: Arc<dyn View> = container.make::<Item>().unwrap();
            assert!(Arc::ptr_eq(&consumer.0, &direct));
            consumer.0.clone()
        })
        .await;
    let next = container
        .scope(container.make_async::<dyn View>())
        .await
        .unwrap();
    assert_eq!(next.number(), 11);
    assert!(!Arc::ptr_eq(&first, &next));
}

#[test]
fn projection_metadata_rejects_mismatched_sources_and_duplicate_or_identity_targets() {
    for projections in [
        vec![class_projection!(Other => dyn View)],
        vec![
            class_projection!(Item => dyn View),
            class_projection!(Item => dyn View),
        ],
        vec![class_projection!(Item => Item)],
    ] {
        let descriptor = projections.into_iter().fold(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(1)))).unwrap(),
            ClassDescriptor::with_projection,
        );
        assert!(matches!(
            Container::default().register_class(descriptor),
            Err(BindingResolutionError::InvalidProjection { .. })
        ));
    }
}

#[tokio::test]
async fn invalid_projection_rejects_object_replacement_and_missing_metadata_can_be_fixed() {
    let container = Container::default();
    container
        .register_class(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(1))))
                .unwrap()
                .with_projection(ClassProjection::__from_coercion::<Item, dyn View>(|_| {
                    Arc::new(Item(2))
                })),
        )
        .unwrap();
    assert!(matches!(
        container
            .get_async::<dyn View>(ServiceKey::typed::<Item>())
            .await,
        Err(BindingResolutionError::InvalidProjection { .. })
    ));

    let missing = Container::default();
    missing
        .register_binding_attributes::<dyn View>(vec![
            BindWhen::new(Bind::implementation::<dyn View, Item>(), |_| Ok(true)).into(),
        ])
        .unwrap();
    assert!(matches!(
        missing.make::<dyn View>(),
        Err(BindingResolutionError::InvalidProjection { .. })
    ));
    missing
        .register_class(
            ClassDescriptor::concrete(vec![], |_| Ok(Arc::new(Item(3))))
                .unwrap()
                .with_projection(class_projection!(Item => dyn View)),
        )
        .unwrap();
    assert_eq!(missing.make_async::<dyn View>().await.unwrap().number(), 3);
}
