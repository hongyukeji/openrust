//! 字段执行与过滤回归；不以这些单测替代真实 HTTP 或完整 Laravel 验证器验收。
use super::{InputContext, Rule, RuleError, Rules, Validate, validate, validate_input};
use crate::core::{Error, Result, ValidationErrors};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn errors(result: Result<()>) -> Result<ValidationErrors> {
    match result {
        Err(Error::Validation(errors)) => Ok(errors),
        Err(error) => Err(error),
        Ok(()) => Err(Error::custom(
            "test_validation_unexpected_success",
            "预期字段验证失败",
        )),
    }
}

#[derive(Serialize)]
struct Address {
    city: String,
}
#[derive(Serialize)]
struct Registration {
    email: String,
    address: Address,
}
impl Validate for Registration {
    fn rules() -> Rules {
        Rules::new()
            .field("email", [Rule::required(), Rule::email()])
            .field(
                "address.city",
                [
                    Rule::required(),
                    Rule::string(),
                    Rule::length(Some(3), None),
                ],
            )
    }
}

// 防止跨协议 validate(T) 丢失既有字段消息与嵌套路径。
#[test]
fn validates_application_dtos_and_preserves_nested_field_paths() -> Result<()> {
    let result = validate(Registration {
        email: "invalid".into(),
        address: Address { city: "x".into() },
    });
    let failures = match result {
        Err(Error::Validation(failures)) => failures,
        Err(error) => return Err(error),
        Ok(_) => {
            return Err(Error::custom(
                "test_validation_unexpected_success",
                "预期 DTO 校验失败",
            ));
        }
    };
    let paths: Vec<_> = failures.iter().map(|(path, _)| path).collect();
    // 与固定Laravel MessageBag一致，保留规则声明顺序，而不是字母排序。
    assert_eq!(paths, ["email", "address.city"]);
    Ok(())
}

// 防止校验成功替换或丢失原 DTO。
#[test]
fn returns_valid_values_without_copying() -> Result<()> {
    let value = Registration {
        email: "person@example.test".into(),
        address: Address {
            city: "Beijing".into(),
        },
    };
    let value = validate(value)?;
    assert_eq!(value.email, "person@example.test");
    assert_eq!(value.address.city, "Beijing");
    Ok(())
}

struct RawOnly;
impl Validate for RawOnly {
    fn rules() -> Rules {
        Rules::new().field("title", [Rule::required()])
    }
}

// 防止先执行全部 custom 再按字段过滤错误，或未提交完整 DTO 就提前反序列化。
#[test]
fn selected_raw_fields_skip_unselected_required_and_custom_rules() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let capture = Arc::clone(&calls);
    let rules = Rules::new()
        .field("title", [Rule::required(), Rule::string(), Rule::min(3)])
        .field(
            "body",
            [
                Rule::required(),
                Rule::custom(move |_| {
                    capture.fetch_add(1, Ordering::SeqCst);
                    Err(RuleError::new("body_must_not_run"))
                }),
            ],
        );
    rules.validate_only(&json!({"title":"valid"}), &["title"], InputContext::Json)?;
    rules.validate_only(
        &json!({"title":"valid","body":"not selected"}),
        &["title"],
        InputContext::Json,
    )?;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let failures = errors(rules.validate(
        &json!({"title":"valid","body":"selected"}),
        InputContext::Json,
    ))?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        failures.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["body"]
    );
    // RawOnly 无 Deserialize 实现，原始验证不应要求构造完整 DTO。
    validate_input::<RawOnly>(&json!({"title":"valid"}), InputContext::Json)?;
    Ok(())
}

#[derive(Serialize)]
struct LegacyText<const MIN: u64> {
    value: String,
}
impl<const MIN: u64> Validate for LegacyText<MIN> {
    fn rules() -> Rules {
        Rules::new().field("value", [Rule::length(Some(MIN), None)])
    }
}

