use openrust::support::{Number, NumberError, NumberValue};

#[test]
fn decimal_precision_and_negative_zero_follow_reference() -> Result<(), NumberError> {
    let number = Number::default();
    assert_eq!(number.format(1_234.567_8, None, None)?, "1,234.568");
    assert_eq!(number.format(1.2, Some(4), Some(3))?, "1.2");
    assert_eq!(number.format(1.2, Some(4), None)?, "1.2000");
    assert_eq!(number.format(-0.001, Some(2), None)?, "0.00");
    assert_eq!(
        number.format(i64::MAX, None, None)?,
        "9,223,372,036,854,775,807"
    );
    assert_eq!(number.format(f64::NEG_INFINITY, None, None)?, "-∞");
    assert_eq!(number.format(f64::NAN, None, None)?, "NaN");
    Ok(())
}

#[test]
fn number_scopes_follow_locale_and_currency_without_mutating_defaults() -> Result<(), NumberError> {
    let number = Number::default();
    assert_eq!(
        number.withLocale("de-DE", |de| de.format(12_345.675, Some(2), None))??,
        "12.345,68"
    );
    assert_eq!(
        number.withLocale("fr-FR", |fr| fr.currency(1_234.5, Some("EUR"), None))??,
        "1\u{202f}234,50\u{a0}€"
    );
    assert_eq!(
        number.withLocale("fr-FR", |fr| fr.percentage(10.25, 2, None))??,
        "10,25\u{a0}%"
    );
    assert_eq!(
        number.withCurrency("JPY", |yen| yen.currency(1_223.5, None, None))??,
        "¥1,224"
    );
    assert_eq!(number.currency(1_234.565, None, Some(3))?, "$1,234.565");
    assert_eq!(number.currency(-0.001, None, None)?, "$0.00");
    assert_eq!(number.defaultLocale().to_string(), "en");
    assert_eq!(number.defaultCurrency().iso_code().as_str(), "USD");
    assert!(matches!(
        number.withLocale("?", |_| ()),
        Err(NumberError::InvalidLocale)
    ));
    assert!(matches!(
        number.withCurrency("yen!", |_| ()),
        Err(NumberError::InvalidCurrency)
    ));
    Ok(())
}

#[test]
fn parses_locale_digits_groups_exponents_and_prefixes() -> Result<(), NumberError> {
    let number = Number::default();
    assert_eq!(
        number.withLocale("de-DE", |de| de.parseFloat("12.345,67 kg"))??,
        12_345.67
    );
    assert_eq!(
        number.withLocale("ar-u-nu-arab", |ar| ar.parse("١٢٣٫٥"))??,
        123.5
    );
    assert!(number.parseFloat("12,34.5").is_err());
    assert!(number.parseFloat(".").is_err());
    assert_eq!(number.parseInt("-1.9")?, -1);
    assert_eq!(number.parseInt("9,223,372,036,854,775,807")?, i64::MAX);
    assert!(number.parseInt("9,223,372,036,854,775,808").is_err());
    assert_eq!(number.parseFloat("1E3")?, 1000.0);
    assert_eq!(number.parseFloat("-∞")?, f64::NEG_INFINITY);
    Ok(())
}

#[test]
fn spellouts_and_readable_units_follow_the_baseline_examples() -> Result<(), NumberError> {
    let number = Number::default();
    assert_eq!(
        number.spell(1234, None, None)?,
        "one thousand two hundred thirty-four"
    );
    assert_eq!(number.spell(1.05, None, None)?, "one point zero five");
    assert_eq!(number.spell(1234, None, Some(1234))?, "1,234");
    assert_eq!(number.spellOrdinal(21)?, "twenty-first");
    assert_eq!(number.ordinal(1021)?, "1,021st");
    assert_eq!(number.ordinal(i64::MAX)?, "9,223,372,036,854,775,807th");
    assert_eq!(number.fileSize(922, 2, None)?, "0.90 KB");
    assert_eq!(number.forHumans(999500, 0, None, false)?, "1 million");
    assert_eq!(number.abbreviate(1234500, 1, None)?, "1.2M");
    assert_eq!(number.abbreviate(-0.001, 0, None)?, "0");
    Ok(())
}

