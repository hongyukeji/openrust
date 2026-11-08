//! 固定 PHP 转换观测与私有已选帧测试。测试中的选参及诊断策略由测试显式装配。
use super::super::class_descriptor::ParameterKey;
use super::php_scalar::{PhpBytes, PhpIntBool, PhpStringable, PhpTarget, ScalarDiagnostic};
use super::{Argument, ClassDescriptor, ClassRegistry, ConstructorParam, ParameterValue};
use crate::container::{
    ContainerBuilder, ParameterOverrides, ScalarDiagnostics, ServiceKey, container_error, lock,
};
use crate::support::InputNumber;
use crate::{Error, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::Deserialize;
use std::fmt::Display;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Encoded {
    Null,
    Bool {
        value: bool,
    },
    Int {
        decimal: String,
    },
    Float {
        bits_be: String,
    },
    String {
        base64: String,
    },
    Array {
        items: Vec<Self>,
    },
    #[serde(rename = "ScalarBinaryStringable")]
    Stringable,
    #[serde(rename = "stdClass")]
    Object,
}

impl Encoded {
    fn input(&self, calls: &Arc<AtomicUsize>) -> Result<ParameterValue> {
        Ok(match self {
            Self::Null => ParameterValue::null(),
            Self::Bool { value } => ParameterValue::new(*value),
            Self::Int { decimal } => ParameterValue::new(decimal.parse::<i64>().map_err(fixture)?),
            Self::Float { bits_be } => {
                let bits = u64::from_str_radix(bits_be, 16).map_err(fixture)?;
                ParameterValue::new(f64::from_bits(bits))
            }
            Self::String { base64 } => ParameterValue::new(PhpBytes(Arc::from(
                STANDARD.decode(base64).map_err(fixture)?,
            ))),
            Self::Array { items } => ParameterValue::new(
                items
                    .iter()
                    .map(|value| value.input(calls))
                    .collect::<Result<Vec<_>>>()?,
            ),
            Self::Stringable => {
                let calls = Arc::clone(calls);
                ParameterValue::new(PhpStringable(Box::new(move || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Ok(PhpBytes(Arc::from(&b"a\0\xff"[..])))
                })))
            }
            Self::Object => ParameterValue::new(Ordinary),
        })
    }

    fn verify(&self, id: &str, target: PhpTarget, value: &ParameterValue) -> Result<()> {
        match self {
            Self::Null => assert!(value.is_null(), "{id}"),
            Self::Bool { value: expected } => match target {
                PhpTarget::IntBool => {
                    assert_eq!(
                        *value.downcast::<PhpIntBool>()?,
                        PhpIntBool::Bool(*expected),
                        "{id}"
                    );
                }
                _ => assert_eq!(*value.downcast::<bool>()?, *expected, "{id}"),
            },
            Self::Int { decimal } => {
                let expected = decimal.parse::<i64>().map_err(fixture)?;
                match target {
                    PhpTarget::Number => {
                        assert!(
                            matches!(
                                *value.downcast::<InputNumber>()?,
                                InputNumber::Integer(value) if value == expected
                            ),
                            "{id}"
                        );
                    }
                    PhpTarget::IntBool => {
                        assert_eq!(
                            *value.downcast::<PhpIntBool>()?,
                            PhpIntBool::Int(expected),
                            "{id}"
                        );
                    }
                    _ => assert_eq!(*value.downcast::<i64>()?, expected, "{id}"),
                }
            }
            Self::Float { bits_be } => {
                let expected = u64::from_str_radix(bits_be, 16).map_err(fixture)?;
                if matches!(target, PhpTarget::Number) {
                    assert!(
                        matches!(
                            *value.downcast::<InputNumber>()?,
                            InputNumber::Float(value) if value.to_bits() == expected
                        ),
                        "{id}"
                    );
                } else {
                    assert_eq!(value.downcast::<f64>()?.to_bits(), expected, "{id}");
                }
            }
            Self::String { base64 } => {
                let expected = STANDARD.decode(base64).map_err(fixture)?;
                assert_eq!(
                    value.downcast::<PhpBytes>()?.0.as_ref(),
                    expected.as_slice(),
                    "{id}"
                );
            }
            Self::Array { .. } | Self::Stringable | Self::Object => {
                return Err(fixture("PHP 转换结果不是支持的标量"));
            }
        }
        Ok(())
    }
}

