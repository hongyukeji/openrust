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
        lengths.insert(denmark.name().split_whitespace().count());
    }
    assert_eq!(lengths, [2, 3].into_iter().collect());
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

#[test]
fn central_european_person_names_keep_local_surnames_and_married_forms() {
    use std::collections::HashSet;

    for (locale, male_count, female_count, male, female) in [
        ("cs-CZ", 990, 984, "Adam", "Adamová"),
        ("sk-SK", 263, 89, "Sloboda", "Slobodová"),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let males: HashSet<_> = (0..male_count)
            .map(|_| unique.lastNameMale().unwrap())
            .collect();
        let females: HashSet<_> = (0..female_count)
            .map(|_| unique.lastNameFemale().unwrap())
            .collect();
        assert!(males.contains(male));
        assert!(females.contains(female));
        for _ in 0..32 {
            assert!(females.contains(&faker.lastNameWithGender(Some("female"))));
            let full = faker.nameWithGender(Some("female"));
            assert!(females.iter().any(|surname| full.contains(surname)));
        }
        assert_ne!(faker.titleMale(), "Mr.");
    }

    let hungary = fake(Some("hu-HU")).unwrap();
    let unique = hungary.unique(true);
    assert_ne!(
        unique.firstNameMaleNe().unwrap(),
        unique.firstNameMaleNe().unwrap()
    );
    assert_ne!(
        unique.lastNameFemaleMarried().unwrap(),
        unique.lastNameFemaleMarried().unwrap()
    );
    assert!(fake(Some("de-AT")).unwrap().firstNameMaleNe().is_err());
}

#[test]
fn greek_bulgarian_and_slovenian_names_use_their_person_providers() {
    use std::collections::HashSet;

    for (locale, male_count, female_count) in [("el-GR", 137, 70), ("bg-BG", 121, 44)] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let males: HashSet<_> = (0..male_count)
            .map(|_| unique.lastNameMale().unwrap())
            .collect();
        let females: HashSet<_> = (0..female_count)
            .map(|_| unique.lastNameFemale().unwrap())
            .collect();
        assert!(unique.lastNameMale().is_err());
        assert!(unique.lastNameFemale().is_err());
        for _ in 0..32 {
            let surname = faker.lastName();
            assert!(males.contains(&surname) || females.contains(&surname));
            assert!(females.contains(&faker.lastNameWithGender(Some("female"))));
            let name = faker.nameWithGender(Some("male"));
            assert!(males.iter().any(|surname| name.contains(surname)));
        }
    }

    let slovenia = fake(Some("sl-SI")).unwrap();
    let unique = slovenia.unique(true);
    let surnames: HashSet<_> = (0..200).map(|_| unique.lastName().unwrap()).collect();
    assert!(surnames.contains("Novak"));
    assert!(surnames.contains(&slovenia.lastNameMale().unwrap()));
    assert!(surnames.contains(&slovenia.lastNameFemale().unwrap()));
    let titles: HashSet<_> = (0..11)
        .map(|_| unique.title(Some("male")).unwrap())
        .collect();
    assert!(titles.contains("univ. dipl. inž."));
    assert!(titles.contains(slovenia.title(Some("female"))));
    assert!(["Mr.", "Dr.", "Prof."].contains(&slovenia.titleMale()));
}

#[test]
fn west_asian_and_egyptian_person_names_use_the_pinned_catalogs() {
    use std::collections::HashSet;

    for (locale, count, marker, titles) in [
        (
            "ar-EG",
            57,
            "عبد الرحيم",
            &["السيدة", "الآنسة", "الدكتورة", "المهندسة"][..],
        ),
        (
            "ar-JO",
            88,
            "آلهامي",
            &["السيدة", "الآنسة", "الدكتورة", "المهندسة"][..],
        ),
        ("he-IL", 101, "אויערבך", &["גב.", "ד\"ר", "פרופ"][..]),
        ("hy-AM", 22, "Կարապետյան", &["տիկին", "օրիորդ"][..]),
        ("ka-GE", 201, "აბაშიძე", &["ქნ.", "დოქ.", "პროფ."][..]),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let surnames: HashSet<_> = (0..count).map(|_| unique.lastName().unwrap()).collect();
        assert!(surnames.contains(marker));
        assert!(unique.lastName().is_err());
        let actual_titles: HashSet<_> = (0..titles.len())
            .map(|_| unique.titleFemale().unwrap())
            .collect();
        assert_eq!(actual_titles, titles.iter().copied().collect());
        let mut egyptian_prefix = false;
        for _ in 0..128 {
            for name in [faker.name(), faker.nameWithGender(Some("female"))] {
                assert!(
                    surnames.iter().any(|surname| name.ends_with(surname)),
                    "{locale}: {name}"
                );
                egyptian_prefix |= ["أ.", "د.", "أ.د", "م."].iter().any(|prefix| {
                    name.strip_prefix(prefix)
                        .is_some_and(|rest| rest.starts_with(' '))
                });
            }
        }
        if locale == "ar-EG" {
            assert!(egyptian_prefix);
        }
    }
}

