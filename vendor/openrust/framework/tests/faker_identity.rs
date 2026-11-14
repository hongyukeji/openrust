use openrust::database::eloquent::factories::fake;
use regex::Regex;

fn luhn(sample: &str) {
    let sum: u32 = sample
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, digit)| {
            let digit = u32::from(digit - b'0') * if i % 2 == 0 { 1 } else { 2 };
            digit / 10 + digit % 10
        })
        .sum();
    assert_eq!(sum % 10, 0);
}

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

#[test]
fn dni_ruc_and_licence_categories_follow_their_regional_providers() {
    use std::collections::HashSet;

    let es = fake(Some("es-ES")).unwrap();
    let pe = fake(Some("es-PE")).unwrap();
    let za = fake(Some("en-ZA")).unwrap();
    let es_unique = es.unique(true);
    let pe_unique = pe.unique(true);
    let za_unique = za.unique(true);

    for _ in 0..128 {
        let dni = es_unique.dni().unwrap();
        let number: usize = dni[..8].parse().unwrap();
        assert_eq!(dni.len(), 9);
        assert_eq!(dni.as_bytes()[8], b"TRWAGMYFPDXBNJZSQVHLCKE"[number % 23]);

        let dni = pe_unique.dni().unwrap();
        assert_eq!(dni.len(), 8);
        assert!(dni.bytes().all(|byte| byte.is_ascii_digit()));

        for (natural, prefix) in [(None, "20"), (Some(false), "20"), (Some(true), "10")] {
            let ruc = pe_unique.ruc(natural).unwrap();
            assert_eq!(ruc.len(), 11);
            assert!(ruc.starts_with(prefix));
            assert!(ruc.bytes().all(|byte| byte.is_ascii_digit()));
        }
    }

    let es_codes = (0..14)
        .map(|_| es_unique.licenceCode().unwrap())
        .collect::<HashSet<_>>();
    let za_codes = (0..12)
        .map(|_| za_unique.licenceCode().unwrap())
        .collect::<HashSet<_>>();
    assert_eq!(
        es_codes,
        HashSet::from([
            "AM", "A1", "A2", "A", "B", "B+E", "C1", "C1+E", "C", "C+E", "D1", "D1+E", "D", "D+E",
        ])
    );
    assert_eq!(
        za_codes,
        HashSet::from([
            "A", "A1", "B", "C", "C1", "C2", "EB", "EC", "EC1", "I", "L", "L1"
        ])
    );
    assert!(es_unique.licenceCode().is_err());
    assert!(za_unique.licenceCode().is_err());
    assert!(es.unique(true).licenceCode().is_ok());

    assert!(es.ruc(None).is_err());
    assert!(pe.licenceCode().is_err());
    assert!(za.dni().is_err());
}

#[test]
fn regional_id_numbers_match_their_pinned_providers() {
    use chrono::NaiveDate;

    let nl = fake(Some("nl-NL")).unwrap();
    let za = fake(Some("en-ZA")).unwrap();
    let sa = fake(Some("ar-SA")).unwrap();
    let mn = fake(Some("mn-MN")).unwrap();
    let eg = fake(Some("ar-EG")).unwrap();
    let date = NaiveDate::from_ymd_opt(2000, 2, 29).unwrap();
    let alphabet = "АЛХБМЦВНЧГОШДӨЩЕПЪЁРЫЖСЬЗТЭИУЮЙҮЯКФ";
    let governorates = [
        "02", "19", "28", "25", "29", "31", "18", "22", "03", "35", "01", "12", "11", "26", "04",
        "13", "34", "16", "23", "14", "27", "15", "33", "17", "24", "32",
    ];
    for _ in 0..128 {
        let dutch = nl.unique(false).idNumber().unwrap();
        assert_eq!(dutch.len(), 9);
        assert!(dutch.as_bytes()[0] <= b'6');
        let weighted: u16 = dutch[..8]
            .bytes()
            .zip((2..=9).rev())
            .map(|(digit, factor)| u16::from(digit - b'0') * factor)
            .sum();
        assert_eq!(weighted % 11, u16::from(dutch.as_bytes()[8] - b'0'));

        let za_sample = za
            .unique(false)
            .idNumberWithBirthdate(Some(date), Some(false), Some("FEMALE"))
            .unwrap();
        assert_eq!(za_sample.len(), 13);
        assert_eq!(&za_sample[..6], "000229");
        assert!(matches!(za_sample.as_bytes()[6], b'0'..=b'4'));
        assert!(za_sample[10..].starts_with('1'));
        assert!(matches!(za_sample.as_bytes()[11], b'8' | b'9'));
        luhn(&za_sample);

        for (sample, prefixes) in [
            (sa.idNumber().unwrap(), "12"),
            (sa.nationalIdNumber().unwrap(), "1"),
            (sa.foreignerIdNumber().unwrap(), "2"),
        ] {
            assert_eq!(sample.len(), 10);
            assert!(prefixes.contains(sample.chars().next().unwrap()));
            luhn(&sample);
        }

        let mongolian = mn.idNumber().unwrap();
        let mut chars = mongolian.chars();
        assert!(
            alphabet.contains(chars.next().unwrap()) && alphabet.contains(chars.next().unwrap())
        );
        assert_eq!(chars.as_str().len(), 8);
        assert!(chars.as_str().bytes().all(|byte| byte.is_ascii_digit()));

        let egyptian = eg.nationalIdNumberWithGender(Some("male")).unwrap();
        assert_eq!(egyptian.len(), 14);
        let century = match egyptian.as_bytes()[0] {
            b'2' => "19",
            b'3' => "20",
            other => panic!("{other}"),
        };
        let birth =
            NaiveDate::parse_from_str(&format!("{century}{}", &egyptian[1..7]), "%Y%m%d").unwrap();
        assert!(
            (NaiveDate::from_ymd_opt(1950, 1, 10).unwrap()
                ..=NaiveDate::from_ymd_opt(2005, 12, 25).unwrap())
                .contains(&birth)
        );
        assert!(governorates.contains(&&egyptian[7..9]));
        assert_eq!((egyptian.as_bytes()[12] - b'0') % 2, 1);
        assert!(matches!(egyptian.as_bytes()[13], b'1'..=b'9'));
    }
    let symbols = mn.unique(true);
    let actual: std::collections::HashSet<_> =
        (0..35).map(|_| symbols.alphabet().unwrap()).collect();
    assert_eq!(actual, alphabet.chars().collect());
    assert!(symbols.alphabet().is_err());
    assert!(nl.idNumberWithBirthdate(None, None, None).is_err());
    assert!(eg.foreignerIdNumber().is_err());
}