struct Ordinary;

#[derive(Deserialize)]
struct Observation(String, Encoded, Option<Encoded>, Option<u16>, bool);

fn fixture(detail: impl Display) -> Error {
    container_error("fixture", detail.to_string())
}

fn one<T: Send + Sync + 'static>(value: T) -> Argument {
    Argument::One(ParameterValue::new(value))
}

fn fail<T>(result: Result<T>, code: &str) {
    assert!(
        matches!(result, Err(error) if error.code() == code),
        "{code}"
    );
}

fn record(events: &mut Vec<ScalarDiagnostic>) -> impl FnMut(ScalarDiagnostic) -> Result<()> + '_ {
    |event| {
        events.push(event);
        Ok(())
    }
}

fn verify_no_diagnostic(_: ScalarDiagnostic) -> Result<()> {
    Err(fixture("不应产生诊断"))
}

fn captured(
    parameters: Vec<ConstructorParam>,
    read: impl for<'a> Fn(super::ResolvedArgs<'a>) -> Result<ParameterValue> + Send + Sync + 'static,
) -> Result<ClassRegistry> {
    ClassRegistry::from_descriptors(vec![ClassDescriptor::concrete::<ParameterValue>(
        "Test/Coerced",
        parameters,
        move |arguments| read(arguments).map(Arc::new),
    )])
}

fn read_result(value: super::Value) -> Result<Arc<ParameterValue>> {
    let typed = Arc::unwrap_or_clone(value)
        .into_erased()
        .downcast::<Arc<ParameterValue>>()
        .map_err(|_| fixture("缺少构造输出"))?;
    Ok(Arc::clone(typed.as_ref()))
}

fn target(name: &str) -> Result<(PhpTarget, bool)> {
    Ok(match name {
        "int" => (PhpTarget::Int, false),
        "nullable_int" => (PhpTarget::Int, true),
        "float" => (PhpTarget::Float, false),
        "string" => (PhpTarget::Bytes, false),
        "bool" => (PhpTarget::Bool, false),
        "number_union" => (PhpTarget::Number, false),
        "int_bool_union" => (PhpTarget::IntBool, false),
        _ => return Err(fixture(name)),
    })
}

fn expected_diagnostic(
    input: &Encoded,
    target: PhpTarget,
    severity: Option<u16>,
) -> Result<Option<ScalarDiagnostic>> {
    Ok(match (severity, input, target) {
        (None, _, _) => None,
        (Some(8192), Encoded::Float { .. }, _) => Some(ScalarDiagnostic::FloatLosesPrecision),
        (Some(8192), Encoded::String { .. }, _) => {
            Some(ScalarDiagnostic::NumericStringLosesPrecision)
        }
        (Some(2), Encoded::Float { .. }, PhpTarget::Bytes) => Some(ScalarDiagnostic::NanToBytes),
        (Some(2), Encoded::Float { .. }, PhpTarget::Bool | PhpTarget::IntBool) => {
            Some(ScalarDiagnostic::NanToBool)
        }
        _ => return Err(fixture("无法识别 PHP 诊断")),
    })
}