#[test]
fn russian_ukrainian_and_kazakh_person_names_keep_local_templates() {
    use std::collections::HashSet;

    for (locale, count) in [("ru-RU", 13), ("uk-UA", 20)] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let males: HashSet<_> = (0..count)
            .map(|_| unique.middleNameMale().unwrap())
            .collect();
        let females: HashSet<_> = (0..count)
            .map(|_| unique.middleNameFemale().unwrap())
            .collect();
        assert!(unique.middleNameMale().is_err());
        assert!(unique.middleNameFemale().is_err());
        assert!(females.contains(faker.middleNameWithGender(Some("female")).unwrap()));
        let default = faker.middleName().unwrap();
        assert!(males.contains(default) || females.contains(default));
        let grouped: HashSet<_> = (0..count)
            .map(|_| unique.middleNameWithGender(Some("female")).unwrap())
            .collect();
        assert_eq!(grouped, females);
        assert!(unique.middleNameWithGender(Some("female")).is_err());
        let mut patronymic_seen = false;
        for _ in 0..128 {
            let name = faker.nameWithGender(Some("female"));
            patronymic_seen |= females
                .iter()
                .any(|middle| name.split_whitespace().any(|part| part == *middle));
        }
        assert!(patronymic_seen);
    }

    let russian = fake(Some("ru-RU")).unwrap();
    for _ in 0..64 {
        assert!(russian.lastNameWithGender(Some("female")).ends_with('а'));
    }
    let kazakh = fake(Some("kk-KZ")).unwrap();
    assert!(kazakh.middleNameMale().is_err());
    assert_eq!(kazakh.name().split_whitespace().count(), 2);
}

#[test]
fn south_and_southeast_asian_and_korean_person_names_follow_local_templates() {
    let bengali = fake(Some("bn-BD")).unwrap();
    let indonesian = fake(Some("id-ID")).unwrap();
    let malaysian = fake(Some("ms-MY")).unwrap();
    let korean = fake(Some("ko-KR")).unwrap();
    let thai = fake(Some("th-TH")).unwrap();
    let vietnamese = fake(Some("vi-VN")).unwrap();
    let mut unique_surnames = std::collections::HashSet::new();
    let mut regional_malaysian_name_seen = false;

    for _ in 0..64 {
        assert!(
            ["খান", "শেখ", "শিকদার", "আলী", "তাসনীম", "তাবাসসুম"].contains(&bengali.lastName().as_str())
        );
        assert_eq!(bengali.titleMale(), "মি.");
        assert!((2..=3).contains(&bengali.name().split_whitespace().count()));

        let surname = indonesian.unique(false).lastNameFemale().unwrap();
        assert!(unique_surnames.insert(surname));
        assert_ne!(indonesian.lastNameWithGender(Some("male")), "Doe");
        assert!(["dr.", "drg.", "Dr.", "Hj."].contains(&indonesian.titleFemale()));

        assert_eq!(malaysian.firstNameWithGender(Some("male")), "John");
        assert_eq!(malaysian.firstNameWithGender(Some("female")), "Jane");
        regional_malaysian_name_seen |= !["John", "Jane"].contains(&malaysian.firstName().as_str());
        assert_ne!(malaysian.lastName(), "Doe");
        assert!(
            [
                "En.",
                "Dr.",
                "Prof.",
                "Datuk",
                "Dato'",
                "Datuk Seri",
                "Dato' Sri",
                "Tan Sri",
                "Tun"
            ]
            .contains(&malaysian.titleMale())
        );

        let name = korean.name();
        assert!(name.chars().all(|c| ('가'..='힣').contains(&c)));
        assert!(name.chars().count() >= 2);
        assert!(
            korean
                .firstNameFemale()
                .chars()
                .all(|c| ('가'..='힣').contains(&c))
        );

        assert!(
            thai.lastName()
                .chars()
                .all(|c| c == ' ' || ('\u{0e00}'..='\u{0e7f}').contains(&c))
        );
        assert!(
            thai.firstName()
                .chars()
                .all(|c| ('\u{0e00}'..='\u{0e7f}').contains(&c))
        );
        assert!(["Cụ", "Bà", "Bác", "Cô", "Chị", "Em"].contains(&vietnamese.titleFemale()));
        assert!(!vietnamese.middleName().unwrap().is_empty());
        assert!(
            !vietnamese
                .middleNameWithGender(Some("female"))
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(unique_surnames.len(), 64);
    assert!(regional_malaysian_name_seen);
}

#[test]
fn malaysian_person_formatters_preserve_groups_separators_and_unique_history() {
    let malaysia = fake(Some("ms-MY")).unwrap();
    let other = fake(Some("en-US")).unwrap();
    assert!(other.firstNameMaleMalay().is_err());
    assert!(other.unique(false).myKadNumber(None, false, None).is_err());
    let unique = malaysia.unique(true);
    let mut initials = std::collections::HashSet::new();
    let mut cards = std::collections::HashSet::new();
    for _ in 0..24 {
        assert!(initials.insert(unique.initialIndian().unwrap()));
        assert!(!malaysia.firstNameMaleMalay().unwrap().is_empty());
        assert_eq!(
            malaysia
                .firstNameFemaleChinese()
                .unwrap()
                .split(' ')
                .count(),
            2
        );
        assert!(["", "Haji ", "Hj "].contains(&malaysia.haji().unwrap().as_str()));
        let compact = unique.myKadNumber(Some("male"), false, Some("/")).unwrap();
        assert_eq!(compact.len(), 12);
        assert!(compact.bytes().all(|b| b.is_ascii_digit()));
        assert!((1..=12).contains(&compact[2..4].parse::<u8>().unwrap()));
        assert!((1..=31).contains(&compact[4..6].parse::<u8>().unwrap()));
        let place = compact[6..8].parse::<u8>().unwrap();
        assert!((1..=59).contains(&place) && !(17..=20).contains(&place));
        assert!(matches!(
            compact.as_bytes().last(),
            Some(b'1' | b'3' | b'5' | b'7' | b'9')
        ));
        assert!(cards.insert(compact));
        let custom = malaysia
            .myKadNumber(Some("female"), true, Some("／"))
            .unwrap();
        assert_eq!(
            custom.split('／').map(str::len).collect::<Vec<_>>(),
            [6, 2, 4]
        );
        assert!(matches!(
            custom.as_bytes().last(),
            Some(b'0' | b'2' | b'4' | b'6' | b'8')
        ));
        assert_eq!(
            malaysia
                .myKadNumber(None, true, None)
                .unwrap()
                .matches('-')
                .count(),
            2
        );
    }
}

#[test]
fn romanian_and_serbian_faker_names_preserve_person_catalogs() {
    for locale in ["ro-RO", "ro-MD"] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..32 {
            assert!(["dl.", "ing.", "dr."].contains(&faker.titleMale()));
            assert!(["d-na.", "d-șoara", "ing.", "dr."].contains(&faker.titleFemale()));
            assert_ne!(faker.firstNameMale(), "John");
            assert_ne!(faker.lastName(), "Doe");
        }
    }

    for (locale, cyrillic) in [("sr-RS", true), ("sr-Cyrl-RS", true), ("sr-Latn-RS", false)] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..32 {
            let name = faker.name();
            assert_eq!(
                name.chars().any(|c| ('\u{0400}'..='\u{04ff}').contains(&c)),
                cyrillic
            );
        }
    }
}

