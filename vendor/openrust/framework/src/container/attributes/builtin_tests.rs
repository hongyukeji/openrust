use super::*;
use crate::config::repository::ConfigRepository;
use crate::container::{
    ClassAttribute, ClassDescriptor, ConstructorParam, ContainerBuilder, ParameterKey,
};
use crate::foundation::resources::ResourceOwner;
use std::sync::atomic::{AtomicUsize, Ordering};

struct ConfigValues(bool, u32, u32);
struct InvalidConfig;

#[tokio::test]
async fn builtin_config_distinguishes_missing_null_false_and_type_error() -> Result<()> {
    let config = ConfigRepository::new(figment::Figment::new());
    config.set("example.flag", serde_json::json!(false))?;
    config.set("example.null", serde_json::Value::Null)?;
    config.set("example.invalid", serde_json::json!("private-value-probe"))?;
    let resources = ResourceOwner::new();
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<ConfigValues>(
            "tests::ConfigValues",
            vec![
                ConstructorParam::scalar::<bool>("flag").with_attribute(
                    ClassAttribute::contextual(|| {
                        Ok(Config::<bool>::new("example.flag").with_default(true))
                    }),
                ),
                ConstructorParam::scalar::<u32>("missing").with_attribute(
                    ClassAttribute::contextual(|| {
                        Ok(Config::<u32>::new("example.missing").with_default(19))
                    }),
                ),
                ConstructorParam::scalar::<u32>("null")
                    .with_default(|| Ok(ParameterValue::new(23u32)))
                    .with_attribute(ClassAttribute::contextual(|| {
                        Ok(Config::<u32>::new("example.null").with_default(29))
                    })),
            ],
            |args| {
                Ok(Arc::new(ConfigValues(
                    *args.value(0)?.downcast::<bool>()?,
                    *args.value(1)?.downcast::<u32>()?,
                    *args.value(2)?.downcast::<u32>()?,
                )))
            },
        ),
        ClassDescriptor::concrete::<InvalidConfig>(
            "tests::InvalidConfig",
            vec![ConstructorParam::scalar::<u32>("invalid").with_attribute(
                ClassAttribute::contextual(|| Ok(Config::<u32>::new("example.invalid"))),
            )],
            |_| Ok(Arc::new(InvalidConfig)),
        ),
    ])?;
    builder.context(config, resources.registrar());
    let container = builder.build();
    let values = container.resolve::<ConfigValues>().await?;
    assert_eq!((values.0, values.1, values.2), (false, 19, 23));
    let failed = container.resolve::<InvalidConfig>().await;
    assert!(
        matches!(failed, Err(error) if error.code() == "container_contextual_config_type_mismatch" && !error.to_string().contains("private-value-probe"))
    );
    Ok(())
}

trait View: Send + Sync {
    fn number(&self) -> u32;
}
struct Concrete(u32);
impl View for Concrete {
    fn number(&self) -> u32 {
        self.0
    }
}
struct Selection(Arc<dyn View>, Arc<dyn View>);