#[test]
fn turkish_identity_and_baltic_passports_follow_pinned_formats() {
    use openrust::database::eloquent::factories::Faker;

    let turkey = fake(Some("tr-TR")).unwrap();
    let lithuania = fake(Some("lt-LT")).unwrap();
    let latvia = fake(Some("lv-LV")).unwrap();
    let format = Regex::new(r"^[a-z]{2}[0-9]{7}$").unwrap();

    assert_eq!(Faker::tcNoChecksum("100000001").unwrap(), "46");
    assert_eq!(Faker::tcNoChecksum("090909090").unwrap(), "-60");
    assert!(Faker::tcNoIsValid("10000000146").unwrap());
    assert!(!Faker::tcNoIsValid("10000000147").unwrap());
    assert!(Faker::tcNoIsValid("090909090-60").is_err());
    assert!(Faker::tcNoChecksum("12345678a").is_err());
    assert!(Faker::tcNoIsValid("你").is_err());

    for _ in 0..128 {
        let turkish = turkey.unique(false).tcNo().unwrap();
        let (prefix, suffix) = turkish.split_at(9);
        assert_eq!(Faker::tcNoChecksum(prefix).unwrap(), suffix);
        if suffix.len() == 2 {
            assert!(Faker::tcNoIsValid(&turkish).unwrap());
        }
        let lt = lithuania.unique(false).passportNumber().unwrap();
        assert_eq!(lt.len(), 8);
        assert!(lt.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(format.is_match(&latvia.unique(false).passportNumber().unwrap()));
    }
    assert!(latvia.tcNo().is_err());
    assert!(turkey.passportNumber().is_err());
}

#[test]
fn us_and_russian_tax_identifiers_follow_pinned_formats() {
    use openrust::database::eloquent::factories::Faker;

    let us = fake(Some("en-US")).unwrap();
    let ru = fake(Some("ru-RU")).unwrap();
    let ein_format = Regex::new(
        r"^(0[1-6]|1[0-6]|2[0-7]|3[0-9]|4[0-8]|5[0-9]|6[0-8]|7[1-7]|8[0-8]|9[0-589])-[0-9]{7}$",
    )
    .unwrap();
    let us_unique = us.unique(true);
    let ru_unique = ru.unique(true);
    let mut eins = std::collections::HashSet::new();
    let mut inns = std::collections::HashSet::new();
    for _ in 0..128 {
        let ein = us_unique.ein().unwrap();
        assert!(ein_format.is_match(&ein));
        assert_eq!(&ein[2..3], "-");
        assert!(ein[3..].bytes().all(|digit| digit.is_ascii_digit()));
        assert!(eins.insert(ein));
        let inn = ru_unique.inn10(Some("7")).unwrap();
        assert_eq!(&inn[..2], "07");
        assert!(Faker::inn10IsValid(&inn));
        assert_eq!(ru.kpp(Some(&inn)).unwrap(), format!("{}01001", &inn[..4]));
        assert!(inns.insert(inn));
        let area = ru.inn(Some("00")).unwrap()[..2].parse::<u8>().unwrap();
        assert!((1..=91).contains(&area));
    }
    assert_eq!(Faker::inn10Checksum("770708389").unwrap(), "3");
    assert!(Faker::inn10IsValid("7707083893"));
    assert!(!Faker::inn10IsValid("7707083894"));
    assert!(!Faker::inn10IsValid("你0708389"));
    assert!(Faker::inn10Checksum("12345678").is_err());
    assert!(ru.inn10(Some("100")).is_err());
    assert!(ru.inn10(Some("x7")).is_err());
    assert!(ru.kpp(Some("12é4")).is_err());
    assert_eq!(ru_unique.kpp(Some("7707")).unwrap(), "770701001");
    assert!(ru_unique.kpp(Some("7707")).is_err());
    assert!(ru.kpp(Some("77")).unwrap().ends_with("01001"));
    assert!(ru.ein().is_err());
    assert!(us.inn10(None).is_err());
    assert!(us.kpp(None).is_err());
}

#[test]
fn french_siret_and_polish_regon_follow_pinned_formats() {
    fn regon_valid(sample: &str, weights: &[u16]) {
        assert_eq!(sample.len(), weights.len() + 1);
        assert!(sample.bytes().all(|digit| digit.is_ascii_digit()));
        let sum: u16 = sample
            .bytes()
            .zip(weights)
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum();
        assert_eq!(
            sum % 11 % 10,
            u16::from(sample.as_bytes()[weights.len()] - b'0')
        );
    }

    let french = fake(Some("fr-FR")).unwrap();
    let polish = fake(Some("pl-PL")).unwrap();
    let formatted = Regex::new(r"^[1-9][0-9]{2} [0-9]{3} [0-9]{3} [0-9]{5}$").unwrap();
    let mut sirets = std::collections::HashSet::new();
    let mut regional_ids = std::collections::HashSet::new();
    let mut local_ids = std::collections::HashSet::new();
    let french_unique = french.unique(true);
    let polish_unique = polish.unique(true);
    let weights = [8, 9, 2, 3, 4, 5, 6, 7];
    for _ in 0..128 {
        let sample = french_unique.siret(None).unwrap();
        assert!(formatted.is_match(&sample));
        let sample = sample.replace(' ', "");
        assert!(sirets.insert(sample.clone()));
        luhn(&sample);
        luhn(&sample[..9]);
        let compact = french.siret(Some(false)).unwrap();
        assert_eq!(compact.len(), 14);
        assert!(compact.bytes().all(|digit| digit.is_ascii_digit()));
        luhn(&compact);

        let regional = polish_unique.regon().unwrap();
        regon_valid(&regional, &weights);
        assert_eq!((regional.as_bytes()[1] - b'0') % 2, 1);
        assert!(regional_ids.insert(regional));
        let local = polish_unique.regonLocal().unwrap();
        regon_valid(&local, &[2, 4, 8, 5, 0, 9, 7, 3, 6, 1, 2, 4, 8]);
        regon_valid(&local[..9], &weights);
        assert_eq!((local.as_bytes()[1] - b'0') % 2, 1);
        assert!(local_ids.insert(local));
    }
    assert!(polish.siret(None).is_err());
    assert!(french.regon().is_err());
    assert!(french.regonLocal().is_err());
}

#[test]
fn czech_danish_and_icelandic_business_identifiers_match_pinned_formats() {
    let czech = fake(Some("cs-CZ")).unwrap();
    let danish = fake(Some("da-DK")).unwrap();
    let icelandic = fake(Some("is-IS")).unwrap();

    let czech_unique = czech.unique(true);
    let danish_unique = danish.unique(true);
    let icelandic_unique = icelandic.unique(true);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..128 {
        let ico = czech_unique.ico().unwrap();
        assert_eq!(ico.len(), 8);
        assert!(ico.bytes().all(|digit| digit.is_ascii_digit()));
        let sum: u16 = ico
            .bytes()
            .zip((2_u16..=8).rev())
            .map(|(digit, weight)| u16::from(digit - b'0') * weight)
            .sum();
        let expected = [1, 0, 9, 8, 7, 6, 5, 4, 3, 2, 1][usize::from(sum % 11)];
        assert_eq!(ico.as_bytes()[7] - b'0', expected);
        assert!(seen.insert(("ico", ico)));
        for (formatter, value, len) in [
            ("cvr", danish_unique.cvr().unwrap(), 8),
            ("p", danish_unique.p().unwrap(), 10),
            ("vsk", icelandic_unique.vsk().unwrap(), 5),
        ] {
            assert_eq!(value.len(), len);
            assert!(value.bytes().all(|digit| digit.is_ascii_digit()));
            assert_ne!(value.as_bytes()[0], b'0');
            assert!(seen.insert((formatter, value)));
        }
    }
    assert!(danish.ico().is_err());
    assert!(czech.cvr().is_err());
    assert!(icelandic.p().is_err());
    assert!(danish.vsk().is_err());
}

#[test]
fn saudi_and_egyptian_company_numbers_match_pinned_formats() {
    let saudi = fake(Some("ar-SA")).unwrap();
    let egypt = fake(Some("ar-EG")).unwrap();
    let saudi_unique = saudi.unique(true);
    let egypt_unique = egypt.unique(true);
    let mut seen = std::collections::HashSet::new();

    for _ in 0..128 {
        let saudi_id = saudi_unique.companyIdNumber().unwrap();
        assert!(saudi_id.starts_with("700"));
        for (number, length) in [
            (saudi_id, 10),
            (egypt_unique.companyTaxIdNumber().unwrap(), 10),
            (egypt_unique.companyTradeRegisterNumber().unwrap(), 7),
        ] {
            assert_eq!(number.len(), length);
            assert!(number.bytes().all(|digit| digit.is_ascii_digit()));
            luhn(&number);
        }
        assert!(seen.insert(egypt_unique.companyTradeRegisterNumber().unwrap()));
    }

    assert!(egypt.companyIdNumber().is_err());
    assert!(saudi.companyTaxIdNumber().is_err());
    assert!(saudi.companyTradeRegisterNumber().is_err());
}

#[test]
fn kazakh_and_south_african_company_numbers_use_pinned_dates_and_formats() {
    use chrono::{Datelike, NaiveDate, Utc};
    let kazakh = fake(Some("kk-KZ")).unwrap();
    let south_african = fake(Some("en-ZA")).unwrap();
    let fixed = NaiveDate::from_ymd_opt(2000, 2, 29).unwrap();
    let bin = Regex::new(r"^0002[4-6][0-3][0-9]{6}$").unwrap();
    let company =
        Regex::new(r"^[0-9]{4}/[1-9][0-9]{5}/(0[126789]|1[0124567]|2[0-6]|3[01]|80)$").unwrap();
    let year = Utc::now().year();
    let mut bins = std::collections::HashSet::new();
    let mut companies = std::collections::HashSet::new();
    let unique_kazakh = kazakh.unique(true);
    let unique_south_african = south_african.unique(true);
    for _ in 0..128 {
        let id = unique_kazakh
            .businessIdentificationNumber(Some(fixed))
            .unwrap();
        assert!(bin.is_match(&id));
        assert!(bins.insert(id));
        let id = unique_south_african.companyNumber().unwrap();
        assert!(company.is_match(&id));
        assert!((year - 50..=year).contains(&id[..4].parse::<i32>().unwrap()));
        assert!(companies.insert(id));
    }
    let sampled = kazakh.businessIdentificationNumber(None).unwrap();
    assert_eq!(&sampled[..2], format!("{:02}", year.rem_euclid(100)));
    assert!((1..=Utc::now().month()).contains(&sampled[2..4].parse::<u32>().unwrap()));
    assert!(kazakh.companyNumber().is_err());
    assert!(south_african.businessIdentificationNumber(None).is_err());
}

#[test]
fn taiwanese_vat_and_greek_business_catalogs_follow_pinned_formats() {
    let taiwan = fake(Some("zh-Hant")).unwrap();
    let greek = fake(Some("el-GR")).unwrap();
    let vat = taiwan.unique(true);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..128 {
        let number = vat.VAT().unwrap();
        assert!((10_000_000..=99_999_999).contains(&number));
        assert!(seen.insert(number));
    }
    let compat = taiwan.vat(Some(false)).unwrap();
    assert_eq!(compat.len(), 8);
    assert!(compat.bytes().all(|digit| digit.is_ascii_digit()));
    let unique = greek.unique(true);
    let offices: std::collections::HashSet<_> = (0..21).map(|_| unique.doy().unwrap()).collect();
    let activities: std::collections::HashSet<_> =
        (0..19).map(|_| unique.object().unwrap()).collect();
    assert_eq!(offices.len(), 21);
    assert_eq!(activities.len(), 19);
    assert!(offices.contains("Α' Αθήνας"));
    assert!(offices.contains("ΔΟΥ ΠΛΟΙΩΝ"));
    assert!(activities.contains("Μηχανολόγος-μηχανικός"));
    assert!(activities.contains("Εισαγωγαί-εξαγωγαί"));
    assert!(unique.doy().is_err());
    assert!(unique.object().is_err());
    assert!(greek.VAT().is_err());
    assert!(taiwan.doy().is_err());
    assert!(taiwan.object().is_err());
}

#[test]
fn company_suffixes_and_templates_use_the_selected_provider() {
    use std::collections::HashSet;

    for (locale, suffixes) in [
        (
            "en-US",
            &["Inc", "and Sons", "LLC", "Group", "PLC", "Ltd"][..],
        ),
        (
            "fr-FR",
            &["SA", "S.A.", "SARL", "S.A.R.L.", "SAS", "S.A.S.", "et Fils"],
        ),
        ("it-IT", &["SPA", "e figli", "Group", "s.r.l."]),
        (
            "pl-PL",
            &[
                "S.A.",
                "i syn",
                "sp. z o.o.",
                "sp. j.",
                "sp. p.",
                "sp. k.",
                "S.K.A",
                "s. c.",
                "P.P.O.F",
            ],
        ),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());

        let mut names = HashSet::new();
        for _ in 0..64 {
            let value = unique.company().unwrap();
            assert!(!value.trim().is_empty());
            if locale == "it-IT" {
                assert!(
                    suffixes
                        .iter()
                        .any(|suffix| value.ends_with(&format!(" {suffix}")))
                );
            }
            assert!(names.insert(value));
        }
    }

    let fallback = fake(Some("ja-JP")).unwrap();
    assert!(!fallback.company().is_empty());
    assert!(!fallback.companySuffix().is_empty());
}

