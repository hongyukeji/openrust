use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::support::Str;
use serde::Deserialize;

#[derive(Deserialize)]
struct Oracle {
    cases: Vec<OracleCase>,
}

#[derive(Deserialize)]
struct OracleCase {
    name: String,
    input_base64: String,
    default: String,
    unknown_null: String,
    unknown_empty: String,
    unknown_custom: String,
}

#[test]
fn transliteration_matches_the_fixed_php_oracle_for_valid_utf8() {
    let oracle: Oracle =
        serde_json::from_str(include_str!("fixtures/string-transliteration-php85.json")).unwrap();
    assert_eq!(oracle.cases.len(), 34);
    let mut invalid_utf8 = 0;
    for case in oracle.cases {
        let bytes = STANDARD.decode(case.input_base64).unwrap();
        let Ok(value) = std::str::from_utf8(&bytes) else {
            assert_eq!(case.name, "invalid_utf8");
            invalid_utf8 += 1;
            continue;
        };
        assert_eq!(Str::transliterate(value), case.default, "{}", case.name);
        for (unknown, expected) in [
            (None, case.unknown_null),
            (Some(""), case.unknown_empty),
            (Some("[unknown]"), case.unknown_custom),
        ] {
            assert_eq!(
                Str::transliterateWithUnknown(value, unknown),
                expected,
                "{}",
                case.name
            );
        }
    }
    assert_eq!(invalid_utf8, 1);
}

#[test]
fn string_boundaries_use_utf8_scalars_and_preserve_original_slices() {
    let subject = "begin [中] [é🙂] end";
    assert_eq!(Str::between(subject, "[", "]"), "中] [é🙂");
    assert_eq!(Str::betweenFirst(subject, "[", "]"), "中");
    assert_eq!(Str::afterLast(subject, "]"), " end");
    assert_eq!(Str::before(subject, ""), subject);
    assert_eq!(Str::after(subject, "missing"), subject);
    assert_eq!(Str::charAt("é中🙂", 1), Some('中'));
    assert_eq!(Str::charAt("é中🙂", -1), Some('🙂'));
    assert_eq!(Str::charAt("é中🙂", isize::MIN), None);
    assert_eq!(Str::length("e\u{301}🙂"), 3);
    assert!(!Str::contains(subject, &[""], true));
    assert!(Str::contains(subject, &["É🙂"], true));
    assert_eq!(Str::of("  OpenRust:中  ").trim().after(":").length(), 1);
}

#[test]
fn string_transformations_cover_empty_unicode_and_negative_boundaries() {
    assert_eq!(Str::substr("é中🙂z", -3, Some(-1)), "中🙂");
    assert_eq!(Str::substr("é中🙂z", isize::MIN, Some(1)), "é");
    assert_eq!(Str::substr("é中🙂z", 1, Some(isize::MIN)), "");
    assert!(Str::is(&["library/*"], "library/中\n🙂", false));
    assert!(!Str::is(&["library"], "library/中", false));
    assert!(Str::containsAll("É中🙂", &["é", "🙂"], true));
    assert!(!Str::startsWith("é", &[""]));
    assert_eq!(Str::start("////example", "/"), "/example");
    assert_eq!(Str::finish("example::::", "::"), "example::");
    assert_eq!(Str::replaceLast("中", "🙂", "中-é-中"), "中-é-🙂");
    assert_eq!(Str::words("  这是   中文 🙂 ", 2, "…"), "  这是   中文…");
    assert_eq!(Str::words("短文本 ", 2, "…"), "短文本 ");
    assert_eq!(Str::studly("CBOR-payload", true), "CborPayload");
    assert_eq!(Str::camel("open_rust framework"), "openRustFramework");
    assert_eq!(Str::snake("OpenRust HTTP", "_"), "open_rust_h_t_t_p");
    assert_eq!(
        Str::fromBase64(&Str::toBase64([0, 0xff, 0x80])).unwrap(),
        [0, 0xff, 0x80]
    );
    assert!(Str::fromBase64("%%%%").is_err());
    assert!(Str::isJson("null"));
    assert!(!Str::isJson("{]"));
}
