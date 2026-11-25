use openrust_pulse::recorders::concerns::*;
use serde_json::json;

#[test]
fn php_pcre_groups_retry_empty_matches_and_preserve_replacement_escaping() {
    for (pattern, subject, replacement, expected) in [
        (r"/(?=a)|a/", "a", "X", "XX"),
        (r"/a*?/", "a", "X", "XXX"),
        (r"/(?=é)|é/u", "é", "X", "XX"),
        (r"/(?=)/u", "é", "X", "XéX"),
        (r"/(*CRLF)(?=)/", "\r\n", "X", "X\rX\nX"),
        (r"/(?=(a))|(a)/", "a", "[${1}:${2}]", "[a:][:a]"),
        (r"/(a)/", "a", r"\{1}", r"\{1}"),
        (r"/(a)/", "a", r"\$1", "$1"),
        (r"/(a)/", "a", r"\q", r"\q"),
        (r"/(a)/", "a", "$$1", "$a"),
    ] {
        assert_eq!(
            group(&json!([[pattern, replacement]]), subject).unwrap(),
            expected,
            "{pattern:?} {replacement:?}"
        );
    }
    // 字节模式能匹配 UTF-8 码点内部，无法将该替换结果用作文本存储键。
    assert!(group(&json!([[r"/(?=)/", "X"]]), "é").is_err());
}

#[test]
fn php_pcre_modifiers_preserve_compilation_and_matching_options() {
    for (pattern, subject, expected) in [
        (r"/(*UTF)foo/A", "foo", true),
        (r"/(*UTF)foo/A", "xfoo", false),
        (r"/foo$/D", "foo\n", false),
        (r"/foo$/Dm", "foo\nbar", true),
        (r"/(?<value>a)|(?<value>b)/J", "b", true),
        (r"/foo/SX", "foo", true),
        (r"/^\w+$/", "中文", false),
        (r"/^\w+$/u", "中文", true),
        (r"/K/iu", "K", true),
    ] {
        assert_eq!(matches(pattern, subject).unwrap(), expected, "{pattern:?}");
    }
    let restricted = matches(r"/K/iur", "K");
    if pcre2::bytes::Regex::new("(?r)K").is_ok() {
        assert!(!restricted.unwrap());
    } else {
        assert!(restricted.is_err());
    }
    assert_eq!(
        group(&json!([[r"/(foo)(?<word>bar)/n", "$1"]]), "foobar").unwrap(),
        "bar"
    );
    assert_eq!(group(&json!([[r"/<.*>/U", "x"]]), "<a><b>").unwrap(), "xx");
    assert_eq!(
        threshold(&json!([[r"/slow$/D", 10], ["default", 100]]), "slow\n").unwrap(),
        100.0
    );
}

#[test]
fn php_pcre_delimiters_preserve_escaping_and_modifier_whitespace() {
    for pattern in [" /foo/i", "/foo/ i", "/foo/i ", "/foo/i\n\r"] {
        assert!(matches(pattern, "FOO").unwrap());
    }
    assert!(matches(r"/foo\/bar/", "foo/bar").unwrap());
    assert!(matches("{a{2}}", "aa").unwrap());
    assert!(matches(r"{a\}b}", "a}b").unwrap());
    assert!(matches(r"/\C/", "x").unwrap());
    for pattern in ["/foo/bar/", "/foo/i\t", r"/\q/", r"/\q/X", r"/\C/u"] {
        assert!(matches(pattern, "foo").is_err(), "{pattern:?}");
    }
}

#[test]
fn php_pcre_groups_thresholds_and_deterministic_sampling_preserve_order() {
    assert!(matches(r"#^/api/(?<name>\w+)$#i", "/API/users").unwrap());
    assert!(matches(r"/中文/u", "中文").unwrap());
    assert!(matches("invalid", "value").is_err());
    assert!(matches("/ok/z", "ok").is_err());
    assert!(ignored(&json!([r"/^a/", r"/z$/"]), "xyz").unwrap());
    assert_eq!(
        group(
            &json!([[r"/users\/(\d+)/", "users:$1"], [r"/.*/", "all"]]),
            "users/12"
        )
        .unwrap(),
        "users:12"
    );
    assert_eq!(
        group(&json!([[r"/(\d+)/", r"${1}:\1"]]), "a12 b34").unwrap(),
        "a12:12 b34:34"
    );
    assert_eq!(
        threshold(&json!([["/^z/", 20], ["/.*/", 30], ["default", 40]]), "zzz").unwrap(),
        20.0
    );
    assert_eq!(threshold(&json!({"default":40}), "x").unwrap(), 40.0);
    assert_eq!(threshold(&json!(125), "x").unwrap(), 125.0);
    assert!(!sample(0.0, None).unwrap());
    assert!(sample(1.0, None).unwrap());
    assert!(sample(0.5, Some("hello")).unwrap());
    assert!(!sample(0.1, Some("hello")).unwrap());
    assert!(sample(1.1, None).is_err());
}
