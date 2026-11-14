use openrust::database::eloquent::factories::fake;
use regex::Regex;

#[test]
fn regional_identity_samples_match_the_fakerphp_rules() {
    let us = fake(Some("en-US")).unwrap();
    let uk = fake(Some("en-GB")).unwrap();
    let tw = fake(Some("zh-Hant")).unwrap();
    let ssn = Regex::new(r"^([0-9]{3})-([0-9]{2})-([0-9]{4})$").unwrap();
    let nino = Regex::new(r"^[ABCEGHJKLMNOPRSTWXYZ][ABCEGHJKLMNPRSTWXYZ][0-9]{6}[A-D]$").unwrap();
    for _ in 0..256 {
        let sample = us.unique(false).ssn().unwrap();
        let digits = ssn.captures(&sample).unwrap();
        let area: u16 = digits[1].parse().unwrap();
        assert!((1..=899).contains(&area) && area != 666);
        assert_ne!(&digits[2], "00");
        assert_ne!(&digits[3], "0000");

        let sample = uk.unique(false).nino().unwrap();
        assert!(nino.is_match(&sample));
        assert!(!["BG", "GB", "KN", "NK", "NT", "TN", "ZZ"].contains(&&sample[..2]));

        for (gender, code) in [
            (Some("male"), b'1'),
            (Some("female"), b'2'),
            (Some("other"), b'2'),
        ] {
            let sample = tw.unique(false).personalIdentityNumber(gender).unwrap();
            let bytes = sample.as_bytes();
            assert_eq!(bytes.len(), 10);
            assert_eq!(bytes[1], code);
            assert!(bytes[2].is_ascii_digit() && bytes[2] != b'0');
            assert!(bytes[2..].iter().all(u8::is_ascii_digit));
            let prefix = match bytes[0] {
                b'A'..=b'H' => bytes[0] - b'A' + 10,
                b'J' | b'K' => bytes[0] - b'J' + 18,
                b'M' | b'N' => bytes[0] - b'M' + 21,
                b'P' | b'Q' => bytes[0] - b'P' + 23,
                b'T'..=b'V' => bytes[0] - b'T' + 27,
                b'I' => 34,
                b'O' => 35,
                b'W' => 32,
                b'X' => 30,
                b'Z' => 33,
                other => panic!("unexpected birthplace symbol {other}"),
            };
            let sum: u16 = u16::from(prefix / 10 + 9 * (prefix % 10))
                + bytes[1..]
                    .iter()
                    .zip([8, 7, 6, 5, 4, 3, 2, 1, 1])
                    .map(|(digit, weight)| u16::from(digit - b'0') * weight)
                    .sum::<u16>();
            assert_eq!(sum % 10, 0);
        }
    }
    assert!(matches!(
        tw.personalIdentityNumber(None).unwrap().as_bytes()[1],
        b'1' | b'2'
    ));
    let zh = fake(Some("zh-Hans")).unwrap();
    assert!(zh.ssn().is_err());
    assert!(zh.nino().is_err());
    assert!(zh.personalIdentityNumber(None).is_err());
}

#[test]
fn regional_ssn_samples_cover_thai_austrian_and_icelandic_providers() {
    use chrono::{Datelike as _, NaiveDate, Utc};
    use std::collections::HashSet;

    let th = fake(Some("th-TH")).unwrap();
    let at = fake(Some("de-AT")).unwrap();
    let is = fake(Some("is-IS")).unwrap();
    let fixed = NaiveDate::from_ymd_opt(2000, 2, 29).unwrap();
    let ssn = Regex::new(r"^([0-9]{3})-([0-9]{2})-([0-9]{4})$").unwrap();
    let icelandic = Regex::new(r"^[0-9]{6}-[0-9]{4}$").unwrap();
    let sum = |digits: &str, weights: &[u16]| {
        digits
            .bytes()
            .zip(weights)
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum::<u16>()
    };
    let unique = at.unique(true);
    let mut seen = HashSet::new();
    for _ in 0..128 {
        let sample = th.ssn().unwrap();
        let parts = ssn.captures(&sample).unwrap();
        let area: u16 = parts[1].parse().unwrap();
        assert!((1..=899).contains(&area) && area != 666);
        assert_ne!(&parts[2], "00");
        assert_ne!(&parts[3], "0000");

        let sample = unique.ssnWithBirthdate(Some(fixed)).unwrap();
        assert_eq!(sample.len(), 10);
        assert!(sample.bytes().all(|digit| digit.is_ascii_digit()));
        assert!(seen.insert(sample.clone()));
        assert_eq!(&sample[4..], "290200");
        let total = sum(&sample[..3], &[3, 7, 9]) + sum(&sample[4..], &[5, 8, 4, 2, 1, 6]);
        assert_eq!(total % 11, sample[3..4].parse::<u16>().unwrap());

        let sample = is.ssn().unwrap();
        assert!(icelandic.is_match(&sample));
        let weights = sum(&sample[..6], &[3, 2, 7, 6, 5, 4]) + sum(&sample[7..9], &[3, 2]);
        assert_eq!(
            (11 - weights % 11) % 11,
            sample[9..10].parse::<u16>().unwrap()
        );
        let current_year = Utc::now().year();
        let year_suffix = sample[4..6].parse::<i32>().unwrap();
        let century_digit = sample[10..].parse::<i32>().unwrap();
        let year = (current_year - 101..=current_year)
            .find(|&year| (year / 100) % 10 == century_digit && year % 100 == year_suffix)
            .unwrap();
        assert!(NaiveDate::parse_from_str(&format!("{}{year}", &sample[..4]), "%d%m%Y").is_ok());
    }
    let generated = at.ssn().unwrap();
    assert_eq!(generated.len(), 10);
    assert!(th.ssnWithBirthdate(Some(fixed)).is_err());
}

