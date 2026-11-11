use openrust::{
    container::{
        AttributeParameter, AttributeValue, Callback, ClassAttribute, ClassDescriptor,
        ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue, ServiceKey,
    },
    contracts::container::{BindingResolutionError, Container as _, ContextualAttribute},
};
use std::sync::{Arc, Mutex};

struct Pick(Arc<Mutex<Vec<&'static str>>>);
impl ContextualAttribute for Pick {
    fn resolve(
        &self,
        _: &Container,
        _: AttributeParameter,
    ) -> Result<AttributeValue, BindingResolutionError> {
        self.0.lock().unwrap().push("default resolver");
        Ok(ParameterValue::new(99_u32).into())
    }

    fn after(&self, value: &AttributeValue, _: &Container) -> Result<(), BindingResolutionError> {
        assert!(matches!(value, AttributeValue::One(_)));
        self.0.lock().unwrap().push("attribute after");
        Ok(())
    }
}

struct Marker;
struct Item(u32);

#[tokio::test]
async fn constructor_attribute_selection_and_class_callbacks_follow_resolution_order() {
    let container = Container::default();
    let trace = Arc::new(Mutex::new(Vec::new()));
    let pick = trace.clone();
    let new = trace.clone();
    container
        .registerClass(
            ClassDescriptor::concrete(
                vec![
                    ConstructorParam::scalar::<u32>("number")
                        .withDefault(|| Ok(ParameterValue::new(7_u32)))
                        .withAttribute(ClassAttribute::contextual(move || Ok(Pick(pick.clone()))))
                        .withAttribute(ClassAttribute::marker(|| Ok(Marker))),
                ],
                move |args| {
                    new.lock().unwrap().push("constructor");
                    Ok(Arc::new(Item(*args.get::<u32>(0)?)))
                },
            )
            .unwrap()
            .withAttribute(ClassAttribute::marker(|| Ok(Marker))),
        )
        .unwrap();
    container.whenHasAttribute::<Pick>(|attribute, _, parameter| {
        assert_eq!(parameter.name, "number");
        attribute.0.lock().unwrap().push("registered resolver");
        Ok(ParameterValue::null().into())
    });
    container.afterResolvingAttribute::<Pick>({
        let trace = trace.clone();
        move |event, _| {
            assert!(
                matches!(event.value, AttributeValue::One(value) if *value.downcast::<u32>()? == 7)
            );
            trace.lock().unwrap().push("registered after");
            Ok(())
        }
    });
    container.afterResolvingAttribute::<Item>({
        let trace = trace.clone();
        move |event, _| {
            assert!(event.parameter.is_none());
            event.attribute.downcast::<Marker>()?;
            assert!(matches!(event.value, AttributeValue::One(value) if value.downcast::<Arc<Item>>().is_ok()));
            trace.lock().unwrap().push("class after");
            Ok(())
        }
    });
    container
        .extend::<Item, _>(ServiceKey::typed::<Item>(), {
            let trace = trace.clone();
            move |value, _| {
                trace.lock().unwrap().push("extender");
                Ok(Arc::new(Item(value.0 + 1)))
            }
        })
        .unwrap();
    container
        .afterResolving(Some(ServiceKey::typed::<Item>()), {
            let trace = trace.clone();
            move |_, _| {
                trace.lock().unwrap().push("resolved");
                Ok(())
            }
        })
        .unwrap();
    assert_eq!(container.make::<Item>().unwrap().0, 8);
    assert_eq!(
        *trace.lock().unwrap(),
        [
            "registered resolver",
            "attribute after",
            "registered after",
            "constructor",
            "class after",
            "extender",
            "resolved",
        ]
    );

    trace.lock().unwrap().clear();
    let mut overrides = ParameterOverrides::new();
    overrides.insert(ParameterKey::name("number"), ParameterValue::new(30_u32));
    assert_eq!(
        container.makeWithAsync::<Item>(overrides).await.unwrap().0,
        31
    );
    assert_eq!(
        *trace.lock().unwrap(),
        ["constructor", "class after", "extender", "resolved"]
    );
}

struct Null;
impl ContextualAttribute for Null {
    fn resolve(
        &self,
        _: &Container,
        _: AttributeParameter,
    ) -> Result<AttributeValue, BindingResolutionError> {
        Ok(ParameterValue::null().into())
    }
}

#[tokio::test]
async fn call_prefers_named_override_then_attribute_then_class_override_and_preserves_null() {
    let container = Container::default();
    let observed = Arc::new(Mutex::new(Vec::new()));
    container.afterResolvingAttribute::<Null>({
        let observed = observed.clone();
        move |event, _| {
            let AttributeValue::One(value) = event.value else {
                unreachable!()
            };
            observed.lock().unwrap().push(value.is_null());
            Ok(())
        }
    });
    let signature = [ConstructorParam::class::<u32>("value")
        .nullable()
        .withAttribute(ClassAttribute::contextual(|| Ok(Null)))];
    let call = |args: openrust::container::CallArguments| async move {
        Ok(args.resolved.optionalService::<u32>(0)?.map(|value| *value))
    };
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::class::<u32>(),
        ParameterValue::new(Arc::new(18_u32)),
    );
    assert_eq!(
        container
            .call(Callback::new(&signature, call), parameters.clone())
            .await
            .unwrap(),
        None
    );
    parameters.insert(
        ParameterKey::name("value"),
        ParameterValue::new(Arc::new(19_u32)),
    );
    assert_eq!(
        container
            .call(Callback::new(&signature, call), parameters)
            .await
            .unwrap(),
        Some(19)
    );
    assert_eq!(*observed.lock().unwrap(), [true, false]);
}

