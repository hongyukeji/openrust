use openrust::translation::{MessageSelector, TranslationError};

#[test]
fn choice_applies_ranges_then_locale_rules_and_preserves_exact_integers() {
    let selector = MessageSelector;
    for (message, count, locale, expected) in [
        ("{0} none|{1} one|[2,*] many", 0, "en", "none"),
        ("{0} none|{1} one|[2,*] many", 8, "en", "many"),
        ("[*,-1] negative|[0,*] positive", -5, "en", "negative"),
        ("first|second", 2, "en-US", "second"),
        ("first|second", 2, "en-US-u-nu-latn", "second"),
        ("first|second", 2, "zh-Hant", "first"),
        ("first|second", 2, "EN-gb", "second"),
        ("first|second", 2, "ZH-hant", "first"),
        ("one|few|many", 22, "ru-RU", "few"),
        ("one|few|many", 11, "ru-RU", "many"),
        (
            "{9007199254740992} first|{9007199254740993} second",
            9_007_199_254_740_993,
            "en",
            "second",
        ),
    ] {
        assert_eq!(selector.choose(message, count, locale).unwrap(), expected);
    }
    assert_eq!(
        selector.choose("{1.5} fraction|other", 1.5, "en").unwrap(),
        "fraction"
    );
    assert!(matches!(
        selector.choose("first|second", 2, "zh_hant"),
        Err(TranslationError::InvalidLocale)
    ));
    assert!(matches!(
        selector.choose("first|second", f64::NAN, "en"),
        Err(TranslationError::InvalidCount)
    ));
}

#[tokio::test]
async fn translator_uses_replaced_selector_and_exposes_its_real_loader() {
    use openrust::contracts::translation::Loader as _;
    use openrust::translation::{
        FileLoader, Line, PluralCount, Selector, TranslationResult, Translator,
    };
    use std::{collections::BTreeMap, sync::Arc};

    struct Custom;
    impl Selector for Custom {
        fn choose(
            &self,
            line: &str,
            count: PluralCount,
            locale: &str,
        ) -> TranslationResult<String> {
            assert_eq!(line, "one|many");
            assert_eq!(count, PluralCount::Integer(2));
            assert_eq!(locale, "en");
            Ok(":count selected".into())
        }
    }
    let resources = tempfile::tempdir().unwrap();
    std::fs::create_dir(resources.path().join("en")).unwrap();
    std::fs::write(
        resources.path().join("en/file.json"),
        r#"{"name":"loaded"}"#,
    )
    .unwrap();
    let translator = Translator::new(
        FileLoader::new(resources.path().to_owned()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    translator
        .add_lines(
            BTreeMap::from([("messages.items".into(), Line::Text("one|many".into()))]),
            "en",
            None,
        )
        .unwrap();
    let loader = translator.get_loader();
    assert!(Arc::ptr_eq(&loader, &translator.get_loader()));
    assert_eq!(
        loader.load("en", "file", None).await.unwrap().get("name"),
        Some(&Line::Text("loaded".into()))
    );
    assert_eq!(
        translator
            .string("file.name", &BTreeMap::new(), None)
            .unwrap(),
        "loaded"
    );
    let original = translator.get_selector();
    assert_eq!(
        translator
            .choose("messages.items", 2_i64, &BTreeMap::new(), None)
            .unwrap(),
        "many"
    );
    translator.set_selector(Custom);
    assert_eq!(
        translator
            .choose("messages.items", 2_i64, &BTreeMap::new(), None)
            .unwrap(),
        "2 selected"
    );
    assert_eq!(
        original.choose("one|many", 2_i64.into(), "en").unwrap(),
        "many"
    );
    translator.set_selector(MessageSelector);
    assert_eq!(
        translator
            .choose("messages.items", 2_i64, &BTreeMap::new(), None)
            .unwrap(),
        "many"
    );
}