#[test]
fn romanian_cnp_keeps_the_pinned_date_county_and_checksum_contract() {
    let faker = fake(Some("ro-RO")).unwrap();
    let unique = faker.unique(true);
    let mut cards = std::collections::HashSet::new();
    for _ in 0..16 {
        for (gender, date, resident, prefix) in [
            ("male", "1804-02-29", true, "304022952"),
            ("female", "1981-06-16", true, "281061652"),
            ("male", "2000-01-01", true, "500010152"),
            ("female", "2085-03-01", false, "985030152"),
        ] {
            let card = unique
                .cnp(Some(gender), Some(date), Some("GR"), resident)
                .unwrap();
            assert_eq!(card.len(), 13);
            assert!(card.starts_with(prefix));
            assert_ne!(card.as_bytes()[11], b'0');
            let digits = card
                .as_bytes()
                .iter()
                .map(|&b| u32::from(b - b'0'))
                .collect::<Vec<_>>();
            let sum = digits
                .iter()
                .zip([2, 7, 9, 1, 4, 6, 3, 5, 8, 2, 7, 9])
                .map(|(digit, weight)| digit * weight)
                .sum::<u32>()
                % 11;
            assert_eq!(digits[12], if sum == 10 { 1 } else { sum });
            assert!(cards.insert(card));
        }
        assert!(
            faker
                .cnp(Some("female"), Some("2085-03"), Some("B1"), true)
                .unwrap()
                .starts_with("68503")
        );
        assert_eq!(faker.cnp(None, Some("2000"), None, true).unwrap().len(), 13);
    }
    assert!(faker.cnp(Some("other"), None, None, true).is_err());
    assert!(faker.cnp(None, None, Some("gr"), true).is_err());
    for invalid in ["1799", "2100", "1981-13", "1900-02-29", "2000-01-01-01"] {
        assert!(faker.cnp(None, Some(invalid), None, true).is_err());
    }
    assert!(
        fake(Some("ro-MD"))
            .unwrap()
            .cnp(None, None, None, true)
            .is_err()
    );
}

#[test]
fn estonian_latvian_and_montenegrin_person_use_local_names() {
    for locale in ["et-EE", "lv-LV", "me-ME"] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..64 {
            assert!(!faker.name().is_empty());
            assert_ne!(faker.lastName(), "Doe");
            assert!(["Mr.", "Dr.", "Prof."].contains(&faker.titleMale()));
        }
    }
}

#[test]
fn latvian_driver_licence_uses_the_pinned_mask_and_unique_history() {
    let faker = fake(Some("lv-LV")).unwrap();
    let unique = faker.unique(true);
    let mut licences = std::collections::HashSet::new();
    for _ in 0..64 {
        let licence = unique.driverLicence().unwrap();
        let bytes = licence.as_bytes();
        assert_eq!(bytes.len(), 8);
        assert!(bytes[..2].iter().all(u8::is_ascii_lowercase));
        assert!(bytes[2..].iter().all(u8::is_ascii_digit));
        assert!(licences.insert(licence));
    }
    assert!(fake(Some("et-EE")).unwrap().driverLicence().is_err());
}

#[test]
fn indian_and_nepali_names_use_regional_middle_name_catalogs() {
    let indian = fake(Some("en-IN")).unwrap();
    let nepali = fake(Some("ne-NP")).unwrap();

    assert!(indian.middleName().is_err());
    assert!(indian.middleNameFemale().is_err());
    assert!(nepali.middleName().is_err());

    for (faker, gender, expected) in [
        (
            &indian,
            "male",
            &[
                "Dev", "Chandra", "Kumar", "Lal", "Prasad", "Raj", "Singh", "Rao", "Ram", "Pratap",
                "Bhai",
            ][..],
        ),
        (
            &nepali,
            "male",
            &[
                "Bahadur", "Dev", "Kumar", "Man", "Mani", "Nath", "Prasad", "Raj", "Ratna",
            ][..],
        ),
        (&nepali, "female", &["Devi", "Kumari"][..]),
    ] {
        let unique = faker.unique(true);
        let mut names = std::collections::HashSet::new();
        for _ in expected {
            let name = unique.middleNameWithGender(Some(gender)).unwrap();
            assert!(expected.contains(&name));
            assert!(names.insert(name));
        }
        assert!(unique.middleNameWithGender(Some(gender)).is_err());
    }
}