#[test]
fn east_asian_company_names_follow_their_locale_templates() {
    let mainland = fake(Some("zh-Hans")).unwrap();
    let taiwan = fake(Some("zh-Hant")).unwrap();
    let japan = fake(Some("ja-JP")).unwrap();
    let korea = fake(Some("ko-KR")).unwrap();
    let hangul = Regex::new(r"^(\(주\)|\(유\))?\p{Hangul}+$").unwrap();

    for _ in 0..128 {
        let name = mainland.company();
        assert!(
            [
                "科技有限公司",
                "网络有限公司",
                "信息有限公司",
                "传媒有限公司"
            ]
            .iter()
            .any(|suffix| name.ends_with(suffix))
        );
        assert!(taiwan.company().chars().all(|ch| !ch.is_ascii()));
        let japanese_name = japan.company();
        assert!(japanese_name.starts_with("株式会社 ") || japanese_name.starts_with("有限会社 "));
        assert!(hangul.is_match(&korea.company()));
    }

    let unique_japan = japan.unique(true);
    let prefixes = [
        unique_japan.companyPrefix().unwrap(),
        unique_japan.companyPrefix().unwrap(),
    ];
    assert!(prefixes.contains(&"株式会社") && prefixes.contains(&"有限会社"));
    assert!(unique_japan.companyPrefix().is_err());
    assert_eq!(japan.companySuffix(), "Ltd");
    let unique_taiwan = taiwan.unique(true);
    let modifiers: std::collections::HashSet<_> = (0..17)
        .map(|_| unique_taiwan.companyModifier().unwrap())
        .collect();
    assert_eq!(modifiers.len(), 17);
    assert!(unique_taiwan.companyModifier().is_err());
    let suffixes: std::collections::HashSet<_> = (0..6)
        .map(|_| unique_taiwan.companyEnSuffix().unwrap())
        .collect();
    assert_eq!(
        suffixes,
        ["Inc", "and Sons", "LLC", "Group", "PLC", "Ltd"].into()
    );
    assert!(unique_taiwan.companyEnSuffix().is_err());
    assert!(japan.companyModifier().is_err());
    assert!(mainland.companyEnSuffix().is_err());
}

#[test]
fn arabic_company_names_use_regional_templates_and_preserve_suffix_spacing() {
    use std::collections::HashSet;
    let starts = ["شركة", "مؤسسة", "مجموعة", "مكتب", "أكاديمية", "معرض"];
    let egyptian_suffixes = [
        " ش.م.م",
        " للتجاره العامه",
        "للأجهزة الطبيه",
        "للتوريدات",
        "للمقاولات",
        "للتطوير العقاري",
        "للدعايه و الاعلان",
        "للحلول المتقدمه",
        "للخدمات الدولية",
        "الدولية",
        "للانظمة المتكاملة",
    ];
    let jordanian_saudi_suffixes = [
        "وأولاده",
        "للمساهمة المحدودة",
        " ذ.م.م",
        "مساهمة عامة",
        "وشركائه",
    ];

    for (locale, suffixes) in [
        ("ar-EG", egyptian_suffixes.as_slice()),
        ("ar-JO", jordanian_saudi_suffixes.as_slice()),
        ("ar-SA", jordanian_saudi_suffixes.as_slice()),
    ] {
        let provider = fake(Some(locale)).unwrap();
        let unique = provider.unique(true);
        let prefixes: HashSet<_> = (0..starts.len())
            .map(|_| unique.companyPrefix().unwrap())
            .collect();
        assert_eq!(prefixes, starts.into());
        assert!(unique.companyPrefix().is_err());
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(sampled, suffixes.iter().map(|s| (*s).to_owned()).collect());
        assert!(unique.companySuffix().is_err());
        for _ in 0..128 {
            let name = provider.company();
            assert!(
                name.chars()
                    .any(|ch| ('\u{0600}'..='\u{06ff}').contains(&ch))
            );
            assert!(
                starts.iter().any(|s| name.starts_with(&format!("{s} ")))
                    || suffixes.iter().any(|s| name.ends_with(&format!(" {s}")))
            );
        }
    }
}