#[test]
fn numeric_operations_preserve_types_and_boundaries() -> Result<(), NumberError> {
    assert_eq!(
        Number::clamp(i64::MAX - 1, i64::MIN, i64::MAX),
        NumberValue::Integer(i64::MAX - 1)
    );
    assert_eq!(Number::clamp(1.0, 0, 1), NumberValue::Integer(1));
    assert_eq!(Number::clamp(0.0, 0, 1), NumberValue::Float(0.0));
    assert_eq!(Number::clamp(f64::NAN, -1, 1), NumberValue::Integer(-1));
    assert_eq!(Number::trim(-0.0), NumberValue::Integer(0));
    assert_eq!(Number::trim(1.25), NumberValue::Float(1.25));
    assert_eq!(Number::pairs(10, -5, 0, 1)?, [(0, 4), (5, 9), (10, 10)]);
    assert!(matches!(
        Number::pairs(10, 0, 0, 1),
        Err(NumberError::InvalidStep)
    ));
    assert_eq!(
        Number::pairsFloat(1.0, 0.5, 0.0, 0.25)?,
        [(0.0, 0.25), (0.5, 0.75), (1.0, 1.0)]
    );
    Ok(())
}

#[test]
fn spellout_matches_the_fixed_reference_locale_matrix() -> Result<(), NumberError> {
    let mut number = Number::default();
    for line in include_str!("fixtures/number-spellout-php85.tsv").lines() {
        let [locale, value, words, ordinal] = spellout_fields(line);
        number.useLocale(&locale.replace('_', "-"))?;
        let value = if value.contains('.') || value == "-0" {
            NumberValue::Float(value.parse().unwrap())
        } else {
            NumberValue::Integer(value.parse().unwrap())
        };
        assert_eq!(
            number.spell(value, None, None)?,
            words,
            "{locale} {value:?}"
        );
        assert_eq!(number.spellOrdinal(value)?, ordinal, "{locale} {value:?}");
    }
    Ok(())
}

#[test]
fn spellout_matches_icu_nonfinite_values() -> Result<(), NumberError> {
    let mut number = Number::default();
    for line in include_str!("fixtures/number-spellout-special-icu76.tsv").lines() {
        let [locale, value, words, ordinal] = spellout_fields(line);
        number.useLocale(&locale.replace('_', "-"))?;
        let value: f64 = value.parse().unwrap();
        assert_eq!(number.spell(value, None, None)?, words, "{locale} {value}");
        assert_eq!(number.spellOrdinal(value)?, ordinal, "{locale} {value}");
    }
    Ok(())
}

fn spellout_fields(line: &str) -> [&str; 4] {
    let mut fields = line.split('\t');
    let result = std::array::from_fn(|_| fields.next().expect("spellout fixture field"));
    assert!(fields.next().is_none(), "unexpected field in {line:?}");
    result
}

#[test]
fn spellout_resolves_region_aliases_and_keeps_thresholds_inclusive() -> Result<(), NumberError> {
    let number = Number::default();
    assert_eq!(
        number.withLocale("zh-Hant", |number| number.spell(1.25, None, None))??,
        "一點二五"
    );
    assert_eq!(
        number.withLocale("fr-CH", |number| number.spell(80, None, None))??,
        "huitante"
    );
    assert_eq!(
        number.withLocale("ru", |number| number.spell(1.25, None, None))??,
        "одна целый двадцать пять сотых"
    );
    assert_eq!(
        number.withLocale("de-DE", |number| number.spell(1.25, Some(2), None))??,
        "1,25"
    );
    assert_eq!(number.spell(1234, Some(1234), None)?, "1,234");
    assert_eq!(number.spell(1234, None, Some(1234))?, "1,234");
    assert_eq!(
        number.spell(1e40, None, None)?,
        "10,000,000,000,000,000,000,000,000,000,000,000,000,000"
    );
    assert!(matches!(
        number.withLocale("zz-ZZ", |number| number.spell(1, None, None))?,
        Err(NumberError::InvalidLocale)
    ));
    Ok(())
}