#[test]
fn ugandan_surnames_select_the_general_or_explicit_gender_catalog() {
    let faker = fake(Some("en-UG")).unwrap();
    let unique = faker.unique(true);
    let general: std::collections::HashSet<_> =
        (0..123).map(|_| unique.lastName().unwrap()).collect();
    assert_eq!(general.len(), 123);
    assert!(general.contains("Abayisenga"));
    assert!(!general.contains("Mubiru"));
    assert!(!general.contains("Abol"));
    assert!(unique.lastName().is_err());

    let male: std::collections::HashSet<_> =
        (0..41).map(|_| unique.lastNameMale().unwrap()).collect();
    assert_eq!(male.len(), 41);
    assert!(male.contains("Mubiru"));
    assert!(unique.lastNameMale().is_err());

    let female: std::collections::HashSet<_> =
        (0..106).map(|_| unique.lastNameFemale().unwrap()).collect();
    assert_eq!(female.len(), 106);
    assert!(female.contains("Abol"));
    assert!(unique.lastNameFemale().is_err());

    assert!(male.contains(&faker.lastNameWithGender(Some("male"))));
    assert!(female.contains(&faker.lastNameWithGender(Some("female"))));
    assert!(general.contains(&faker.lastNameWithGender(None)));
    assert!(fake(Some("en-NG")).unwrap().lastNameFemale().is_err());
}

#[test]
fn mongolian_person_keeps_initials_distinct_from_the_identity_alphabet() {
    let faker = fake(Some("mn-MN")).unwrap();
    let unique = faker.unique(true);
    let initials: std::collections::HashSet<_> =
        (0..30).map(|_| unique.namePrefix().unwrap()).collect();
    assert_eq!(initials.len(), 30);
    assert!(unique.namePrefix().is_err());
    let alphabet: std::collections::HashSet<_> =
        (0..35).map(|_| unique.alphabet().unwrap()).collect();
    assert_eq!(alphabet.len(), 35);
    assert!(initials.is_subset(&alphabet));

    for gender in [None, Some("male"), Some("female")] {
        for _ in 0..32 {
            let name = faker.nameWithGender(gender);
            let (prefix, given) = name.split_once('.').unwrap();
            assert_eq!(prefix.chars().count(), 1);
            assert!(initials.contains(&prefix.chars().next().unwrap()));
            assert!(!given.is_empty());
        }
    }
    assert_eq!(faker.lastName(), "Doe");
    assert!(fake(Some("en-US")).unwrap().namePrefix().is_err());
}

#[test]
fn singapore_person_keeps_base_names_and_identity_boundaries() {
    use chrono::NaiveDate;

    let faker = fake(Some("en-SG")).unwrap();
    assert_eq!(faker.nameWithGender(Some("male")), "John Doe");
    assert_eq!(faker.nameWithGender(Some("female")), "Jane Doe");
    assert_eq!(faker.lastName(), "Doe");
    let unique = faker.unique(true);
    let mut numbers = std::collections::HashSet::new();

    for (year, month, day, foreigner, prefix, year_digits) in [
        (1967, 12, 31, false, b'S', None),
        (1968, 1, 1, false, b'S', Some("68")),
        (1999, 12, 31, false, b'S', Some("99")),
        (2000, 1, 1, false, b'T', Some("00")),
        (1999, 12, 31, true, b'F', None),
        (2000, 1, 1, true, b'G', None),
    ] {
        let date = NaiveDate::from_ymd_opt(year, month, day);
        for _ in 0..16 {
            let number = if foreigner {
                unique.fin(date)
            } else {
                unique.nric(date)
            }
            .unwrap();
            let bytes = number.as_bytes();
            assert_eq!(bytes.len(), 9);
            assert_eq!(bytes[0], prefix);
            assert!(bytes[1..8].iter().all(u8::is_ascii_digit));
            if let Some(year_digits) = year_digits {
                assert_eq!(&number[1..3], year_digits);
            } else if !foreigner {
                assert!(["00", "01"].contains(&&number[1..3]));
            }
            let total: usize = bytes[1..8]
                .iter()
                .zip([2, 7, 6, 5, 4, 3, 2])
                .map(|(digit, weight)| usize::from(digit - b'0') * weight)
                .sum();
            let offset = if matches!(prefix, b'T' | b'G') { 4 } else { 0 };
            let table = if foreigner {
                b"XWUTRQPNMLK"
            } else {
                b"JZIHGFEDCBA"
            };
            assert_eq!(bytes[8], table[(total + offset) % 11]);
            assert!(numbers.insert(number));
        }
    }
    assert_eq!(unique.singaporeId(None, false).unwrap().len(), 9);
    assert!(fake(Some("en-US")).unwrap().nric(None).is_err());
}

#[test]
fn belgian_dutch_person_uses_pinned_names_and_rrn_rules() {
    let faker = fake(Some("nl-BE")).unwrap();
    let unique = faker.unique(true);
    let mut numbers = std::collections::HashSet::new();
    for gender in [None, Some("male"), Some("female")] {
        for _ in 0..64 {
            assert_ne!(faker.lastName(), "Doe");
            let rrn = unique.rrn(gender).unwrap();
            assert_eq!(rrn.len(), 11);
            assert!(rrn.bytes().all(|byte| byte.is_ascii_digit()));
            let serial = rrn[6..9].parse::<u64>().unwrap();
            match gender {
                Some("male") => assert!((1..=997).contains(&serial) && serial % 2 == 1),
                Some("female") => assert!((2..=998).contains(&serial) && serial % 2 == 0),
                _ => assert!((1..=997).contains(&serial)),
            }
            let body = rrn[..9].parse::<u64>().unwrap();
            let checksum = rrn[9..].parse::<u64>().unwrap();
            assert!(
                [0, 2_000_000_000]
                    .into_iter()
                    .any(|century| checksum == 97 - (century + body) % 97)
            );
            assert!(numbers.insert(rrn));
        }
    }
    assert!(fake(Some("nl-NL")).unwrap().rrn(None).is_err());
}

