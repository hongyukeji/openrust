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
