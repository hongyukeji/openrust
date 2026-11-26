use super::*;
use serde_json::json;

#[test]
fn relative_date_strings_follow_carbon_default_cascade() {
    for (text, expected) in [
        ("1 minute, 15 seconds", 75_000_000),
        ("1 minute and 15 seconds", 75_000_000),
        ("1 minute -15 seconds", 45_000_000),
        ("1 week", 604_800_000_000),
        ("1 month", 2_419_200_000_000),
        ("1 year", 29_030_400_000_000),
        ("-15 seconds", -15_000_000),
        ("2 hours ago", -7_200_000_000),
        ("tomorrow", 86_400_000_000),
        ("next Monday", 0),
        ("100 milliseconds", 100_000),
        ("100 microseconds", 100),
    ] {
        assert_eq!(
            InputInterval::from_date_string(text)
                .unwrap()
                .total_microseconds(),
            expected,
            "{text}"
        );
    }
    for text in ["1.5 seconds", "90s", "PT15S", "", "1 MINUTE AND 15 SECONDS"] {
        assert!(InputInterval::from_date_string(text).is_err(), "{text}");
    }
}

#[test]
fn parsed_intervals_do_not_invent_total_days_from_day_components() {
    for text in ["P1Y2M3D", "3 days", "1 week ago"] {
        let interval = InputInterval::parse(Some(&serde_json::json!(text)), None)
            .unwrap()
            .unwrap();
        assert_eq!(interval.total_days(), None);
    }
    let interval = InputInterval::parse(Some(&serde_json::json!(90)), Some("day"))
        .unwrap()
        .unwrap();
    assert_eq!(interval.days, 90);
    assert_eq!(interval.total_days(), None);
}

#[test]
fn of_is_only_consumed_by_complete_calendar_instructions() {
    for text in [
        "of 2 days",
        "of of 2 days",
        "1 day of 2 days",
        "Monday of next month",
        "first day of of next month",
    ] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Err(IntervalError::InvalidSyntax)
        );
    }
    assert_eq!(
        InputInterval::parse(Some(&json!("first monday of next month")), None)
            .unwrap()
            .unwrap()
            .months,
        1
    );
    assert_eq!(
        InputInterval::parse(Some(&json!("second Tuesday of next month")), None)
            .unwrap()
            .unwrap()
            .days,
        7
    );
    assert_eq!(
        InputInterval::parse(Some(&json!("first day of 2 days")), None)
            .unwrap()
            .unwrap()
            .days,
        2
    );
}
#[test]
fn one_shared_term_budget_bounds_compact_human_and_relative_consumption() {
    for term in ["0d", "0 days ", "tomorrow "] {
        assert!(InputInterval::parse(Some(&json!(term.repeat(1024))), None).is_ok());
        assert_eq!(
            InputInterval::parse(Some(&json!(term.repeat(1025))), None),
            Err(IntervalError::Limit)
        );
    }
}

// 固定 Carbon 3.8.4 + PHP 8.5 golden，区分 missing/null、非字符串与无单位数字字符串。
#[test]
fn absence_and_invalid_are_different() {
    for value in [
        Value::Null,
        json!(" \t\n\0"),
        json!(90),
        json!(false),
        json!([]),
    ] {
        assert_eq!(InputInterval::parse(Some(&value), None), Ok(None));
    }
    assert_eq!(InputInterval::parse(None, Some("bad")), Ok(None));
    for text in [
        "90",
        "0",
        "invalid",
        "P1X",
        "-P1D",
        "p1d",
        "P-1D",
        "2 weekdays",
    ] {
        assert!(
            InputInterval::parse(Some(&json!(text)), None).is_err(),
            "{text}"
        );
    }
    assert_eq!(
        InputInterval::parse(Some(&json!("0 days")), None),
        Ok(Some(InputInterval::default()))
    );
}

