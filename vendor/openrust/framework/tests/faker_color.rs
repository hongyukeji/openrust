use std::collections::HashSet;

use openrust::database::eloquent::factories::fake;
use regex::Regex;

#[test]
fn faker_color_formats_preserve_php_ranges_and_delimiters() {
    let faker = fake(None).unwrap();
    let hex = Regex::new(r"^#[0-9a-f]{6}$").unwrap();
    let rgb = |value: &str| {
        let values: Vec<u8> = value
            .split(',')
            .map(|channel| channel.parse::<u8>().unwrap())
            .collect();
        assert_eq!(values.len(), 3);
        assert!(values.iter().any(|&channel| channel != 0));
    };
    for _ in 0..256 {
        let sample = faker.hexColor();
        assert!(hex.is_match(&sample));
        assert_ne!(sample, "#000000");
        let sample = faker.safeHexColor();
        assert!(hex.is_match(&sample));
        let bytes = sample.as_bytes();
        assert_eq!(&bytes[1..3], b"00");
        assert_eq!(bytes[3], bytes[4]);
        assert_eq!(bytes[5], bytes[6]);

        assert_ne!(faker.rgbColorAsArray(), [0, 0, 0]);
        rgb(&faker.rgbColor());
        let sample = faker.rgbCssColor();
        rgb(sample
            .strip_prefix("rgb(")
            .unwrap()
            .strip_suffix(')')
            .unwrap());
        let sample = faker.rgbaCssColor();
        let (channels, alpha) = sample
            .strip_prefix("rgba(")
            .unwrap()
            .strip_suffix(')')
            .unwrap()
            .rsplit_once(',')
            .unwrap();
        rgb(channels);
        assert!(
            matches!(alpha, "0" | "1")
                || (alpha.len() == 3
                    && alpha.starts_with("0.")
                    && matches!(alpha.as_bytes()[2], b'1'..=b'9'))
        );

        for [hue, saturation, lightness] in [
            faker.hslColorAsArray(),
            faker
                .hslColor()
                .split(',')
                .map(|component| component.parse::<u16>().unwrap())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        ] {
            assert!(hue <= 360 && saturation <= 100 && lightness <= 100);
        }
    }
}

#[test]
fn faker_color_names_follow_locale_fallbacks_and_unique_history() {
    for (locale, count, safe, full) in [
        ("en-US", 15, "purple", "AliceBlue"),
        ("zh-Hans", 15, "绿色", "绿色"),
        ("zh-Hant", 15, "綠色", "綠色"),
        ("es-ES", 15, "marrón", "AliceBlue"),
        ("fr-CA", 14, "noir", "Blanc cassé"),
        ("de-DE", 15, "purple", "AliceBlue"),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let names: HashSet<_> = (0..count)
            .map(|_| unique.safeColorName().unwrap())
            .collect();
        assert!(names.contains(safe), "{locale}");
        assert!(unique.safeColorName().is_err(), "{locale}");
        assert!(faker.unique(true).safeColorName().is_ok(), "{locale}");

        let unique = faker.unique(true);
        let mut found = false;
        while let Ok(name) = unique.colorName() {
            found |= name == full;
        }
        assert!(found, "{locale} missing {full}");
    }

    let faker = fake(None).unwrap();
    let unique = faker.unique(true);
    assert_ne!(
        unique.rgbColorAsArray().unwrap(),
        unique.rgbColorAsArray().unwrap()
    );
    assert_ne!(
        unique.hslColorAsArray().unwrap(),
        unique.hslColorAsArray().unwrap()
    );
}