#[test]
fn person_affixes_support_inherited_values_null_and_empty_strings() {
    let english = fake(None).unwrap();
    let unique = english.unique(true);
    let expected = [
        "Jr.", "Sr.", "I", "II", "III", "IV", "V", "MD", "DDS", "PhD", "DVM",
    ];
    let mut suffixes = std::collections::HashSet::new();
    for _ in expected {
        let suffix = unique.suffix().unwrap().unwrap();
        assert!(expected.contains(&suffix));
        assert!(suffixes.insert(suffix));
    }
    assert!(unique.suffix().is_err());
    assert!(english.prefix().is_err());

    for locale in ["fr-FR", "fr-CH"] {
        let faker = fake(Some(locale)).unwrap();
        assert!(["de", "de la", "Le", "du"].contains(&faker.prefix().unwrap()));
    }
    assert!(fake(Some("fr-BE")).unwrap().prefix().is_err());

    for (locale, value) in [
        ("it-IT", None),
        ("it-CH", None),
        ("hy-AM", Some("")),
        ("zh-TW", Some("")),
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        assert_eq!(unique.suffix().unwrap(), value);
        assert!(unique.suffix().is_err());
    }
    assert!(fake(Some("en-SG")).unwrap().suffix().is_err());
}

#[test]
fn japanese_kana_names_select_gender_and_exhaust_the_pinned_catalogs() {
    let faker = fake(Some("ja-JP")).unwrap();
    let unique = faker.unique(true);
    let male: std::collections::HashSet<_> = (0..30)
        .map(|_| unique.firstKanaNameMale().unwrap())
        .collect();
    let female: std::collections::HashSet<_> = (0..21)
        .map(|_| unique.firstKanaNameFemale().unwrap())
        .collect();
    let surnames: std::collections::HashSet<_> =
        (0..49).map(|_| unique.lastKanaName().unwrap()).collect();
    assert_eq!((male.len(), female.len(), surnames.len()), (30, 21, 49));
    assert!(unique.firstKanaNameMale().is_err());
    assert!(unique.firstKanaNameFemale().is_err());
    assert!(unique.lastKanaName().is_err());
    for (gender, names) in [("male", &male), ("female", &female)] {
        for _ in 0..32 {
            let full = faker.kanaName(Some(gender)).unwrap();
            let (last, first) = full.split_once(' ').unwrap();
            assert!(surnames.contains(last));
            assert!(names.contains(first));
            assert!(names.contains(faker.firstKanaName(Some(gender)).unwrap()));
        }
    }
    assert!(fake(Some("en-US")).unwrap().kanaName(None).is_err());
}

#[test]
fn swiss_identity_numbers_use_dotted_ean_and_locale_aliases() {
    let mut numbers = std::collections::HashSet::new();
    for locale in ["de-CH", "fr-CH", "it-CH"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..64 {
            let number = unique.avs13().unwrap();
            let parts: Vec<_> = number.split('.').collect();
            assert_eq!(
                parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
                [3, 4, 4, 2]
            );
            assert_eq!(parts[0], "756");
            let digits = parts.concat();
            assert!(digits.bytes().all(|byte| byte.is_ascii_digit()));
            let total: u32 = digits
                .bytes()
                .rev()
                .enumerate()
                .map(|(index, digit)| u32::from(digit - b'0') * if index % 2 == 0 { 1 } else { 3 })
                .sum();
            assert_eq!(total % 10, 0);
            assert!(numbers.insert((locale, number)));
        }
        if locale == "de-CH" {
            assert!(unique.ahv13().unwrap().starts_with("756."));
        } else {
            assert!(unique.ahv13().is_err());
        }
    }
    let unrelated = fake(Some("de-DE")).unwrap();
    assert!(unrelated.avs13().is_err());
    assert!(unrelated.ahv13().is_err());
}

#[test]
fn czech_birth_numbers_keep_era_offsets_and_optional_slashes() {
    use chrono::Datelike;

    let faker = fake(Some("cs-CZ")).unwrap();
    let unique = faker.unique(true);
    let now = chrono::Utc::now().date_naive();
    for (year, female, slash) in [(1953, false, 0), (1954, true, 100), (2004, true, 100)] {
        let age = u32::try_from(now.year() - year).unwrap();
        let mut unique_numbers = std::collections::HashSet::new();
        for _ in 0..32 {
            let gender = if female { "female" } else { "male" };
            let number = unique.birthNumber(Some(gender), age, age, slash).unwrap();
            assert_eq!(number.as_bytes().get(6) == Some(&b'/'), slash == 100);
            let digits = number.replace('/', "");
            assert_eq!(digits.len(), if year < 1954 { 9 } else { 10 });
            assert_eq!(digits[..2].parse::<i32>().unwrap(), year % 100);
            let month = digits[2..4].parse::<u32>().unwrap() - if female { 50 } else { 0 };
            assert!(
                month == now.month()
                    || (year >= 2004 && month == now.month() + 20)
                    || (now.month() == 2 && now.day() == 29 && month == 3)
            );
            if year >= 1954 {
                let remainder = digits[..9].parse::<u32>().unwrap() % 11;
                assert_eq!(u32::from(digits.as_bytes()[9] - b'0'), remainder % 10);
            }
            assert!(unique_numbers.insert(number));
        }
    }
    for (number, range) in [
        (faker.birthNumberMale().unwrap(), 1..=32),
        (faker.birthNumberFemale().unwrap(), 51..=82),
    ] {
        assert!(range.contains(&number[2..4].parse::<u32>().unwrap()));
    }
    assert!(faker.birthNumber(None, 21, 20, 50).is_err());
    assert!(faker.birthNumber(None, 0, u32::MAX, 50).is_err());
}