// 发现错误 cascade、负数方向和年月小数被错误换算的缺陷。
#[test]
fn original_components_and_fraction_rules() {
    for (text, expected) in [
        (
            "2hours30minutes",
            InputInterval {
                hours: 2,
                minutes: 30,
                ..InputInterval::default()
            },
        ),
        (
            "2 hours -90 minutes",
            InputInterval {
                hours: 2,
                minutes: -90,
                ..InputInterval::default()
            },
        ),
        (
            "-1.5 hours",
            InputInterval {
                hours: -1,
                minutes: -30,
                ..InputInterval::default()
            },
        ),
        (
            "-0.5 weeks",
            InputInterval {
                days: -3,
                hours: -12,
                ..InputInterval::default()
            },
        ),
        (
            "1.5 months",
            InputInterval {
                months: 1,
                ..InputInterval::default()
            },
        ),
        (
            "-1.5 microseconds",
            InputInterval {
                microseconds: -1,
                ..InputInterval::default()
            },
        ),
        (
            "3 µs",
            InputInterval {
                microseconds: 3,
                ..InputInterval::default()
            },
        ),
    ] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Ok(Some(expected)),
            "{text}"
        );
    }
}

// 发现显式单位误用 human 别名、数组数值转换以及十位舍入缺陷。
#[test]
fn explicit_units_cast_and_decimal_precision() {
    for unit in ["m", "ms", "micro", "min", " hour ", ""] {
        assert_eq!(
            InputInterval::parse(Some(&json!(1)), Some(unit)),
            Err(IntervalError::InvalidUnit)
        );
    }
    for (value, unit, expected) in [
        (
            json!([1]),
            "hour",
            InputInterval {
                hours: 1,
                ..InputInterval::default()
            },
        ),
        (
            json!("1ab"),
            "HOURS",
            InputInterval {
                hours: 1,
                ..InputInterval::default()
            },
        ),
        (
            json!("0.12345678905"),
            "second",
            InputInterval {
                microseconds: 123_457,
                ..InputInterval::default()
            },
        ),
        (
            json!("0.99999999996"),
            "second",
            InputInterval {
                seconds: 1,
                ..InputInterval::default()
            },
        ),
        (
            json!("-0.000001"),
            "second",
            InputInterval {
                microseconds: -1,
                ..InputInterval::default()
            },
        ),
        (json!("0.000053"), "millisecond", InputInterval::default()),
    ] {
        assert_eq!(
            InputInterval::parse(Some(&value), Some(unit)),
            Ok(Some(expected))
        );
    }
}

// 发现 ISO 的分量保留、秒截断和替代日期格式被拒绝的缺陷。
#[test]
fn iso_and_relative_components() {
    for (text, expected) in [
        (
            "P1Y2M3DT4H5M6S",
            InputInterval {
                years: 1,
                months: 2,
                days: 3,
                hours: 4,
                minutes: 5,
                seconds: 6,
                ..InputInterval::default()
            },
        ),
        (
            "P0001-02-03T04:05:06",
            InputInterval {
                years: 1,
                months: 2,
                days: 3,
                hours: 4,
                minutes: 5,
                seconds: 6,
                ..InputInterval::default()
            },
        ),
        (
            "PT90M",
            InputInterval {
                minutes: 90,
                ..InputInterval::default()
            },
        ),
        (
            "P0.5W",
            InputInterval {
                days: 3,
                hours: 12,
                ..InputInterval::default()
            },
        ),
        (
            "last day of next month",
            InputInterval {
                months: 1,
                ..InputInterval::default()
            },
        ),
        (
            "next Monday +2 days",
            InputInterval {
                days: 2,
                ..InputInterval::default()
            },
        ),
        (
            "2 days ago",
            InputInterval {
                days: -2,
                ..InputInterval::default()
            },
        ),
    ] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Ok(Some(expected)),
            "{text}"
        );
    }
    for text in ["now", "next Monday"] {
        assert_eq!(InputInterval::parse(Some(&json!(text)), None), Ok(None));
    }
}