#[test]
fn central_european_company_names_preserve_regional_suffixes_and_templates() {
    use std::collections::HashSet;
    let germany = [
        "e.G.",
        "e.V.",
        "GbR",
        "OHG mbH",
        "GmbH & Co. OHG",
        "AG & Co. OHG",
        "GmbH",
        "AG",
        "KG",
        "GmbH & Co. KG",
        "AG & Co. KG",
        "Stiftung & Co. KG",
        "KGaA",
        "GmbH & Co. KGaA",
        "AG & Co. KGaA",
        "Stiftung & Co. KGaA",
    ];
    let austria = [
        "AG",
        "EWIV",
        "Ges.m.b.H.",
        "GmbH",
        "KEG",
        "KG",
        "OEG",
        "OG",
        "OHG",
        "SE",
    ];
    let switzerland = ["AG", "GmbH"];
    let czechia = ["s.r.o.", "a.s.", "o.p.s.", "o.s."];
    for (locale, suffixes) in [
        ("de-DE", germany.as_slice()),
        ("de-AT", austria.as_slice()),
        ("de-CH", switzerland.as_slice()),
        ("cs-CZ", czechia.as_slice()),
    ] {
        let provider = fake(Some(locale)).unwrap();
        let unique = provider.unique(true);
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|item| (*item).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
        let mut qualified = 0;
        for _ in 0..256 {
            let name = provider.company();
            assert!(!name.trim().is_empty());
            qualified += usize::from(
                suffixes
                    .iter()
                    .any(|suffix| name.ends_with(&format!(" {suffix}"))),
            );
        }
        if locale == "cs-CZ" {
            assert_eq!(qualified, 256);
        } else {
            assert!((0..256).contains(&qualified) && qualified > 0);
        }
    }
}

#[test]
fn nordic_company_names_use_regional_suffixes_and_connectors() {
    use std::collections::HashSet;
    for (locale, suffixes, connector) in [
        ("da-DK", &["ApS", "A/S", "I/S", "K/S"][..], " og "),
        ("is-IS", &["ehf.", "hf.", "sf."], " og "),
        (
            "fi-FI",
            &["OY", "AB", "OY AB", "RY", "OYj", "Tmi.", "Inc.", "Ltd"],
            " and ",
        ),
        ("sv-SE", &["AB", "HB"], " och "),
        (
            "nb-NO",
            &[
                "ANS", "AS", "ASA", "BA", "DA", "ENK", "GFS", "KTRF", "NUF", "PK", "SA", "SPA",
                "STI", "VIFE",
            ],
            " og ",
        ),
    ] {
        let provider = fake(Some(locale)).unwrap();
        let unique = provider.unique(true);
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());

        let names: Vec<_> = (0..128).map(|_| provider.company()).collect();
        assert!(names.iter().any(|name| name.contains(connector)));
        assert!(names.iter().any(|name| {
            suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        }));
    }
}

#[test]
fn spanish_and_portuguese_companies_use_regional_suffixes_and_prefixes() {
    use std::collections::HashSet;
    let common_spanish = [
        "e Hijo", "e Hija", "e Hijos", "y Asoc.", "y Flia.", "SRL", "SA", "S. de H.",
    ];
    let spanish_prefixes = [
        "Asociación",
        "Centro",
        "Corporación",
        "Empresa",
        "Gestora",
        "Global",
        "Grupo",
        "Viajes",
        "Air",
    ];
    let venezuelan_prefixes = [
        "Asociación",
        "Centro",
        "Corporación",
        "Cooperativa",
        "Empresa",
        "Gestora",
        "Global",
        "Grupo",
        "Viajes",
        "Inversiones",
        "Lic.",
        "Dr.",
    ];
    for (locale, suffixes, prefixes, connector) in [
        ("es-AR", common_spanish.as_slice(), &[][..], " y "),
        (
            "es-ES",
            common_spanish.as_slice(),
            spanish_prefixes.as_slice(),
            " y ",
        ),
        ("es-PE", common_spanish.as_slice(), &[][..], " y "),
        (
            "es-VE",
            &["S.R.L.", "C.A.", "S.A.", "R.L.", "etc"],
            venezuelan_prefixes.as_slice(),
            " y ",
        ),
        (
            "pt-BR",
            &["e Filhos", "e Associados", "Ltda.", "S.A."],
            &[][..],
            " e ",
        ),
        (
            "pt-PT",
            &["e Filhos", "e Associados", "Lda.", "S.A."],
            &[][..],
            " e ",
        ),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
        let sampled: HashSet<_> = (0..prefixes.len())
            .map(|_| unique.companyPrefix().unwrap())
            .collect();
        assert_eq!(sampled, prefixes.iter().copied().collect());
        assert!(unique.companyPrefix().is_err());

        let names: Vec<_> = (0..128).map(|_| faker.company()).collect();
        assert!(names.iter().any(|name| name.contains(connector)));
        assert!(names.iter().any(|name| {
            suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        }));
        if !prefixes.is_empty() {
            assert!(names.iter().any(|name| {
                prefixes
                    .iter()
                    .any(|prefix| name.starts_with(&format!("{prefix} ")))
            }));
        }
    }
}

#[test]
fn dutch_company_names_use_store_patterns_and_belgian_suffixes() {
    use std::collections::HashSet;
    let dutch = fake(Some("nl-NL")).unwrap();
    let belgian = fake(Some("nl-BE")).unwrap();
    let dutch_suffixes = [
        "VOF",
        "CV",
        "LLP",
        "BV",
        "NV",
        "IBC",
        "CSL",
        "EESV",
        "SE",
        "Stichting",
        "& Zonen",
        "& Zn",
    ];
    let belgian_suffixes = [
        "VZW", "Comm.V", "VOF", "BVBA", "EBVBA", "ESV", "NV", "Comm.VA", "CVOA", "CVBA", "& Zonen",
        "& Zn",
    ];
    for (faker, suffixes) in [(&dutch, dutch_suffixes), (&belgian, belgian_suffixes)] {
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(actual, suffixes.into_iter().map(str::to_owned).collect());
        assert!(unique.companySuffix().is_err());
        assert!(faker.companyPrefix().is_err());
    }
    let names: Vec<_> = (0..256)
        .map(|_| {
            let name = dutch.company();
            let stem = dutch_suffixes
                .iter()
                .find_map(|suffix| name.strip_suffix(&format!(" {suffix}")));
            stem.unwrap_or(&name).to_owned()
        })
        .collect();
    assert!(names.iter().any(|name| !name.contains(' ')));
    assert!(names.iter().any(|name| name.contains(' ')));
    assert!(names.iter().any(|name| {
        ["Boekhandel ", "Fietsenmaker ", "Café ", "Garage "]
            .iter()
            .any(|prefix| name.starts_with(prefix))
    }));
}

#[test]
fn croatian_slovak_and_hungarian_companies_use_local_formats() {
    use std::collections::HashSet;
    for (locale, suffixes) in [
        ("hr-HR", &["d.o.o.", "j.d.o.o.", "Security"][..]),
        ("sk-SK", &["s.r.o.", "a.s.", "v.o.s."][..]),
        (
            "hu-HU",
            &["Kft.", "és Tsa", "Kht", "Zrt.", "Nyrt.", "Bt."][..],
        ),
    ] {
        let provider = fake(Some(locale)).unwrap();
        let unique = provider.unique(true);
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|item| (*item).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
        let names: Vec<_> = (0..128).map(|_| provider.company()).collect();
        let has_suffix = |name: &String| {
            suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        };
        assert!(names.iter().any(has_suffix));
        assert!(names.iter().any(|name| !has_suffix(name)));
        if locale == "sk-SK" {
            assert!(names.iter().any(|name| name.contains(" and ")));
        }
        if locale == "hr-HR" {
            let prefixes: HashSet<_> = (0..11).map(|_| unique.companyPrefix().unwrap()).collect();
            assert!(prefixes.contains("Cvjećarnica"));
            assert!(unique.companyPrefix().is_err());
            assert!(names.iter().any(|name| {
                prefixes
                    .iter()
                    .any(|prefix| name.starts_with(&format!("{prefix} ")))
            }));
        } else {
            assert!(provider.companyPrefix().is_err());
        }
    }
}

