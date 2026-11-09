//! 保护固定上游选择顺序、原始分支文字和各地区旧版复数规则。

use super::select;
use crate::Result;
use crate::support::InputNumber::{Float, Integer};
use crate::translation::Locale;

#[test]
fn conditional_choice_precedes_plural_index_and_preserves_plain_text() -> Result<()> {
    let locale = Locale::new("en")?;
    for (line, number, expected) in [
        ("first", Integer(10), "first"),
        ("first|second", Integer(1), "first"),
        ("first|second", Integer(0), "second"),
        ("{0}  first|{1}second", Integer(0), "first"),
        ("{0}  first|{1}second", Integer(1), "second"),
        ("{2}first|{1}second", Integer(2), "first"),
        ("{9}first|{10}second", Integer(0), "second"),
        ("{9}first|{10}second", Integer(1), "first"),
        ("{0}|{1}second", Integer(0), ""),
        ("{0}first|{1}", Integer(1), ""),
        ("{1.3}first|{2.3}second", Float(0.3), "second"),
        ("{1.3}first|{2.3}second", Float(1.3), "first"),
        ("{1.3}first|{2.3}second", Float(2.3), "second"),
        (
            "{1}first\n    line|{2}second",
            Integer(1),
            "first\n    line",
        ),
        ("{0}first|[1,9]second", Integer(10), "second"),
        ("{0}first|[2,9]second", Integer(1), "first"),
        ("[4,*]first|[1,3]second", Integer(1), "second"),
        ("[4,*]first|[1,3]second", Integer(100), "first"),
        ("[1,5]first|[6,10]second", Integer(7), "second"),
        ("[*,4]first|[5,*]second", Integer(4), "first"),
        ("[5,*]first|[*,4]second", Integer(0), "second"),
        ("{0}first|[1,3]second|[4,*]third", Integer(9), "third"),
        ("[*,-1] first|{0} second|[1,*] third", Integer(-4), "first"),
        ("[-5,-1]first|{0}second|[1,*]third", Integer(-4), "first"),
        ("{-1} minus one|[*,*] other", Integer(-1), "minus one"),
        ("[*,*] any|{-1}later", Integer(-1), "any"),
        ("{0} first | { 1 } second", Integer(0), "first"),
        ("{0}first| {1}second", Integer(2), " {1}second"),
        (
            "[first](//example.com)|[second](//test.com)",
            Integer(2),
            "[second](//test.com)",
        ),
        (
            "{0}[first](//example.com)|[2,*][second](//test.com)",
            Integer(10),
            "[second](//test.com)",
        ),
        ("  first  |\t second  ", Integer(2), "\t second  "),
        ("{9} first |{10} second ", Integer(2), " second "),
        ("one|", Integer(2), ""),
        ("", Integer(2), ""),
        ("[1} mixed ]", Integer(1), "mixed ]"),
        ("{}  first| second", Integer(2), " second"),
        (
            "[1,2,3] lexical comparison | second",
            Integer(2),
            "lexical comparison",
        ),
        ("[?,*] never | second", Integer(2), " second"),
        ("{1}\0 \t\r\n\u{b}edge\u{b}\0", Integer(1), "edge"),
        (
            "{1}\u{c}\u{a0}keep\u{a0}\u{c}",
            Integer(1),
            "\u{c}\u{a0}keep\u{a0}\u{c}",
        ),
    ] {
        assert_eq!(
            select(line, number, &locale)?,
            expected,
            "{line:?} @ {number:?}"
        );
    }
    Ok(())
}

#[test]
fn conditions_keep_signed_integer_identity_beyond_double_precision() -> Result<()> {
    let locale = Locale::new("en")?;
    for (line, number, expected) in [
        (
            "{9007199254740992}wrong|{9007199254740993}exact",
            Integer(9_007_199_254_740_993),
            "exact",
        ),
        (
            "{9223372036854775806}wrong|{9223372036854775807}max",
            Integer(i64::MAX),
            "max",
        ),
        (
            "{-9223372036854775807}wrong|{-9223372036854775808}min",
            Integer(i64::MIN),
            "min",
        ),
        (
            "[-9223372036854775808,-9223372036854775808]min|other",
            Integer(i64::MIN),
            "min",
        ),
        (
            "[9223372036854775806,9223372036854775806]inside|other",
            Integer(i64::MAX),
            "other",
        ),
        (
            "[*,9223372036854775806]inside|outside",
            Integer(i64::MAX),
            "outside",
        ),
    ] {
        assert_eq!(
            select(line, number, &locale)?,
            expected,
            "{line:?} @ {number:?}"
        );
    }
    Ok(())
}