#[test]
fn danish_cpr_preserves_the_date_and_nonzero_serial_prefix() {
    let faker = fake(Some("da-DK")).unwrap();
    let unique = faker.unique(true);
    let mut numbers = std::collections::HashSet::new();
    for _ in 0..64 {
        let number = unique.cpr().unwrap();
        let (date, serial) = number.split_once('-').unwrap();
        assert_eq!((date.len(), serial.len()), (6, 4));
        assert!(
            date.bytes()
                .chain(serial.bytes())
                .all(|byte| byte.is_ascii_digit())
        );
        assert_ne!(serial.as_bytes()[0], b'0');
        assert!(numbers.insert(number));
    }
    assert!(faker.birthNumberMale().is_err());
    assert!(fake(Some("cs-CZ")).unwrap().cpr().is_err());
}

#[test]
fn french_nir_and_italian_tax_id_preserve_the_pinned_patterns() {
    let mask = regex::Regex::new(r"^[A-Z]{6}[0-9]{2}[A-Z][0-9]{2}[A-Z][0-9]{3}[A-Z]$").unwrap();
    for locale in ["it-IT", "it-CH"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let mut values = std::collections::HashSet::new();
        for _ in 0..32 {
            let value = unique.taxId().unwrap();
            assert!(mask.is_match(&value));
            assert!(values.insert(value));
        }
        assert!(faker.nir(None, false).is_err());
    }

    for locale in ["fr-FR", "fr-CH"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let mut values = std::collections::HashSet::new();
        for gender in [None, Some("male"), Some("female")] {
            for formatted in [false, true] {
                let value = unique.nir(gender, formatted).unwrap();
                if formatted {
                    assert_eq!(
                        value.split(' ').map(str::len).collect::<Vec<_>>(),
                        [1, 2, 2, 2, 3, 3, 2]
                    );
                }
                let compact = value.replace(' ', "");
                assert_eq!(compact.len(), 15);
                match gender {
                    Some("male") => assert!(compact.starts_with('1')),
                    Some("female") => assert!(compact.starts_with('2')),
                    _ => assert!(compact.starts_with(['1', '2'])),
                }
                assert!((1..=12).contains(&compact[3..5].parse::<u8>().unwrap()));
                let numeric = compact[..13].replace("2A", "19").replace("2B", "18");
                let check = 97 - numeric.parse::<u64>().unwrap() % 97;
                assert_eq!(compact[13..].parse::<u64>().unwrap(), check);
                assert!(values.insert(value));
            }
        }
        assert!(faker.taxId().is_err());
    }
}

#[test]
fn iranian_national_code_and_indonesian_nik_keep_their_pinned_parts() {
    let iran = fake(Some("fa-IR")).unwrap();
    let indonesia = fake(Some("id-ID")).unwrap();
    let iran_unique = iran.unique(true);
    let indonesia_unique = indonesia.unique(true);
    let date = chrono::NaiveDate::from_ymd_opt(2000, 2, 29);
    let mut codes = std::collections::HashSet::new();
    let mut nik_values = std::collections::HashSet::new();
    for _ in 0..64 {
        let code = iran_unique.nationalCode().unwrap();
        assert_eq!(code.len(), 10);
        assert!(code.bytes().all(|digit| digit.is_ascii_digit()));
        assert_ne!(&code[..3], "000");
        let sum: u32 = code
            .bytes()
            .zip((2..=10).rev())
            .map(|(digit, weight)| u32::from(digit - b'0') * weight)
            .sum();
        let remainder = sum % 11;
        let check = if remainder < 2 {
            remainder
        } else {
            11 - remainder
        };
        assert_eq!(u32::from(code.as_bytes()[9] - b'0'), check);
        assert!(codes.insert(code));

        for (gender, digits) in [("male", "290200"), ("female", "690200")] {
            let nik = indonesia_unique.nik(Some(gender), date).unwrap();
            assert_eq!(nik.len(), 16);
            assert!(nik.bytes().all(|digit| digit.is_ascii_digit()));
            assert_eq!(&nik[6..12], digits);
            assert!(nik_values.insert(nik));
        }
    }
    assert_eq!(indonesia.nik(None, None).unwrap().len(), 16);
    assert!(iran.nik(None, None).is_err());
    assert!(indonesia.nationalCode().is_err());
}

