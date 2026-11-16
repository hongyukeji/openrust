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

#[test]
fn markdown_and_english_inflection_cover_runtime_values() {
    use openrust::support::{MarkdownHtml, MarkdownOptions};

    assert_eq!(
        Str::markdown("# 海\n\n**strong**"),
        "<h1>海</h1>\n<p><strong>strong</strong></p>\n"
    );
    assert_eq!(
        Str::of("**好** ~~旧~~").inlineMarkdown().value(),
        "<strong>好</strong> <del>旧</del>"
    );
    assert!(Str::markdown("| head |\n|---|\n| cell |").contains("<table>"));
    assert!(Str::markdown("- [x] done").contains("type=\"checkbox\""));
    let html = Str::markdown("[unsafe](javascript:alert) <i>字</i> [ok](https://example.test/)");
    assert!(html.contains("href=\"\""));
    assert!(html.contains("&lt;i&gt;字&lt;/i&gt;"));
    assert!(html.contains("href=\"https://example.test/\""));
    assert_eq!(
        Str::inlineMarkdownWith(
            "<i>字</i>",
            MarkdownOptions {
                html: MarkdownHtml::Strip,
                ..Default::default()
            }
        ),
        "字"
    );
    assert_eq!(
        Str::inlineMarkdownWith(
            "<i>字</i>",
            MarkdownOptions {
                html: MarkdownHtml::Allow,
                ..Default::default()
            }
        ),
        "<i>字</i>"
    );
    assert_eq!(Str::plural("person", -1), "person");
    assert_eq!(Str::plural("child", 0), "children");
    assert_eq!(Str::plural("RELATED", 2), "RELATED");
    assert_eq!(Str::plural("An alert!", 2), "An alert!");
    assert_eq!(Str::singular("children"), "child");
    assert_eq!(Str::of("UserChild").pluralStudly(2).value(), "UserChildren");
    assert_eq!(Str::counted("person", 1_234), "1,234 people");
    assert_eq!(
        Str::counted("person", i64::MIN),
        "-9,223,372,036,854,775,808 people"
    );
}

#[test]
fn literal_and_unicode_editing_respects_scalar_offsets() {
    assert_eq!(Str::chopStart("你好世界", &["", "你好", "你"]), "世界");
    assert_eq!(Str::chopEnd("你好世界", &["", "界", "世界"]), "你好世");
    assert_eq!(Str::replaceStart("你", "🙂", "你好你"), "🙂好你");
    assert_eq!(Str::replaceEnd("你", "🙂", "你好你"), "你好🙂");
    assert_eq!(Str::replaceEnd("", "oops", "unchanged"), "unchanged");
    assert_eq!(Str::deduplicate("好好  ___好", &['好', ' ', '_']), "好 _好");
    assert_eq!(Str::position("你🙂好🙂", "🙂", -2), Some(3));
    assert_eq!(Str::position("你🙂", "", 2), Some(2));
    assert_eq!(Str::position("你🙂", "你", isize::MIN), None);
    assert_eq!(Str::take("你🙂好🙂", -2), "好🙂");
    assert_eq!(Str::mask("你好世界🙂", "*#", -4, Some(-1)), "你*#*🙂");
    assert_eq!(Str::mask("你🙂", "❄", 999, None), "你🙂");
    assert_eq!(Str::mask("你🙂", "", 0, None), "你🙂");
    assert_eq!(Str::ltrim("\u{feff}你好\u{200b}"), "你好\u{200b}");
    assert_eq!(
        Str::of("\u{1160} 你好\t\u{3164} 🙂 \u{feff}")
            .squish()
            .value(),
        "你好 🙂"
    );
}

