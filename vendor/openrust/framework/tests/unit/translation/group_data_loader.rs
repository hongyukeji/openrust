//! 防止结构化分组被当成无效 ICU 或在捕获时丢失叶值，保留全局来源的严格校验。

use super::{CatalogMessage, MAX_ENTRY_DEPTH, parse_file_lines, parse_resource, validate_lines};
use crate::Result;
use crate::translation::{
    FileTranslationLoader, Locale, MessageArgs, TranslationConfig, TranslationLoader,
    TranslationResource, Translator,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

fn resource(group: &str) -> Result<TranslationResource> {
    Ok(TranslationResource {
        locale: Locale::new("en")?,
        group: group.to_owned(),
        namespace: None,
    })
}

#[test]
fn grouped_resources_preserve_nested_shapes_and_keep_icu_validation() -> Result<()> {
    let group = resource("settings")?;
    let nested = json!({
        "tabs": ["label :name", 7, true, null, {"plural": "opaque nested member"}],
        "empty": {},
        "order": ["left", "right"]
    });
    let lines = BTreeMap::from([
        ("title".to_owned(), json!("")),
        ("nested".to_owned(), nested.clone()),
        ("count".to_owned(), json!(42)),
        ("disabled".to_owned(), json!(false)),
        ("unset".to_owned(), Value::Null),
        (
            "plural".to_owned(),
            json!({"plural":{"argument":"count","forms":{"one":"item","other":"items"}}}),
        ),
        (
            "select".to_owned(),
            json!({"select":{"argument":"mode","cases":{"on":"on","other":"off"}}}),
        ),
    ]);
    validate_lines(&group, &lines)?;
    let parsed = parse_resource(&group, &lines)?;
    assert!(matches!(parsed.get("title"), Some(CatalogMessage::Text(value)) if value.is_empty()));
    assert!(matches!(parsed.get("nested"), Some(CatalogMessage::Data(value)) if value == &nested));
    assert!(
        matches!(parsed.get("count"), Some(CatalogMessage::Data(value)) if value == &json!(42))
    );
    assert!(
        matches!(parsed.get("disabled"), Some(CatalogMessage::Data(value)) if value == &json!(false))
    );
    assert!(matches!(
        parsed.get("unset"),
        Some(CatalogMessage::Data(Value::Null))
    ));
    assert!(matches!(
        parsed.get("plural"),
        Some(CatalogMessage::Plural { .. })
    ));
    assert!(matches!(
        parsed.get("select"),
        Some(CatalogMessage::Select { .. })
    ));

    for malformed in [
        json!({"plural": {"argument": "count"}}),
        json!({"select":{"argument":"mode","cases":{"on":"on"}}}),
        json!({"plural":{"argument":"count","forms":{"other":"items"}},"extra":true}),
    ] {
        let invalid = BTreeMap::from([("entry".to_owned(), malformed)]);
        assert!(matches!(validate_lines(&group, &invalid), Err(error)
                if error.code() == "i18n_translation_resource_invalid"));
    }
    Ok(())
}

#[test]
fn global_entries_keep_strict_types_and_group_depth_is_bounded() -> Result<()> {
    let global = resource("*")?;
    for data in [
        json!({"detail":"nested"}),
        json!(["nested"]),
        json!(42),
        Value::Null,
    ] {
        let lines = BTreeMap::from([("entry".to_owned(), data)]);
        assert!(matches!(validate_lines(&global, &lines), Err(error)
                if error.code() == "i18n_translation_resource_invalid"));
    }

    let group = resource("settings")?;
    let max_depth = (0..MAX_ENTRY_DEPTH).fold(Value::Null, |child, _| Value::Array(vec![child]));
    validate_lines(
        &group,
        &BTreeMap::from([("entry".to_owned(), max_depth.clone())]),
    )?;
    let too_deep = BTreeMap::from([("entry".to_owned(), Value::Array(vec![max_depth]))]);
    assert!(matches!(validate_lines(&group, &too_deep), Err(error)
            if error.code() == "i18n_translation_resource_invalid"));
    Ok(())
}

#[tokio::test]
async fn grouped_file_arrays_preserve_indices_and_duplicate_nested_keys_fail() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("en");
    std::fs::create_dir(&path)?;
    std::fs::write(
        path.join("menu.json"),
        br#"["home", {"links":["profile", 2]}]"#,
    )?;
    std::fs::write(directory.path().join("en.json"), br#"{"global":"line"}"#)?;

    let loader = FileTranslationLoader::new(directory.path());
    let session = loader.begin_snapshot().await?;
    let locale = Locale::new("en")?;
    assert_eq!(
        session.load(&locale, "menu", None)?,
        BTreeMap::from([
            ("0".to_owned(), json!("home")),
            ("1".to_owned(), json!({"links":["profile",2]})),
        ])
    );
    assert_eq!(
        session.load(&locale, "*", None)?,
        BTreeMap::from([("global".to_owned(), json!("line"))])
    );

    for invalid in [
        parse_file_lines(br#"{"entry":{"duplicate":1,"duplicate":2}}"#, "menu"),
        parse_file_lines(br#"["global"]"#, "*"),
        parse_file_lines(b"true", "menu"),
    ] {
        assert!(
            matches!(invalid, Err(error) if error.code() == "i18n_translation_resource_invalid")
        );
    }
    Ok(())
}

#[tokio::test]
async fn grouped_file_text_and_choice_use_nested_paths_without_masking_fallback() -> Result<()> {
    let directory = tempfile::tempdir()?;
    for (file, data) in [
        (
            "fr/menu.json",
            r#"{"nested":{"labels":["Accueil","{1} :count entrée|:count entrées"],"fallback":false},"nested.labels.0":"Priorité","actions":{"select":"Choisir","plural":"Plusieurs"}}"#,
        ),
        ("en/menu.json", r#"{"nested":{"fallback":"Fallback"}}"#),
        ("fr/steps.json", r#"["Start",{"name":"Continue"}]"#),
    ] {
        let path = directory.path().join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)?;
    }
    let fr = Locale::new("fr")?;
    let translator = Translator::from_loader(
        TranslationConfig {
            default_locale: fr.clone(),
            fallback_locale: Locale::english(),
        },
        Arc::new(FileTranslationLoader::new(directory.path())),
    )
    .await?;
    let args = MessageArgs::new();
    for (key, expected) in [
        ("menu.nested.labels.0", "Priorité"),
        ("menu.nested.labels.01", "menu.nested.labels.01"),
        ("menu.nested.fallback", "Fallback"),
        ("steps.1.name", "Continue"),
        ("menu.actions.select", "Choisir"),
        ("menu.actions.plural", "Plusieurs"),
    ] {
        assert_eq!(translator.translate(&fr, key, &args), expected);
    }
    assert_eq!(
        translator.choice("menu.nested.labels.1", 2_i64.into(), &args, None)?,
        "2 entrées",
    );
    assert!(matches!(
        translator.choice("menu.nested.labels", 2_i64.into(), &args, None),
        Err(error) if error.code() == "translation_choice_message_invalid"
    ));
    Ok(())
}
