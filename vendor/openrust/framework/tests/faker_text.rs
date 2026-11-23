use openrust::database::eloquent::factories::fake;

#[test]
fn text_respects_bounds_and_locale_fallback() {
    for locale in [
        "ar-EG", "ar-JO", "ar-SA", "cs-CZ", "de-AT", "de-CH", "de-DE", "el-GR", "en-US", "es-ES",
        "fr-CA", "fr-CH", "fr-FR", "hu-HU", "it-CH", "it-IT", "ja-JP", "ka-GE", "kk-KZ", "ko-KR",
        "nl-BE", "nl-NL", "pl-PL", "pt-BR", "ro-MD", "ro-RO", "ru-RU", "uk-UA", "zh-Hans",
        "zh-Hant",
    ] {
        let faker = fake(Some(locale)).unwrap();
        let text = faker
            .real_text_between(Some(50), Some(120), None)
            .unwrap_or_else(|error| panic!("{locale}: {error}"));
        assert!(
            (51..=120).contains(&text.chars().count()),
            "{locale}: {text}"
        );
        match locale {
            "zh-Hant" => assert!(text.ends_with(['。', '」', '』', '！', '？'])),
            "ja-JP" => assert!(text.ends_with('。')),
            "zh-Hans" => assert!(text.is_ascii()),
            _ => assert!(text.ends_with('.')),
        }
    }

    let faker = fake(None).unwrap();
    for index in 1..=5 {
        let text = faker.real_text(Some(150), Some(index)).unwrap();
        assert!((121..=150).contains(&text.chars().count()));
    }
    for (min, max, index) in [(0, 20, 2), (2, 9, 2), (20, 20, 2), (2, 20, 0), (2, 20, 6)] {
        assert!(
            faker
                .real_text_between(Some(min), Some(max), Some(index))
                .is_err()
        );
    }
    let unique = faker.unique(true);
    let first = unique.real_text(None, None).unwrap();
    assert_ne!(unique.real_text(None, None).unwrap(), first);
}

#[test]
fn real_text_keeps_the_persian_byte_limit_and_czech_quote_cleanup() {
    let persian = fake(Some("fa-IR")).unwrap();
    assert!(persian.real_text_between(None, None, None).is_err());
    for _ in 0..20 {
        let text = persian.real_text(Some(80), None).unwrap();
        assert!(text.ends_with('.') && text.len() <= 80);
    }
    let czech = fake(Some("cs-CZ")).unwrap();
    for _ in 0..20 {
        assert!(!czech.real_text(None, None).unwrap().contains(['„', '“']));
    }
}