#[test]
fn greek_and_montenegrin_companies_follow_effective_upstream_templates() {
    use std::collections::HashSet;
    let greek = fake(Some("el-GR")).unwrap();
    let montenegrin = fake(Some("me-ME")).unwrap();
    let greek_suffixes = ["Ο.Ε", "Ε.Ε", "Α.Ε", "Ε.Π.Ε"];
    let types = ["A.D.", "A.D PODGORICA"];
    let unique = greek.unique(true);
    let actual: HashSet<_> = (0..greek_suffixes.len())
        .map(|_| unique.companySuffix().unwrap())
        .collect();
    assert_eq!(actual, greek_suffixes.map(str::to_owned).into());
    assert!(unique.companySuffix().is_err());
    let unique = montenegrin.unique(true);
    let actual = [unique.companyType().unwrap(), unique.companyType().unwrap()];
    assert_eq!(actual.into_iter().collect::<HashSet<_>>(), types.into());
    assert!(unique.companyType().is_err());
    let mut names = HashSet::new();
    for _ in 0..64 {
        let name = greek.company();
        assert!(
            greek_suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        );
        let name = montenegrin.company();
        assert!(types.iter().any(|kind| name.ends_with(&format!(" {kind}"))));
        assert!(names.insert(unique.companyName().unwrap()));
    }
    assert_eq!(montenegrin.companySuffix(), "Ltd");
    assert!(greek.companyName().is_err());
    assert!(greek.companyType().is_err());
}

#[test]
fn british_south_african_and_cypriot_company_names_follow_local_templates() {
    use std::collections::HashSet;

    for locale in ["en-GB", "en-ZA"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        assert_eq!(unique.companySuffix().unwrap(), "Ltd");
        assert!(unique.companySuffix().is_err());
        let mut names = HashSet::new();
        for _ in 0..64 {
            let company = unique.company().unwrap();
            assert!(
                company
                    .strip_suffix(" Ltd")
                    .is_some_and(|name| !name.is_empty())
            );
            assert!(names.insert(company));
        }
    }

    let faker = fake(Some("el-CY")).unwrap();
    let unique = faker.unique(true);
    let suffixes = ["ΛΤΔ", "Δημόσια εταιρεία", "& Υιοι", "& ΣΙΑ"];
    let actual: HashSet<_> = (0..4).map(|_| unique.companySuffix().unwrap()).collect();
    assert_eq!(actual, suffixes.map(str::to_owned).into());
    assert!(unique.companySuffix().is_err());
    let samples: Vec<_> = (0..128).map(|_| faker.company()).collect();
    let suffix_name = |name: &String| {
        suffixes
            .iter()
            .any(|suffix| name.ends_with(&format!(" {suffix}")))
    };
    assert!(samples.iter().any(suffix_name));
    assert!(
        samples
            .iter()
            .any(|name| name.contains('-') && !suffix_name(name))
    );
    assert!(faker.companyPrefix().is_err());
}

#[test]
fn french_and_italian_regional_companies_follow_inherited_templates() {
    use std::collections::HashSet;
    for (locale, suffixes) in [
        (
            "fr-BE",
            &[
                "ASBL",
                "SCS",
                "SNC",
                "SPRL",
                "Associations",
                "Entreprise individuelle",
                "GEIE",
                "GIE",
                "SA",
                "SCA",
                "SCRI",
                "SCRL",
            ][..],
        ),
        (
            "fr-CA",
            &["SA", "S.A.", "SARL", "S.A.R.L.", "SAS", "S.A.S.", "et Fils"][..],
        ),
        ("fr-CH", &["AG", "Sàrl", "SA", "GmbH"][..]),
        ("it-CH", &["SA", "Sarl"][..]),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
        let names: Vec<_> = (0..256).map(|_| faker.company()).collect();
        let has_suffix = |name: &String| {
            suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        };
        assert!(names.iter().any(has_suffix));
        assert!(names.iter().any(|name| !has_suffix(name)));
        assert!(faker.companyPrefix().is_err());
    }
}

#[test]
fn lithuanian_slovenian_and_turkish_companies_preserve_local_formats() {
    use std::collections::HashSet;

    let lithuanian = fake(Some("lt-LT")).unwrap();
    let slovenian = fake(Some("sl-SI")).unwrap();
    let turkish = fake(Some("tr-TR")).unwrap();
    for (faker, suffixes) in [
        (&lithuanian, &["UAB", "AB", "IĮ", "MB", "VŠĮ"][..]),
        (
            &slovenian,
            &["d.o.o.", "d.d.", "k.d.", "k.d.d.", "d.n.o.", "so.p."][..],
        ),
        (&turkish, &["A.Ş.", "Ltd. Şti."][..]),
    ] {
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
    }
    let names: Vec<_> = (0..128).map(|_| lithuanian.company()).collect();
    assert!(names.iter().all(|name| {
        ["UAB", "AB", "IĮ", "MB", "VŠĮ"]
            .iter()
            .any(|suffix| name.starts_with(&format!("{suffix} ")))
    }));
    assert!(names.iter().any(|name| name.ends_with('"')));
    assert!((0..128).any(|_| slovenian.company().ends_with(" s.p.")));
    assert!((0..128).any(|_| turkish.company().contains("oğlu ")));
    let unique = turkish.unique(true);
    let fields: HashSet<_> = (0..49).map(|_| unique.companyField().unwrap()).collect();
    assert_eq!(fields.len(), 49);
    assert!(fields.contains("Dış Ticaret"));
    assert!(unique.companyField().is_err());
    assert!(lithuanian.companyField().is_err());
}

#[test]
fn russian_ukrainian_and_kazakh_companies_use_local_components() {
    for (locale, prefixes, suffix_count) in [
        (
            "ru-RU",
            &["ООО", "ЗАО", "ООО Компания", "ОАО", "ПАО", "МКК", "МФО"][..],
            18,
        ),
        ("kk-KZ", &["АҚ", "ЖШС", "ЖАҚ"][..], 8),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: std::collections::HashSet<_> = (0..prefixes.len())
            .map(|_| unique.companyPrefix().unwrap())
            .collect();
        assert_eq!(actual, prefixes.iter().copied().collect());
        assert!(unique.companyPrefix().is_err());
        for _ in 0..suffix_count {
            unique.companyNameSuffix().unwrap();
        }
        assert!(unique.companyNameSuffix().is_err());
        assert!(!faker.companyNameElement().unwrap().is_empty());
        assert_eq!(faker.companySuffix(), "Ltd");
        for _ in 0..64 {
            let name = faker.company();
            assert!(
                prefixes
                    .iter()
                    .any(|prefix| name.starts_with(&format!("{prefix} ")))
            );
        }
    }
    let faker = fake(Some("uk-UA")).unwrap();
    assert!(!faker.companyName().unwrap().is_empty());
    assert!(faker.companyNameElement().is_err());
    assert!(faker.companyNameSuffix().is_err());
    assert!((0..128).any(|_| faker.company().contains('"')));
    assert!((0..128).any(|_| faker.company().contains('-')));
}

#[test]
fn west_asian_companies_keep_regional_templates_and_components() {
    let iran = fake(Some("fa-IR")).unwrap();
    let israel = fake(Some("he-IL")).unwrap();
    let armenia = fake(Some("hy-AM")).unwrap();
    let georgia = fake(Some("ka-GE")).unwrap();

    for (provider, suffixes) in [
        (&iran, &["Ltd"][..]),
        (&israel, &["בע\"מ", "ובניו", "סוכנויות", "משווקים"][..]),
        (&armenia, &["ՍՊԸ", "և որդիներ", "ՓԲԸ", "ԲԲԸ"][..]),
        (&georgia, &["Ltd"][..]),
    ] {
        let unique = provider.unique(true);
        let actual: std::collections::HashSet<_> = (0..suffixes.len())
            .map(|_| unique.companySuffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.companySuffix().is_err());
    }

    let unique = iran.unique(true);
    for _ in 0..6 {
        unique.contract().unwrap();
    }
    assert!(unique.contract().is_err());
    for _ in 0..10 {
        unique.companyField().unwrap();
    }
    assert!(unique.companyField().is_err());
    assert!(israel.contract().is_err());
    assert!((0..128).any(|_| israel.company().contains(" את ")));
    assert!((0..128).any(|_| armenia.company().ends_with(" եղբայրներ")));
    let prefixes = [
        "შპს",
        "სს",
        "კს",
        "სს კორპორაცია",
        "იმ",
        "სპს",
        "კოოპერატივი",
    ];
    for _ in 0..64 {
        let name = georgia.company();
        assert!(
            prefixes
                .iter()
                .any(|prefix| name.starts_with(&format!("{prefix} ")))
        );
        assert!(!georgia.companyNameElement().unwrap().is_empty());
        assert!(!georgia.companyNameSuffix().unwrap().is_empty());
    }
}

