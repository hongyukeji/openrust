use openrust::support::Str;

#[test]
fn literal_editing_matches_pinned_utf8_cases() {
    assert_eq!(Str::padBoth("中", 6, "é🙂"), "é🙂中é🙂é");
    assert_eq!(Str::padLeft("中", 4, "．"), "．．．中");
    assert_eq!(Str::padRight("中", 4, ""), "中");
    assert_eq!(
        Str::replace("é é É ß ẞ", &[("É", "e"), ("ß", "ss")], false),
        "e e e ss ss"
    );
    assert_eq!(Str::replace("K K", &[("k", "$1")], false), "$1 K");
    assert_eq!(Str::replaceArray("?", &["甲"], "?/?/?"), "甲/?/?");
    assert_eq!(
        Str::substrReplace("甲乙丙丁", "🙂", -3, Some(1)),
        "甲🙂丙丁"
    );
    assert_eq!(
        Str::swap("cat dog cab", &[("cat", "dog"), ("c", "k"), ("dog", "cat")]),
        "dog cat kab"
    );
    assert_eq!(
        Str::excerpt("甲乙中心丙丁", "中心", 1, "…").as_deref(),
        Some("…乙中心丙…")
    );
    assert_eq!(Str::excerpt("甲乙丙", "x", 100, "..."), None);
}

#[test]
fn casing_counts_url_and_fluent_forms() {
    assert_eq!(Str::title("jEAN-lUC DON'T"), "Jean-Luc Don't");
    assert_eq!(Str::headline("helloWorld__École"), "Hello World École");
    assert_eq!(
        Str::apa("a tale of two cities: the end"),
        "A Tale of Two Cities: The End"
    );
    assert_eq!(
        Str::of("a<!-- comment --><b title=\"x>y\">b</b><?php echo 1; ?><i>c</i>")
            .stripTags(&["b"])
            .toString(),
        "a<b title=\"x>y\">b</b>c"
    );
    assert_eq!(Str::initials("open 中文", true), "O中");
    assert_eq!(Str::ucfirst("ßeta"), "Sseta");
    assert_eq!(Str::wordCount("中国 foo don't -bar-", Some("0..9")), 3);
    assert!(Str::isUrl("https://example.test/中文", &[]));
    assert!(Str::isUrl("foo://example.test/中文", &["foo"]));
    assert!(!Str::isUrl("https://example.test/%GG", &[]));
    assert!(!Str::isUrl("https://example.test/one two", &[]));
    assert_eq!(
        Str::of("helloWorld__École")
            .whenContains(&["World"], false, |text| text.headline())
            .padRight(20, "．")
            .toString(),
        "Hello World École．．．"
    );
    assert_eq!(
        Str::of("/path/file.txt").basename(".txt").toString(),
        "file"
    );
}