#[tokio::test]
async fn fixed_php_scalar_matrix_runs_through_resolver_with_both_diagnostic_policies() -> Result<()>
{
    let rows: Vec<Observation> =
        serde_json::from_str(include_str!("php_scalar_oracle.json")).map_err(fixture)?;
    assert_eq!(rows.len(), 231);
    for Observation(id, input, expected, severity, framework_throws) in rows {
        let (name, _) = id.split_once('/').ok_or_else(|| fixture(&id))?;
        let (target, nullable) = target(name)?;
        let expected_event = expected_diagnostic(&input, target, severity)?;
        for raises_warning in [false, true] {
            let mut param = ConstructorParam::php("value", target);
            if nullable {
                param = param.nullable();
            }
            let constructor_calls = Arc::new(AtomicUsize::new(0));
            let calls = Arc::clone(&constructor_calls);
            let mut builder =
                ContainerBuilder::with_class_descriptors(vec![ClassDescriptor::concrete::<
                    ParameterValue,
                >(
                    "Test/Coerced",
                    vec![param],
                    move |args| {
                        calls.fetch_add(1, Ordering::Relaxed);
                        args.value(0).cloned().map(Arc::new)
                    },
                )])?;
            let events = Arc::new(Mutex::new(Vec::new()));
            let observed = Arc::clone(&events);
            builder.scalar_diagnostics(ScalarDiagnostics::new(move |event| {
                lock(&observed).push(event);
                if raises_warning
                    && matches!(
                        event,
                        ScalarDiagnostic::NanToBytes | ScalarDiagnostic::NanToBool
                    )
                {
                    return Err(container_error("fixture_warning", "测试诊断处理器升级警告"));
                }
                Ok(())
            }));
            let container = builder.build();
            let stringable_calls = Arc::new(AtomicUsize::new(0));
            let mut parameters = ParameterOverrides::new();
            parameters.insert(ParameterKey::name("value"), input.input(&stringable_calls)?);
            let result = container.resolve_with::<ParameterValue>(parameters).await;
            let events = lock(&events);
            assert!(
                events.iter().copied().eq(expected_event),
                "{id}: {events:?}"
            );
            let constructed = if raises_warning && framework_throws {
                fail(result, "fixture_warning");
                false
            } else if let Some(expected) = &expected {
                expected.verify(&id, target, result?.as_ref())?;
                true
            } else {
                let code = if matches!(input, Encoded::Null) {
                    "container_parameter_null"
                } else {
                    "container_parameter_type_mismatch"
                };
                fail(result, code);
                false
            };
            assert_eq!(
                constructor_calls.load(Ordering::Relaxed),
                usize::from(constructed),
                "{id}"
            );
            let stringified =
                matches!(input, Encoded::Stringable) && matches!(target, PhpTarget::Bytes);
            assert_eq!(
                stringable_calls.load(Ordering::Relaxed),
                usize::from(stringified),
                "{id}"
            );
        }
    }
    Ok(())
}

#[test]
fn integer_half_open_range_and_raw_float_bits_survive_the_private_boundary() -> Result<()> {
    let min = 0xc3e0_0000_0000_0000;
    let max = 0x43e0_0000_0000_0000;
    let mut diagnostic = verify_no_diagnostic;
    for (bits, expected) in [
        (min, i64::MIN),
        (min - 1, i64::MIN + 1024),
        (max - 1, i64::MAX - 1023),
    ] {
        let value = ParameterValue::new(f64::from_bits(bits));
        let result = PhpTarget::Int.materialize(&value, &mut diagnostic)?;
        assert_eq!(*result.downcast::<i64>()?, expected);
    }
    for bits in [min + 1, max, max + 1] {
        let value = ParameterValue::new(f64::from_bits(bits));
        fail(
            PhpTarget::Int.materialize(&value, &mut diagnostic),
            "container_parameter_type_mismatch",
        );
    }
    let payload = 0xfff8_1234_5678_9abc;
    let value = ParameterValue::new(f64::from_bits(payload));
    assert_eq!(
        PhpTarget::Float
            .materialize(&value, &mut diagnostic)?
            .downcast::<f64>()?
            .to_bits(),
        payload
    );
    assert!(matches!(
        *PhpTarget::Number.materialize(&value, &mut diagnostic)?.downcast::<InputNumber>()?,
        InputNumber::Float(number) if number.to_bits() == payload
    ));
    Ok(())
}