struct Values;
impl ContextualAttribute for Values {
    fn resolve(
        &self,
        _: &Container,
        parameter: AttributeParameter,
    ) -> Result<AttributeValue, BindingResolutionError> {
        assert!(parameter.variadic);
        Ok(AttributeValue::Many(vec![
            ParameterValue::new(1_u32),
            ParameterValue::new(2_u32),
        ]))
    }
}

#[tokio::test]
async fn constructor_variadic_attribute_reports_the_whole_list() {
    let container = Container::default();
    let signature = vec![
        ConstructorParam::scalar::<u32>("values")
            .variadic()
            .withAttribute(ClassAttribute::contextual(|| Ok(Values))),
    ];
    container
        .registerClass(
            ClassDescriptor::concrete(signature.clone(), |args| {
                Ok(Arc::new(Item(
                    args.values(0)?.iter().try_fold(0, |sum, value| {
                        Ok::<_, BindingResolutionError>(sum + *value.downcast::<u32>()?)
                    })?,
                )))
            })
            .unwrap(),
        )
        .unwrap();
    container.afterResolvingAttribute::<Values>(|event, _| {
        assert!(matches!(event.value, AttributeValue::Many(values) if values.len() == 2));
        Ok(())
    });
    assert_eq!(container.buildAsync::<Item>().await.unwrap().0, 3);
    assert!(matches!(
        container
            .call(
                Callback::new(&signature, |_| async { Ok(()) }),
                ParameterOverrides::new()
            )
            .await,
        Err(BindingResolutionError::InvalidAttributeArguments { .. })
    ));
}

struct RequiresHandler;
impl ContextualAttribute for RequiresHandler {}

#[test]
fn missing_handlers_can_be_registered_and_default_resolvers_stay_container_local() {
    let first = Container::default();
    let second = Container::default();
    let attribute = ClassAttribute::contextual(|| Ok(RequiresHandler));
    let parameter = AttributeParameter {
        name: "value",
        variadic: false,
    };
    assert!(matches!(
        first.resolveFromAttribute(&attribute, parameter),
        Err(BindingResolutionError::AttributeHandlerMissing { .. })
    ));
    first.whenHasAttribute::<RequiresHandler>(|_, _, _| Ok(ParameterValue::new(4_u32).into()));
    assert!(
        matches!(first.resolveFromAttribute(&attribute, parameter).unwrap(), AttributeValue::One(value) if *value.downcast::<u32>().unwrap() == 4)
    );
    assert!(second.resolveFromAttribute(&attribute, parameter).is_err());

    let trace = Arc::new(Mutex::new(Vec::new()));
    let default = ClassAttribute::contextual({
        let trace = trace.clone();
        move || Ok(Pick(trace.clone()))
    });
    assert!(
        matches!(second.resolveFromAttribute(&default, parameter).unwrap(), AttributeValue::One(value) if *value.downcast::<u32>().unwrap() == 99)
    );
    assert_eq!(*trace.lock().unwrap(), ["default resolver"]);
}