#[test]
fn south_and_southeast_asian_companies_keep_pinned_formats() {
    let bangladesh = fake(Some("bn-BD")).unwrap();
    let indonesia = fake(Some("id-ID")).unwrap();
    let malaysia = fake(Some("ms-MY")).unwrap();
    let thailand = fake(Some("th-TH")).unwrap();
    let mut names = std::collections::HashSet::new();

    let company = bangladesh.unique(true);
    for _ in 0..9 {
        let value = company.company().unwrap();
        let (name, kind) = value.split_once(' ').unwrap();
        assert!(["রহিম", "করিম", "বাবলু"].contains(&name));
        assert!(["সিমেন্ট", "সার", "ঢেউটিন"].contains(&kind));
        assert!(names.insert(value));
    }
    assert!(company.company().is_err());
    assert_eq!(bangladesh.companySuffix(), "Ltd");
    assert!(["রহিম", "করিম", "বাবলু"].contains(&bangladesh.companyName().unwrap()));
    assert!(["সিমেন্ট", "সার", "ঢেউটিন"].contains(&bangladesh.companyType().unwrap()));

    let id_prefixes = ["PT", "Fa", "CV", "UD", "PJ", "PD", "Perum", "Yayasan"];
    let my_suffixes = [
        "Berhad",
        "Bhd",
        "Bhd.",
        "Enterprise",
        "Sdn Bhd",
        "Sdn. Bhd.",
    ];
    let thai = Regex::new(r"^[\p{Thai} ]+ Ltd$").unwrap();
    let slogan =
        Regex::new(r"^(เชื่อมต่อ|สรรสร้าง|เชื่อมโยง|ส่งเสริม|เปลี่ยน|ประสาน|พัฒนา)[\p{Thai}0-9 ]+$").unwrap();

    for _ in 0..128 {
        let value = indonesia.company();
        let (prefix, body) = value.split_once(' ').unwrap();
        assert!(id_prefixes.contains(&prefix));
        assert!(!body.is_empty());
        assert!(id_prefixes.contains(&indonesia.companyPrefix().unwrap()));
        assert!(["Tbk", "(Persero) Tbk"].contains(&indonesia.companySuffix().as_str()));

        let value = malaysia.company();
        assert!(
            my_suffixes
                .iter()
                .any(|suffix| value.ends_with(&format!(" {suffix}")))
        );
        assert!(my_suffixes.contains(&malaysia.companySuffix().as_str()));
        assert!(thai.is_match(&thailand.company()));
        assert!(slogan.is_match(&thailand.slogan().unwrap()));
    }

    let unique = malaysia.unique(true);
    assert_ne!(unique.companyName().unwrap(), unique.companyName().unwrap());
    assert_ne!(unique.industry().unwrap(), unique.industry().unwrap());
    let unique = thailand.unique(true);
    assert_ne!(unique.slogan().unwrap(), unique.slogan().unwrap());
    assert_eq!(thailand.companySuffix(), "Ltd");
    assert!(indonesia.industry().is_err());
    assert!(malaysia.companyType().is_err());
    assert!(bangladesh.slogan().is_err());
}

#[test]
fn job_titles_use_the_company_provider_and_share_unique_history() {
    let swedish = fake(Some("sv-SE")).unwrap();
    let titles = swedish.unique(true);
    let catalog: std::collections::HashSet<_> =
        (0..22).map(|_| titles.jobTitle().unwrap()).collect();
    assert_eq!(catalog.len(), 22);
    assert!(catalog.contains("Automationsingenjör"));
    assert!(catalog.contains("Webbutvecklare"));
    assert!(catalog.contains(swedish.jobTitle().as_str()));
    assert!(swedish.uniqueWithRetries(false, 1).jobTitle().is_err());
    assert!(catalog.contains(swedish.unique(true).jobTitle().unwrap().as_str()));

    let german = fake(Some("de-DE")).unwrap();
    let german_titles = german.unique(true);
    let catalog: std::collections::HashSet<_> =
        (0..25).map(|_| german_titles.jobTitle().unwrap()).collect();
    assert!(catalog.contains("Abteilungsdirektor"));
    assert!(catalog.contains("Werksleiter"));

    let french = fake(Some("fr-CA")).unwrap();
    let french_titles = french.unique(true);
    let catalog: std::collections::HashSet<_> = (0..300)
        .map(|_| french_titles.jobTitle().unwrap())
        .collect();
    assert!(catalog.contains("Agent d'accueil"));
    assert!(catalog.contains("Vitrier"));
    for locale in ["fr-FR", "fr-BE", "fr-CH"] {
        assert!(catalog.contains(&fake(Some(locale)).unwrap().jobTitle()));
    }

    let fallback_locale = fake(Some("en-CA")).unwrap();
    let inherited_locale = fake(Some("zh-Hant")).unwrap();
    let inherited = inherited_locale.unique(true);
    for _ in 0..32 {
        let title = inherited.jobTitle().unwrap();
        assert!(title.bytes().all(|byte| byte.is_ascii_lowercase()));
    }
    // en-CA lacks a dedicated Company provider and uses the US title list.
    assert!((0..32).any(|_| {
        fallback_locale
            .jobTitle()
            .bytes()
            .any(|byte| byte.is_ascii_uppercase())
    }));
}