// 发现整数越界通过饱和转换被静默接收的缺陷。
#[test]
fn component_overflow_is_checked() {
    for text in [
        "P9223372036854775808D",
        "9000000000000000000 weeks",
        "9000000000000000000 years 9000000000000000000 years",
    ] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Err(IntervalError::OutOfRange)
        );
    }
}
// 程序注入的值不受 HTTP body 限额保护，解析预算必须独立检查。
#[test]
fn value_and_parameter_work_budget() {
    let value = json!("x".repeat(20 * 1024 * 1024));
    assert_eq!(
        InputInterval::parse(Some(&value), Some("s")),
        Err(IntervalError::Limit)
    );
}

// 真实补充 probe：ISO 秒超出微秒非零精度会失败，其他单位丢弃亚秒。
#[test]
fn iso_precision_and_relative_supplement() {
    for text in ["PT0.1234561S", "PT0.0000001S"] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Err(IntervalError::InvalidSyntax)
        );
    }
    for (text, expected) in [
        (
            "PT0.1234560S",
            InputInterval {
                microseconds: 123_456,
                ..InputInterval::default()
            },
        ),
        (
            "P0.1234567D",
            InputInterval {
                hours: 2,
                minutes: 57,
                seconds: 46,
                ..InputInterval::default()
            },
        ),
        (
            "P1DT-1H",
            InputInterval {
                days: 1,
                hours: -1,
                ..InputInterval::default()
            },
        ),
        (
            "PT-0.5S",
            InputInterval {
                microseconds: 500_000,
                ..InputInterval::default()
            },
        ),
        (
            "tomorrow",
            InputInterval {
                days: 1,
                ..InputInterval::default()
            },
        ),
        (
            "Monday next week",
            InputInterval {
                days: 7,
                ..InputInterval::default()
            },
        ),
        (
            "second Tuesday of next month",
            InputInterval {
                months: 1,
                days: 7,
                ..InputInterval::default()
            },
        ),
        (
            "next fortnight",
            InputInterval {
                days: 14,
                ..InputInterval::default()
            },
        ),
        (
            "yesterday 2 days ago",
            InputInterval {
                days: -1,
                ..InputInterval::default()
            },
        ),
        (
            "2 days ago 3 hours",
            InputInterval {
                days: -2,
                hours: 3,
                ..InputInterval::default()
            },
        ),
    ] {
        assert_eq!(
            InputInterval::parse(Some(&json!(text)), None),
            Ok(Some(expected)),
            "{text}"
        );
    }
}

// PHP 8.5.0 + Carbon 3.8.4 的149原始 interval 行及追加真实探针，捕获类型/分流/精度偏差。
#[test]
fn actual_php_carbon_golden_matrix() {
    let cases: Value =
        serde_json::from_str(include_str!("../fixtures/calendar_input_intervals.json")).unwrap();
    for row in cases.as_array().unwrap() {
        let id = row[0].as_str().unwrap();
        let input = if row[1] == "__MISSING__" {
            None
        } else {
            Some(&row[1])
        };
        let actual = InputInterval::parse(input, row[2].as_str());
        match row[3].as_u64().unwrap() {
            0 => assert_eq!(actual, Ok(None), "{id}"),
            1 => {
                let expected = &row[4];
                let values = expected
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_i64().unwrap())
                    .collect::<Vec<_>>();
                assert_eq!(
                    actual,
                    Ok(Some(InputInterval {
                        years: values[0],
                        months: values[1],
                        days: values[2],
                        hours: values[3],
                        minutes: values[4],
                        seconds: values[5],
                        microseconds: values[6],
                        invert: false,
                        total_days: None
                    })),
                    "{id}"
                );
            }
            _ => assert!(actual.is_err(), "{id}: {actual:?}"),
        }
    }
}