#[test]
fn kazakh_iin_and_venezuelan_national_id_follow_the_pinned_masks() {
    use openrust::database::eloquent::factories::Faker;

    assert_eq!(Faker::checkSum("00022961021").unwrap(), 1);
    assert_eq!(Faker::checkSum("00022961020").unwrap(), 10);
    assert!(Faker::checkSum("000229610212").is_err());
    assert!(Faker::checkSum("0002296102x").is_err());

    let kazakh = fake(Some("kk-KZ")).unwrap();
    let unique = kazakh.unique(true);
    let venezuelan = fake(Some("es-VE")).unwrap();
    let venezuela_unique = venezuelan.unique(true);
    let mut identifiers = std::collections::HashSet::new();
    for (year, gender, marker) in [
        (1800, Some("male"), "1"),
        (1899, Some("female"), "2"),
        (1900, None, "3"),
        (1999, Some("female"), "4"),
        (2000, Some("male"), "5"),
        (2099, Some("female"), "6"),
    ] {
        let date = chrono::NaiveDate::from_ymd_opt(year, 1, 1);
        for _ in 0..16 {
            let number = unique.individualIdentificationNumber(date, gender).unwrap();
            assert_eq!(number.len(), 12);
            assert_eq!(&number[..7], format!("{:02}0101{marker}", year % 100));
            assert!((1000..=2000).contains(&number[7..11].parse::<u16>().unwrap()));
            assert_eq!(
                Faker::checkSum(&number[..11]).unwrap(),
                number.as_bytes()[11] - b'0'
            );
            assert!(identifiers.insert(number));
            let number = venezuela_unique.nationalId(Some("／")).unwrap();
            let (prefix, serial) = number.split_once('／').unwrap();
            let lower = match prefix {
                "V" => 10_000,
                "E" => 80_000_000,
                _ => panic!("unknown prefix"),
            };
            assert!((lower..=100_000_000).contains(&serial.parse::<u32>().unwrap()));
            assert!(identifiers.insert(number));
        }
    }
    assert_eq!(
        kazakh
            .individualIdentificationNumber(None, None)
            .unwrap()
            .len(),
        12
    );
    assert!(
        kazakh
            .individualIdentificationNumber(chrono::NaiveDate::from_ymd_opt(2100, 1, 1), None)
            .is_err()
    );
    assert!(
        kazakh
            .individualIdentificationNumber(None, Some("invalid"))
            .is_err()
    );
    assert!(
        venezuelan
            .individualIdentificationNumber(None, None)
            .is_err()
    );
    assert!(kazakh.nationalId(None).is_err());
}

#[test]
fn portuguese_nif_and_taiwanese_random_names_follow_the_pinned_helpers() {
    use openrust::database::eloquent::factories::Faker;

    assert_eq!(Faker::dvCalcMod11("").unwrap(), 0);
    assert_eq!(Faker::dvCalcMod11("000000001").unwrap(), 9);
    assert_eq!(Faker::dvCalcMod11("123456789").unwrap(), 7);
    assert!(Faker::dvCalcMod11("١").is_err());
    let portugal = fake(Some("pt-PT")).unwrap();
    let unique = portugal.unique(true);
    for _ in 0..32 {
        let nif = unique.taxpayerIdentificationNumber().unwrap();
        assert_eq!(nif.len(), 9);
        assert!(
            [
                "1", "2", "3", "45", "5", "6", "70", "71", "72", "77", "79", "8", "90", "98", "99"
            ]
            .iter()
            .any(|prefix| nif.starts_with(prefix))
        );
        assert_eq!(
            nif.as_bytes()[8] - b'0',
            Faker::dvCalcMod11(&nif[..8]).unwrap()
        );
    }
    let taiwan = fake(Some("zh-TW")).unwrap();
    assert_eq!(taiwan.randomName(&["明"], 3).unwrap(), "明明明");
    assert_eq!(taiwan.randomName(&["明"], 0).unwrap(), "");
    assert_eq!(taiwan.randomName(&[], 2).unwrap(), "");
    let unique = taiwan.unique(true);
    assert_eq!(unique.randomName(&["明"], 1).unwrap(), "明");
    assert!(unique.randomName(&["明"], 1).is_err());
    assert!(portugal.randomName(&["明"], 1).is_err());
}

#[test]
fn lithuanian_driver_licence_is_numeric_and_supports_unique() {
    let faker = fake(Some("lt-LT")).unwrap();
    let unique = faker.unique(true);
    let mut values = std::collections::HashSet::new();
    for _ in 0..64 {
        let value = unique.driverLicence().unwrap();
        assert_eq!(value.len(), 8);
        assert!(value.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(values.insert(value));
    }
}

#[test]
fn missing_person_provider_uses_us_names_and_optional_formatters() {
    let english = fake(Some("en-US")).unwrap();
    let surnames: std::collections::HashSet<_> = (0..1200)
        .map_while(|_| english.unique(false).lastName().ok())
        .collect();
    assert!(surnames.len() >= 400);

    for locale in ["en-CA", "cy-GB", "en"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..16 {
            assert!(!faker.name().is_empty());
            assert!(!faker.firstName().is_empty());
            assert!(surnames.contains(&faker.lastName()));
            assert_eq!(unique.ssn().unwrap().len(), 11);
        }
        assert!(unique.suffix().unwrap().is_some());
        assert!(faker.prefix().is_err());
        assert!(faker.nino().is_err());
    }

    for locale in ["en-SG", "zh-CN", "en-GB"] {
        let faker = fake(Some(locale)).unwrap();
        assert!(faker.ssn().is_err());
        assert!(faker.suffix().is_err());
    }
}

#[test]
fn venezuelan_rif_preserves_prefix_digits_and_optional_separator() {
    let faker = fake(Some("es-VE")).unwrap();
    let unique = faker.unique(true);
    let mut prefixes = std::collections::HashSet::new();
    let mut samples = std::collections::HashSet::new();
    for _ in 0..160 {
        let compact = unique.taxpayerIdentificationNumber().unwrap();
        assert_eq!(compact.len(), 10);
        assert!(b"JGVEPC".contains(&compact.as_bytes()[0]));
        assert!(compact[1..].bytes().all(|digit| digit.is_ascii_digit()));
        prefixes.insert(compact.as_bytes()[0]);
        assert!(samples.insert(compact));

        let explicit = unique
            .taxpayerIdentificationNumberWithSeparator(None)
            .unwrap();
        assert!(samples.insert(explicit));
        let separated = unique
            .taxpayerIdentificationNumberWithSeparator(Some("／-"))
            .unwrap();
        let fields: Vec<_> = separated.split("／-").collect();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].len(), 1);
        assert!(b"JGVEPC".contains(&fields[0].as_bytes()[0]));
        assert_eq!(fields[1].len(), 8);
        assert_eq!(fields[2].len(), 1);
        assert!(
            fields[1..]
                .iter()
                .flat_map(|part| part.bytes())
                .all(|digit| digit.is_ascii_digit())
        );
        assert!(samples.insert(separated));
    }
    assert_eq!(prefixes.len(), 6);
    for (locale, length) in [("pl-PL", 10), ("pt-PT", 9)] {
        let faker = fake(Some(locale)).unwrap();
        assert_eq!(faker.taxpayerIdentificationNumber().unwrap().len(), length);
        assert!(
            faker
                .taxpayerIdentificationNumberWithSeparator(Some("-"))
                .is_err()
        );
    }
}