#[test]
fn selected_defaults_and_override_lookup_precede_all_weak_conversion() -> Result<()> {
    fn failed_selection() -> Result<ParameterValue> {
        Err(container_error("fixture_selection", "晚槽选参失败"))
    }

    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&constructor_calls);
    let registry = captured(
        vec![
            ConstructorParam::php("first", PhpTarget::Bytes),
            ConstructorParam::php("late", PhpTarget::Int).with_default(failed_selection),
        ],
        move |args| {
            calls.fetch_add(1, Ordering::Relaxed);
            args.value(1).cloned()
        },
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let stringable_calls = Arc::new(AtomicUsize::new(0));
    let mut overrides = ParameterOverrides::new();
    assert!(
        overrides
            .insert(
                ParameterKey::name("first"),
                Encoded::Stringable.input(&stringable_calls)?
            )
            .is_none()
    );
    assert!(
        overrides
            .insert(ParameterKey::Index(1), ParameterValue::new(999_i64))
            .is_none()
    );

    // 这是测试选参方，生产 Resolver 及 contextual 尚未接入。
    let select = |overrides: &ParameterOverrides| -> Result<Vec<Argument>> {
        prepared
            .parameters()
            .iter()
            .map(|param| {
                if let Some(argument) = param.override_argument(overrides) {
                    Ok(argument)
                } else {
                    param
                        .evaluate_default()?
                        .map(Argument::One)
                        .ok_or_else(|| fixture("缺少已选实参"))
                }
            })
            .collect()
    };
    let mut diagnostics = Vec::new();
    let result = select(&overrides)
        .and_then(|frame| prepared.construct_selected(&frame, &mut record(&mut diagnostics)));
    fail(result, "fixture_selection");
    assert_eq!(stringable_calls.load(Ordering::Relaxed), 0);
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 0);
    assert_eq!(diagnostics, []);

    assert!(
        overrides
            .insert(ParameterKey::name("late"), ParameterValue::null())
            .is_none()
    );
    let selected = select(&overrides)?;
    fail(
        prepared.construct_selected(&selected, &mut record(&mut diagnostics)),
        "container_parameter_null",
    );
    assert_eq!(stringable_calls.load(Ordering::Relaxed), 1);
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 0);
    assert_eq!(diagnostics, []);

    let param = ConstructorParam::php("value", PhpTarget::Int)
        .with_default(|| Ok(ParameterValue::new("2.75".to_owned())));
    let raw = param
        .evaluate_default()?
        .ok_or_else(|| fixture("缺少默认值"))?;
    assert_eq!(raw.downcast::<String>()?.as_str(), "2.75");
    let registry = captured(vec![param], |args| args.value(0).cloned())?;
    let prepared = registry.prepare(&key, &key)?;
    let value = read_result(
        prepared
            .construct_selected(&[Argument::One(raw.clone())], &mut record(&mut diagnostics))?,
    )?;
    assert_eq!(*value.downcast::<i64>()?, 2);
    assert_eq!(raw.downcast::<String>()?.as_str(), "2.75");
    assert_eq!(diagnostics, [ScalarDiagnostic::NumericStringLosesPrecision]);
    Ok(())
}

#[test]
fn matched_int_union_diagnostic_failure_prevents_fallback_and_later_stringable() -> Result<()> {
    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&constructor_calls);
    let registry = captured(
        vec![
            ConstructorParam::php("first", PhpTarget::IntBool),
            ConstructorParam::php("last", PhpTarget::Bytes),
        ],
        move |args| {
            calls.fetch_add(1, Ordering::Relaxed);
            args.value(0).cloned()
        },
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let stringable_calls = Arc::new(AtomicUsize::new(0));
    let selected = [
        one("2.75".to_owned()),
        Argument::One(Encoded::Stringable.input(&stringable_calls)?),
    ];
    let mut seen = Vec::new();
    fail(
        prepared.construct_selected(&selected, &mut |diagnostic| {
            seen.push(diagnostic);
            Err(container_error("fixture_diagnostic", "保留诊断处理失败"))
        }),
        "fixture_diagnostic",
    );
    assert_eq!(seen, [ScalarDiagnostic::NumericStringLosesPrecision]);
    assert_eq!(stringable_calls.load(Ordering::Relaxed), 0);
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 0);

    let result = read_result(prepared.construct_selected(&selected, &mut record(&mut seen))?)?;
    assert_eq!(*result.downcast::<PhpIntBool>()?, PhpIntBool::Int(2));
    assert_eq!(seen, [ScalarDiagnostic::NumericStringLosesPrecision; 2]);
    assert_eq!(stringable_calls.load(Ordering::Relaxed), 1);
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn stringable_error_preserves_source_and_stops_before_constructor() -> Result<()> {
    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&constructor_calls);
    let registry = captured(
        vec![ConstructorParam::php("value", PhpTarget::Bytes)],
        move |args| {
            calls.fetch_add(1, Ordering::Relaxed);
            args.value(0).cloned()
        },
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let called = Arc::new(AtomicUsize::new(0));
    let capture = Arc::clone(&called);
    let source = Arc::new(PhpStringable(Box::new(move || {
        capture.fetch_add(1, Ordering::Relaxed);
        Err(container_error(
            "fixture_stringable",
            "保留 Stringable 业务错误",
        ))
    })));
    let selected = [Argument::One(ParameterValue::from_arc(Arc::clone(&source)))];
    fail(
        prepared.construct_selected(&selected, &mut verify_no_diagnostic),
        "fixture_stringable",
    );
    assert_eq!(called.load(Ordering::Relaxed), 1);
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 0);
    assert_eq!(Arc::strong_count(&source), 2);
    drop(selected);
    assert_eq!(Arc::strong_count(&source), 1);
    Ok(())
}

