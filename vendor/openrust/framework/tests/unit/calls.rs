use openrust::{
    container::{
        Callback, ConstructorParam, Container, ParameterKey, ParameterOverrides, ParameterValue,
    },
    contracts::container::{BindingResolutionError, Container as _},
};
use std::sync::Arc;
#[tokio::test]
async fn callable_consumes_named_then_type_keys_and_keeps_remaining_values_in_input_order() {
    struct Dependency(u32);
    let c = Container::default();
    c.bind::<Dependency, _>(|_| panic!("explicit dependencies should not resolve"))
        .unwrap();
    let signature = [
        ConstructorParam::class::<Dependency>("first"),
        ConstructorParam::class::<Dependency>("second"),
        ConstructorParam::scalar::<u32>("number").withDefault(|| Ok(ParameterValue::new(1u32))),
    ];
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::class::<Dependency>(),
        ParameterValue::new(Arc::new(Dependency(5))),
    );
    parameters.insert(ParameterKey::Index(0), ParameterValue::new(6u32));
    parameters.insert(
        ParameterKey::name("first"),
        ParameterValue::new(Arc::new(Dependency(2))),
    );
    parameters.insert(ParameterKey::name("extra"), ParameterValue::new(7u32));
    let result = c
        .call(
            Callback::new(&signature, |args| async move {
                let tail = args
                    .remaining
                    .iter()
                    .map(|v| v.downcast::<u32>().map(|n| *n))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((
                    args.resolved.service::<Dependency>(0)?.0,
                    args.resolved.service::<Dependency>(1)?.0,
                    *args.resolved.get::<u32>(2)?,
                    tail,
                ))
            }),
            parameters,
        )
        .await
        .unwrap();
    assert_eq!(result, (2, 5, 1, vec![6, 7]));
}
#[tokio::test]
async fn method_primitive_policy_ignores_context_and_nullable_does_not_supply_required_values() {
    struct Caller(u32);
    let c = Container::default();
    c.when::<Caller>()
        .unwrap()
        .needsNamed::<u32>("$number")
        .give(|_| panic!("method primitives must not read contextual values"))
        .unwrap();
    c.bindAsync::<Caller, _>(|c| {
        Box::pin(async move {
            let signature = [ConstructorParam::scalar::<u32>("number")
                .withDefault(|| Ok(ParameterValue::new(1u32)))];
            let n = c
                .call(
                    Callback::new(&signature, |args| async move {
                        Ok(*args.resolved.get::<u32>(0)?)
                    }),
                    ParameterOverrides::new(),
                )
                .await?;
            Ok(Arc::new(Caller(n)))
        })
    })
    .await
    .unwrap();
    assert_eq!(c.makeAsync::<Caller>().await.unwrap().0, 1);
    let signature = [ConstructorParam::scalar::<String>("required").nullable()];
    assert!(matches!(
        c.call(
            Callback::new(&signature, |_| async { Ok(()) }),
            ParameterOverrides::new()
        )
        .await,
        Err(BindingResolutionError::UnresolvableParameter { .. })
    ));
}
#[tokio::test]
async fn primitive_variadics_append_unconsumed_values_without_spreading_explicit_arrays() {
    let c = Container::default();
    let signature = [ConstructorParam::scalar::<u32>("numbers").variadic()];
    let mut args = ParameterOverrides::new();
    args.insert(ParameterKey::Index(0), ParameterValue::new(2u32));
    args.insert(ParameterKey::name("numbers"), ParameterValue::new(1u32));
    args.insert(ParameterKey::name("extra"), ParameterValue::new(3u32));
    let numbers = c
        .call(
            Callback::new(&signature, |args| async move {
                assert!(args.remaining.is_empty());
                args.resolved
                    .values(0)?
                    .iter()
                    .map(|v| v.downcast::<u32>().map(|n| *n))
                    .collect::<Result<Vec<_>, _>>()
            }),
            args,
        )
        .await
        .unwrap();
    assert_eq!(numbers, [1, 2, 3]);
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::name("numbers"),
        ParameterValue::new(vec![1u32, 2]),
    );
    assert!(matches!(
        c.call(Callback::new(&signature, |_| async { Ok(()) }), args)
            .await,
        Err(BindingResolutionError::ParameterTypeMismatch { .. })
    ));
}
#[tokio::test]
async fn method_class_variadics_do_not_swallow_missing_dependencies() {
    struct Missing;
    let c = Container::default();
    let signature = [ConstructorParam::class::<Missing>("items").variadic()];
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::Index(0),
        ParameterValue::new(Arc::new(Missing)),
    );
    assert!(matches!(
        c.call(Callback::new(&signature, |_| async { Ok(()) }), args)
            .await,
        Err(BindingResolutionError::NotBound { .. })
    ));
    let mut args = ParameterOverrides::new();
    args.insert(
        ParameterKey::class::<Missing>(),
        ParameterValue::new(Arc::new(Missing)),
    );
    assert_eq!(
        c.call(
            Callback::new(&signature, |args| async move {
                Ok(args.resolved.services::<Missing>(0)?.len())
            }),
            args
        )
        .await
        .unwrap(),
        1
    );
}
#[tokio::test]
async fn anonymous_callbacks_can_borrow_mutably_and_return_borrowed_data() {
    let c = Container::default();
    let mut text = String::from("before");
    let borrowed = &mut text;
    let value = c
        .call(
            Callback::new(&[], move |_| async move {
                borrowed.push_str(" after");
                Ok(borrowed.as_str())
            }),
            ParameterOverrides::new(),
        )
        .await
        .unwrap();
    assert_eq!(value, "before after");
}
