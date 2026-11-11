use openrust::translation::{MessageSelector, TranslationError};

#[test]
fn choice_applies_ranges_then_locale_rules_and_preserves_exact_integers() {
    let selector = MessageSelector;
    for (message, count, locale, expected) in [
        ("{0} none|{1} one|[2,*] many", 0, "en", "none"),
        ("{0} none|{1} one|[2,*] many", 8, "en", "many"),
        ("[*,-1] negative|[0,*] positive", -5, "en", "negative"),
        ("first|second", 2, "en_US", "second"),
        ("first|second", 2, "zh_TW", "first"),
        ("one|few|many", 22, "ru_RU", "few"),
        ("one|few|many", 11, "ru_RU", "many"),
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
        selector.choose("first|second", f64::NAN, "en"),
        Err(TranslationError::InvalidCount)
    ));
}