#[test]
fn many_preserves_element_order_and_stops_at_first_failed_conversion() -> Result<()> {
    let constructor_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&constructor_calls);
    let sequence = Arc::new(Mutex::new(Vec::new()));
    let constructor_sequence = Arc::clone(&sequence);
    let registry = captured(
        vec![
            ConstructorParam::php("first", PhpTarget::Bytes),
            ConstructorParam::php("items", PhpTarget::Int).variadic(),
        ],
        move |args| {
            calls.fetch_add(1, Ordering::Relaxed);
            constructor_sequence
                .lock()
                .map_err(fixture)?
                .push("construct");
            assert_eq!(args.value(0)?.downcast::<PhpBytes>()?.0.as_ref(), b"first");
            let items = args
                .values(1)?
                .iter()
                .map(|value| value.downcast::<i64>().map(|value| *value))
                .collect::<Result<Vec<_>>>()?;
            Ok(ParameterValue::new(items))
        },
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let stringable_sequence = Arc::clone(&sequence);
    let first = ParameterValue::new(PhpStringable(Box::new(move || {
        stringable_sequence
            .lock()
            .map_err(fixture)?
            .push("stringable");
        Ok(PhpBytes(Arc::from(&b"first"[..])))
    })));
    let mut diagnostic = |event| {
        let kind = match event {
            ScalarDiagnostic::FloatLosesPrecision => "float",
            ScalarDiagnostic::NumericStringLosesPrecision => "numeric_string",
            _ => return Err(fixture("意外的转换诊断")),
        };
        sequence.lock().map_err(fixture)?.push(kind);
        Ok(())
    };

    let values = vec![
        ParameterValue::new(-2.75_f64),
        ParameterValue::new("3.25".to_owned()),
        ParameterValue::new(Ordinary),
        ParameterValue::new("5.25".to_owned()),
    ];
    fail(
        prepared.construct_selected(
            &[Argument::One(first.clone()), Argument::Many(values)],
            &mut diagnostic,
        ),
        "container_parameter_type_mismatch",
    );
    assert_eq!(
        *sequence.lock().map_err(fixture)?,
        ["stringable", "float", "numeric_string"]
    );
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 0);
    sequence.lock().map_err(fixture)?.clear();

    let values = vec![
        ParameterValue::new(-2.75_f64),
        ParameterValue::new("3.25".to_owned()),
    ];
    let result = read_result(prepared.construct_selected(
        &[Argument::One(first.clone()), Argument::Many(values)],
        &mut diagnostic,
    )?)?;
    assert_eq!(result.downcast::<Vec<i64>>()?.as_slice(), [-2, 3]);
    assert_eq!(
        *sequence.lock().map_err(fixture)?,
        ["stringable", "float", "numeric_string", "construct"]
    );
    let result = read_result(prepared.construct_selected(
        &[Argument::One(first.clone()), one("9".to_owned())],
        &mut diagnostic,
    )?)?;
    assert_eq!(result.downcast::<Vec<i64>>()?.as_slice(), [9]);
    fail(
        prepared.construct_selected(
            &[Argument::One(first.clone()), one(vec![1_i64, 2])],
            &mut diagnostic,
        ),
        "container_parameter_type_mismatch",
    );
    let result = read_result(prepared.construct_selected(
        &[Argument::One(first), Argument::Many(vec![])],
        &mut diagnostic,
    )?)?;
    assert!(result.downcast::<Vec<i64>>()?.is_empty());
    assert_eq!(constructor_calls.load(Ordering::Relaxed), 3);
    Ok(())
}

