//! 只用公开日期API，区分PHP数值格式化与U.u字段解析。

#[cfg(test)]
mod tests {
    use openrust::support::calendar::{format_php_numeric_timestamp, parse_php_numeric_timestamp};

    #[test]
    fn public_numeric_date_keeps_php_fields_without_clock_or_timezone() -> openrust::Result<()> {
        let date = parse_php_numeric_timestamp(&serde_json::json!(-1.25))?;
        assert_eq!(date.epoch_seconds(), -1);
        assert_eq!(date.microseconds(), 250_000);
        assert_eq!(format_php_numeric_timestamp(-1.25)?, "-1.250000");
        let negative_zero = parse_php_numeric_timestamp(&serde_json::json!(-0.25))?;
        assert_eq!(negative_zero.epoch_seconds(), 0);
        assert_eq!(negative_zero.microseconds(), 250_000);
        assert_eq!(format_php_numeric_timestamp(-0.00000049)?, "0.000000");
        let exponent = parse_php_numeric_timestamp(&serde_json::json!(" \t1.25e0\n"))?;
        assert_eq!(exponent.epoch_seconds(), 1);
        assert_eq!(exponent.microseconds(), 250_000);
        let six_spaces = parse_php_numeric_timestamp(&serde_json::json!(
            " \t\n\r\u{b}\u{c}+1.25\u{c}\u{b}\r\n\t "
        ))?;
        assert_eq!(six_spaces.epoch_seconds(), 1);
        assert_eq!(six_spaces.microseconds(), 250_000);
        assert_eq!(format_php_numeric_timestamp(0.000_000_5)?, "0.000001");
        assert_eq!(format_php_numeric_timestamp(-0.000_000_5)?, "-0.000001");
        Ok(())
    }

    #[test]
    fn public_numeric_date_rejects_json_non_numeric_and_range_inputs() {
        for value in [
            serde_json::json!(true),
            serde_json::Value::Null,
            serde_json::json!([]),
            serde_json::json!({}),
            serde_json::json!("1\u{0}"),
            serde_json::json!("\u{0}1"),
            serde_json::json!(""),
            serde_json::json!(" \t\r\n"),
            serde_json::json!(i64::MAX),
        ] {
            assert!(parse_php_numeric_timestamp(&value).is_err());
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(format_php_numeric_timestamp(value).is_err());
        }
    }
}