#[test]
fn taiwanese_english_company_names_require_a_surname_formatter() {
    use openrust::database::DatabaseError;

    let taiwan = fake(Some("zh-Hant")).unwrap();
    assert!(matches!(
        taiwan.companyEn(),
        Err(DatabaseError::InvalidQuery(
            "unknown Faker formatter lastNameEn"
        ))
    ));
    let unique = taiwan.unique(true);
    assert!(unique.companyEn().is_err());
    let titles: std::collections::HashSet<_> = (0..8)
        .map(|_| {
            unique
                .companyEnWithLastName(|| Ok("Lin".to_owned()))
                .unwrap()
        })
        .collect();
    assert_eq!(
        titles,
        [
            "Lin Inc",
            "Lin and Sons",
            "Lin LLC",
            "Lin Group",
            "Lin PLC",
            "Lin Ltd",
            "Lin-Lin",
            "Lin, Lin and Lin"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
    assert!(
        taiwan
            .uniqueWithRetries(false, 1)
            .companyEnWithLastName(|| Ok("Lin".to_owned()))
            .is_err()
    );

    let mut calls = 0;
    let name = taiwan
        .companyEnWithLastName(|| {
            calls += 1;
            Ok(format!("Surname{calls}"))
        })
        .unwrap();
    assert!(match calls {
        1 => name.starts_with("Surname1 "),
        2 => name == "Surname1-Surname2",
        3 => name == "Surname1, Surname2 and Surname3",
        _ => false,
    });

    assert!(matches!(
        taiwan.companyEnWithLastName(|| Err(DatabaseError::InvalidQuery("surname source failed"))),
        Err(DatabaseError::InvalidQuery("surname source failed"))
    ));
    let other = fake(Some("en-US")).unwrap();
    assert!(matches!(
        other.companyEnWithLastName(|| panic!("unsupported locale must skip the formatter")),
        Err(DatabaseError::InvalidQuery(
            "unsupported Faker English company name locale"
        ))
    ));
}

#[test]
fn person_and_company_share_the_pinned_surnames() {
    let cases = [
        ("en-US", 473, "Abbott", "Zulauf"),
        ("fr-FR", 400, "Martin", "Valette"),
        ("it-IT", 101, "Rossi", "Verdi"),
        ("ja-JP", 51, "青田", "渡辺"),
    ];
    for (locale, count, first, last) in cases {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let catalog: std::collections::HashSet<_> =
            (0..count).map(|_| unique.lastName().unwrap()).collect();
        assert_eq!(catalog.len(), count);
        assert!(catalog.contains(first));
        assert!(catalog.contains(last));
        assert!(faker.uniqueWithRetries(false, 1).lastName().is_err());
        for _ in 0..32 {
            assert!(catalog.contains(&faker.lastName()));
            for name in [
                faker.name(),
                faker.nameWithGender(Some("male")),
                faker.company(),
            ] {
                assert!(
                    catalog
                        .iter()
                        .any(
                            |surname| name.match_indices(surname).any(|(start, matched)| {
                                let end = start + matched.len();
                                name[..start]
                                    .chars()
                                    .next_back()
                                    .is_none_or(|c| matches!(c, ' ' | '-' | ','))
                                    && name[end..]
                                        .chars()
                                        .next()
                                        .is_none_or(|c| matches!(c, ' ' | '-' | ','))
                            })
                        )
                );
            }
        }
    }
}

#[test]
fn localized_person_surname_catalogs_are_used_by_public_formatters() {
    use std::collections::HashSet;

    let cases = [
        ("ar-SA", 51, "العتيبي"),
        ("de-DE", 500, "Ackermann"),
        ("fa-IR", 346, "آختاچی"),
        ("pt-BR", 182, "Abreu"),
        ("pt-PT", 96, "Abreu"),
        ("tr-TR", 197, "Abacı"),
        ("zh-CN", 297, "李"),
        ("zh-TW", 504, "趙"),
    ];
    for (locale, count, expected) in cases {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..count).map(|_| unique.lastName().unwrap()).collect();
        assert_eq!(actual.len(), count);
        assert!(actual.contains(expected));
        assert!(faker.uniqueWithRetries(false, 1).lastName().is_err());
        assert!(actual.contains(&faker.lastName()));
    }

    let dutch = fake(Some("nl-NL")).unwrap();
    let surnames: HashSet<_> = (0..12_000).map(|_| dutch.lastName()).collect();
    for expected in ["Albinus genaamd Weiss von Weissenlöw", "de Jong", "Yilmaz"] {
        assert!(surnames.contains(expected));
    }
    assert!(!surnames.contains("Doe"));

    for locale in ["zh-Hans", "zh-Hant"] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..64 {
            let name = faker.name();
            assert!(name.chars().all(|character| !character.is_ascii()));
            let first = faker.firstName();
            assert!((1..=2).contains(&first.chars().count()));
        }
    }
}

#[test]
fn usernames_use_regional_internet_templates_and_keep_email_local_parts() {
    let japan = fake(Some("ja-JP")).unwrap();
    let china = fake(Some("zh-Hans")).unwrap();
    let lithuania = fake(Some("lt-LT")).unwrap();
    let mut chinese_underscore = false;
    let mut lithuanian_pair = false;
    let mut japanese_pair = false;
    for _ in 0..256 {
        let username = japan.userName();
        japanese_pair |= username.contains('.');
        assert!(
            username
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.'))
        );

        let username = china.userName();
        chinese_underscore |= username.contains('_');

        let username = lithuania.userName();
        if let Some((last, _)) = username.split_once('.') {
            lithuanian_pair = true;
            assert!(
                [
                    "balciunaite",
                    "baranauskas",
                    "butkus",
                    "butkute",
                    "jankauskaite",
                    "jankauskas",
                    "jankauskiene",
                    "kateiva",
                    "kavaliauskaite",
                    "kavaliauskas",
                    "kavaliauskiene",
                    "kazlauskaite",
                    "kazlauskas",
                    "kazlauskiene",
                    "lukoseviciute",
                    "navickiene",
                    "paulauskas",
                    "paulauskiene",
                    "petrauskaite",
                    "petrauskas",
                    "petrauskiene",
                    "pocius",
                    "pociute",
                    "sakalauskas",
                    "stankeviciene",
                    "stankevicius",
                    "stankeviciute",
                    "urbonas",
                    "urboniene",
                    "vasiliauskaite",
                    "vasiliauskas",
                    "vasiliauskiene",
                    "zukauskas",
                    "zukauskiene"
                ]
                .contains(&last)
            );
        }
    }
    assert!(chinese_underscore && lithuanian_pair && japanese_pair);

    for locale in [
        "ar-SA", "ar-JO", "ar-EG", "fa-IR", "ko-KR", "ja-JP", "lt-LT", "zh-Hans", "zh-Hant",
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let samples: std::collections::HashSet<_> =
            (0..32).map(|_| unique.userName().unwrap()).collect();
        assert_eq!(samples.len(), 32);
        for value in [faker.safeEmail(), faker.freeEmail()] {
            let (local, domain) = value.split_once('@').unwrap();
            assert!(!domain.is_empty());
            assert!(!local.is_empty());
            assert!(
                local
                    .bytes()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_'))
            );
        }
    }
}

#[test]
fn internet_domains_use_pinned_catalogs_and_nepalese_email_templates() {
    use std::collections::HashSet;

    for (locale, domains, suffixes) in [
        (
            "en-GB",
            &[
                "gmail.com",
                "yahoo.com",
                "hotmail.com",
                "gmail.co.uk",
                "yahoo.co.uk",
                "hotmail.co.uk",
            ][..],
            &["com", "biz", "info", "net", "org", "co.uk"][..],
        ),
        (
            "fr-CH",
            &[
                "gmail.com",
                "hotmail.com",
                "yahoo.com",
                "googlemail.com",
                "gmx.ch",
                "bluewin.ch",
                "swissonline.ch",
            ][..],
            &["com", "net", "org", "li", "ch"][..],
        ),
        (
            "kk-KZ",
            &["mail.kz", "yandex.kz", "host.kz"][..],
            &["com", "net", "org", "kz"][..],
        ),
        (
            "uk-UA",
            &["gmail.com", "mail.ru", "ukr.net", "i.ua", "rambler.ru"][..],
            &["ua", "com.ua", "org.ua", "net.ua", "com", "net", "org"][..],
        ),
        (
            "ko-KR",
            &["gmail.com", "yahoo.com", "hotmail.com"][..],
            &["biz", "com", "info", "kr", "net", "org"][..],
        ),
        (
            "ne-NP",
            &["gmail.com", "yahoo.com", "hotmail.com"][..],
            &["com", "net", "org"][..],
        ),
        (
            "en-US",
            &["gmail.com", "yahoo.com", "hotmail.com"][..],
            &["com", "biz", "info", "net", "org"][..],
        ),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..domains.len())
            .map(|_| unique.freeEmailDomain().unwrap())
            .collect();
        assert_eq!(actual, domains.iter().copied().collect());
        assert!(unique.freeEmailDomain().is_err());
        let actual: HashSet<_> = (0..suffixes.len()).map(|_| unique.tld().unwrap()).collect();
        assert_eq!(actual, suffixes.iter().copied().collect());
        assert!(unique.tld().is_err());
    }

    let japan = fake(Some("ja-JP")).unwrap();
    let domain = japan.domainName().unwrap();
    let (name, _) = domain.split_once('.').unwrap();
    assert!(
        [
            "aota",
            "aoyama",
            "ishida",
            "idaka",
            "ito",
            "uno",
            "ekoda",
            "ogaki",
            "kato",
            "kanou",
            "kijima",
            "kimura",
            "kiriyama",
            "kudo",
            "koizumi",
            "kobayashi",
            "kondo",
            "saito",
            "sakamoto",
            "sasaki",
            "sato",
            "sasada",
            "suzuki",
            "sugiyama",
            "takahashi",
            "tanaka",
            "tanabe",
            "tsuda",
            "nakajima",
            "nakamura",
            "nagisa",
            "nakatsugawa",
            "nishinosono",
            "nomura",
            "harada",
            "hamada",
            "hirokawa",
            "fujimoto",
            "matsumoto",
            "miyake",
            "miyazawa",
            "murayama",
            "yamagishi",
            "yamaguchi",
            "yamada",
            "yamamoto",
            "yoshida",
            "yoshimoto",
            "wakamatsu",
            "watanabe"
        ]
        .contains(&name)
    );

    let nepal = fake(Some("ne-NP")).unwrap();
    let mut categories = HashSet::new();
    for _ in 0..256 {
        let email = nepal.email().unwrap();
        let (_, host) = email.split_once('@').unwrap();
        categories.insert(if host.ends_with(".np") {
            assert!(
                [".com.np", ".net.np", ".org.np"]
                    .iter()
                    .any(|suffix| host.ends_with(suffix))
            );
            "nepal"
        } else if ["gmail.com", "yahoo.com", "hotmail.com"].contains(&host) {
            "free"
        } else {
            "company"
        });
    }
    assert_eq!(categories, HashSet::from(["nepal", "free", "company"]));
    assert!(!nepal.companyEmail().unwrap().ends_with(".np"));
}

