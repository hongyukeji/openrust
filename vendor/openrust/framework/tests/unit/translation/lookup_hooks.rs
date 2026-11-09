//! 精确 JSON、分组顺序、缺键参数和同步重入边界。
use crate::Result;
use crate::translation::{
    FileTranslationLoader, Locale, MessageArgs, MessageCatalog, MissingReplacements, Replacement,
    TranslationConfig, Translator,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

async fn translator() -> Result<Translator> {
    let dir = tempfile::tempdir()?;
    for (path, data) in [
        (
            "fr.json",
            r#"{"present-empty":"","present-zero":"0","messages.label":"JSON FR"}"#,
        ),
        (
            "fr/messages.json",
            r#"{"empty":"","label":"FR","custom":"FR group"}"#,
        ),
        ("en/messages.json", r#"{"label":"EN","custom":"EN group"}"#),
        (
            "de/messages.json",
            r#"{"label":"DE","fallback":"DE fallback","custom":"DE group"}"#,
        ),
        (
            "fr/plural.json",
            r#"{"items":":count chose|:count choses"}"#,
        ),
    ] {
        let path = dir.path().join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, data)?;
    }
    Translator::from_loader(
        TranslationConfig {
            default_locale: Locale::new("fr")?,
            fallback_locale: Locale::english(),
        },
        Arc::new(FileTranslationLoader::new(dir.path())),
    )
    .await
}

#[tokio::test]
async fn missing_hook_receives_original_context_and_suppresses_recursive_calls() -> Result<()> {
    let translator = translator().await?;
    let args = MessageArgs::from([("name".into(), "ada".into())]);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    // 仅在一次调用中把同一译器交给回调，避免闭包自持有译器。
    let recursive = Arc::new(Mutex::new(Some(translator.clone())));
    let nested = recursive.clone();
    translator.handle_missing_keys_using(move |missing| {
        let MissingReplacements::Text(args) = missing.replacements else {
            return None;
        };
        observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((
                missing.key.to_owned(),
                args.to_vec(),
                missing.locale.as_str().to_owned(),
                missing.fallback,
            ));
        match missing.key {
            "outer" => {
                let nested = nested
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()?;
                let inner = nested
                    .string("inner", &MessageArgs::new(), None, true)
                    .ok()?;
                Some(format!("out[{inner}] :Name"))
            }
            "numbers" => Some("{1} :count thing|[2,*] :count things".into()),
            _ => None,
        }
    });
    assert_eq!(
        translator.string("outer", &args, None, true)?,
        "out[inner] Ada"
    );
    assert!(!translator.has("hidden", None, true));
    let empty = MessageArgs::new();
    assert_eq!(
        translator.string("present-empty", &empty, None, true)?,
        "present-empty"
    );
    assert_eq!(
        translator.string("present-zero", &empty, None, true)?,
        "present-zero"
    );
    assert_eq!(translator.string("messages.empty", &empty, None, true)?, "");
    assert_eq!(
        translator.string("missing :name", &args, Some(&Locale::new("de")?), false)?,
        "missing ada",
    );
    assert_eq!(
        translator.choice("numbers", 2_i64.into(), &args, None)?,
        "2 things"
    );
    let calls = calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        calls.as_slice(),
        [
            (
                "outer".into(),
                vec![("name".into(), "ada".into())],
                "fr".into(),
                true
            ),
            (
                "missing :name".into(),
                vec![("name".into(), "ada".into())],
                "de".into(),
                false
            ),
            ("numbers".into(), vec![], "en".into(), true),
        ],
    );
    translator.clear_missing_key_handler();
    assert_eq!(translator.string("outer", &args, None, true)?, "outer");
    Ok(())
}

