use super::*;
use std::sync::atomic::AtomicUsize;

type Trace = Arc<Mutex<Vec<&'static str>>>;

struct Pick(Trace);
struct Observe(Trace);
struct ClassTag;
struct Decorated(String);

impl ContextualAttribute for Pick {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            lock(&self.0).push("内建选值");
            Ok(AttributeValue::One(ParameterValue::new(String::from(
                "内建",
            ))))
        })
    }

    fn after(
        self: Arc<Self>,
        value: AttributeValue,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            assert_eq!(
                resolver.currently_resolving(),
                Some(&ServiceKey::typed::<Decorated>())
            );
            let AttributeValue::One(value) = value else {
                return Err(container_error("test_attribute_shape", "应接收单个值"));
            };
            assert_eq!(&*value.downcast::<String>()?, "上下文");
            lock(&self.0).push("属性内建后置");
            Ok(())
        })
    }
}

impl ContextualAttribute for Observe {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async move {
            lock(&self.0).push("第二属性选值");
            Ok(AttributeValue::One(ParameterValue::null()))
        })
    }

    fn after(self: Arc<Self>, _: AttributeValue, _: Resolver) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            lock(&self.0).push("第二属性后置");
            Ok(())
        })
    }
}

#[tokio::test]
async fn attribute_selection_uses_first_handler_and_preserves_constructor_order() -> Result<()> {
    let trace = Trace::default();
    let pick = Arc::clone(&trace);
    let observe = Arc::clone(&trace);
    let new = Arc::clone(&trace);
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Decorated>(
            "tests::Decorated",
            vec![
                ConstructorParam::scalar::<String>("label")
                    .with_attribute(ClassAttribute::contextual(move || {
                        Ok(Pick(Arc::clone(&pick)))
                    }))
                    .with_attribute(ClassAttribute::contextual(move || {
                        Ok(Observe(Arc::clone(&observe)))
                    })),
            ],
            move |args| {
                lock(&new).push("构造");
                Ok(Arc::new(Decorated(
                    args.value(0)?.downcast::<String>()?.as_ref().clone(),
                )))
            },
        )
        .with_attribute(ClassAttribute::marker(|| Ok(ClassTag))),
    ])?;
    builder
        .when(ServiceKey::typed::<Decorated>())?
        .needs_parameter("label")
        .give_value(ParameterValue::new(String::from("上下文")))?;
    builder.when_has_attribute::<Pick>(|attribute, param, _| {
        Box::pin(async move {
            assert_eq!(param.name(), "label");
            lock(&attribute.0).push("登记选值");
            Ok(AttributeValue::One(ParameterValue::null()))
        })
    })?;
    builder.after_resolving_attribute::<Pick>(|event, _| {
        Box::pin(async move {
            lock(&event.attribute().downcast::<Pick>()?.0).push("属性登记后置");
            Ok(())
        })
    })?;
    let class = Arc::clone(&trace);
    builder.after_resolving_attribute::<Decorated>(move |event, resolver| {
        let trace = Arc::clone(&class);
        Box::pin(async move {
            event.attribute().downcast::<ClassTag>()?;
            assert!(event.service::<Decorated>()?.is_some());
            assert!(resolver.currently_resolving().is_none());
            lock(&trace).push("类后置");
            Ok(())
        })
    })?;
    let resolved = Arc::clone(&trace);
    builder.resolving(Some(ServiceKey::typed::<Decorated>()), move |_, _| {
        let trace = Arc::clone(&resolved);
        Box::pin(async move {
            lock(&trace).push("解析");
            Ok(())
        })
    })?;
    let container = builder.build();
    let extended = Arc::clone(&trace);
    container
        .extend::<Decorated, _>(move |value, _| {
            let trace = Arc::clone(&extended);
            Box::pin(async move {
                lock(&trace).push("装饰");
                Ok(value)
            })
        })
        .await?;
    assert_eq!(container.resolve::<Decorated>().await?.0, "上下文");
    assert_eq!(
        *lock(&trace),
        [
            "登记选值",
            "属性内建后置",
            "属性登记后置",
            "第二属性后置",
            "构造",
            "类后置",
            "装饰",
            "解析"
        ]
    );
    lock(&trace).clear();
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("label"),
        ParameterValue::new(String::from("覆盖")),
    );
    assert_eq!(container.resolve_with::<Decorated>(args).await?.0, "覆盖");
    assert_eq!(*lock(&trace), ["构造", "类后置", "装饰", "解析"]);
    Ok(())
}