#[test]
fn nepalese_urls_and_ukrainian_company_labels_follow_the_pinned_templates() {
    use openrust::database::DatabaseError;
    use std::collections::HashSet;

    let nepal = fake(Some("ne-NP")).unwrap();
    let unique = nepal.unique(true);
    let samples: HashSet<_> = (0..512).map(|_| unique.url().unwrap()).collect();
    assert_eq!(samples.len(), 512);
    let shapes: HashSet<_> = samples
        .iter()
        .map(|url| {
            let (host, path) = url
                .strip_prefix("http://")
                .unwrap()
                .split_once('/')
                .unwrap();
            (
                host.starts_with("www."),
                host.ends_with(".np"),
                !path.is_empty(),
                path.ends_with(".html"),
            )
        })
        .collect();
    assert_eq!(
        shapes,
        HashSet::from([
            (true, true, false, false),
            (false, true, false, false),
            (true, true, true, false),
            (true, true, true, true),
            (false, true, true, false),
            (false, false, true, true),
            (true, false, false, false),
            (false, false, false, false),
        ])
    );

    let ukraine = fake(Some("uk-UA")).unwrap();
    let unique = ukraine.unique(true);
    let mut shapes = HashSet::new();
    for _ in 0..64 {
        let label = unique.companyUrl().unwrap();
        assert!(label.chars().all(|ch| ch == '-' || ch.is_alphabetic()));
        if let Some((_, suffix)) = label.rsplit_once('-') {
            assert!(["Сервіс", "Плюс", "Груп", "Стиль", "Дизайн"].contains(&suffix));
        }
        shapes.insert(label.contains('-'));
    }
    assert_eq!(shapes, HashSet::from([true, false]));
    assert!(matches!(
        nepal.companyUrl(),
        Err(DatabaseError::InvalidQuery(
            "unsupported Faker company URL locale"
        ))
    ));
}

#[test]
fn nordic_names_follow_effective_person_templates() {
    for locale in ["sv-SE", "nb-NO"] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..64 {
            assert_eq!(faker.name().split_whitespace().count(), 2);
        }
    }

    let denmark = fake(Some("da-DK")).unwrap();
    let mut lengths = std::collections::HashSet::new();
    for _ in 0..128 {
        lengths.insert(denmark.name().split([' ', '-']).count());
    }
    assert_eq!(lengths, [2, 3, 4].into_iter().collect());
    let unique = denmark.unique(true);
    assert_ne!(unique.middleName().unwrap(), unique.middleName().unwrap());

    let iceland = fake(Some("is-IS")).unwrap();
    for _ in 0..64 {
        assert!(iceland.lastName().ends_with('s'));
        assert!(iceland.nameWithGender(Some("male")).ends_with("sson"));
        assert!(iceland.nameWithGender(Some("female")).ends_with("sdóttir"));
    }
    assert!(iceland.middleName().is_ok());

    let finland = fake(Some("fi-FI")).unwrap();
    assert!(["Hra.", "Tri."].contains(&finland.titleMale()));
    assert!(["Rva.", "Nti.", "Tri."].contains(&finland.titleFemale()));
    assert!(finland.middleName().is_err());
    assert!(finland.lastNameMale().is_err());
}

#[test]
fn polish_and_lithuanian_names_follow_gendered_surname_templates() {
    let lithuania = fake(Some("lt-LT")).unwrap();
    let names = lithuania.unique(true);
    let male: std::collections::HashSet<_> =
        (0..14).map(|_| names.lastNameMale().unwrap()).collect();
    let female: std::collections::HashSet<_> =
        (0..20).map(|_| names.lastNameFemale().unwrap()).collect();
    assert!(male.contains("Kazlauskas"));
    assert!(female.contains("Kazlauskienė"));
    for _ in 0..64 {
        assert!(male.contains(&lithuania.lastNameWithGender(Some("male"))));
        assert!(female.contains(&lithuania.lastNameWithGender(Some("female"))));
        let default = lithuania.lastName();
        assert!(!male.contains(&default) || default == "Urbonas");
        let name = lithuania.nameWithGender(Some("female"));
        assert!(female.contains(name.rsplit_once(' ').unwrap().1));
    }
    assert!(["p.", "ponas"].contains(&lithuania.titleMale()));
    let poland = fake(Some("pl-PL")).unwrap();
    assert!(["mgr", "inż.", "dr", "doc."].contains(&poland.titleFemale()));
    for _ in 0..64 {
        let name = poland.nameWithGender(Some("male"));
        assert!((2..=4).contains(&name.split_whitespace().count()));
    }
}

#[test]
fn seven_regional_person_providers_use_their_company_surname_catalogs() {
    use std::collections::HashSet;

    for (locale, count, marker) in [
        ("en-GB", 99, "Adams"),
        ("en-ZA", 493, "Naidoo"),
        ("el-CY", 130, "Άκη"),
        ("fr-BE", 218, "Adam"),
        ("fr-CA", 130, "Allard"),
        ("fr-CH", 200, "Aebischer"),
        ("it-CH", 200, "Agustoni"),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..count).map(|_| unique.lastName().unwrap()).collect();
        assert!(actual.contains(marker));
        assert!(faker.uniqueWithRetries(false, 1).lastName().is_err());

        for _ in 0..32 {
            for name in [
                faker.name(),
                faker.nameWithGender(Some("female")),
                faker.company(),
            ] {
                assert!(
                    actual.iter().any(|surname| name.contains(surname)),
                    "{locale}: {name}"
                );
            }
        }
    }

    for (locale, titles) in [
        ("el-CY", &["δις.", "δνις.", "κα."][..]),
        ("fr-BE", &["Mme.", "Mlle", "Dr.", "Pr.", "Me."][..]),
        (
            "en-ZA",
            &["Mrs.", "Ms.", "Miss", "Dr.", "Prof.", "Rev.", "Hon."][..],
        ),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let actual: HashSet<_> = (0..titles.len())
            .map(|_| unique.titleFemale().unwrap())
            .collect();
        assert_eq!(actual, titles.iter().copied().collect());
    }
}

#[test]
fn spanish_person_providers_share_surnames_and_keep_local_templates() {
    use std::collections::HashSet;

    let venezuelan = fake(Some("es-VE")).unwrap();
    let mut lengths = HashSet::new();
    for _ in 0..256 {
        lengths.insert(
            venezuelan
                .nameWithGender(Some("male"))
                .split_whitespace()
                .count(),
        );
    }
    assert!(lengths.contains(&2) && lengths.contains(&4));
    for (locale, male, female) in [
        ("es-AR", "Dn.", "Srita."),
        ("es-ES", "D.", "Dña"),
        ("es-PE", "Dn.", "Dra."),
        ("es-VE", "Lcdo.", "Lcda."),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let male_titles: HashSet<_> = (0..5).map(|_| unique.titleMale().unwrap()).collect();
        let female_titles: HashSet<_> = (0..if locale == "es-ES" { 6 } else { 5 })
            .map(|_| unique.titleFemale().unwrap())
            .collect();
        assert!(male_titles.contains(male));
        assert!(female_titles.contains(female));
        let surname = unique.lastName().unwrap();
        assert!(!surname.is_empty());
        assert_ne!(surname, unique.lastName().unwrap());
        assert!(!faker.firstName().is_empty());
    }
}
