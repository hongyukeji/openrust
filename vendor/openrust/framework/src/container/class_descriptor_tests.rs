//! 编译目录、预检和具名参数的负例。参数求值策略及生产 Resolver 留给后继。
use super::*;
use crate::container::class_projection::class_projection;
use std::sync::atomic::{AtomicUsize, Ordering};

trait View: Send + Sync {
    fn value(&self) -> usize;
}
struct First(usize);
struct Second;
struct Report;
struct Abstract;
impl View for First {
    fn value(&self) -> usize {
        self.0
    }
}
impl View for Second {
    fn value(&self) -> usize {
        2
    }
}

fn first(calls: &Arc<AtomicUsize>, parameters: Vec<ConstructorParam>) -> ClassDescriptor {
    let calls = Arc::clone(calls);
    ClassDescriptor::concrete::<First>("App/First", parameters, move |_| {
        calls.fetch_add(1, Ordering::AcqRel);
        Ok(Arc::new(First(1)))
    })
}
fn error<T>(result: Result<T>, code: &str) {
    assert!(matches!(result, Err(error) if error.code() == code));
}
fn key<T: ?Sized + 'static>() -> ServiceKey {
    ServiceKey::typed::<T>()
}
fn one<T: Send + Sync + 'static>(value: T) -> Argument {
    Argument::One(ParameterValue::new(value))
}

#[test]
fn heterogeneous_batch_has_one_identity_and_distinct_noninstantiable_kinds() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let directory = ClassDirectory::from_descriptors(vec![
        first(&calls, vec![]).with_projection(class_projection!(First => dyn View)),
        ClassDescriptor::concrete::<Second>("App/Second", vec![], |_| Ok(Arc::new(Second)))
            .with_projection(class_projection!(Second => dyn View)),
        ClassDescriptor::interface::<dyn View>("App/View"),
        ClassDescriptor::abstract_class::<Abstract>("App/Abstract"),
    ])?;
    assert_eq!(
        directory
            .identities()
            .canonical_key(&ServiceKey::named("App/First")),
        key::<First>()
    );
    assert_eq!(
        directory.identities().symbol(&key::<Second>()),
        Some("App/Second")
    );
    assert!(std::ptr::eq(
        directory
            .class(&key::<First>())
            .ok_or_else(|| container_error("fixture", "缺少类"))?,
        directory
            .class(&ServiceKey::named("App/First"))
            .ok_or_else(|| container_error("fixture", "缺少符号"))?,
    ));
    assert_eq!(
        directory.class(&key::<First>()).map(ClassRecord::kind),
        Some(ClassKind::Concrete)
    );
    assert_eq!(
        directory.class(&key::<dyn View>()).map(ClassRecord::kind),
        Some(ClassKind::Interface)
    );
    assert_eq!(
        directory.class(&key::<Abstract>()).map(ClassRecord::kind),
        Some(ClassKind::Abstract)
    );
    error(
        directory.prepare(&key::<dyn View>(), &key::<dyn View>()),
        "container_class_not_instantiable",
    );
    error(
        directory.prepare(&key::<Abstract>(), &key::<Abstract>()),
        "container_class_not_instantiable",
    );
    error(
        directory.prepare(&ServiceKey::named("missing"), &key::<First>()),
        "container_class_missing",
    );
    error(
        directory.prepare(&key::<First>(), &key::<Second>()),
        "service_type_mismatch",
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    Ok(())
}