#[test]
fn european_identity_numbers_keep_dates_and_regional_checksums() {
    use chrono::NaiveDate;

    let date = NaiveDate::from_ymd_opt(2000, 2, 29).unwrap();
    for (locale, pattern) in [
        ("lt-LT", r"^[56]000229[0-9]{4}$"),
        ("lv-LV", r"^290200-2[0-9]{4}$"),
        ("fi-FI", r"^290200A[0-8][0-9]{2}[0-9A-Z]$"),
        ("nb-NO", r"^290200[0-9]{5}$"),
        ("sv-SE", r"^000229-[0-9]{4}$"),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let format = Regex::new(pattern).unwrap();
        for _ in 0..32 {
            for (gender, parity) in [("male", 1), ("female", 0)] {
                let sample = faker
                    .unique(false)
                    .personalIdentityNumberWithBirthdate(Some(date), Some(gender))
                    .unwrap();
                assert!(format.is_match(&sample), "{locale}: {sample}");
                let bytes = sample.as_bytes();
                let weights = |part: &str, weights: &[u16]| {
                    part.bytes()
                        .zip(weights)
                        .map(|(d, w)| u16::from(d - b'0') * w)
                        .sum::<u16>()
                };
                match locale {
                    "lt-LT" => assert_eq!(bytes[0], b'6' - parity),
                    "lv-LV" => {
                        let sum = weights(&sample[..6], &[1, 6, 3, 7, 9, 10])
                            + weights(&sample[7..11], &[5, 8, 4, 2]);
                        assert_eq!((1101 - sum) % 11 % 10, u16::from(bytes[11] - b'0'));
                    }
                    "fi-FI" => {
                        let serial: usize = sample[7..10].parse().unwrap();
                        assert!((2..=899).contains(&serial));
                        assert_eq!(serial % 2, usize::from(parity));
                        assert_eq!(
                            bytes[10],
                            b"0123456789ABCDEFHJKLMNPRSTUVWXY"[(290_200_000 + serial) % 31]
                        );
                    }
                    "nb-NO" => assert_eq!((bytes[8] - b'0') % 2, parity),
                    _ => {
                        assert_eq!((bytes[9] - b'0') % 2, parity);
                        let digits = sample.replace('-', "");
                        let total = digits
                            .bytes()
                            .enumerate()
                            .map(|(i, d)| {
                                let n = (d - b'0') * if i % 2 == 0 { 2 } else { 1 };
                                u16::from(n / 10 + n % 10)
                            })
                            .sum::<u16>();
                        assert_eq!(total % 10, 0);
                    }
                }
            }
        }
        assert!(faker.personalIdentityNumber(None).is_ok());
    }

    let lt = fake(Some("lt-LT")).unwrap();
    let unique = lt.uniqueWithRetries(true, 1);
    assert_eq!(
        unique
            .personalIdentityNumberWithSerial(Some(date), None, Some("0079"))
            .unwrap(),
        "50002290079"
    );
    assert!(
        unique
            .personalIdentityNumberWithSerial(Some(date), None, Some("007"))
            .is_err()
    );
    assert!(
        lt.personalIdentityNumberWithSerial(Some(date), None, Some("é12"))
            .is_err()
    );
    let fi = fake(Some("fi-FI")).unwrap();
    let outside = NaiveDate::from_ymd_opt(2100, 1, 1).unwrap();
    assert!(
        fi.personalIdentityNumberWithBirthdate(Some(outside), None)
            .is_err()
    );

    let pl = fake(Some("pl-PL")).unwrap();
    let format = Regex::new(r"^A[ABCDEFGHIJKLMNPRSTUVWXYZ]{2}[0-9]{6}$").unwrap();
    for _ in 0..64 {
        let sample = pl.unique(false).personalIdentityNumber(None).unwrap();
        assert!(format.is_match(&sample));
        let bytes = sample.as_bytes();
        let values = bytes[..3]
            .iter()
            .map(|&b| b - 55)
            .chain(bytes[4..].iter().map(|&b| b - b'0'));
        let total = values
            .zip([7, 3, 1, 7, 3, 1, 7, 3])
            .map(|(digit, weight)| u16::from(digit) * weight)
            .sum::<u16>();
        assert_eq!(total % 10, u16::from(bytes[3] - b'0'));
    }
}