#[tokio::test]
async fn exact_json_precedes_resolver_and_choice_keeps_its_chosen_language() -> Result<()> {
    let translator = translator().await?;
    let fr = Locale::new("fr")?;
    let de = Locale::new("de")?;
    let en = Locale::english();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let recorded = calls.clone();
    translator.determine_locales_using(move |locales| {
        recorded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(locales.to_vec());
        vec![de.clone(), de.clone(), fr.clone(), en.clone()]
    });
    let empty = MessageArgs::new();
    assert_eq!(
        translator.string("messages.label", &empty, None, true)?,
        "JSON FR"
    );
    assert_eq!(
        translator.string("messages.custom", &empty, None, true)?,
        "DE group"
    );
    assert_eq!(
        translator.string("messages.fallback", &empty, None, true)?,
        "DE fallback"
    );
    assert_eq!(
        translator.string("messages.fallback", &empty, None, false)?,
        "messages.fallback",
    );
    assert!(!translator.has_for_locale("messages.fallback", None));
    assert!(translator.has("messages.fallback", None, true));
    assert_eq!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len(),
        3
    );
    let expected = [Locale::new("fr")?, Locale::english()];
    assert!(
        calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .all(|input| *input == expected)
    );

    let fr = Locale::new("fr")?;
    translator.determine_locales_using(move |_| vec![fr.clone()]);
    // 实际资源来自 fr，但显式选定 en 的复数规则，0 取复数。
    assert_eq!(
        translator.choice(
            "plural.items",
            0_i64.into(),
            &empty,
            Some(&Locale::english())
        )?,
        "0 choses",
    );
    translator.clear_locale_resolver();
    assert_eq!(
        translator.string("messages.custom", &empty, None, true)?,
        "FR group"
    );
    Ok(())
}

#[test]
fn mixed_replacements_are_borrowed_and_unwind_releases_the_reentry_guard() -> Result<()> {
    struct Label(&'static str);
    let translator = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    translator.handle_missing_keys_using(|_| {
        std::panic::resume_unwind(Box::new("callback panic fixture"))
    });
    let empty = MessageArgs::new();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            translator.get("missing", &empty, None, true)
        }))
        .is_err()
    );

    translator.handle_missing_keys_using(|missing| {
        let MissingReplacements::Mixed(args) = missing.replacements else {
            return None;
        };
        assert_eq!(args[0].0, "mark");
        assert!(args[0].1.as_tag().is_some());
        assert_eq!(args[1].1.as_text(), Some("ada"));
        assert_eq!(args[2].1.as_object::<Label>()?.0, "label");
        Some("<mark>:name</mark>".into())
    });
    let label = Label("label");
    translator.stringable::<Label>(|label| label.0.to_owned());
    assert_eq!(
        translator.string_with(
            "missing",
            [
                ("mark", Replacement::tag(|part| format!("[{part}]"))),
                ("name", "ada".into()),
                ("object", Replacement::object(&label)),
            ],
            None,
            true,
        )?,
        "[ada]",
    );
    Ok(())
}

#[test]
fn a_lookup_hook_can_register_resources_without_holding_the_catalog_lock() -> Result<()> {
    let translator = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let original = translator.lookup_snapshot();
    let dir = Arc::new(tempfile::tempdir()?);
    std::fs::write(dir.path().join("en.json"), r#"{"name":"published"}"#)?;
    let hooks = Arc::new(Mutex::new(Some(translator.clone())));
    let installed = Arc::new(AtomicBool::new(false));
    let observed = installed.clone();
    translator.determine_locales_using(move |locales| {
        let nested = hooks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(nested) = nested {
            let result = nested.add_namespace("new", dir.path(), dir.path());
            observed.store(result.is_ok(), Ordering::Relaxed);
        }
        locales.to_vec()
    });
    let args = MessageArgs::new();
    assert_eq!(
        translator.string("new::name", &args, None, true)?,
        "new::name"
    );
    assert!(installed.load(Ordering::Relaxed));
    assert!(
        original
            .json_line_at(&Locale::english(), "new::name")
            .is_none()
    );
    assert_eq!(
        translator.string("new::name", &args, None, true)?,
        "published"
    );
    Ok(())
}

#[test]
fn has_suppresses_missing_callbacks_inside_the_locale_resolver() -> Result<()> {
    let i18n = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let reentries = Arc::new(Mutex::new(vec![i18n.clone(), i18n.clone()]));
    let observed = Arc::new(Mutex::new(Vec::new()));
    let received = observed.clone();
    i18n.handle_missing_keys_using(|missing| {
        (missing.key == "nested").then(|| "from handler".into())
    });
    i18n.determine_locales_using(move |locales| {
        let nested = reentries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop();
        if let Some(nested) = nested {
            received
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(nested.get("nested", &MessageArgs::new(), None, false));
        }
        locales.to_vec()
    });
    assert!(!i18n.has("outer", None, true));
    assert_eq!(
        i18n.string("outer", &MessageArgs::new(), None, true)?,
        "outer"
    );
    assert_eq!(
        observed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_slice(),
        [
            serde_json::json!("nested"),
            serde_json::json!("from handler")
        ],
    );
    Ok(())
}