// 防止兼容 length 被 Laravel 空串 gating 绕过；空白长度仍按原始字符计数。
#[test]
fn legacy_length_validates_constructed_empty_and_short_blank_strings() -> Result<()> {
    errors(
        validate(LegacyText::<1> {
            value: String::new(),
        })
        .map(|_| ()),
    )?;
    for value in ["", " ", " \t"] {
        let failures = errors(
            validate(LegacyText::<3> {
                value: value.into(),
            })
            .map(|_| ()),
        )?;
        assert_eq!(
            failures.iter().map(|(path, _)| path).collect::<Vec<_>>(),
            ["value"]
        );
    }
    let value = validate(LegacyText::<3> {
        value: "   ".into(),
    })?;
    assert_eq!(value.value, "   ");
    for context in [InputContext::Typed, InputContext::Json, InputContext::Query] {
        assert!(
            Rules::new()
                .field("value", [Rule::length(Some(1), None)])
                .validate(&json!({"value":""}), context)
                .is_err()
        );
        assert!(
            Rules::new()
                .field("value", [Rule::range(Some(1), None)])
                .validate(&json!({"value":" "}), context)
                .is_err()
        );
    }
    Ok(())
}

// 防止兼容规则修复扩大到原生 min/max/custom，或先执行未选规则再丢弃错误。
#[test]
fn native_empty_gating_and_unselected_legacy_custom_execution_remain_unchanged() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let capture = Arc::clone(&calls);
    let custom = Rule::custom(move |_| {
        capture.fetch_add(1, Ordering::SeqCst);
        Err(RuleError::new("must_not_run"))
    });
    let native = Rules::new().field("value", [Rule::min(1), Rule::max(0), custom.clone()]);
    let selected = Rules::new()
        .field("title", [Rule::required(), Rule::string()])
        .field(
            "legacy",
            [
                Rule::length(Some(3), None),
                Rule::range(Some(1), None),
                custom,
            ],
        );
    for context in [InputContext::Typed, InputContext::Json, InputContext::Query] {
        for value in ["", " \t"] {
            native.validate(&json!({"value":value}), context)?;
        }
        selected.validate_only(&json!({"title":"valid","legacy":"x"}), &["title"], context)?;
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    errors(selected.validate(&json!({"title":"valid","legacy":"x"}), InputContext::Typed))?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    Ok(())
}