struct Many;
struct List(Vec<String>);

impl ContextualAttribute for Many {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async {
            Ok(AttributeValue::many(
                ["甲", "乙"].map(|value| ParameterValue::new(value.to_owned())),
            ))
        })
    }

    fn after(
        self: Arc<Self>,
        value: AttributeValue,
        _: Resolver,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let AttributeValue::Many(values) = value else {
                return Err(container_error("test_attribute_shape", "应观察整组变参"));
            };
            assert_eq!(values.iter().len(), 2);
            Ok(())
        })
    }
}

#[tokio::test]
async fn constructor_attribute_observes_the_whole_variadic_list() -> Result<()> {
    let container =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<List>(
            "tests::List",
            vec![
                ConstructorParam::scalar::<String>("items")
                    .variadic()
                    .with_attribute(ClassAttribute::contextual(|| Ok(Many))),
            ],
            |args| {
                Ok(Arc::new(List(
                    args.values(0)?
                        .iter()
                        .map(|value| {
                            value
                                .downcast::<String>()
                                .map(|value| value.as_ref().clone())
                        })
                        .collect::<Result<_>>()?,
                )))
            },
        )])?
        .build();
    assert_eq!(container.resolve::<List>().await?.0, ["甲", "乙"]);
    Ok(())
}

struct Null;
struct NewInstance(u32, bool);
struct MethodDependency(u32);

impl ContextualAttribute for Null {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async { Ok(AttributeValue::One(ParameterValue::null())) })
    }

    fn after(
        self: Arc<Self>,
        value: AttributeValue,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            assert_eq!(
                resolver.currently_resolving(),
                Some(&ServiceKey::typed::<NewInstance>())
            );
            assert!(matches!(value, AttributeValue::One(value) if value.is_null()));
            Ok(())
        })
    }
}

impl SelfBuilding for NewInstance {
    fn new_instance(
        args: ResolvedArgs<'_>,
        resolver: Resolver,
    ) -> BoxFuture<'static, Result<Arc<Self>>> {
        let dependency = args.service::<MethodDependency>(0);
        let is_null = args.value(1).map(ParameterValue::is_null);
        Box::pin(async move {
            let base = resolver.resolve::<NewInstance>().await?;
            Ok(Arc::new(Self(base.0 + dependency?.0, is_null?)))
        })
    }
}

#[tokio::test]
async fn self_building_reentry_uses_ordinary_constructor_and_outer_singleton_publishes()
-> Result<()> {
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::self_building::<NewInstance>(
            "tests::NewInstance",
            vec![],
            |_| Ok(Arc::new(NewInstance(7, false))),
            vec![
                ConstructorParam::class::<MethodDependency>("dependency"),
                ConstructorParam::scalar::<u32>("label")
                    .nullable()
                    .with_default(|| Ok(ParameterValue::new(19u32)))
                    .with_attribute(ClassAttribute::contextual(|| Ok(Null))),
            ],
        )
        .with_attribute(ClassAttribute::marker(|| Ok(ClassTag))),
    ])?;
    let phases = Trace::default();
    let events = Arc::clone(&phases);
    builder.after_resolving_attribute::<ClassTag>(move |_, resolver| {
        let events = Arc::clone(&events);
        Box::pin(async move {
            lock(&events).push(if resolver.currently_resolving().is_some() {
                "内层"
            } else {
                "外层"
            });
            Ok(())
        })
    })?;
    builder.instance(Arc::new(MethodDependency(5)));
    let key = ServiceKey::typed::<NewInstance>();
    let entered = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&entered);
    builder.before_resolving(Some(key.clone()), move |_, resolver| {
        let calls = Arc::clone(&calls);
        Box::pin(async move {
            if calls.fetch_add(1, Ordering::Relaxed) == 1 {
                let recursive = resolver.resolve::<NewInstance>().await;
                assert!(matches!(recursive, Err(error) if error.code() == "service_cycle"));
            }
            Ok(())
        })
    })?;
    builder.singleton_class::<NewInstance>(key.clone(), key);
    builder.extend::<NewInstance, _>(|value, resolver| {
        Box::pin(async move {
            if resolver.currently_resolving().is_some() {
                let recursive = resolver.resolve::<NewInstance>().await;
                assert!(matches!(recursive, Err(error) if error.code() == "service_cycle"));
            }
            Ok(value)
        })
    })?;
    builder.resolving(Some(ServiceKey::typed::<NewInstance>()), |_, resolver| {
        Box::pin(async move {
            if resolver.currently_resolving().is_some() {
                let recursive = resolver.resolve::<NewInstance>().await;
                assert!(matches!(recursive, Err(error) if error.code() == "service_cycle"));
            }
            Ok(())
        })
    })?;
    let container = builder.build();
    let shared = container.resolve::<NewInstance>().await?;
    assert_eq!((shared.0, shared.1), (12, true));
    assert_eq!(entered.load(Ordering::Relaxed), 2);
    assert!(Arc::ptr_eq(
        &shared,
        &container.resolve::<NewInstance>().await?
    ));
    let mut overrides = ParameterOverrides::new();
    overrides.insert(ParameterKey::name("label"), ParameterValue::new(41u32));
    let explicit = container.resolve_with::<NewInstance>(overrides).await?;
    assert_eq!((explicit.0, explicit.1), (17, true));
    assert_eq!(*lock(&phases), ["内层", "外层", "外层"]);
    container.bind_new_instance::<NewInstance>(|_| {
        Box::pin(async { Ok(Arc::new(NewInstance(99, false))) })
    })?;
    let mut overrides = ParameterOverrides::new();
    overrides.insert(ParameterKey::name("label"), ParameterValue::new(41u32));
    let bound = container.resolve_with::<NewInstance>(overrides).await?;
    assert_eq!((bound.0, bound.1), (99, false));
    Ok(())
}