#[test]
fn polish_pesel_and_nip_preserve_centuries_sex_and_checksums() {
    use chrono::NaiveDate;

    let faker = fake(Some("pl-PL")).unwrap();
    let unique = faker.unique(true);
    let weighted = |value: &str, weights: &[u16]| {
        value
            .bytes()
            .zip(weights)
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum::<u16>()
    };

    for (year, month) in [
        (1804, "82"),
        (1904, "02"),
        (2004, "22"),
        (2104, "42"),
        (2204, "62"),
    ] {
        let date = NaiveDate::from_ymd_opt(year, 2, 29).unwrap();
        for (sex, parity) in [(None, 1), (Some("F"), 0), (Some("M"), 1)] {
            let sample = unique.pesel(Some(date), sex).unwrap();
            assert_eq!(sample.len(), 11);
            assert!(sample.bytes().all(|digit| digit.is_ascii_digit()));
            assert_eq!(&sample[..6], format!("04{month}29"));
            assert_eq!((sample.as_bytes()[9] - b'0') % 2, parity);
            assert_eq!(
                weighted(&sample, &[1, 3, 7, 9, 1, 3, 7, 9, 1, 3, 1]) % 10,
                0
            );
        }
    }
    for year in [1799, 2300] {
        let invalid = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
        assert!(faker.pesel(Some(invalid), None).is_err());
    }
    assert_eq!(faker.pesel(None, None).unwrap().len(), 11);
    for _ in 0..256 {
        let sample = unique.taxpayerIdentificationNumber().unwrap();
        assert_eq!(sample.len(), 10);
        assert!(sample.bytes().all(|digit| digit.is_ascii_digit()));
        assert!(!sample[..3].contains('0'));
        assert_eq!(
            weighted(&sample, &[6, 5, 7, 2, 3, 4, 5, 6, 7]) % 11,
            u16::from(sample.as_bytes()[9] - b'0')
        );
    }
    let other = fake(Some("en-US")).unwrap();
    assert!(other.pesel(None, None).is_err());
    assert!(other.taxpayerIdentificationNumber().is_err());
}

#[test]
fn brazilian_identifiers_preserve_masks_branches_and_checksums() {
    let faker = fake(Some("pt-BR")).unwrap();
    let unique = faker.unique(true);
    let cpf = Regex::new(r"^[0-9]{3}\.[0-9]{3}\.[0-9]{3}-[0-9]{2}$").unwrap();
    let rg = Regex::new(r"^[0-9]{2}\.[0-9]{3}\.[0-9]{3}-[0-9]$").unwrap();
    let cnpj = Regex::new(r"^[0-9]{2}\.[0-9]{3}\.[0-9]{3}/0001-[0-9]{2}$").unwrap();
    let check = |digits: &str, weights: &[u16]| {
        let sum = digits
            .bytes()
            .zip(weights)
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum::<u16>();
        let digit = 11 - sum % 11;
        if digit < 10 { digit as u8 + b'0' } else { b'0' }
    };
    for _ in 0..128 {
        for (formatted, pattern, width, raw) in [
            (
                unique.cpf(None).unwrap(),
                &cpf,
                11,
                unique.cpf(Some(false)).unwrap(),
            ),
            (
                unique.rg(Some(true)).unwrap(),
                &rg,
                9,
                unique.rg(Some(false)).unwrap(),
            ),
            (
                unique.cnpj(None).unwrap(),
                &cnpj,
                14,
                unique.cnpj(Some(false)).unwrap(),
            ),
        ] {
            assert!(pattern.is_match(&formatted));
            assert_eq!(raw.len(), width);
            assert!(raw.bytes().all(|byte| byte.is_ascii_digit()));
            let compact = formatted.replace(['.', '/', '-'], "");
            for digits in [&compact, &raw] {
                let bytes = digits.as_bytes();
                match width {
                    11 => {
                        assert_eq!(bytes[9], check(&digits[..9], &[10, 9, 8, 7, 6, 5, 4, 3, 2]));
                        assert_eq!(
                            bytes[10],
                            check(&digits[..10], &[11, 10, 9, 8, 7, 6, 5, 4, 3, 2])
                        );
                    }
                    9 => assert_eq!(bytes[8], check(&digits[..8], &[9, 8, 7, 6, 5, 4, 3, 2])),
                    _ => {
                        assert_eq!(&digits[8..12], "0001");
                        assert_eq!(
                            bytes[12],
                            check(&digits[..12], &[5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2])
                        );
                        assert_eq!(
                            bytes[13],
                            check(&digits[..13], &[6, 5, 4, 3, 2, 9, 8, 7, 6, 5, 4, 3, 2])
                        );
                    }
                }
            }
        }
    }
    let fallback = fake(Some("pt-PT")).unwrap();
    assert!(fallback.cpf(None).is_err());
    assert!(fallback.rg(None).is_err());
    assert!(fallback.cnpj(None).is_err());
}