#[tokio::test]
async fn builtin_give_preserves_projection_and_uses_only_its_own_overrides() -> Result<()> {
    let alias = ServiceKey::named("tests.give.alias");
    let shared = alias.clone();
    let fresh = alias.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let mut builder = ContainerBuilder::with_class_descriptors(vec![
        ClassDescriptor::concrete::<Concrete>("tests::Concrete", vec![], |_| {
            Ok(Arc::new(Concrete(0)))
        })
        .with_projection(crate::class_projection!(Concrete => dyn View)),
        ClassDescriptor::concrete::<Selection>(
            "tests::Selection",
            vec![
                ConstructorParam::class::<dyn View>("shared").with_attribute(
                    ClassAttribute::contextual(move || Ok(Give::<dyn View>::new(shared.clone()))),
                ),
                ConstructorParam::class::<dyn View>("fresh").with_attribute(
                    ClassAttribute::contextual(move || {
                        let mut parameters = ParameterOverrides::new();
                        parameters.insert(ParameterKey::name("number"), ParameterValue::new(17u32));
                        Ok(Give::<dyn View>::new(fresh.clone()).with_parameters(parameters))
                    }),
                ),
            ],
            |args| Ok(Arc::new(Selection(args.service(0)?, args.service(1)?))),
        ),
    ])?;
    let constructed = Arc::clone(&count);
    builder.singleton::<Concrete, _>(move |resolver| {
        let count = Arc::clone(&constructed);
        Box::pin(async move {
            count.fetch_add(1, Ordering::Relaxed);
            let parameters = resolver.parameters();
            let number = parameters
                .get(&ParameterKey::name("number"))
                .map(ParameterValue::downcast::<u32>)
                .transpose()?
                .map_or(5, |number| *number);
            Ok(Arc::new(Concrete(number)))
        })
    });
    builder.alias(ServiceKey::typed::<Concrete>(), alias.clone())?;
    let seen = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&seen);
    builder.after_resolving_attribute::<Concrete>(move |event, _| {
        let seen = Arc::clone(&observed);
        let alias = alias.clone();
        Box::pin(async move {
            assert_eq!(
                event.attribute().downcast::<Give<dyn View>>()?.key(),
                &alias
            );
            assert_eq!(
                event.service::<Concrete>()?.as_deref().map(View::number),
                event.service::<dyn View>()?.as_deref().map(View::number)
            );
            seen.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
    })?;
    let container = builder.build();
    let cached = container.resolve::<Concrete>().await?;
    let mut outer = ParameterOverrides::new();
    outer.insert(ParameterKey::name("number"), ParameterValue::new(99u32));
    let first = container.resolve_with::<Selection>(outer).await?;
    let second = container.resolve::<Selection>().await?;
    assert_eq!((first.0.number(), first.1.number()), (5, 17));
    assert_eq!((second.0.number(), second.1.number()), (5, 17));
    let cached_view: Arc<dyn View> = cached;
    assert!(Arc::ptr_eq(&first.0, &cached_view));
    assert!(Arc::ptr_eq(&second.0, &cached_view));
    assert!(!Arc::ptr_eq(&first.1, &second.1));
    assert_eq!(count.load(Ordering::Relaxed), 3);
    assert_eq!(seen.load(Ordering::Relaxed), 4);
    Ok(())
}

struct TaggedSelection(Arc<Tagged<String>>);
struct RetiredParameter(Arc<AtomicUsize>);

impl Drop for RetiredParameter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[tokio::test]
async fn builtin_tag_retires_unconsumed_raw_factory_parameters_before_delivery() -> Result<()> {
    let key = ServiceKey::typed::<TaggedSelection>();
    let mut builder = ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
        TaggedSelection,
    >(
        "tests::TaggedSelection",
        vec![
            ConstructorParam::scalar::<Tagged<String>>("services").with_attribute(
                ClassAttribute::contextual(|| Ok(Tag::<String>::new("tests.raw"))),
            ),
        ],
        |args| Ok(Arc::new(TaggedSelection(args.value(0)?.downcast()?))),
    )])?;
    builder.tag(std::iter::empty::<ServiceKey>(), ["tests.raw"]);
    builder.bind_class::<TaggedSelection>(key.clone(), key.clone());
    let container = builder.build();
    let entry = container
        .get_bindings()?
        .into_iter()
        .find(|entry| entry.key == key)
        .ok_or_else(|| container_error("test_missing_binding", "找不到原工厂"))?;
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut parameters = ParameterOverrides::new();
    parameters.insert(
        ParameterKey::name("unused"),
        ParameterValue::new(RetiredParameter(Arc::clone(&dropped))),
    );
    let selected = entry
        .concrete
        .invoke_with::<TaggedSelection>(container.factory_resolver()?, parameters)
        .await?;
    assert_eq!(dropped.load(Ordering::Relaxed), 1);
    assert!(selected.0.is_empty());
    assert!(selected.0.stream().next().await.is_none());
    Ok(())
}
