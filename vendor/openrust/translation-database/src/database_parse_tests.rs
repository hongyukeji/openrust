//! 无数据库后端也必须执行的完整JSON值与核心`Catalog`校验回归。

use super::*;
use serde_json::json;

pub(super) fn resource(locale: &str, namespace: Option<&str>) -> Result<TranslationResource> {
    Ok(TranslationResource {
        locale: Locale::new(locale)?,
        group: "*".into(),
        namespace: namespace.map(str::to_owned),
    })
}
pub(super) fn put(resource: &TranslationResource, key: &str, value: Value) -> TranslationMutation {
    TranslationMutation::Put {
        resource: resource.clone(),
        key: key.into(),
        value,
        enabled: true,
    }
}

#[test]
fn extension_uses_core_parser_for_nested_duplicates_and_message_validation() -> Result<()> {
    assert!(
        parse_value(br#"{"select":{"argument":"x","cases":{"other":"a","other":"b"}}}"#).is_err()
    );
    let en = resource("en", None)?;
    assert!(validate_mutation(&put(&en, "invalid", json!(42))).is_err());
    assert!(validate_mutation(&put(&en, "empty", json!(""))).is_ok());
    Ok(())
}

#[test]
fn parse_value_requires_one_complete_json_value_without_accepting_wrapper_members() -> Result<()> {
    for invalid in [
        br#""ok","extra":0"#.as_slice(),
        br#""ok","value":"other""#.as_slice(),
        br#"[],"extra":[]"#.as_slice(),
        br#"{},"extra":{}"#.as_slice(),
        br#"true,"extra":false"#.as_slice(),
        br#""ok" trailing"#.as_slice(),
        br#"{"nested":{"same":1,"same":2}}"#.as_slice(),
    ] {
        assert!(matches!(parse_value(invalid), Err(error)
            if error.code() == "i18n_translation_resource_invalid"));
    }
    for (input, expected) in [
        (br#" "ok" "#.as_slice(), json!("ok")),
        (b"null".as_slice(), Value::Null),
        (b"42".as_slice(), json!(42)),
        (b"true".as_slice(), json!(true)),
        (br#"["one",null,2]"#.as_slice(), json!(["one", null, 2])),
        (
            br#"{"ordinary":[1,2]}"#.as_slice(),
            json!({"ordinary":[1,2]}),
        ),
        (
            br#"{"plural":{"argument":"count","forms":{"one":"one","other":"many"}}}"#.as_slice(),
            json!({"plural":{"argument":"count","forms":{"one":"one","other":"many"}}}),
        ),
    ] {
        assert_eq!(parse_value(input)?, expected);
    }
    // 语法解析不自创消息规则；单值合法性之后仍由唯一 session/Catalog 校验决定。
    let english = resource("en", None)?;
    assert!(validate_mutation(&put(&english, "valid", parse_value(br#""ok""#)?)).is_ok());
    assert!(validate_mutation(&put(&english, "invalid", parse_value(b"42")?)).is_err());
    assert!(
        validate_mutation(&put(
            &english,
            "plural",
            parse_value(
                br#"{"plural":{"argument":"count","forms":{"one":"one","other":"many"}}}"#
            )?
        ))
        .is_ok()
    );
    Ok(())
}