// 防止数组展开后未选的索引仍触发 custom，或漏掉现有行内缺失叶的 required。
#[test]
fn expanded_wildcard_rules_select_concrete_indices_before_custom_execution() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let capture = Arc::clone(&calls);
    let rules = Rules::new().field(
        "users.*.name",
        [
            Rule::required(),
            Rule::custom(move |field| {
                assert_eq!(field.attribute, "users.1.name");
                capture.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        ],
    );
    rules.validate_only(
        &json!({"users":[{"name":"ignored"},{"name":"chosen"}]}),
        &["users.1.name"],
        InputContext::Json,
    )?;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let failures = errors(
        Rules::new()
            .field("users.*.name", [Rule::required()])
            .validate_only(
                &json!({"users":[{}]}),
                &["users.*.name"],
                InputContext::Json,
            ),
    )?;
    assert_eq!(
        failures.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["users.0.name"]
    );
    Ok(())
}

// 防止把单段星号变成跨层 globstar、忽略模式空格或破坏转义点字段。
#[test]
fn selection_patterns_preserve_segments_spaces_and_escaped_literal_dots() -> Result<()> {
    let input = json!({"users":[{"name":"x"}],"escaped.dot":"x"," field ":"x"});
    let rules = Rules::new()
        .field("users.*.name", [Rule::min(3)])
        .field(r"escaped\.dot", [Rule::min(3)])
        .field(" field ", [Rule::min(3)]);
    rules.validate_only(&input, &["users.*"], InputContext::Json)?;
    let nested = errors(rules.validate_only(&input, &["users.*.*"], InputContext::Json))?;
    assert_eq!(
        nested.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["users.0.name"]
    );
    let escaped = errors(rules.validate_only(&input, &[r"escaped\.dot"], InputContext::Json))?;
    assert_eq!(
        escaped.iter().map(|(key, _)| key).collect::<Vec<_>>(),
        ["escaped.dot"]
    );
    rules.validate_only(&input, &["field"], InputContext::Json)?;
    assert!(
        rules
            .validate_only(&input, &[" field "], InputContext::Json)
            .is_err()
    );
    rules.validate_only(&input, &[""], InputContext::Json)?;
    assert!(rules.validate(&input, InputContext::Json).is_err());
    let double = Rules::new().field("users.*.name", [Rule::min(3)]);
    double.validate_only(
        &json!({"users":{"a":{"name":"x"}}}),
        &["users.**.name"],
        InputContext::Json,
    )?;
    assert!(
        double
            .validate_only(
                &json!({"users":{"ab":{"name":"x"}}}),
                &["users.**.name"],
                InputContext::Json
            )
            .is_err()
    );
    Ok(())
}

// 防止 nullable 覆盖 required，或 missing/空串/nullable-null 执行普通 custom。
#[test]
fn nullable_required_and_nonimplicit_custom_keep_presence_semantics() -> Result<()> {
    let calls = Arc::new(AtomicUsize::new(0));
    let custom = Rule::custom({
        let calls = Arc::clone(&calls);
        move |_| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    });
    let optional = Rules::new().field("value", [Rule::nullable(), custom.clone()]);
    for raw in [json!({}), json!({"value":null}), json!({"value":" \t"})] {
        optional.validate(&raw, InputContext::Json)?;
    }
    let required = Rules::new().field("value", [Rule::nullable(), Rule::required(), custom]);
    for raw in [json!({}), json!({"value":null}), json!({"value":""})] {
        assert!(required.validate(&raw, InputContext::Json).is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Rules::new()
        .field("zero", [Rule::required()])
        .field("false", [Rule::required()])
        .validate(&json!({"zero":0,"false":false}), InputContext::Json)?;
    Ok(())
}

// 防止自定义消息被 fallback code 或国际化查找覆盖。
#[test]
fn custom_rule_messages_are_preserved_verbatim() -> Result<()> {
    let failures = errors(
        Rules::new()
            .field(
                "value",
                [Rule::custom(|_| {
                    Err(RuleError::new("custom").with_message("固定字段消息".into()))
                })],
            )
            .validate(&json!({"value":"x"}), InputContext::Json),
    )?;
    assert_eq!(
        failures.iter().collect::<Vec<_>>(),
        [("value", &["固定字段消息".to_owned()][..])]
    );
    Ok(())
}

// 固定 ValidatesAttributes integer/boolean 不应被 Rust JSON typed 限制误替换。
#[test]
fn laravel_integer_and_boolean_rules_are_independent_of_typed_input_context() {
    for context in [InputContext::Json, InputContext::Query, InputContext::Typed] {
        for (value, valid) in [
            (json!(1), true),
            (json!(1.0), true),
            (json!(true), true),
            (json!(false), false),
            (json!("1"), true),
            (json!("+1"), true),
            (json!("-0"), true),
            (json!(" 1 "), true),
            (json!("01"), false),
            (json!("1.0"), false),
            (json!("1e3"), false),
        ] {
            assert_eq!(
                Rules::new()
                    .field("value", [Rule::required(), Rule::integer()])
                    .validate(&json!({"value":value}), context)
                    .is_ok(),
                valid
            );
        }
        for (value, valid) in [
            (json!(true), true),
            (json!(false), true),
            (json!(0), true),
            (json!(1), true),
            (json!("0"), true),
            (json!("1"), true),
            (json!(1.0), false),
            (json!(2), false),
            (json!("true"), false),
        ] {
            assert_eq!(
                Rules::new()
                    .field("value", [Rule::required(), Rule::boolean()])
                    .validate(&json!({"value":value}), context)
                    .is_ok(),
                valid
            );
        }
    }
}

// 防止原生 min/max 经 f64 舍入大整数，或忽略数值字符串/字符串与数组的不同 size。
#[test]
fn native_min_max_use_exact_decimal_comparison_and_declared_numeric_family() -> Result<()> {
    let rules = Rules::new().field("value", [Rule::integer(), Rule::min(9_007_199_254_740_993)]);
    assert!(
        rules
            .validate(&json!({"value":"9007199254740992"}), InputContext::Json)
            .is_err()
    );
    rules.validate(&json!({"value":"9007199254740993"}), InputContext::Json)?;
    Rules::new()
        .field("value", [Rule::integer(), Rule::max(i64::MAX)])
        .validate(&json!({"value":i64::MAX}), InputContext::Json)?;
    for (value, bound, expected) in [
        ("0.01", 0, std::cmp::Ordering::Greater),
        ("0", 1, std::cmp::Ordering::Less),
        ("-0.1", 0, std::cmp::Ordering::Less),
        ("1.5", 2, std::cmp::Ordering::Less),
        ("1e3", 1000, std::cmp::Ordering::Equal),
    ] {
        assert_eq!(
            super::numeric::compare(&json!(value), bound),
            Ok(Some(expected))
        );
    }
    Rules::new()
        .field("text", [Rule::string(), Rule::min(2), Rule::max(2)])
        .field("items", [Rule::array(), Rule::min(2), Rule::max(2)])
        .validate(&json!({"text":"中文","items":[0,1]}), InputContext::Json)?;
    Ok(())
}

// 防止缺少原始字段时注入 serde 默认值，或跨协议已构造默认值跳过同一规则。
#[test]
fn serde_defaults_are_construction_semantics_and_typed_validate_checks_actual_values() -> Result<()>
{
    #[derive(Serialize, Deserialize)]
    struct Page {
        #[serde(default = "default_page")]
        page: u64,
    }
    fn default_page() -> u64 {
        1
    }
    impl Validate for Page {
        fn rules() -> Rules {
            Rules::new().field("page", [Rule::integer(), Rule::range(Some(1), None)])
        }
    }
    validate_input::<Page>(&json!({}), InputContext::Query)?;
    let page: Page = serde_json::from_value(json!({})).map_err(Error::internal)?;
    assert_eq!(validate(page)?.page, 1);
    assert!(validate(Page { page: 0 }).is_err());
    Page::rules().validate(&json!({"page":"2"}), InputContext::Query)?;
    Ok(())
}

// 防止规则、模式、展开和错误预算被静默裁剪而产生错误成功。
#[test]
fn validation_limits_and_invalid_schema_fail_explicitly() -> Result<()> {
    let rule = Rules::new().field("value", [Rule::required()]);
    let patterns = vec!["value"; 129];
    let failure = rule
        .validate_only(&json!({"value":"x"}), &patterns, InputContext::Json)
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期模式预算错误"))?;
    assert_eq!(failure.code(), "validation_limits_exceeded");
    let mut expanded = Rules::new();
    for key in 0..9 {
        expanded = expanded.field(format!("rows.*.field{key}"), [Rule::required()]);
    }
    let raw = json!({"rows":vec![json!({});512]});
    let failure = expanded
        .validate_only(&raw, &["unselected"], InputContext::Json)
        .err()
        .ok_or_else(|| Error::custom("test_expected_error", "预期展开预算错误"))?;
    assert_eq!(failure.code(), "validation_limits_exceeded");
    let duplicate = Rules::new()
        .field("same", [Rule::required()])
        .field("same", [Rule::string()]);
    assert_eq!(
        duplicate
            .validate_only(&json!({}), &[], InputContext::Json)
            .err()
            .map(|e| e.code().to_owned()),
        Some("validation_rules_invalid".into())
    );
    let many = Rules::new().field(
        "value",
        vec![Rule::custom(|_| Err(RuleError::new("custom"))); 1025],
    );
    assert_eq!(
        many.validate(&json!({"value":"x"}), InputContext::Json)
            .err()
            .map(|e| e.code().to_owned()),
        Some("validation_limits_exceeded".into())
    );
    Ok(())
}