#[test]
fn repeated_symbol_type_and_identical_entry_fail_whole_batch_without_user_calls() {
    let calls = Arc::new(AtomicUsize::new(0));
    for descriptors in [
        vec![
            first(&calls, vec![]),
            ClassDescriptor::concrete::<Second>("App/First", vec![], |_| Ok(Arc::new(Second))),
        ],
        vec![
            first(&calls, vec![]),
            ClassDescriptor::concrete::<First>("Other", vec![], |_| Ok(Arc::new(First(2)))),
        ],
        vec![first(&calls, vec![]), first(&calls, vec![])],
    ] {
        error(
            ClassDirectory::from_descriptors(descriptors),
            "container_class_identity_duplicate",
        );
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn duplicate_and_empty_parameter_names_and_invalid_variadic_combinations_fail_before_new() {
    let calls = Arc::new(AtomicUsize::new(0));
    for parameters in [
        vec![
            ConstructorParam::untyped("x"),
            ConstructorParam::class::<First>("x"),
        ],
        vec![ConstructorParam::untyped("")],
    ] {
        error(
            ClassDirectory::from_descriptors(vec![first(&calls, parameters)]),
            "container_constructor_parameter_name",
        );
    }
    for parameters in [
        vec![
            ConstructorParam::untyped("x").variadic(),
            ConstructorParam::untyped("y"),
        ],
        vec![
            ConstructorParam::untyped("x")
                .variadic()
                .with_default(|| Ok(ParameterValue::null())),
        ],
        vec![
            ConstructorParam::untyped("x").variadic(),
            ConstructorParam::untyped("y").variadic(),
        ],
    ] {
        error(
            ClassDirectory::from_descriptors(vec![first(&calls, parameters)]),
            "container_constructor_variadic_invalid",
        );
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

#[test]
fn wrong_source_duplicate_target_abstract_source_and_redundant_identity_projection_fail() {
    let calls = Arc::new(AtomicUsize::new(0));
    for descriptor in [
        first(&calls, vec![]).with_projection(class_projection!(Second => dyn View)),
        first(&calls, vec![])
            .with_projection(class_projection!(First => dyn View))
            .with_projection(class_projection!(First => dyn View)),
        first(&calls, vec![]).with_projection(class_projection!(First => First)),
        ClassDescriptor::abstract_class::<First>("AbstractFirst")
            .with_projection(class_projection!(First => dyn View)),
    ] {
        error(
            ClassDirectory::from_descriptors(vec![descriptor]),
            "container_class_projection_invalid",
        );
    }
    assert_eq!(calls.load(Ordering::Acquire), 0);
}

static DEFAULTS: AtomicUsize = AtomicUsize::new(0);
fn default_failed() -> Result<ParameterValue> {
    Err(Error::custom("default_business", "默认函数业务错误"))
}

#[test]
fn named_override_null_and_index_do_not_evaluate_default_and_defaults_are_fresh() -> Result<()> {
    DEFAULTS.store(0, Ordering::Release);
    let calls = Arc::new(AtomicUsize::new(0));
    let directory = ClassDirectory::from_descriptors(vec![first(
        &calls,
        vec![
            ConstructorParam::scalar::<String>("title")
                .nullable()
                .with_default(|| {
                    DEFAULTS.fetch_add(1, Ordering::AcqRel);
                    Ok(ParameterValue::new(String::from("default")))
                }),
        ],
    )])?;
    let prepared = directory.prepare(&key::<First>(), &key::<First>())?;
    let parameter = &prepared.parameters()[0];
    assert_eq!(parameter.name(), "title");
    assert!(
        matches!(parameter.kind(), ParameterKind::Scalar { rust_type, .. } if *rust_type == TypeId::of::<String>())
    );
    assert_eq!(DEFAULTS.load(Ordering::Acquire), 0);
    let mut overrides = ParameterOverrides::new();
    overrides.insert(
        ParameterKey::Index(0),
        ParameterValue::new(String::from("index")),
    );
    assert!(parameter.override_argument(&overrides).is_none());
    overrides.insert(ParameterKey::name("title"), ParameterValue::null());
    let selected = parameter
        .override_argument(&overrides)
        .ok_or_else(|| container_error("fixture", "显式 null 应命中"))?;
    assert!(matches!(&selected, Argument::One(value) if value.is_null()));
    prepared.construct(&[selected])?;
    assert_eq!(DEFAULTS.load(Ordering::Acquire), 0);
    let a = parameter
        .evaluate_default()?
        .ok_or_else(|| container_error("fixture", "缺少默认值"))?
        .downcast::<String>()?;
    let b = parameter
        .evaluate_default()?
        .ok_or_else(|| container_error("fixture", "缺少默认值"))?
        .downcast::<String>()?;
    assert_eq!(DEFAULTS.load(Ordering::Acquire), 2);
    assert_eq!(*a, "default");
    assert_eq!(*b, "default");
    assert!(!Arc::ptr_eq(&a, &b));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    error(
        ConstructorParam::scalar::<String>("wrong")
            .with_default(|| Ok(ParameterValue::new(5_u32)))
            .evaluate_default(),
        "container_parameter_type_mismatch",
    );
    error(
        ConstructorParam::scalar::<String>("business")
            .with_default(default_failed)
            .evaluate_default(),
        "default_business",
    );
    assert!(
        ConstructorParam::scalar::<String>("null")
            .nullable()
            .with_default(|| Ok(ParameterValue::null()))
            .evaluate_default()?
            .is_some_and(|value| value.is_null())
    );
    error(
        ConstructorParam::scalar::<String>("null")
            .with_default(|| Ok(ParameterValue::null()))
            .evaluate_default(),
        "container_parameter_null",
    );
    assert!(
        ConstructorParam::class::<dyn View>("service")
            .nullable()
            .evaluate_default()?
            .is_none()
    );
    assert!(
        ConstructorParam::untyped("untyped")
            .evaluate_default()?
            .is_none()
    );
    Ok(())
}

#[test]
fn count_shape_exact_type_null_and_class_boxing_are_checked_before_constructor() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let directory = ClassDirectory::from_descriptors(vec![first(
        &calls,
        vec![
            ConstructorParam::class::<dyn View>("view"),
            ConstructorParam::scalar::<u32>("number"),
        ],
    )])?;
    let prepared = directory.prepare(&key::<First>(), &key::<First>())?;
    let object = Arc::new(First(3));
    let view: Arc<dyn View> = object.clone();
    error(
        prepared.construct(&[]),
        "container_constructor_argument_count",
    );
    error(
        prepared.construct(&[Argument::Many(vec![]), one(7_u32)]),
        "container_constructor_argument_shape",
    );
    error(
        prepared.construct(&[Argument::One(ParameterValue::from_arc(object)), one(7_u32)]),
        "container_parameter_type_mismatch",
    );
    error(
        prepared.construct(&[Argument::One(ParameterValue::null()), one(7_u32)]),
        "container_parameter_null",
    );
    error(
        prepared.construct(&[one(Arc::clone(&view)), one(7_u64)]),
        "container_parameter_type_mismatch",
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let arguments = [one(Arc::clone(&view)), one(7_u32)];
    let resolved = ResolvedArgs::validate(prepared.parameters(), &arguments)?;
    assert!(Arc::ptr_eq(&resolved.service::<dyn View>(0)?, &view));
    error(
        resolved.service::<First>(0),
        "container_parameter_type_mismatch",
    );
    error(
        resolved.service::<dyn View>(1),
        "container_parameter_type_mismatch",
    );
    error(resolved.value(2), "container_constructor_argument_slot");
    error(resolved.values(1), "container_constructor_argument_shape");
    assert_eq!(*resolved.value(1)?.downcast::<u32>()?, 7);
    prepared.construct(&arguments)?;
    assert_eq!(calls.load(Ordering::Acquire), 1);
    Ok(())
}

#[test]
fn variadic_one_retains_vec_value_and_many_retains_order_including_empty() -> Result<()> {
    let parameters = [ConstructorParam::untyped("items").variadic()];
    let mut overrides = ParameterOverrides::new();
    overrides.insert(
        ParameterKey::name("items"),
        ParameterValue::new(vec![3_u32, 1, 2]),
    );
    let one = [parameters[0]
        .override_argument(&overrides)
        .ok_or_else(|| container_error("fixture", "显式数组应作为单项命中"))?];
    let resolved = ResolvedArgs::validate(&parameters, &one)?;
    assert_eq!(resolved.values(0)?.len(), 1);
    assert_eq!(
        *resolved.values(0)?[0].downcast::<Vec<u32>>()?,
        vec![3, 1, 2]
    );
    error(resolved.value(0), "container_constructor_argument_shape");
    let many = [Argument::Many(vec![
        ParameterValue::new(3_u32),
        ParameterValue::new(1_u32),
        ParameterValue::new(2_u32),
    ])];
    let resolved = ResolvedArgs::validate(&parameters, &many)?;
    let values = resolved
        .values(0)?
        .iter()
        .map(|value| value.downcast::<u32>().map(|value| *value))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(values, vec![3, 1, 2]);
    let empty = [Argument::Many(vec![])];
    assert!(
        ResolvedArgs::validate(&parameters, &empty)?
            .values(0)?
            .is_empty()
    );
    let typed = [ConstructorParam::scalar::<u32>("typed").variadic()];
    error(
        ResolvedArgs::validate(&typed, &one),
        "container_parameter_type_mismatch",
    );
    assert_eq!(ResolvedArgs::validate(&typed, &many)?.values(0)?.len(), 3);
    Ok(())
}

#[test]
fn accessor_shapes_and_nullable_reception_do_not_supply_missing_dependencies() -> Result<()> {
    let params = [
        ConstructorParam::class::<dyn View>("view").nullable(),
        ConstructorParam::untyped("any"),
    ];
    let values = [
        Argument::One(ParameterValue::null()),
        Argument::One(ParameterValue::null()),
    ];
    let resolved = ResolvedArgs::validate(&params, &values)?;
    assert!(resolved.value(0)?.is_null());
    assert!(resolved.value(1)?.is_null());
    error(resolved.service::<dyn View>(0), "container_parameter_null");
    error(
        ResolvedArgs::validate(&params, &[]),
        "container_constructor_argument_count",
    );
    assert!(
        params[0]
            .override_argument(&ParameterOverrides::new())
            .is_none()
    );
    let class_variadic = [ConstructorParam::class::<dyn View>("many").variadic()];
    let view: Arc<dyn View> = Arc::new(First(8));
    let values = [one(view)];
    let resolved = ResolvedArgs::validate(&class_variadic, &values)?;
    error(
        resolved.service::<dyn View>(0),
        "container_constructor_argument_shape",
    );
    assert_eq!(resolved.values(0)?.len(), 1);
    Ok(())
}

#[test]
fn preflight_and_same_arc_projection_preserve_canonical_value_and_business_failure() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let directory = ClassDirectory::from_descriptors(vec![
        first(&calls, vec![]).with_projection(class_projection!(First => dyn View)),
        ClassDescriptor::interface::<dyn View>("App/View"),
        ClassDescriptor::concrete::<Report>("App/Report", vec![], |_| {
            Err(Error::custom("constructor_business", "业务构造错误"))
        }),
    ])?;
    error(
        directory.prepare(&key::<First>(), &key::<Second>()),
        "service_type_mismatch",
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    let prepared = directory.prepare(
        &ServiceKey::named("App/First"),
        &ServiceKey::named("App/View"),
    )?;
    let canonical = prepared.construct(&[])?;
    let source = canonical
        .downcast_ref::<Arc<First>>()
        .ok_or_else(|| container_error("fixture", "规范输出不符"))?;
    let expected: Arc<dyn View> = source.clone();
    let projected = prepared.project(&canonical)?;
    let view = projected
        .downcast_ref::<Arc<dyn View>>()
        .ok_or_else(|| container_error("fixture", "投影输出不符"))?;
    assert!(Arc::ptr_eq(view, &expected));
    assert_eq!(view.value(), 1);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let canonical_view = directory
        .prepare(&key::<First>(), &key::<First>())?
        .project(&canonical)?;
    assert!(Arc::ptr_eq(&canonical, &canonical_view));
    let wrong: Value = Arc::new(Arc::new(Second));
    error(prepared.project(&wrong), "service_type_mismatch");
    error(
        directory
            .prepare(&key::<Report>(), &key::<Report>())?
            .construct(&[]),
        "constructor_business",
    );
    assert_eq!(calls.load(Ordering::Acquire), 1);
    Ok(())
}

struct Dropped(Arc<AtomicUsize>);
impl Drop for Dropped {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}
#[test]
fn early_argument_rejection_borrows_frame_and_does_not_retire_callers_values() {
    let count = Arc::new(AtomicUsize::new(0));
    let parameters = [ConstructorParam::scalar::<u32>("value")];
    let arguments = [one(Dropped(Arc::clone(&count)))];
    error(
        ResolvedArgs::validate(&parameters, &arguments),
        "container_parameter_type_mismatch",
    );
    assert_eq!(count.load(Ordering::Acquire), 0);
    drop(arguments);
    assert_eq!(count.load(Ordering::Acquire), 1);
}

struct BadCapture(Arc<AtomicUsize>);
impl Drop for BadCapture {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("有限独立装配捕获"));
    }
}
#[test]
fn rejected_batch_retires_independent_constructor_captures_without_running_them() -> Result<()> {
    let dropped = Arc::new(AtomicUsize::new(0));
    let invoked = Arc::new(AtomicUsize::new(0));
    let mut descriptors = Vec::new();
    for _ in 0..2 {
        let capture = BadCapture(Arc::clone(&dropped));
        let calls = Arc::clone(&invoked);
        descriptors.push(ClassDescriptor::concrete::<First>(
            "Duplicate",
            vec![],
            move |_| {
                std::hint::black_box(&capture);
                calls.fetch_add(1, Ordering::AcqRel);
                Ok(Arc::new(First(1)))
            },
        ));
    }
    let result = crate::foundation::lifecycle::catch_isolated(|| {
        ClassDirectory::from_descriptors(descriptors)
    });
    let directory = result.map_err(|()| container_error("fixture", "受控捕获不得覆盖装配错误"))?;
    error(directory, "container_class_identity_duplicate");
    assert_eq!(dropped.load(Ordering::Acquire), 2);
    assert_eq!(invoked.load(Ordering::Acquire), 0);
    Ok(())
}

static INVALID_DEFAULT_DROPS: AtomicUsize = AtomicUsize::new(0);
struct InvalidDefault;
impl Drop for InvalidDefault {
    fn drop(&mut self) {
        INVALID_DEFAULT_DROPS.fetch_add(1, Ordering::AcqRel);
        std::panic::resume_unwind(Box::new("有限错误默认值"));
    }
}
#[test]
fn wrong_default_type_preserves_primary_when_rejected_value_cleanup_also_fails() -> Result<()> {
    INVALID_DEFAULT_DROPS.store(0, Ordering::Release);
    let result = ConstructorParam::scalar::<String>("title")
        .with_default(|| Ok(ParameterValue::new(InvalidDefault)))
        .evaluate_default();
    let primary = result
        .err()
        .ok_or_else(|| container_error("fixture", "错误默认类型应拒绝"))?;
    assert_eq!(primary.code(), "resource_task_interrupted");
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&primary);
    let mut found = false;
    while let Some(error) = source {
        if let Some(failure) =
            error.downcast_ref::<crate::foundation::lifecycle::TaskExecutionFailure>()
        {
            assert_eq!(failure.primary.code(), "container_parameter_type_mismatch");
            assert_eq!(failure.cleanup.code(), "resource_task_interrupted");
            found = true;
            break;
        }
        source = error.source();
    }
    assert!(found);
    assert_eq!(INVALID_DEFAULT_DROPS.load(Ordering::Acquire), 1);
    Ok(())
}
