use openrust::support::Str;

#[test]
fn slug_applies_locale_then_ordered_dictionary_and_separator() {
    assert_eq!(Str::slug("Über Café @ Home"), "uber-cafe-at-home");
    assert_eq!(
        Str::of("Falsches_Üben @ Köln")
            .slugWith("-", Some("DE"), &[("@", "at")])
            .to_string(),
        "falsches-ueben-at-koeln"
    );
    assert_eq!(
        Str::slugWith("中文 @ 你好—世界", "_", None, &[("@", "at")]),
        "中文_at_你好世界"
    );
    assert_eq!(
        Str::slugWith("Αθήνα Şehir", "--", Some("tr"), &[]),
        "athina--sehir"
    );
    assert_eq!(
        Str::slugWith("A+B", "-", Some("en"), &[("+", "x"), ("x", "and")]),
        "a-and-b"
    );
    assert_eq!(
        Str::slugWith("ᾗ ไม้ Русский", "-", Some("el"), &[]),
        "russkii"
    );
    assert_eq!(
        Str::slugWith("中文", "-", Some("unknown"), &[]),
        "zhong-wen"
    );
    assert_eq!(Str::slugWith("", "", None, &[]), "");
}

#[test]
fn strict_transliteration_normalizes_and_uses_script_context() {
    assert_eq!(
        Str::transliterateWithOptions("Привет Ελλάδα 你好 ﬃ", Some("?"), true),
        "Privet Ellada ni hao ffi"
    );
    assert_eq!(
        Str::of("ā á Grüße ① ⓐ")
            .transliterateWithOptions(Some("?"), true)
            .to_string(),
        "a a Grusse 1 a"
    );
    assert_eq!(
        Str::transliterateWithOptions("東京 カタカナ 한글", Some("?"), true),
        "dong jing katakana hangeul"
    );
    assert_eq!(Str::transliterateWithOptions("😀", None, true), "😀");
    assert_eq!(Str::transliterateWithOptions("😀", Some("!"), true), "!");
    assert_eq!(
        Str::transliterateWithOptions("a\tb\nc", Some("?"), true),
        "a\tb\nc"
    );
}