#[test]
fn generated_strings_and_ids_respect_their_requested_formats() {
    use openrust::support::{EmptyPasswordAlphabet, PasswordOptions};

    assert!(Str::random(0).is_empty());
    let random = Str::random(64);
    assert_eq!(random.len(), 64);
    assert!(random.bytes().all(|byte| byte.is_ascii_alphanumeric()));
    let password = Str::password(20, PasswordOptions::default()).unwrap();
    assert_eq!(password.len(), 20);
    assert!(password.bytes().any(|byte| byte.is_ascii_alphabetic()));
    assert!(password.bytes().any(|byte| byte.is_ascii_digit()));
    assert!(password.bytes().any(|byte| byte.is_ascii_punctuation()));
    let spaces = PasswordOptions {
        letters: false,
        numbers: false,
        symbols: false,
        spaces: true,
    };
    assert_eq!(Str::password(3, spaces).unwrap(), "   ");
    assert_eq!(
        Str::password(
            3,
            PasswordOptions {
                spaces: false,
                ..spaces
            }
        ),
        Err(EmptyPasswordAlphabet)
    );
    assert!(Str::isUuid(&Str::uuid().unwrap().to_string(), Some(4)));
    let ordered = Str::uuid7().to_string();
    assert!(Str::of(ordered.as_str()).isUuid(Some(7)));
    assert!(!Str::isUuid(&ordered, Some(4)));
    assert!(Str::isUuid("00000000-0000-0000-0000-000000000000", Some(0)));
    assert!(!Str::isUuid("00000000000000000000000000000000", None));
    assert!(Str::of(Str::ulid().to_string()).isUlid());
    assert!(!Str::isUlid("80000000000000000000000000"));
}

#[test]
fn regex_projection_replacement_counts_and_callbacks() {
    assert_eq!(Str::r#match(r"(x)?中", "中").unwrap(), "中");
    assert_eq!(Str::r#match(r"(\d+)", "no match").unwrap(), "");
    assert_eq!(Str::matchAll(r"(x)?中", "中 x中").unwrap(), ["", "x"]);
    assert_eq!(Str::matchAll(r"\p{Han}", "甲 a 乙").unwrap(), ["甲", "乙"]);
    assert!(Str::isMatch(&["^a", r"\p{Han}"], "乙").unwrap());
    assert!(!Str::isMatch(&[], "abc").unwrap());
    assert!(Str::isMatch(&["("], "abc").is_err());
    assert_eq!(
        Str::replaceMatches(r"(\d+)", "[$1]", "中12 a34", Some(1)).unwrap(),
        "中[12] a34"
    );
    assert_eq!(
        Str::replaceMatches(r"(\d+)", "-", "中12", Some(0)).unwrap(),
        "中12"
    );
    assert_eq!(
        Str::of("甲1 乙2")
            .replaceMatchesWith(
                r"(?P<name>\p{Han})(\d+)",
                |captures| format!("{}[{}]", &captures["name"], &captures[2]),
                None
            )
            .unwrap()
            .value(),
        "甲[1] 乙[2]"
    );
}

#[test]
fn display_width_and_wrap_match_the_php_fixture() {
    assert_eq!(Str::width("e\u{301}🙂中\n"), 7);
    assert_eq!(Str::limit("中 abc déf", 6, "…", true), "中…");
    assert_eq!(Str::limit("<b>中文</b> x long", 5, "…", true), "中文…");
    assert_eq!(Str::limit("e\u{301}🙂中", 4, "…", false), "e\u{301}🙂…");
    assert_eq!(Str::of("中文").limit(4, "...", false).value(), "中文");
    for (input, columns, line_break, cut, expected) in [
        ("中a 🙂b 中c", 3, "|", false, "中a|🙂b|中c"),
        ("abcdef 中🙂", 3, "|", true, "abc|def|中🙂"),
        ("α\nβ x", 3, "\n", false, "α\nβ\nx"),
        ("abc def", 0, "|", false, "abc|def"),
        (
            "alpha  beta    gamma",
            4,
            "|",
            true,
            "alph|a |beta|  |gamm|a",
        ),
        ("abcdef", 3, "🙂", true, "abc🙂def"),
    ] {
        assert_eq!(
            Str::wordWrap(input, columns, line_break, cut).unwrap(),
            expected
        );
    }
    assert!(Str::wordWrap("text", 3, "", false).is_err());
    assert!(Str::wordWrap("text", 0, "\n", true).is_err());
}