#[test]
fn exact_class_untyped_and_explicit_bytes_preserve_arc_in_mixed_frame() -> Result<()> {
    struct Object;
    let exact = Arc::new("unchanged".to_owned());
    let class = Arc::new(Object);
    let opaque = Arc::new(Ordinary);
    let bytes = Arc::new(PhpBytes(Arc::from(&b"a\0\xff"[..])));
    let (exact_expected, class_expected, opaque_expected, bytes_expected) = (
        Arc::clone(&exact),
        Arc::clone(&class),
        Arc::clone(&opaque),
        Arc::clone(&bytes),
    );
    let registry = captured(
        vec![
            ConstructorParam::scalar::<String>("exact"),
            ConstructorParam::class::<Object>("class"),
            ConstructorParam::untyped("opaque"),
            ConstructorParam::php("bytes", PhpTarget::Bytes),
        ],
        move |args| {
            assert!(Arc::ptr_eq(
                &args.value(0)?.downcast::<String>()?,
                &exact_expected
            ));
            assert!(Arc::ptr_eq(&args.service::<Object>(1)?, &class_expected));
            assert!(Arc::ptr_eq(
                &args.value(2)?.downcast::<Ordinary>()?,
                &opaque_expected
            ));
            assert!(Arc::ptr_eq(
                &args.value(3)?.downcast::<PhpBytes>()?,
                &bytes_expected
            ));
            args.value(3).cloned()
        },
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let mut selected = [
        Argument::One(ParameterValue::from_arc(exact)),
        one(class),
        Argument::One(ParameterValue::from_arc(opaque)),
        Argument::One(ParameterValue::from_arc(Arc::clone(&bytes))),
    ];
    fail(
        prepared.construct(&selected),
        "container_constructor_weak_frame_required",
    );
    let actual = read_result(prepared.construct_selected(&selected, &mut verify_no_diagnostic)?)?;
    assert!(Arc::ptr_eq(&actual.downcast::<PhpBytes>()?, &bytes));
    assert_eq!(actual.downcast::<PhpBytes>()?.0.as_ref(), b"a\0\xff");
    selected[0] = one(7_i64);
    fail(
        prepared.construct_selected(&selected, &mut verify_no_diagnostic),
        "container_parameter_type_mismatch",
    );
    Ok(())
}

#[test]
fn malformed_selected_frame_fails_before_conversion_effects() -> Result<()> {
    let registry = captured(
        vec![
            ConstructorParam::php("first", PhpTarget::Bytes),
            ConstructorParam::php("late", PhpTarget::Int),
        ],
        |args| args.value(0).cloned(),
    )?;
    let key = ServiceKey::typed::<ParameterValue>();
    let prepared = registry.prepare(&key, &key)?;
    let stringable_calls = Arc::new(AtomicUsize::new(0));
    let source = Encoded::Stringable.input(&stringable_calls)?;
    fail(
        prepared.construct_selected(&[Argument::One(source.clone())], &mut verify_no_diagnostic),
        "container_constructor_argument_count",
    );
    fail(
        prepared.construct_selected(
            &[Argument::One(source), Argument::Many(vec![])],
            &mut verify_no_diagnostic,
        ),
        "container_constructor_argument_shape",
    );
    assert_eq!(stringable_calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn utf8_string_is_weak_input_only_after_explicit_target_selection() -> Result<()> {
    let source = Arc::new("字\0节".to_owned());
    let value = ParameterValue::from_arc(Arc::clone(&source));
    let converted = PhpTarget::Bytes.materialize(&value, &mut verify_no_diagnostic)?;
    assert_eq!(
        converted.downcast::<PhpBytes>()?.0.as_ref(),
        source.as_bytes()
    );
    assert!(Arc::ptr_eq(&value.downcast::<String>()?, &source));
    let converted = PhpTarget::Bool.materialize(&value, &mut verify_no_diagnostic)?;
    assert!(*converted.downcast::<bool>()?);
    fail(
        PhpTarget::Int.materialize(&value, &mut verify_no_diagnostic),
        "container_parameter_type_mismatch",
    );
    fail(
        PhpTarget::Bytes.materialize(
            &ParameterValue::new(vec![0xff_u8]),
            &mut verify_no_diagnostic,
        ),
        "container_parameter_type_mismatch",
    );
    Ok(())
}
