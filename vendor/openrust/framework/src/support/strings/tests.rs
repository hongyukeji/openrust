//! 固定源码及官方默认测试派生向量；实际PHP oracle证据由总控另行关联。
use super::*;

#[test]
fn official_default_vectors_keep_case_spacing_and_language_mapping() -> Result<()> {
    for (input, expected) in [
        ("testiñg", "testing"),
        ("a\0ñ\0c", "anc"),
        ("a\0b\0c", "abc"),
        ("  -ABC-中文空白-  ", "  -ABC-Zhong Wen Kong Bai -  "),
        ("déjà vu ", "deja vu "),
        ("déjà σσς iıii", "deja sss iiii"),
        ("中", "Zhong "),
        ("أحبك", "aHbk"),
        ("キャンパス", "kiyanpasu"),
        ("биологическом", "biologicheskom"),
        ("정, 병호", "jeong, byeongho"),
        ("বাংলা", "baaNlaa"),
        ("∀ i ∈ ℕ", "? i ? N"),
        ("«foobar»", "<<foobar>>"),
    ] {
        assert_eq!(Str::transliterate(input)?, expected);
    }
    Ok(())
}
#[test]
fn fast_paths_and_php_regex_context_are_not_an_unconditional_char_map() -> Result<()> {
    for (input, expected) in [
        ("a\x10b\x13c", "a\x10b\x13c"),
        ("ñ\x10b\x13c", "nbc"),
        ("a\nb", "a\nb"),
        ("ñ\nb", "n\nb"),
        ("\nñb", "\nnb"),
        ("ñb\n", "nb\n"),
        ("‘a\nb’", "'a\nb'"),
        ("ñ\tb\rc", "n\tb\rc"),
        ("a\u{00a0}b\u{200b}c", "a b c"),
        ("a\u{202a}b\u{2069}c", "abc"),
        ("«“‘a’”—…»", "<<\"'a'\"-...>>"),
    ] {
        assert_eq!(Str::transliterate(input)?, expected);
    }
    Ok(())
}
#[test]
fn unknown_variants_and_empty_mappings_match_source_branches() -> Result<()> {
    let input = "😀\u{10ffff}";
    assert_eq!(Str::transliterate(input)?, "??");
    assert_eq!(Str::transliterate_with_unknown(input, None)?, input);
    assert_eq!(Str::transliterate_with_unknown(input, Some(""))?, "");
    assert_eq!(
        Str::transliterate_with_unknown(input, Some("[unknown]"))?,
        "[unknown][unknown]"
    );
    assert_eq!(Str::transliterate("E\u{0301}")?, "E");
    assert_eq!(
        Str::transliterate_with_unknown("E\u{0301}", None)?,
        "E\u{0301}"
    );
    Ok(())
}
#[test]
fn checked_in_data_has_expected_banks_sparse_holes_and_clean_tables() -> Result<()> {
    let data = data()?;
    assert_eq!(data.banks.len(), 189);
    assert_eq!(data.whitespace.len(), 20);
    assert_eq!(data.msword.len(), 15);
    // PHP显式数字键形成稀疏bank；不能把后面的值向前挤成连续数组。
    let bank = data
        .banks
        .get(&0x1f1)
        .ok_or_else(|| Error::custom("test_bank_missing", "缺少固定bank"))?;
    assert_eq!(bank.get(47).and_then(Option::as_deref), None);
    assert_eq!(bank.get(48).and_then(Option::as_deref), Some("A"));
    Ok(())
}

#[derive(Deserialize)]
struct Oracle {
    cases: Vec<OracleCase>,
    throttle_keys: Vec<OracleKey>,
}
#[derive(Deserialize)]
struct OracleCase {
    name: String,
    input_base64: String,
    default: String,
    explicit_default: String,
    unknown_null: String,
    unknown_empty: String,
    unknown_custom: String,
}
#[derive(Deserialize)]
struct OracleKey {
    lower: String,
    ip: String,
    key: String,
}

#[test]
fn actual_php85_oracle_matches_all_valid_utf8_cases_unknown_modes_and_keys() -> Result<()> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let oracle: Oracle =
        serde_json::from_str(include_str!("php85-oracle.json")).map_err(Error::internal)?;
    assert_eq!(oracle.cases.len(), 34);
    assert_eq!(oracle.throttle_keys.len(), 30);
    let mut invalid_utf8 = 0;
    for case in oracle.cases {
        let raw = STANDARD
            .decode(case.input_base64)
            .map_err(Error::internal)?;
        let Ok(value) = std::str::from_utf8(&raw) else {
            // PHP字符串能持有的非法字节不可传入Rust &str；不使用lossy伪造等价输入。
            assert_eq!(case.name, "invalid_utf8");
            invalid_utf8 += 1;
            continue;
        };
        assert_eq!(case.default, case.explicit_default, "{}", case.name);
        assert_eq!(Str::transliterate(value)?, case.default, "{}", case.name);
        assert_eq!(
            Str::transliterate_with_unknown(value, None)?,
            case.unknown_null,
            "{}",
            case.name
        );
        assert_eq!(
            Str::transliterate_with_unknown(value, Some(""))?,
            case.unknown_empty,
            "{}",
            case.name
        );
        assert_eq!(
            Str::transliterate_with_unknown(value, Some("[unknown]"))?,
            case.unknown_custom,
            "{}",
            case.name
        );
    }
    assert_eq!(invalid_utf8, 1);
    for key in oracle.throttle_keys {
        // lower字段由真实mb_strtolower产生，此测试只核转写阶段，不偷称Rustlower已等价。
        assert_eq!(
            Str::transliterate(&format!("{}|{}", key.lower, key.ip))?,
            key.key
        );
    }
    Ok(())
}
