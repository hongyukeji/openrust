use openrust::support::{InflectionLanguage as Language, Str};

#[test]
fn pinned_doctrine_matrix_covers_all_languages() {
    let pairs: Vec<(String, String, String, String)> = serde_json::from_str(include_str!(
        "fixtures/string-inflection-doctrine-2.1.0.json"
    ))
    .unwrap();
    for (language, word, plural, singular) in pairs {
        let language = match language.as_str() {
            "english" => Language::English,
            "esperanto" => Language::Esperanto,
            "french" => Language::French,
            "italian" => Language::Italian,
            "norwegian-bokmal" => Language::NorwegianBokmal,
            "portuguese" => Language::Portuguese,
            "spanish" => Language::Spanish,
            "turkish" => Language::Turkish,
            unknown => panic!("unknown fixture language {unknown}"),
        };
        assert_eq!(
            Str::plural_in(&word, 2, language),
            plural,
            "{language:?} {word}"
        );
        assert_eq!(
            Str::singular_in(&plural, language),
            singular,
            "{language:?} {plural}"
        );
        assert_eq!(Str::plural_in(&word, -1, language), word);
    }
}

#[test]
fn fluent_and_counted_calls_use_explicit_language() {
    assert_eq!(
        Str::of("cheval").plural_in(2, Language::French).to_string(),
        "chevaux"
    );
    assert_eq!(
        Str::plural_pascal_in("TeamCheval", 2, Language::French),
        "TeamChevaux"
    );
    assert_eq!(
        Str::counted_in("cheval", 1250, Language::French),
        "1,250 chevaux"
    );
    assert_eq!(Str::plural("child", 2), "children");
}