struct PanickingAttribute(Arc<AtomicUsize>);
#[allow(clippy::panic, reason = "故障探针必须触发真实析构 panic")]
impl Drop for PanickingAttribute {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
        panic!("属性析构异常");
    }
}
#[allow(clippy::panic, reason = "故障探针必须触发真实同步回调 panic")]
impl ContextualAttribute for PanickingAttribute {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        panic!("回调创建 Future 时异常");
    }
}
struct AbortProbe;

#[tokio::test]
async fn attribute_sync_panic_and_instance_drop_panic_are_isolated() -> Result<()> {
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&drops);
    let container =
        ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<AbortProbe>(
            "tests::AbortProbe",
            vec![ConstructorParam::scalar::<u32>("number").with_attribute(
                ClassAttribute::contextual(move || Ok(PanickingAttribute(Arc::clone(&observed)))),
            )],
            |_| Ok(Arc::new(AbortProbe)),
        )])?
        .build();
    assert!(container.resolve::<AbortProbe>().await.is_err());
    assert_eq!(drops.load(Ordering::Relaxed), 1);
    Ok(())
}

trait Interface: Send + Sync {
    fn number(&self) -> u32;
}
struct Implementation(u32);
impl Interface for Implementation {
    fn number(&self) -> u32 {
        self.0
    }
}
struct Projected(Arc<dyn Interface>);
struct ConcreteAttribute;

impl ContextualAttribute for ConcreteAttribute {
    fn resolve(
        self: Arc<Self>,
        _: AttributeParameter,
        _: Resolver,
    ) -> BoxFuture<'static, Result<AttributeValue>> {
        Box::pin(async {
            Ok(AttributeValue::One(ParameterValue::service(Arc::new(
                Implementation(23),
            ))))
        })
    }
}

#[tokio::test]
async fn attribute_service_value_uses_declared_view_for_class_parameter() -> Result<()> {
    let seen = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Implementation>("tests::Implementation", vec![], |_| {
            Ok(Arc::new(Implementation(0)))
        })
        .with_projection(crate::class_projection!(Implementation => dyn Interface)),
        ClassDescriptor::concrete::<Projected>(
            "tests::Projected",
            vec![
                ConstructorParam::class::<dyn Interface>("dependency")
                    .with_attribute(ClassAttribute::contextual(|| Ok(ConcreteAttribute))),
            ],
            |args| Ok(Arc::new(Projected(args.service::<dyn Interface>(0)?))),
        ),
    ])?;
    let observed = Arc::clone(&seen);
    builder.after_resolving_attribute::<Implementation>(move |event, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            assert_eq!(
                event
                    .service::<Implementation>()?
                    .as_deref()
                    .map(Interface::number),
                Some(23)
            );
            assert_eq!(
                event
                    .service::<dyn Interface>()?
                    .as_deref()
                    .map(Interface::number),
                Some(23)
            );
            observed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
    })?;
    assert_eq!(builder.build().resolve::<Projected>().await?.0.number(), 23);
    assert_eq!(seen.load(Ordering::Relaxed), 1);
    Ok(())
}