#[test]
fn company_phrases_preserve_regional_provider_and_template_rules() {
    let missing = fake(Some("ja-JP")).unwrap();
    assert!(missing.catchPhrase().is_err());
    assert!(missing.bs().is_err());

    let fallback = fake(Some("en-CA")).unwrap();
    let unique = fallback.unique(true);
    let slogans: std::collections::HashSet<_> =
        (0..48).map(|_| unique.catchPhrase().unwrap()).collect();
    assert_eq!(slogans.len(), 48);
    assert!(!fallback.bs().unwrap().is_empty());
    assert!(fallback.catchPhraseVerb().is_err());

    for locale in ["ar-SA", "ar-JO", "hy-AM"] {
        let faker = fake(Some(locale)).unwrap();
        assert_eq!(faker.catchPhrase().unwrap().is_empty(), locale == "hy-AM");
        let unique = faker.uniqueWithRetries(true, 2);
        assert_eq!(unique.bs().unwrap(), "");
        assert!(unique.bs().is_err());
    }

    for locale in ["fr-FR", "fr-CA", "fr-CH", "fr-BE"] {
        let faker = fake(Some(locale)).unwrap();
        assert!(!faker.catchPhraseNoun().unwrap().is_empty());
        assert!(!faker.catchPhraseVerb().unwrap().is_empty());
        assert!(!faker.catchPhraseAttribute().unwrap().is_empty());
        assert!(faker.bs().is_err());
        for _ in 0..128 {
            let phrase = faker.catchPhrase().unwrap();
            assert!(phrase.as_bytes()[0].is_ascii_uppercase());
            for word in ["sécurité", "simpl"] {
                assert!(phrase.matches(word).count() <= 1);
            }
        }
    }

    let czech = fake(Some("cs-CZ")).unwrap();
    assert!(!czech.catchPhraseNoun().unwrap().is_empty());
    assert!(!czech.catchPhraseVerb().unwrap().is_empty());
    assert!(!czech.catchPhraseAttribute().unwrap().is_empty());
    assert!(!czech.catchPhrase().unwrap().is_empty());

    let taiwan = fake(Some("zh-Hant")).unwrap();
    assert!(!taiwan.bs().unwrap().contains(char::is_whitespace));
    let china = fake(Some("zh-Hans")).unwrap();
    assert!(!china.catchPhrase().unwrap().is_empty());
    assert!(china.bs().is_err());
}

#[test]
fn regional_phone_formatters_preserve_options_and_inherited_extensions() {
    for locale in ["en-US", "en-CA", "en"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..48 {
            let number = unique.phoneNumberWithExtension().unwrap();
            let (number, extension) = number.split_once(" x").unwrap();
            assert!((3..=5).contains(&extension.len()));
            assert!(extension.bytes().all(|x| x.is_ascii_digit()));
            assert!((10..=11).contains(&number.bytes().filter(u8::is_ascii_digit).count()));
            let e164 = faker.e164PhoneNumber();
            assert_eq!(e164.len(), 12);
            assert!(e164.starts_with("+1"));
        }
    }
    let malaysia = fake(Some("ms-MY")).unwrap();
    for prefix in [false, true] {
        for formatting in [false, true] {
            for number in [
                malaysia
                    .mobileNumberWithOptions(prefix, formatting)
                    .unwrap(),
                malaysia
                    .fixedLineNumberWithOptions(prefix, formatting)
                    .unwrap(),
            ] {
                let national = match (prefix, formatting) {
                    (true, true) => number.strip_prefix("+6").unwrap(),
                    (true, false) => number.strip_prefix('6').unwrap(),
                    _ => &number,
                };
                assert!(national.starts_with('0'));
                assert_eq!(national.contains('-'), formatting);
                assert_eq!(national.contains(' '), formatting);
            }
        }
    }
    let brazil = fake(Some("pt-BR")).unwrap();
    assert_eq!(brazil.cellphoneNumber(Some(false)).unwrap().len(), 11);
    assert_eq!(brazil.landlineNumber(Some(false)).unwrap().len(), 10);
    let korea = fake(Some("ko-KR")).unwrap();
    let cell = korea.cellPhoneNumber().unwrap();
    assert_eq!(cell.len(), 13);
    assert!(cell.starts_with("010-"));
    let hong_kong = fake(Some("en-HK")).unwrap();
    assert!(matches!(
        hong_kong.landlineNumber(None).unwrap().as_bytes()[0],
        b'2' | b'3'
    ));
    let china = fake(Some("zh-CN")).unwrap();
    assert!(china.mobileNumber().is_err());
    assert!(china.phoneNumberWithExtension().is_err());
    assert!(brazil.mobileNumberWithOptions(false, false).is_err());
}