#[test]
fn fixed_plural_rule_groups_keep_locale_sign_and_fraction_boundaries() -> Result<()> {
    let line = "0|1|2|3|4|5";
    for (tag, number, expected) in [
        ("ja-JP", Integer(-8), "0"),
        ("zh-HK", Float(1.75), "0"),
        ("en-GB", Integer(-1), "0"),
        ("en-GB", Float(-1.25), "1"),
        ("pt-BR", Integer(0), "1"),
        ("fr-CA", Integer(0), "0"),
        ("fr-CA", Integer(-1), "0"),
        ("fr-CA", Float(0.75), "1"),
        ("ru-RU", Integer(-21), "0"),
        ("ru-RU", Float(1.75), "0"),
        ("ru-RU", Float(-2.75), "1"),
        ("ru-RU", Integer(14), "2"),
        ("ru-RU", Integer(22), "1"),
        ("cs-CZ", Integer(-1), "0"),
        ("cs-CZ", Float(-2.75), "1"),
        ("sk-SK", Float(4.1), "2"),
        ("ga-IE", Integer(1), "0"),
        ("ga-IE", Integer(-2), "1"),
        ("ga-IE", Float(2.75), "2"),
        ("lt-LT", Integer(21), "0"),
        ("lt-LT", Float(-9.5), "1"),
        ("lt-LT", Integer(19), "2"),
        ("lt-LT", Integer(i64::MAX), "1"),
        ("sl-SI", Integer(-101), "0"),
        ("sl-SI", Integer(102), "1"),
        ("sl-SI", Float(103.25), "2"),
        ("sl-SI", Integer(105), "3"),
        ("mk-MK", Float(-11.75), "0"),
        ("mk-MK", Integer(12), "1"),
        ("mt-MT", Integer(-1), "0"),
        ("mt-MT", Integer(0), "1"),
        ("mt-MT", Integer(10), "1"),
        ("mt-MT", Integer(11), "2"),
        ("mt-MT", Integer(20), "3"),
        ("lv-LV", Integer(0), "0"),
        ("lv-LV", Float(-21.5), "1"),
        ("lv-LV", Integer(11), "2"),
        ("pl-PL", Integer(-1), "0"),
        ("pl-PL", Float(2.75), "1"),
        ("pl-PL", Integer(14), "2"),
        ("pl-PL", Integer(22), "1"),
        ("cy-GB", Integer(-1), "0"),
        ("cy-GB", Integer(2), "1"),
        ("cy-GB", Integer(11), "2"),
        ("cy-GB", Float(8.5), "3"),
        ("ro-RO", Integer(-1), "0"),
        ("ro-RO", Integer(0), "1"),
        ("ro-RO", Float(119.5), "1"),
        ("ro-RO", Integer(120), "2"),
        ("ar-SS", Integer(0), "0"),
        ("ar-SS", Integer(-1), "1"),
        ("ar-SS", Integer(2), "2"),
        ("ar-SS", Float(-3.75), "3"),
        ("ar-SS", Integer(11), "4"),
        ("ar-SS", Integer(100), "5"),
        ("en-FR", Integer(2), "0"),
        ("en-Latn-GB", Integer(2), "0"),
        ("ar-Latn-EG", Integer(3), "0"),
    ] {
        let locale = Locale::new(tag)?;
        assert_eq!(
            select(line, number, &locale)?,
            expected,
            "{tag} @ {number:?}"
        );
    }
    assert_eq!(
        select(" first | second ", Integer(11), &Locale::new("ar")?)?,
        " first "
    );
    assert_eq!(
        select(
            "{0} zero|{1} one|[2,*] many",
            Float(2.75),
            &Locale::new("pl")?
        )?,
        "many"
    );
    Ok(())
}

#[test]
fn modulo_overflow_follows_laravel_warning_boundary_only_after_no_condition_match() -> Result<()> {
    let russian = Locale::new("ru")?;
    let english = Locale::new("en")?;
    let unknown = Locale::new("ru-Latn-RU")?;
    let huge = Float(1e30);
    for count in [Integer(i64::MIN), huge] {
        assert!(matches!(
            select("single", count, &russian),
            Err(error) if error.code() == "translation_choice_count_out_of_range" && error.status().is_server_error()
        ));
        assert_eq!(select("one|other", count, &english)?, "other");
        assert_eq!(select("first|second", count, &unknown)?, "first");
    }
    assert_eq!(
        select(
            "{-9223372036854775808} min|other",
            Integer(i64::MIN),
            &russian
        )?,
        "min"
    );
    assert_eq!(select("[0,*] huge|other", huge, &russian)?, "huge");
    Ok(())
}
