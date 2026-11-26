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
            let sample = tw.unique(false).personal_identity_number(gender).unwrap();
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
        tw.personal_identity_number(None).unwrap().as_bytes()[1],
        b'1' | b'2'
    ));
    let zh = fake(Some("zh-Hans")).unwrap();
    assert!(zh.ssn().is_err());
    assert!(zh.nino().is_err());
    assert!(zh.personal_identity_number(None).is_err());
}

#[test]
fn regional_ssn_samples_cover_thai_austrian_and_icelandic_providers() {
    use jiff::civil::Date as CivilDate;
    use std::collections::HashSet;

    let th = fake(Some("th-TH")).unwrap();
    let at = fake(Some("de-AT")).unwrap();
    let is = fake(Some("is-IS")).unwrap();
    let fixed = CivilDate::new(2000, 2, 29).unwrap();
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

        let sample = unique.ssn_with_birthdate(Some(fixed)).unwrap();
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
        let current_year = i32::from(
            jiff::Timestamp::now()
                .to_zoned(jiff::tz::TimeZone::UTC)
                .year(),
        );
        let year_suffix = sample[4..6].parse::<i32>().unwrap();
        let century_digit = sample[10..].parse::<i32>().unwrap();
        let year = (current_year - 101..=current_year)
            .find(|&year| (year / 100) % 10 == century_digit && year % 100 == year_suffix)
            .unwrap();
        assert!(CivilDate::strptime("%d%m%Y", format!("{}{year}", &sample[..4])).is_ok());
    }
    let generated = at.ssn().unwrap();
    assert_eq!(generated.len(), 10);
    assert!(th.ssn_with_birthdate(Some(fixed)).is_err());
}

#[test]
fn european_identity_numbers_keep_dates_and_regional_checksums() {
    use jiff::civil::Date as CivilDate;

    let date = CivilDate::new(2000, 2, 29).unwrap();
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
                    .personal_identity_number_with_birthdate(Some(date), Some(gender))
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
        assert!(faker.personal_identity_number(None).is_ok());
    }

    let lt = fake(Some("lt-LT")).unwrap();
    let unique = lt.unique_with_retries(true, 1);
    assert_eq!(
        unique
            .personal_identity_number_with_serial(Some(date), None, Some("0079"))
            .unwrap(),
        "50002290079"
    );
    assert!(
        unique
            .personal_identity_number_with_serial(Some(date), None, Some("007"))
            .is_err()
    );
    assert!(
        lt.personal_identity_number_with_serial(Some(date), None, Some("é12"))
            .is_err()
    );
    let fi = fake(Some("fi-FI")).unwrap();
    let outside = CivilDate::new(2100, 1, 1).unwrap();
    assert!(
        fi.personal_identity_number_with_birthdate(Some(outside), None)
            .is_err()
    );

    let pl = fake(Some("pl-PL")).unwrap();
    let format = Regex::new(r"^A[ABCDEFGHIJKLMNPRSTUVWXYZ]{2}[0-9]{6}$").unwrap();
    for _ in 0..64 {
        let sample = pl.unique(false).personal_identity_number(None).unwrap();
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
    use jiff::civil::Date as CivilDate;

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
        let date = CivilDate::new(year, 2, 29).unwrap();
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
        let invalid = CivilDate::new(year, 1, 1).unwrap();
        assert!(faker.pesel(Some(invalid), None).is_err());
    }
    assert_eq!(faker.pesel(None, None).unwrap().len(), 11);
    for _ in 0..256 {
        let sample = unique.taxpayer_identification_number().unwrap();
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
    assert!(other.taxpayer_identification_number().is_err());
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
        .map(|_| es_unique.licence_code().unwrap())
        .collect::<HashSet<_>>();
    let za_codes = (0..12)
        .map(|_| za_unique.licence_code().unwrap())
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
    assert!(es_unique.licence_code().is_err());
    assert!(za_unique.licence_code().is_err());
    assert!(es.unique(true).licence_code().is_ok());

    assert!(es.ruc(None).is_err());
    assert!(pe.licence_code().is_err());
    assert!(za.dni().is_err());
}

#[test]
fn regional_id_numbers_match_their_pinned_providers() {
    use jiff::civil::Date as CivilDate;

    let nl = fake(Some("nl-NL")).unwrap();
    let za = fake(Some("en-ZA")).unwrap();
    let sa = fake(Some("ar-SA")).unwrap();
    let mn = fake(Some("mn-MN")).unwrap();
    let eg = fake(Some("ar-EG")).unwrap();
    let date = CivilDate::new(2000, 2, 29).unwrap();
    let alphabet = "АЛХБМЦВНЧГОШДӨЩЕПЪЁРЫЖСЬЗТЭИУЮЙҮЯКФ";
    let governorates = [
        "02", "19", "28", "25", "29", "31", "18", "22", "03", "35", "01", "12", "11", "26", "04",
        "13", "34", "16", "23", "14", "27", "15", "33", "17", "24", "32",
    ];
    for _ in 0..128 {
        let dutch = nl.unique(false).id_number().unwrap();
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
            .id_number_with_birthdate(Some(date), Some(false), Some("FEMALE"))
            .unwrap();
        assert_eq!(za_sample.len(), 13);
        assert_eq!(&za_sample[..6], "000229");
        assert!(matches!(za_sample.as_bytes()[6], b'0'..=b'4'));
        assert!(za_sample[10..].starts_with('1'));
        assert!(matches!(za_sample.as_bytes()[11], b'8' | b'9'));
        luhn(&za_sample);

        for (sample, prefixes) in [
            (sa.id_number().unwrap(), "12"),
            (sa.national_id_number().unwrap(), "1"),
            (sa.foreigner_id_number().unwrap(), "2"),
        ] {
            assert_eq!(sample.len(), 10);
            assert!(prefixes.contains(sample.chars().next().unwrap()));
            luhn(&sample);
        }

        let mongolian = mn.id_number().unwrap();
        let mut chars = mongolian.chars();
        assert!(
            alphabet.contains(chars.next().unwrap()) && alphabet.contains(chars.next().unwrap())
        );
        assert_eq!(chars.as_str().len(), 8);
        assert!(chars.as_str().bytes().all(|byte| byte.is_ascii_digit()));

        let egyptian = eg.national_id_number_with_gender(Some("male")).unwrap();
        assert_eq!(egyptian.len(), 14);
        let century = match egyptian.as_bytes()[0] {
            b'2' => "19",
            b'3' => "20",
            other => panic!("{other}"),
        };
        let birth = CivilDate::strptime("%Y%m%d", format!("{century}{}", &egyptian[1..7])).unwrap();
        assert!(
            (CivilDate::new(1950, 1, 10).unwrap()..=CivilDate::new(2005, 12, 25).unwrap())
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
    assert!(nl.id_number_with_birthdate(None, None, None).is_err());
    assert!(eg.foreigner_id_number().is_err());
}

#[test]
fn turkish_identity_and_baltic_passports_follow_pinned_formats() {
    use openrust::database::eloquent::factories::Faker;

    let turkey = fake(Some("tr-TR")).unwrap();
    let lithuania = fake(Some("lt-LT")).unwrap();
    let latvia = fake(Some("lv-LV")).unwrap();
    let format = Regex::new(r"^[a-z]{2}[0-9]{7}$").unwrap();

    assert_eq!(Faker::tc_no_checksum("100000001").unwrap(), "46");
    assert_eq!(Faker::tc_no_checksum("090909090").unwrap(), "-60");
    assert!(Faker::tc_no_is_valid("10000000146").unwrap());
    assert!(!Faker::tc_no_is_valid("10000000147").unwrap());
    assert!(Faker::tc_no_is_valid("090909090-60").is_err());
    assert!(Faker::tc_no_checksum("12345678a").is_err());
    assert!(Faker::tc_no_is_valid("你").is_err());

    for _ in 0..128 {
        let turkish = turkey.unique(false).tc_no().unwrap();
        let (prefix, suffix) = turkish.split_at(9);
        assert_eq!(Faker::tc_no_checksum(prefix).unwrap(), suffix);
        if suffix.len() == 2 {
            assert!(Faker::tc_no_is_valid(&turkish).unwrap());
        }
        let lt = lithuania.unique(false).passport_number().unwrap();
        assert_eq!(lt.len(), 8);
        assert!(lt.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(format.is_match(&latvia.unique(false).passport_number().unwrap()));
    }
    assert!(latvia.tc_no().is_err());
    assert!(turkey.passport_number().is_err());
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
        assert!(Faker::inn10_is_valid(&inn));
        assert_eq!(ru.kpp(Some(&inn)).unwrap(), format!("{}01001", &inn[..4]));
        assert!(inns.insert(inn));
        let area = ru.inn(Some("00")).unwrap()[..2].parse::<u8>().unwrap();
        assert!((1..=91).contains(&area));
    }
    assert_eq!(Faker::inn10_checksum("770708389").unwrap(), "3");
    assert!(Faker::inn10_is_valid("7707083893"));
    assert!(!Faker::inn10_is_valid("7707083894"));
    assert!(!Faker::inn10_is_valid("你0708389"));
    assert!(Faker::inn10_checksum("12345678").is_err());
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
        let local = polish_unique.regon_local().unwrap();
        regon_valid(&local, &[2, 4, 8, 5, 0, 9, 7, 3, 6, 1, 2, 4, 8]);
        regon_valid(&local[..9], &weights);
        assert_eq!((local.as_bytes()[1] - b'0') % 2, 1);
        assert!(local_ids.insert(local));
    }
    assert!(polish.siret(None).is_err());
    assert!(french.regon().is_err());
    assert!(french.regon_local().is_err());
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
        let saudi_id = saudi_unique.company_id_number().unwrap();
        assert!(saudi_id.starts_with("700"));
        for (number, length) in [
            (saudi_id, 10),
            (egypt_unique.company_tax_id_number().unwrap(), 10),
            (egypt_unique.company_trade_register_number().unwrap(), 7),
        ] {
            assert_eq!(number.len(), length);
            assert!(number.bytes().all(|digit| digit.is_ascii_digit()));
            luhn(&number);
        }
        assert!(seen.insert(egypt_unique.company_trade_register_number().unwrap()));
    }

    assert!(egypt.company_id_number().is_err());
    assert!(saudi.company_tax_id_number().is_err());
    assert!(saudi.company_trade_register_number().is_err());
}

#[test]
fn kazakh_and_south_african_company_numbers_use_pinned_dates_and_formats() {
    use jiff::civil::Date as CivilDate;
    let kazakh = fake(Some("kk-KZ")).unwrap();
    let south_african = fake(Some("en-ZA")).unwrap();
    let fixed = CivilDate::new(2000, 2, 29).unwrap();
    let bin = Regex::new(r"^0002[4-6][0-3][0-9]{6}$").unwrap();
    let company =
        Regex::new(r"^[0-9]{4}/[1-9][0-9]{5}/(0[126789]|1[0124567]|2[0-6]|3[01]|80)$").unwrap();
    let year = i32::from(
        jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .year(),
    );
    let mut bins = std::collections::HashSet::new();
    let mut companies = std::collections::HashSet::new();
    let unique_kazakh = kazakh.unique(true);
    let unique_south_african = south_african.unique(true);
    for _ in 0..128 {
        let id = unique_kazakh
            .business_identification_number(Some(fixed))
            .unwrap();
        assert!(bin.is_match(&id));
        assert!(bins.insert(id));
        let id = unique_south_african.company_number().unwrap();
        assert!(company.is_match(&id));
        assert!((year - 50..=year).contains(&id[..4].parse::<i32>().unwrap()));
        assert!(companies.insert(id));
    }
    let sampled = kazakh.business_identification_number(None).unwrap();
    assert_eq!(&sampled[..2], format!("{:02}", year.rem_euclid(100)));
    assert!(
        (1..=jiff::Timestamp::now()
            .to_zoned(jiff::tz::TimeZone::UTC)
            .month() as u32)
            .contains(&sampled[2..4].parse::<u32>().unwrap())
    );
    assert!(kazakh.company_number().is_err());
    assert!(south_african.business_identification_number(None).is_err());
}

#[test]
fn taiwanese_vat_and_greek_business_catalogs_follow_pinned_formats() {
    let taiwan = fake(Some("zh-Hant")).unwrap();
    let greek = fake(Some("el-GR")).unwrap();
    let vat = taiwan.unique(true);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..128 {
        let number = vat.taiwan_vat().unwrap();
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
    assert!(greek.taiwan_vat().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());

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
    assert!(!fallback.company_suffix().is_empty());
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
        unique_japan.company_prefix().unwrap(),
        unique_japan.company_prefix().unwrap(),
    ];
    assert!(prefixes.contains(&"株式会社") && prefixes.contains(&"有限会社"));
    assert!(unique_japan.company_prefix().is_err());
    assert_eq!(japan.company_suffix(), "Ltd");
    let unique_taiwan = taiwan.unique(true);
    let modifiers: std::collections::HashSet<_> = (0..17)
        .map(|_| unique_taiwan.company_modifier().unwrap())
        .collect();
    assert_eq!(modifiers.len(), 17);
    assert!(unique_taiwan.company_modifier().is_err());
    let suffixes: std::collections::HashSet<_> = (0..6)
        .map(|_| unique_taiwan.company_en_suffix().unwrap())
        .collect();
    assert_eq!(
        suffixes,
        ["Inc", "and Sons", "LLC", "Group", "PLC", "Ltd"].into()
    );
    assert!(unique_taiwan.company_en_suffix().is_err());
    assert!(japan.company_modifier().is_err());
    assert!(mainland.company_en_suffix().is_err());
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
            .map(|_| unique.company_prefix().unwrap())
            .collect();
        assert_eq!(prefixes, starts.into());
        assert!(unique.company_prefix().is_err());
        let sampled: HashSet<_> = (0..suffixes.len())
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(sampled, suffixes.iter().map(|s| (*s).to_owned()).collect());
        assert!(unique.company_suffix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|item| (*item).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());

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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
        let sampled: HashSet<_> = (0..prefixes.len())
            .map(|_| unique.company_prefix().unwrap())
            .collect();
        assert_eq!(sampled, prefixes.iter().copied().collect());
        assert!(unique.company_prefix().is_err());

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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(actual, suffixes.into_iter().map(str::to_owned).collect());
        assert!(unique.company_suffix().is_err());
        assert!(faker.company_prefix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            sampled,
            suffixes.iter().map(|item| (*item).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
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
            let prefixes: HashSet<_> = (0..11).map(|_| unique.company_prefix().unwrap()).collect();
            assert!(prefixes.contains("Cvjećarnica"));
            assert!(unique.company_prefix().is_err());
            assert!(names.iter().any(|name| {
                prefixes
                    .iter()
                    .any(|prefix| name.starts_with(&format!("{prefix} ")))
            }));
        } else {
            assert!(provider.company_prefix().is_err());
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
        .map(|_| unique.company_suffix().unwrap())
        .collect();
    assert_eq!(actual, greek_suffixes.map(str::to_owned).into());
    assert!(unique.company_suffix().is_err());
    let unique = montenegrin.unique(true);
    let actual = [
        unique.company_type().unwrap(),
        unique.company_type().unwrap(),
    ];
    assert_eq!(actual.into_iter().collect::<HashSet<_>>(), types.into());
    assert!(unique.company_type().is_err());
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
        assert!(names.insert(unique.company_name().unwrap()));
    }
    assert_eq!(montenegrin.company_suffix(), "Ltd");
    assert!(greek.company_name().is_err());
    assert!(greek.company_type().is_err());
}

#[test]
fn british_south_african_and_cypriot_company_names_follow_local_templates() {
    use std::collections::HashSet;

    for locale in ["en-GB", "en-ZA"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        assert_eq!(unique.company_suffix().unwrap(), "Ltd");
        assert!(unique.company_suffix().is_err());
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
    let actual: HashSet<_> = (0..4).map(|_| unique.company_suffix().unwrap()).collect();
    assert_eq!(actual, suffixes.map(str::to_owned).into());
    assert!(unique.company_suffix().is_err());
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
    assert!(faker.company_prefix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
        let names: Vec<_> = (0..256).map(|_| faker.company()).collect();
        let has_suffix = |name: &String| {
            suffixes
                .iter()
                .any(|suffix| name.ends_with(&format!(" {suffix}")))
        };
        assert!(names.iter().any(has_suffix));
        assert!(names.iter().any(|name| !has_suffix(name)));
        assert!(faker.company_prefix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
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
    let fields: HashSet<_> = (0..49).map(|_| unique.company_field().unwrap()).collect();
    assert_eq!(fields.len(), 49);
    assert!(fields.contains("Dış Ticaret"));
    assert!(unique.company_field().is_err());
    assert!(lithuanian.company_field().is_err());
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
            .map(|_| unique.company_prefix().unwrap())
            .collect();
        assert_eq!(actual, prefixes.iter().copied().collect());
        assert!(unique.company_prefix().is_err());
        for _ in 0..suffix_count {
            unique.company_name_suffix().unwrap();
        }
        assert!(unique.company_name_suffix().is_err());
        assert!(!faker.company_name_element().unwrap().is_empty());
        assert_eq!(faker.company_suffix(), "Ltd");
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
    assert!(!faker.company_name().unwrap().is_empty());
    assert!(faker.company_name_element().is_err());
    assert!(faker.company_name_suffix().is_err());
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
            .map(|_| unique.company_suffix().unwrap())
            .collect();
        assert_eq!(
            actual,
            suffixes.iter().map(|suffix| (*suffix).to_owned()).collect()
        );
        assert!(unique.company_suffix().is_err());
    }

    let unique = iran.unique(true);
    for _ in 0..6 {
        unique.contract().unwrap();
    }
    assert!(unique.contract().is_err());
    for _ in 0..10 {
        unique.company_field().unwrap();
    }
    assert!(unique.company_field().is_err());
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
        assert!(!georgia.company_name_element().unwrap().is_empty());
        assert!(!georgia.company_name_suffix().unwrap().is_empty());
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
    assert_eq!(bangladesh.company_suffix(), "Ltd");
    assert!(["রহিম", "করিম", "বাবলু"].contains(&bangladesh.company_name().unwrap()));
    assert!(["সিমেন্ট", "সার", "ঢেউটিন"].contains(&bangladesh.company_type().unwrap()));

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
        assert!(id_prefixes.contains(&indonesia.company_prefix().unwrap()));
        assert!(["Tbk", "(Persero) Tbk"].contains(&indonesia.company_suffix().as_str()));

        let value = malaysia.company();
        assert!(
            my_suffixes
                .iter()
                .any(|suffix| value.ends_with(&format!(" {suffix}")))
        );
        assert!(my_suffixes.contains(&malaysia.company_suffix().as_str()));
        assert!(thai.is_match(&thailand.company()));
        assert!(slogan.is_match(&thailand.slogan().unwrap()));
    }

    let unique = malaysia.unique(true);
    assert_ne!(
        unique.company_name().unwrap(),
        unique.company_name().unwrap()
    );
    assert_ne!(unique.industry().unwrap(), unique.industry().unwrap());
    let unique = thailand.unique(true);
    assert_ne!(unique.slogan().unwrap(), unique.slogan().unwrap());
    assert_eq!(thailand.company_suffix(), "Ltd");
    assert!(indonesia.industry().is_err());
    assert!(malaysia.company_type().is_err());
    assert!(bangladesh.slogan().is_err());
}

#[test]
fn job_titles_use_the_company_provider_and_share_unique_history() {
    let swedish = fake(Some("sv-SE")).unwrap();
    let titles = swedish.unique(true);
    let catalog: std::collections::HashSet<_> =
        (0..22).map(|_| titles.job_title().unwrap()).collect();
    assert_eq!(catalog.len(), 22);
    assert!(catalog.contains("Automationsingenjör"));
    assert!(catalog.contains("Webbutvecklare"));
    assert!(catalog.contains(swedish.job_title().as_str()));
    assert!(swedish.unique_with_retries(false, 1).job_title().is_err());
    assert!(catalog.contains(swedish.unique(true).job_title().unwrap().as_str()));

    let german = fake(Some("de-DE")).unwrap();
    let german_titles = german.unique(true);
    let catalog: std::collections::HashSet<_> = (0..25)
        .map(|_| german_titles.job_title().unwrap())
        .collect();
    assert!(catalog.contains("Abteilungsdirektor"));
    assert!(catalog.contains("Werksleiter"));

    let french = fake(Some("fr-CA")).unwrap();
    let french_titles = french.unique(true);
    let catalog: std::collections::HashSet<_> = (0..300)
        .map(|_| french_titles.job_title().unwrap())
        .collect();
    assert!(catalog.contains("Agent d'accueil"));
    assert!(catalog.contains("Vitrier"));
    for locale in ["fr-FR", "fr-BE", "fr-CH"] {
        assert!(catalog.contains(&fake(Some(locale)).unwrap().job_title()));
    }

    let fallback_locale = fake(Some("en-CA")).unwrap();
    let inherited_locale = fake(Some("zh-Hant")).unwrap();
    let inherited = inherited_locale.unique(true);
    for _ in 0..32 {
        let title = inherited.job_title().unwrap();
        assert!(title.bytes().all(|byte| byte.is_ascii_lowercase()));
    }
    // en-CA lacks a dedicated Company provider and uses the US title list.
    assert!((0..32).any(|_| {
        fallback_locale
            .job_title()
            .bytes()
            .any(|byte| byte.is_ascii_uppercase())
    }));
}

#[test]
fn taiwanese_english_company_names_require_a_surname_formatter() {
    use openrust::database::DatabaseError;

    let taiwan = fake(Some("zh-Hant")).unwrap();
    assert!(matches!(
        taiwan.company_en(),
        Err(DatabaseError::InvalidQuery(
            "unknown Faker formatter lastNameEn"
        ))
    ));
    let unique = taiwan.unique(true);
    assert!(unique.company_en().is_err());
    let titles: std::collections::HashSet<_> = (0..8)
        .map(|_| {
            unique
                .company_en_with_last_name(|| Ok("Lin".to_owned()))
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
            .unique_with_retries(false, 1)
            .company_en_with_last_name(|| Ok("Lin".to_owned()))
            .is_err()
    );

    let mut calls = 0;
    let name = taiwan
        .company_en_with_last_name(|| {
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
        taiwan.company_en_with_last_name(|| Err(DatabaseError::InvalidQuery(
            "surname source failed"
        ))),
        Err(DatabaseError::InvalidQuery("surname source failed"))
    ));
    let other = fake(Some("en-US")).unwrap();
    assert!(matches!(
        other.company_en_with_last_name(|| panic!("unsupported locale must skip the formatter")),
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
            (0..count).map(|_| unique.last_name().unwrap()).collect();
        assert_eq!(catalog.len(), count);
        assert!(catalog.contains(first));
        assert!(catalog.contains(last));
        assert!(faker.unique_with_retries(false, 1).last_name().is_err());
        for _ in 0..32 {
            assert!(catalog.contains(&faker.last_name()));
            for name in [
                faker.name(),
                faker.name_with_gender(Some("male")),
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
        let actual: HashSet<_> = (0..count).map(|_| unique.last_name().unwrap()).collect();
        assert_eq!(actual.len(), count);
        assert!(actual.contains(expected));
        assert!(faker.unique_with_retries(false, 1).last_name().is_err());
        assert!(actual.contains(&faker.last_name()));
    }

    let dutch = fake(Some("nl-NL")).unwrap();
    let surnames: HashSet<_> = (0..12_000).map(|_| dutch.last_name()).collect();
    for expected in ["Albinus genaamd Weiss von Weissenlöw", "de Jong", "Yilmaz"] {
        assert!(surnames.contains(expected));
    }
    assert!(!surnames.contains("Doe"));

    for locale in ["zh-Hans", "zh-Hant"] {
        let faker = fake(Some(locale)).unwrap();
        for _ in 0..64 {
            let name = faker.name();
            assert!(name.chars().all(|character| !character.is_ascii()));
            let first = faker.first_name();
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
        let username = japan.user_name();
        japanese_pair |= username.contains('.');
        assert!(
            username
                .bytes()
                .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'.'))
        );

        let username = china.user_name();
        chinese_underscore |= username.contains('_');

        let username = lithuania.user_name();
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
            (0..32).map(|_| unique.user_name().unwrap()).collect();
        assert_eq!(samples.len(), 32);
        for value in [faker.safe_email(), faker.free_email()] {
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
            .map(|_| unique.free_email_domain().unwrap())
            .collect();
        assert_eq!(actual, domains.iter().copied().collect());
        assert!(unique.free_email_domain().is_err());
        let actual: HashSet<_> = (0..suffixes.len()).map(|_| unique.tld().unwrap()).collect();
        assert_eq!(actual, suffixes.iter().copied().collect());
        assert!(unique.tld().is_err());
    }

    let japan = fake(Some("ja-JP")).unwrap();
    let domain = japan.domain_name().unwrap();
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
    assert!(!nepal.company_email().unwrap().ends_with(".np"));
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
        let label = unique.company_url().unwrap();
        assert!(label.chars().all(|ch| ch == '-' || ch.is_alphabetic()));
        if let Some((_, suffix)) = label.rsplit_once('-') {
            assert!(["Сервіс", "Плюс", "Груп", "Стиль", "Дизайн"].contains(&suffix));
        }
        shapes.insert(label.contains('-'));
    }
    assert_eq!(shapes, HashSet::from([true, false]));
    assert!(matches!(
        nepal.company_url(),
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
    assert_ne!(unique.middle_name().unwrap(), unique.middle_name().unwrap());

    let iceland = fake(Some("is-IS")).unwrap();
    for _ in 0..64 {
        assert!(iceland.last_name().ends_with('s'));
        assert!(iceland.name_with_gender(Some("male")).ends_with("sson"));
        assert!(
            iceland
                .name_with_gender(Some("female"))
                .ends_with("sdóttir")
        );
    }
    assert!(iceland.middle_name().is_ok());

    let finland = fake(Some("fi-FI")).unwrap();
    assert!(["Hra.", "Tri."].contains(&finland.title_male()));
    assert!(["Rva.", "Nti.", "Tri."].contains(&finland.title_female()));
    assert!(finland.middle_name().is_err());
    assert!(finland.last_name_male().is_err());
}

#[test]
fn polish_and_lithuanian_names_follow_gendered_surname_templates() {
    let lithuania = fake(Some("lt-LT")).unwrap();
    let names = lithuania.unique(true);
    let male: std::collections::HashSet<_> =
        (0..14).map(|_| names.last_name_male().unwrap()).collect();
    let female: std::collections::HashSet<_> =
        (0..20).map(|_| names.last_name_female().unwrap()).collect();
    assert!(male.contains("Kazlauskas"));
    assert!(female.contains("Kazlauskienė"));
    for _ in 0..64 {
        assert!(male.contains(&lithuania.last_name_with_gender(Some("male"))));
        assert!(female.contains(&lithuania.last_name_with_gender(Some("female"))));
        let default = lithuania.last_name();
        assert!(!male.contains(&default) || default == "Urbonas");
        let name = lithuania.name_with_gender(Some("female"));
        assert!(female.contains(name.rsplit_once(' ').unwrap().1));
    }
    assert!(["p.", "ponas"].contains(&lithuania.title_male()));
    let poland = fake(Some("pl-PL")).unwrap();
    assert!(["mgr", "inż.", "dr", "doc."].contains(&poland.title_female()));
    for _ in 0..64 {
        let name = poland.name_with_gender(Some("male"));
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
        let actual: HashSet<_> = (0..count).map(|_| unique.last_name().unwrap()).collect();
        assert!(actual.contains(marker));
        assert!(faker.unique_with_retries(false, 1).last_name().is_err());

        for _ in 0..32 {
            for name in [
                faker.name(),
                faker.name_with_gender(Some("female")),
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
            .map(|_| unique.title_female().unwrap())
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
                .name_with_gender(Some("male"))
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
        let male_titles: HashSet<_> = (0..5).map(|_| unique.title_male().unwrap()).collect();
        let female_titles: HashSet<_> = (0..if locale == "es-ES" { 6 } else { 5 })
            .map(|_| unique.title_female().unwrap())
            .collect();
        assert!(male_titles.contains(male));
        assert!(female_titles.contains(female));
        let surname = unique.last_name().unwrap();
        assert!(!surname.is_empty());
        assert_ne!(surname, unique.last_name().unwrap());
        assert!(!faker.first_name().is_empty());
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
            .map(|_| unique.last_name_male().unwrap())
            .collect();
        let females: HashSet<_> = (0..female_count)
            .map(|_| unique.last_name_female().unwrap())
            .collect();
        assert!(males.contains(male));
        assert!(females.contains(female));
        for _ in 0..32 {
            assert!(females.contains(&faker.last_name_with_gender(Some("female"))));
            let full = faker.name_with_gender(Some("female"));
            assert!(females.iter().any(|surname| full.contains(surname)));
        }
        assert_ne!(faker.title_male(), "Mr.");
    }

    let hungary = fake(Some("hu-HU")).unwrap();
    let unique = hungary.unique(true);
    assert_ne!(
        unique.first_name_male_ne().unwrap(),
        unique.first_name_male_ne().unwrap()
    );
    assert_ne!(
        unique.last_name_female_married().unwrap(),
        unique.last_name_female_married().unwrap()
    );
    assert!(fake(Some("de-AT")).unwrap().first_name_male_ne().is_err());
}

#[test]
fn greek_bulgarian_and_slovenian_names_use_their_person_providers() {
    use std::collections::HashSet;

    for (locale, male_count, female_count) in [("el-GR", 137, 70), ("bg-BG", 121, 44)] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        let males: HashSet<_> = (0..male_count)
            .map(|_| unique.last_name_male().unwrap())
            .collect();
        let females: HashSet<_> = (0..female_count)
            .map(|_| unique.last_name_female().unwrap())
            .collect();
        assert!(unique.last_name_male().is_err());
        assert!(unique.last_name_female().is_err());
        for _ in 0..32 {
            let surname = faker.last_name();
            assert!(males.contains(&surname) || females.contains(&surname));
            assert!(females.contains(&faker.last_name_with_gender(Some("female"))));
            let name = faker.name_with_gender(Some("male"));
            assert!(males.iter().any(|surname| name.contains(surname)));
        }
    }

    let slovenia = fake(Some("sl-SI")).unwrap();
    let unique = slovenia.unique(true);
    let surnames: HashSet<_> = (0..200).map(|_| unique.last_name().unwrap()).collect();
    assert!(surnames.contains("Novak"));
    assert!(surnames.contains(&slovenia.last_name_male().unwrap()));
    assert!(surnames.contains(&slovenia.last_name_female().unwrap()));
    let titles: HashSet<_> = (0..11)
        .map(|_| unique.title(Some("male")).unwrap())
        .collect();
    assert!(titles.contains("univ. dipl. inž."));
    assert!(titles.contains(slovenia.title(Some("female"))));
    assert!(["Mr.", "Dr.", "Prof."].contains(&slovenia.title_male()));
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
        let surnames: HashSet<_> = (0..count).map(|_| unique.last_name().unwrap()).collect();
        assert!(surnames.contains(marker));
        assert!(unique.last_name().is_err());
        let actual_titles: HashSet<_> = (0..titles.len())
            .map(|_| unique.title_female().unwrap())
            .collect();
        assert_eq!(actual_titles, titles.iter().copied().collect());
        let mut egyptian_prefix = false;
        for _ in 0..128 {
            for name in [faker.name(), faker.name_with_gender(Some("female"))] {
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
            .map(|_| unique.middle_name_male().unwrap())
            .collect();
        let females: HashSet<_> = (0..count)
            .map(|_| unique.middle_name_female().unwrap())
            .collect();
        assert!(unique.middle_name_male().is_err());
        assert!(unique.middle_name_female().is_err());
        assert!(females.contains(faker.middle_name_with_gender(Some("female")).unwrap()));
        let default = faker.middle_name().unwrap();
        assert!(males.contains(default) || females.contains(default));
        let grouped: HashSet<_> = (0..count)
            .map(|_| unique.middle_name_with_gender(Some("female")).unwrap())
            .collect();
        assert_eq!(grouped, females);
        assert!(unique.middle_name_with_gender(Some("female")).is_err());
        let mut patronymic_seen = false;
        for _ in 0..128 {
            let name = faker.name_with_gender(Some("female"));
            patronymic_seen |= females
                .iter()
                .any(|middle| name.split_whitespace().any(|part| part == *middle));
        }
        assert!(patronymic_seen);
    }

    let russian = fake(Some("ru-RU")).unwrap();
    for _ in 0..64 {
        assert!(russian.last_name_with_gender(Some("female")).ends_with('а'));
    }
    let kazakh = fake(Some("kk-KZ")).unwrap();
    assert!(kazakh.middle_name_male().is_err());
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
            ["খান", "শেখ", "শিকদার", "আলী", "তাসনীম", "তাবাসসুম"].contains(&bengali.last_name().as_str())
        );
        assert_eq!(bengali.title_male(), "মি.");
        assert!((2..=3).contains(&bengali.name().split_whitespace().count()));

        let surname = indonesian.unique(false).last_name_female().unwrap();
        assert!(unique_surnames.insert(surname));
        assert_ne!(indonesian.last_name_with_gender(Some("male")), "Doe");
        assert!(["dr.", "drg.", "Dr.", "Hj."].contains(&indonesian.title_female()));

        assert_eq!(malaysian.first_name_with_gender(Some("male")), "John");
        assert_eq!(malaysian.first_name_with_gender(Some("female")), "Jane");
        regional_malaysian_name_seen |=
            !["John", "Jane"].contains(&malaysian.first_name().as_str());
        assert_ne!(malaysian.last_name(), "Doe");
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
            .contains(&malaysian.title_male())
        );

        let name = korean.name();
        assert!(name.chars().all(|c| ('가'..='힣').contains(&c)));
        assert!(name.chars().count() >= 2);
        assert!(
            korean
                .first_name_female()
                .chars()
                .all(|c| ('가'..='힣').contains(&c))
        );

        assert!(
            thai.last_name()
                .chars()
                .all(|c| c == ' ' || ('\u{0e00}'..='\u{0e7f}').contains(&c))
        );
        assert!(
            thai.first_name()
                .chars()
                .all(|c| ('\u{0e00}'..='\u{0e7f}').contains(&c))
        );
        assert!(["Cụ", "Bà", "Bác", "Cô", "Chị", "Em"].contains(&vietnamese.title_female()));
        assert!(!vietnamese.middle_name().unwrap().is_empty());
        assert!(
            !vietnamese
                .middle_name_with_gender(Some("female"))
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
    assert!(other.first_name_male_malay().is_err());
    assert!(
        other
            .unique(false)
            .my_kad_number(None, false, None)
            .is_err()
    );
    let unique = malaysia.unique(true);
    let mut initials = std::collections::HashSet::new();
    let mut cards = std::collections::HashSet::new();
    for _ in 0..24 {
        assert!(initials.insert(unique.initial_indian().unwrap()));
        assert!(!malaysia.first_name_male_malay().unwrap().is_empty());
        assert_eq!(
            malaysia
                .first_name_female_chinese()
                .unwrap()
                .split(' ')
                .count(),
            2
        );
        assert!(["", "Haji ", "Hj "].contains(&malaysia.haji().unwrap().as_str()));
        let compact = unique
            .my_kad_number(Some("male"), false, Some("/"))
            .unwrap();
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
            .my_kad_number(Some("female"), true, Some("／"))
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
                .my_kad_number(None, true, None)
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
            assert!(["dl.", "ing.", "dr."].contains(&faker.title_male()));
            assert!(["d-na.", "d-șoara", "ing.", "dr."].contains(&faker.title_female()));
            assert_ne!(faker.first_name_male(), "John");
            assert_ne!(faker.last_name(), "Doe");
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
            assert_ne!(faker.last_name(), "Doe");
            assert!(["Mr.", "Dr.", "Prof."].contains(&faker.title_male()));
        }
    }
}

#[test]
fn latvian_driver_licence_uses_the_pinned_mask_and_unique_history() {
    let faker = fake(Some("lv-LV")).unwrap();
    let unique = faker.unique(true);
    let mut licences = std::collections::HashSet::new();
    for _ in 0..64 {
        let licence = unique.driver_licence().unwrap();
        let bytes = licence.as_bytes();
        assert_eq!(bytes.len(), 8);
        assert!(bytes[..2].iter().all(u8::is_ascii_lowercase));
        assert!(bytes[2..].iter().all(u8::is_ascii_digit));
        assert!(licences.insert(licence));
    }
    assert!(fake(Some("et-EE")).unwrap().driver_licence().is_err());
}

#[test]
fn indian_and_nepali_names_use_regional_middle_name_catalogs() {
    let indian = fake(Some("en-IN")).unwrap();
    let nepali = fake(Some("ne-NP")).unwrap();

    assert!(indian.middle_name().is_err());
    assert!(indian.middle_name_female().is_err());
    assert!(nepali.middle_name().is_err());

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
            let name = unique.middle_name_with_gender(Some(gender)).unwrap();
            assert!(expected.contains(&name));
            assert!(names.insert(name));
        }
        assert!(unique.middle_name_with_gender(Some(gender)).is_err());
    }
}

#[test]
fn ugandan_surnames_select_the_general_or_explicit_gender_catalog() {
    let faker = fake(Some("en-UG")).unwrap();
    let unique = faker.unique(true);
    let general: std::collections::HashSet<_> =
        (0..123).map(|_| unique.last_name().unwrap()).collect();
    assert_eq!(general.len(), 123);
    assert!(general.contains("Abayisenga"));
    assert!(!general.contains("Mubiru"));
    assert!(!general.contains("Abol"));
    assert!(unique.last_name().is_err());

    let male: std::collections::HashSet<_> =
        (0..41).map(|_| unique.last_name_male().unwrap()).collect();
    assert_eq!(male.len(), 41);
    assert!(male.contains("Mubiru"));
    assert!(unique.last_name_male().is_err());

    let female: std::collections::HashSet<_> = (0..106)
        .map(|_| unique.last_name_female().unwrap())
        .collect();
    assert_eq!(female.len(), 106);
    assert!(female.contains("Abol"));
    assert!(unique.last_name_female().is_err());

    assert!(male.contains(&faker.last_name_with_gender(Some("male"))));
    assert!(female.contains(&faker.last_name_with_gender(Some("female"))));
    assert!(general.contains(&faker.last_name_with_gender(None)));
    assert!(fake(Some("en-NG")).unwrap().last_name_female().is_err());
}

#[test]
fn mongolian_person_keeps_initials_distinct_from_the_identity_alphabet() {
    let faker = fake(Some("mn-MN")).unwrap();
    let unique = faker.unique(true);
    let initials: std::collections::HashSet<_> =
        (0..30).map(|_| unique.name_prefix().unwrap()).collect();
    assert_eq!(initials.len(), 30);
    assert!(unique.name_prefix().is_err());
    let alphabet: std::collections::HashSet<_> =
        (0..35).map(|_| unique.alphabet().unwrap()).collect();
    assert_eq!(alphabet.len(), 35);
    assert!(initials.is_subset(&alphabet));

    for gender in [None, Some("male"), Some("female")] {
        for _ in 0..32 {
            let name = faker.name_with_gender(gender);
            let (prefix, given) = name.split_once('.').unwrap();
            assert_eq!(prefix.chars().count(), 1);
            assert!(initials.contains(&prefix.chars().next().unwrap()));
            assert!(!given.is_empty());
        }
    }
    assert_eq!(faker.last_name(), "Doe");
    assert!(fake(Some("en-US")).unwrap().name_prefix().is_err());
}

#[test]
fn singapore_person_keeps_base_names_and_identity_boundaries() {
    use jiff::civil::Date as CivilDate;

    let faker = fake(Some("en-SG")).unwrap();
    assert_eq!(faker.name_with_gender(Some("male")), "John Doe");
    assert_eq!(faker.name_with_gender(Some("female")), "Jane Doe");
    assert_eq!(faker.last_name(), "Doe");
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
        let date = CivilDate::new(year, month, day).ok();
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
    assert_eq!(unique.singapore_id(None, false).unwrap().len(), 9);
    assert!(fake(Some("en-US")).unwrap().nric(None).is_err());
}

#[test]
fn belgian_dutch_person_uses_pinned_names_and_rrn_rules() {
    let faker = fake(Some("nl-BE")).unwrap();
    let unique = faker.unique(true);
    let mut numbers = std::collections::HashSet::new();
    for gender in [None, Some("male"), Some("female")] {
        for _ in 0..64 {
            assert_ne!(faker.last_name(), "Doe");
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
        .map(|_| unique.first_kana_name_male().unwrap())
        .collect();
    let female: std::collections::HashSet<_> = (0..21)
        .map(|_| unique.first_kana_name_female().unwrap())
        .collect();
    let surnames: std::collections::HashSet<_> =
        (0..49).map(|_| unique.last_kana_name().unwrap()).collect();
    assert_eq!((male.len(), female.len(), surnames.len()), (30, 21, 49));
    assert!(unique.first_kana_name_male().is_err());
    assert!(unique.first_kana_name_female().is_err());
    assert!(unique.last_kana_name().is_err());
    for (gender, names) in [("male", &male), ("female", &female)] {
        for _ in 0..32 {
            let full = faker.kana_name(Some(gender)).unwrap();
            let (last, first) = full.split_once(' ').unwrap();
            assert!(surnames.contains(last));
            assert!(names.contains(first));
            assert!(names.contains(faker.first_kana_name(Some(gender)).unwrap()));
        }
    }
    assert!(fake(Some("en-US")).unwrap().kana_name(None).is_err());
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
    let faker = fake(Some("cs-CZ")).unwrap();
    let unique = faker.unique(true);
    let now = jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date();
    for (year, female, slash) in [(1953, false, 0), (1954, true, 100), (2004, true, 100)] {
        let age = u32::try_from(i32::from(now.year()) - year).unwrap();
        let mut unique_numbers = std::collections::HashSet::new();
        for _ in 0..32 {
            let gender = if female { "female" } else { "male" };
            let number = unique.birth_number(Some(gender), age, age, slash).unwrap();
            assert_eq!(number.as_bytes().get(6) == Some(&b'/'), slash == 100);
            let digits = number.replace('/', "");
            assert_eq!(digits.len(), if year < 1954 { 9 } else { 10 });
            assert_eq!(digits[..2].parse::<i32>().unwrap(), year % 100);
            let month = digits[2..4].parse::<u32>().unwrap() - if female { 50 } else { 0 };
            assert!(
                month == now.month() as u32
                    || (year >= 2004 && month == now.month() as u32 + 20)
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
        (faker.birth_number_male().unwrap(), 1..=32),
        (faker.birth_number_female().unwrap(), 51..=82),
    ] {
        assert!(range.contains(&number[2..4].parse::<u32>().unwrap()));
    }
    assert!(faker.birth_number(None, 21, 20, 50).is_err());
    assert!(faker.birth_number(None, 0, u32::MAX, 50).is_err());
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
    assert!(faker.birth_number_male().is_err());
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
            let value = unique.tax_id().unwrap();
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
        assert!(faker.tax_id().is_err());
    }
}

#[test]
fn iranian_national_code_and_indonesian_nik_keep_their_pinned_parts() {
    let iran = fake(Some("fa-IR")).unwrap();
    let indonesia = fake(Some("id-ID")).unwrap();
    let iran_unique = iran.unique(true);
    let indonesia_unique = indonesia.unique(true);
    let date = jiff::civil::Date::new(2000, 2, 29).ok();
    let mut codes = std::collections::HashSet::new();
    let mut nik_values = std::collections::HashSet::new();
    for _ in 0..64 {
        let code = iran_unique.national_code().unwrap();
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
    assert!(indonesia.national_code().is_err());
}

#[test]
fn kazakh_iin_and_venezuelan_national_id_follow_the_pinned_masks() {
    use openrust::database::eloquent::factories::Faker;

    assert_eq!(Faker::check_sum("00022961021").unwrap(), 1);
    assert_eq!(Faker::check_sum("00022961020").unwrap(), 10);
    assert!(Faker::check_sum("000229610212").is_err());
    assert!(Faker::check_sum("0002296102x").is_err());

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
        let date = jiff::civil::Date::new(year, 1, 1).ok();
        for _ in 0..16 {
            let number = unique
                .individual_identification_number(date, gender)
                .unwrap();
            assert_eq!(number.len(), 12);
            assert_eq!(&number[..7], format!("{:02}0101{marker}", year % 100));
            assert!((1000..=2000).contains(&number[7..11].parse::<u16>().unwrap()));
            assert_eq!(
                Faker::check_sum(&number[..11]).unwrap(),
                number.as_bytes()[11] - b'0'
            );
            assert!(identifiers.insert(number));
            let number = venezuela_unique.national_id(Some("／")).unwrap();
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
            .individual_identification_number(None, None)
            .unwrap()
            .len(),
        12
    );
    assert!(
        kazakh
            .individual_identification_number(jiff::civil::Date::new(2100, 1, 1).ok(), None)
            .is_err()
    );
    assert!(
        kazakh
            .individual_identification_number(None, Some("invalid"))
            .is_err()
    );
    assert!(
        venezuelan
            .individual_identification_number(None, None)
            .is_err()
    );
    assert!(kazakh.national_id(None).is_err());
}

#[test]
fn portuguese_nif_and_taiwanese_random_names_follow_the_pinned_helpers() {
    use openrust::database::eloquent::factories::Faker;

    assert_eq!(Faker::dv_calc_mod11("").unwrap(), 0);
    assert_eq!(Faker::dv_calc_mod11("000000001").unwrap(), 9);
    assert_eq!(Faker::dv_calc_mod11("123456789").unwrap(), 7);
    assert!(Faker::dv_calc_mod11("١").is_err());
    let portugal = fake(Some("pt-PT")).unwrap();
    let unique = portugal.unique(true);
    for _ in 0..32 {
        let nif = unique.taxpayer_identification_number().unwrap();
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
            Faker::dv_calc_mod11(&nif[..8]).unwrap()
        );
    }
    let taiwan = fake(Some("zh-TW")).unwrap();
    assert_eq!(taiwan.random_name(&["明"], 3).unwrap(), "明明明");
    assert_eq!(taiwan.random_name(&["明"], 0).unwrap(), "");
    assert_eq!(taiwan.random_name(&[], 2).unwrap(), "");
    let unique = taiwan.unique(true);
    assert_eq!(unique.random_name(&["明"], 1).unwrap(), "明");
    assert!(unique.random_name(&["明"], 1).is_err());
    assert!(portugal.random_name(&["明"], 1).is_err());
}

#[test]
fn lithuanian_driver_licence_is_numeric_and_supports_unique() {
    let faker = fake(Some("lt-LT")).unwrap();
    let unique = faker.unique(true);
    let mut values = std::collections::HashSet::new();
    for _ in 0..64 {
        let value = unique.driver_licence().unwrap();
        assert_eq!(value.len(), 8);
        assert!(value.bytes().all(|byte| byte.is_ascii_digit()));
        assert!(values.insert(value));
    }
}

#[test]
fn missing_person_provider_uses_us_names_and_optional_formatters() {
    let english = fake(Some("en-US")).unwrap();
    let surnames: std::collections::HashSet<_> = (0..1200)
        .map_while(|_| english.unique(false).last_name().ok())
        .collect();
    assert!(surnames.len() >= 400);

    for locale in ["en-CA", "cy-GB", "en"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..16 {
            assert!(!faker.name().is_empty());
            assert!(!faker.first_name().is_empty());
            assert!(surnames.contains(&faker.last_name()));
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
        let compact = unique.taxpayer_identification_number().unwrap();
        assert_eq!(compact.len(), 10);
        assert!(b"JGVEPC".contains(&compact.as_bytes()[0]));
        assert!(compact[1..].bytes().all(|digit| digit.is_ascii_digit()));
        prefixes.insert(compact.as_bytes()[0]);
        assert!(samples.insert(compact));

        let explicit = unique
            .taxpayer_identification_number_with_separator(None)
            .unwrap();
        assert!(samples.insert(explicit));
        let separated = unique
            .taxpayer_identification_number_with_separator(Some("／-"))
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
        assert_eq!(
            faker.taxpayer_identification_number().unwrap().len(),
            length
        );
        assert!(
            faker
                .taxpayer_identification_number_with_separator(Some("-"))
                .is_err()
        );
    }
}

#[test]
fn company_phrases_preserve_regional_provider_and_template_rules() {
    let missing = fake(Some("ja-JP")).unwrap();
    assert!(missing.catch_phrase().is_err());
    assert!(missing.bs().is_err());

    let fallback = fake(Some("en-CA")).unwrap();
    let unique = fallback.unique(true);
    let slogans: std::collections::HashSet<_> =
        (0..48).map(|_| unique.catch_phrase().unwrap()).collect();
    assert_eq!(slogans.len(), 48);
    assert!(!fallback.bs().unwrap().is_empty());
    assert!(fallback.catch_phrase_verb().is_err());

    for locale in ["ar-SA", "ar-JO", "hy-AM"] {
        let faker = fake(Some(locale)).unwrap();
        assert_eq!(faker.catch_phrase().unwrap().is_empty(), locale == "hy-AM");
        let unique = faker.unique_with_retries(true, 2);
        assert_eq!(unique.bs().unwrap(), "");
        assert!(unique.bs().is_err());
    }

    for locale in ["fr-FR", "fr-CA", "fr-CH", "fr-BE"] {
        let faker = fake(Some(locale)).unwrap();
        assert!(!faker.catch_phrase_noun().unwrap().is_empty());
        assert!(!faker.catch_phrase_verb().unwrap().is_empty());
        assert!(!faker.catch_phrase_attribute().unwrap().is_empty());
        assert!(faker.bs().is_err());
        for _ in 0..128 {
            let phrase = faker.catch_phrase().unwrap();
            assert!(phrase.as_bytes()[0].is_ascii_uppercase());
            for word in ["sécurité", "simpl"] {
                assert!(phrase.matches(word).count() <= 1);
            }
        }
    }

    let czech = fake(Some("cs-CZ")).unwrap();
    assert!(!czech.catch_phrase_noun().unwrap().is_empty());
    assert!(!czech.catch_phrase_verb().unwrap().is_empty());
    assert!(!czech.catch_phrase_attribute().unwrap().is_empty());
    assert!(!czech.catch_phrase().unwrap().is_empty());

    let taiwan = fake(Some("zh-Hant")).unwrap();
    assert!(!taiwan.bs().unwrap().contains(char::is_whitespace));
    let china = fake(Some("zh-Hans")).unwrap();
    assert!(!china.catch_phrase().unwrap().is_empty());
    assert!(china.bs().is_err());
}

#[test]
fn regional_phone_formatters_preserve_options_and_inherited_extensions() {
    for locale in ["en-US", "en-CA", "en"] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..48 {
            let number = unique.phone_number_with_extension().unwrap();
            let (number, extension) = number.split_once(" x").unwrap();
            assert!((3..=5).contains(&extension.len()));
            assert!(extension.bytes().all(|x| x.is_ascii_digit()));
            assert!((10..=11).contains(&number.bytes().filter(u8::is_ascii_digit).count()));
            let e164 = faker.e164_phone_number();
            assert_eq!(e164.len(), 12);
            assert!(e164.starts_with("+1"));
        }
    }
    let malaysia = fake(Some("ms-MY")).unwrap();
    for prefix in [false, true] {
        for formatting in [false, true] {
            for number in [
                malaysia
                    .mobile_number_with_options(prefix, formatting)
                    .unwrap(),
                malaysia
                    .fixed_line_number_with_options(prefix, formatting)
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
    assert_eq!(brazil.cellphone_number(Some(false)).unwrap().len(), 11);
    assert_eq!(brazil.landline_number(Some(false)).unwrap().len(), 10);
    let korea = fake(Some("ko-KR")).unwrap();
    let cell = korea.cell_phone_number().unwrap();
    assert_eq!(cell.len(), 13);
    assert!(cell.starts_with("010-"));
    let hong_kong = fake(Some("en-HK")).unwrap();
    assert!(matches!(
        hong_kong.landline_number(None).unwrap().as_bytes()[0],
        b'2' | b'3'
    ));
    let china = fake(Some("zh-CN")).unwrap();
    assert!(china.mobile_number().is_err());
    assert!(china.phone_number_with_extension().is_err());
    assert!(brazil.mobile_number_with_options(false, false).is_err());
}

#[test]
fn phone_service_formatters_respect_region_and_voip_options() {
    let canada = fake(Some("en-CA")).unwrap();
    let unique = canada.unique(true);
    let samples: std::collections::HashSet<_> = (0..48)
        .map(|_| unique.toll_free_phone_number().unwrap())
        .collect();
    assert_eq!(samples.len(), 48);
    for sample in samples {
        let digits: String = sample.chars().filter(char::is_ascii_digit).collect();
        let number = if digits.len() == 11 {
            &digits[1..]
        } else {
            &digits
        };
        assert_eq!(number.len(), 10);
        assert!(["800", "844", "855", "866", "877", "888"].contains(&&number[..3]));
    }

    let malaysia = fake(Some("ms-MY")).unwrap();
    for prefix in [false, true] {
        for formatting in [false, true] {
            let sample = malaysia
                .voip_number_with_options(prefix, formatting)
                .unwrap();
            let local = match (prefix, formatting) {
                (true, true) => sample.strip_prefix("+6").unwrap(),
                (true, false) => sample.strip_prefix('6').unwrap(),
                _ => &sample,
            };
            assert!(local.starts_with("015"));
            assert_eq!(local.contains('-'), formatting);
            assert_eq!(local.contains(' '), formatting);
            assert_eq!(local.chars().filter(char::is_ascii_digit).count(), 10);
        }
    }

    let french = fake(Some("fr-FR")).unwrap();
    for _ in 0..64 {
        let service = french.service_number().unwrap();
        let digits: String = service.chars().filter(char::is_ascii_digit).collect();
        let local = if service.starts_with('+') {
            digits.strip_prefix("33").unwrap().trim_start_matches('0')
        } else {
            digits.strip_prefix('0').unwrap()
        };
        assert_eq!(local.len(), 9);
        assert!(local.starts_with('8'));
        let prefix = &local[1..3];
        assert!(
            matches!(prefix.as_bytes()[0], b'0'..=b'2')
                || ["91", "92", "93", "95", "97", "98", "99"].contains(&prefix)
        );
    }

    let singapore = fake(Some("en-SG")).unwrap();
    assert!(
        singapore
            .toll_free_line_number()
            .unwrap()
            .starts_with("1800")
    );
    assert!(
        singapore
            .toll_free_international_number()
            .unwrap()
            .starts_with("800")
    );
    assert!(
        singapore
            .premium_phone_number()
            .unwrap()
            .starts_with("1900")
    );
    assert!(singapore.voip_number_with_options(false, false).is_err());

    let romania = fake(Some("ro-RO")).unwrap();
    assert!(romania.toll_free_phone_number().unwrap().starts_with("08"));
    assert!(
        romania
            .premium_rate_phone_number()
            .unwrap()
            .starts_with("09")
    );
    assert!(romania.toll_free_number().is_err());
    assert!(
        fake(Some("ja-JP"))
            .unwrap()
            .toll_free_phone_number()
            .is_err()
    );
}

#[test]
fn display_phone_numbers_use_pinned_provider_templates() {
    for locale in [
        "en-US", "en", "en-GB", "en-CA", "de-DE", "fi-FI", "fr-FR", "el-GR", "en-SG", "ms-MY",
        "vi-VN", "ro-RO", "zh-CN", "zh-TW",
    ] {
        let faker = fake(Some(locale)).unwrap();
        let unique = faker.unique(true);
        for _ in 0..48 {
            let phone = unique.phone_number().unwrap();
            assert!(phone.bytes().filter(u8::is_ascii_digit).count() >= 7);
            assert!(!phone.contains(['#', '{', '}']));
        }
    }
    let bangladesh = fake(Some("bn-BD")).unwrap();
    let phone = bangladesh.phone_number();
    let digits = phone.strip_prefix("+৮৮০").unwrap();
    assert!((1..=7).contains(&digits.chars().count()));
    assert!(digits.chars().all(|c| ('০'..='৯').contains(&c)));

    let brazil = fake(Some("pt-BR")).unwrap();
    let unique = brazil.unique(true);
    for _ in 0..64 {
        let number = unique.phone_number_cleared().unwrap();
        assert!(matches!(number.len(), 10 | 11));
        assert!(number.bytes().all(|x| x.is_ascii_digit()));
        assert!(brazil.phone_number().starts_with('('));
    }
    assert!(bangladesh.phone_number_cleared().is_err());
}

#[test]
fn phone_components_preserve_prefixes_and_placeholder_masks() {
    let finland = fake(Some("fi-FI")).unwrap();
    assert!(finland.land_line_area_code().unwrap().starts_with('0'));
    assert!(!finland.e164land_line_area_code().unwrap().starts_with('0'));
    assert!(["### ####", "#######"].contains(&finland.number_format().unwrap()));
    assert!([" ", "-"].contains(&finland.separator().unwrap()));

    let malaysia = fake(Some("ms-MY")).unwrap();
    assert_eq!(malaysia.country_code_prefix(true).unwrap(), "+6");
    assert_eq!(malaysia.country_code_prefix(false).unwrap(), "6");
    let brazil = fake(Some("pt-BR")).unwrap();
    assert_eq!(brazil.area_code().unwrap().len(), 2);
    assert_eq!(brazil.cellphone(Some(false)).unwrap().len(), 9);
    assert_eq!(brazil.landline(Some(false)).unwrap().len(), 8);

    let canada = fake(Some("en-CA")).unwrap();
    let unique = canada.unique(true);
    let codes: std::collections::HashSet<_> = (0..6)
        .map(|_| unique.toll_free_area_code().unwrap())
        .collect();
    assert_eq!(codes.len(), 6);
    assert!(
        canada
            .unique_with_retries(false, 2)
            .toll_free_area_code()
            .is_err()
    );
    assert!(finland.exchange_code().is_err());
}

#[test]
fn internet_ascii_names_are_locale_specific_and_support_unique() {
    let egypt = fake(Some("ar-EG")).unwrap();
    let names: std::collections::HashSet<_> = {
        let unique = egypt.unique(true);
        (0..14)
            .map(|_| unique.first_name_ascii().unwrap())
            .collect()
    };
    assert_eq!(names.len(), 14);
    assert!(
        egypt
            .unique_with_retries(false, 2)
            .first_name_ascii()
            .is_err()
    );
    assert!(egypt.last_name_ascii().is_ok());

    for locale in ["ar-SA", "ar-JO", "fa-IR", "ja-JP", "ko-KR"] {
        let faker = fake(Some(locale)).unwrap();
        assert!(!faker.first_name_ascii().unwrap().is_empty());
        assert!(!faker.last_name_ascii().unwrap().is_empty());
    }
    let us = fake(Some("en-US")).unwrap();
    assert!(us.first_name_ascii().is_err());
    assert!(us.unique(false).last_name_ascii().is_err());
}

#[test]
fn regional_date_names_and_czech_display_dates_follow_the_provider() {
    let epoch = jiff::Timestamp::UNIX_EPOCH;
    for (locale, month, weekday, period) in [
        ("cs-CZ", "leden", "čtvrtek", "am"),
        ("tr-TR", "Ocak", "Perşembe", "öö"),
        ("zh-CN", "一月", "星期四", "上午"),
        ("zh-TW", "一月", "星期四", "上午"),
        ("ka-GE", "იანვარი", "ხუთშაბათი", "am"),
        ("en-US", "January", "Thursday", "am"),
    ] {
        let faker = fake(Some(locale)).unwrap();
        assert_eq!(faker.month_name(Some(epoch)), month);
        assert_eq!(faker.day_of_week(Some(epoch)), weekday);
        assert_eq!(faker.am_pm(Some(epoch)), period);
        assert_eq!(
            faker.day_of_month(Some(epoch)),
            if locale == "cs-CZ" { "1" } else { "01" }
        );
        if locale != "cs-CZ" {
            assert!(faker.month_name_genitive(Some(epoch)).is_err());
            assert!(faker.formatted_date().is_err());
        }
    }

    let czech = fake(Some("cs-CZ")).unwrap();
    let unique = czech.unique_with_retries(true, 2);
    assert_eq!(unique.month_name_genitive(Some(epoch)).unwrap(), "ledna");
    assert!(unique.month_name_genitive(Some(epoch)).is_err());

    czech.set_default_timezone(Some(
        openrust::support::Date::timezone("America/New_York").unwrap(),
    ));
    assert_eq!(czech.month_name_genitive(Some(epoch)).unwrap(), "prosince");
    assert_eq!(czech.day_of_month(Some(epoch)), "31");
    czech.set_default_timezone(None);
    for _ in 0..64 {
        let date = czech.formatted_date().unwrap();
        let (day, rest) = date.split_once(". ").unwrap();
        let (month, year) = rest.split_once(' ').unwrap();
        assert!((1..=31).contains(&day.parse::<u32>().unwrap()));
        assert!(!day.starts_with('0'));
        assert!(!month.is_empty());
        assert!(year.parse::<u32>().unwrap() >= 1970);
    }
}

#[test]
fn php_date_formats_preserve_native_formats_and_share_unique_history() {
    let faker = fake(Some("zh-Hant")).unwrap();
    let epoch = Some(jiff::Timestamp::UNIX_EPOCH);

    faker.set_default_timezone(Some(
        openrust::support::Date::timezone("America/New_York").unwrap(),
    ));
    assert_eq!(
        faker
            .date_with_php_format(r"Y-m-d\TH:i:sP e T W o z B", epoch)
            .unwrap(),
        "1969-12-31T19:00:00-05:00 America/New_York EST 01 1970 364 041"
    );
    assert_eq!(
        faker.time_with_php_format("g:i A", epoch).unwrap(),
        "7:00 PM"
    );
    assert_eq!(
        faker.date_with_php_format(r"l F \年 Q %", epoch).unwrap(),
        "Wednesday December 年 Q %"
    );
    assert_eq!(faker.date_with_php_format("\\", epoch).unwrap(), "\0");
    assert_eq!(faker.date_with_php_format("", epoch).unwrap(), "");

    let unique = faker.unique_with_retries(true, 2);
    assert_eq!(unique.date(Some("%Y-%m-%d"), epoch).unwrap(), "1969-12-31");
    assert!(unique.date_with_php_format("Y-m-d", epoch).is_err());
    assert_eq!(
        unique.time_with_php_format("Y-m-d", epoch).unwrap(),
        "1969-12-31"
    );
    assert!(unique.time(Some("%Y-%m-%d"), epoch).is_err());

    faker.set_default_timezone(Some(
        openrust::support::Date::timezone("Asia/Kathmandu").unwrap(),
    ));
    assert_eq!(
        faker.date_with_php_format("e T P", epoch).unwrap(),
        "Asia/Kathmandu +0530 +05:30"
    );
}

#[test]
fn zoned_date_formatters_keep_the_interval_and_zone_identity() {
    use jiff::{SignedDuration, Timestamp};
    let new_york = openrust::support::Date::timezone("America/New_York").unwrap();
    let shanghai = openrust::support::Date::timezone("Asia/Shanghai").unwrap();
    let taipei = openrust::support::Date::timezone("Asia/Taipei").unwrap();
    let utc = jiff::tz::TimeZone::UTC;
    let etc_utc = openrust::support::Date::timezone("Etc/UTC").unwrap();

    let faker = fake(Some("en-US")).unwrap();
    let epoch = Timestamp::UNIX_EPOCH;
    faker.set_default_timezone(Some(shanghai.clone()));

    let date = faker
        .date_time_between_in_timezone(epoch, epoch, None)
        .unwrap();
    assert_eq!(
        (date.timestamp().as_second(), date.time_zone().clone()),
        (0, shanghai.clone())
    );

    let date = faker
        .date_time_between_expressions_in_timezone(
            "1970-01-01 00:00:00 UTC",
            "0",
            Some(new_york.clone()),
        )
        .unwrap();
    assert_eq!(
        date.strftime("%Y-%m-%dT%H:%M:%S%.f%:z").to_string(),
        "1969-12-31T19:00:00-05:00"
    );
    assert_eq!(faker.get_default_timezone(), Some(shanghai.clone()));
    assert_eq!(
        faker
            .date_time_in_interval_in_timezone(
                Some(epoch),
                Some(SignedDuration::ZERO),
                Some(utc.clone())
            )
            .unwrap()
            .timestamp()
            .as_second(),
        0,
    );
    assert!(
        faker
            .date_time_between_in_timezone(
                epoch + SignedDuration::from_secs(1),
                epoch,
                Some(utc.clone())
            )
            .is_err()
    );
    assert!(
        faker
            .date_time_in_interval_expressions_in_timezone(
                Some("bad date"),
                Some("+0 seconds"),
                Some(utc.clone())
            )
            .is_err()
    );

    let unique = faker.unique_with_retries(true, 2);
    assert_eq!(unique.date_time(Some(epoch)).unwrap(), epoch);
    assert!(
        unique
            .date_time_in_timezone(Some(epoch), Some(utc.clone()))
            .is_err()
    );
    assert_eq!(
        unique
            .date_time_in_timezone(Some(epoch), Some(etc_utc.clone()))
            .unwrap()
            .time_zone()
            .clone(),
        etc_utc
    );
    assert_eq!(
        unique
            .date_time_in_timezone(Some(epoch), None)
            .unwrap()
            .time_zone()
            .clone(),
        shanghai
    );
    assert_eq!(
        unique
            .date_time_in_timezone(Some(epoch), Some(taipei.clone()))
            .unwrap()
            .time_zone()
            .clone(),
        taipei
    );
    assert!(
        unique
            .date_time_in_timezone(Some(epoch), Some(taipei.clone()))
            .is_err()
    );

    assert_eq!(unique.date_time_between(epoch, epoch).unwrap(), epoch);
    assert!(
        unique
            .date_time_between_in_timezone(epoch, epoch, Some(utc.clone()))
            .is_err()
    );
    assert_eq!(
        unique
            .date_time_between_expressions_in_timezone(
                "1970-01-01 00:00:00 UTC",
                "0",
                Some(taipei.clone())
            )
            .unwrap()
            .time_zone()
            .clone(),
        taipei,
    );
    assert!(
        unique
            .date_time_between_in_timezone(epoch, epoch, Some(taipei.clone()))
            .is_err()
    );
    unique
        .date_time_in_interval_in_timezone(
            Some(epoch),
            Some(SignedDuration::ZERO),
            Some(utc.clone()),
        )
        .unwrap();
    assert!(
        unique
            .date_time_in_interval_expressions_in_timezone(
                Some("1970-01-01 00:00:00 UTC"),
                Some("+0 seconds"),
                Some(utc.clone())
            )
            .is_err()
    );
    let before_fallback = Timestamp::new(1_730_611_800, 0).unwrap();
    let after_fallback = Timestamp::new(1_730_615_400, 0).unwrap();
    unique
        .date_time_between_in_timezone(before_fallback, before_fallback, Some(new_york.clone()))
        .unwrap();
    assert!(
        unique
            .date_time_between_in_timezone(after_fallback, after_fallback, Some(new_york.clone()))
            .is_err()
    );
    let ad = Timestamp::new(-62_135_597_361, 0).unwrap();
    assert_eq!(
        unique
            .date_time_ad_in_timezone(Some(ad), Some(new_york.clone()))
            .unwrap()
            .timestamp(),
        ad
    );
}

#[test]
fn address_catalogs_follow_the_selected_provider() {
    let us = fake(Some("en-US")).unwrap();
    let cn = fake(Some("zh-Hans")).unwrap();
    let tw = fake(Some("zh-Hant")).unwrap();
    let taiwan_postcode = Regex::new(r"^\d{3}(-\d{2})?$").unwrap();

    for _ in 0..64 {
        assert!(
            ["North", "East", "West", "South", "New", "Lake", "Port"]
                .contains(&us.city_prefix().as_str())
        );
        assert!(us.street_name().contains(' '));
        assert!(cn.street_name().ends_with(" Street"));
        assert!(cn.area().unwrap().ends_with('区'));
        assert_eq!(cn.building_number().len(), 2);
        assert_eq!(tw.state(), "臺灣省");
        assert_eq!(tw.state_abbr(), "臺");
        assert_eq!(tw.city_prefix(), "");
        assert_eq!(tw.city_suffix(), "");
        assert!(!tw.street().unwrap().is_empty());
        assert!(taiwan_postcode.is_match(&tw.postcode()));
    }

    let unique = tw.unique_with_retries(true, 128);
    let mut suffixes = [
        unique.street_suffix().unwrap(),
        unique.street_suffix().unwrap(),
    ];
    suffixes.sort();
    assert_eq!(suffixes, ["街", "路"]);
    assert!(unique.street_suffix().is_err());
    assert!(us.area().is_err());
    assert!(cn.street().is_err());
}

#[test]
fn french_departments_and_taiwanese_address_numbers_preserve_values() {
    let french = fake(Some("fr-FR")).unwrap();
    let taiwan = fake(Some("zh-Hant")).unwrap();
    let unique = french.unique_with_retries(true, 10_000);
    let mut departments = std::collections::BTreeMap::new();
    for _ in 0..101 {
        let (number, name) = unique.department().unwrap();
        assert!(departments.insert(number, name).is_none());
    }
    assert_eq!(departments.get("01"), Some(&"Ain"));
    assert_eq!(departments.get("2A"), Some(&"Corse-du-Sud"));
    assert_eq!(departments.get("2B"), Some(&"Haute-Corse"));
    assert_eq!(departments.get("976"), Some(&"Mayotte"));
    assert!(unique.department().is_err());
    assert_eq!(french.street_suffix(), "Street");
    assert!(french.postcode().chars().all(|ch| ch.is_ascii_digit()));

    let unique = taiwan.unique_with_retries(true, 128);
    let chinese: std::collections::HashSet<_> = (0..9)
        .map(|_| unique.random_chinese_number().unwrap())
        .collect();
    assert_eq!(
        chinese,
        ["一", "二", "三", "四", "五", "六", "七", "八", "九"].into()
    );
    assert!(unique.random_chinese_number().is_err());
    for _ in 0..64 {
        assert!((1..=100).contains(&taiwan.random_number2().unwrap()));
        assert!((1..=1000).contains(&taiwan.random_number3().unwrap()));
    }
    assert!(taiwan.department_number().is_err());
    assert!(french.random_number2().is_err());
}

#[test]
fn french_address_locales_preserve_regional_overrides() {
    let belgium = fake(Some("fr-BE")).unwrap();
    let swiss = fake(Some("fr-CH")).unwrap();
    let canada = fake(Some("fr-CA")).unwrap();
    let belgian_line = Regex::new(r"\n [0-9]{4} .+$").unwrap();
    let swiss_line = Regex::new(r"\n[0-9]{4} .+$").unwrap();
    let canadian_postcode = Regex::new(r"^[A-Z][0-9][A-Z] ?[0-9][A-Z][0-9]$").unwrap();
    for _ in 0..64 {
        assert!(belgian_line.is_match(&belgium.address()));
        assert!(swiss_line.is_match(&swiss.address()));
        assert!(canadian_postcode.is_match(&canada.postcode()));
        assert!(canada.address().split(", ").count() == 3);
        assert!(belgium.region().is_ok());
        assert!(swiss.department().is_ok());
        assert!(canada.street_prefix().is_ok());
    }
    let unique = swiss.unique_with_retries(true, 2048);
    let all: std::collections::BTreeMap<_, _> = (0..26).map(|_| unique.canton().unwrap()).collect();
    assert_eq!(all.len(), 26);
    assert_eq!(all.get("GE"), Some(&"Genève"));
    assert!(unique.canton().is_err());
    assert!(swiss.province().is_err());
    assert!(canada.city_name().is_err());
}

#[test]
fn english_address_locales_use_local_postcodes_and_inherited_vocabularies() {
    let canada = fake(Some("en-CA")).unwrap();
    let australia = fake(Some("en-AU")).unwrap();
    let new_zealand = fake(Some("en-NZ")).unwrap();
    let canadian_postcode = Regex::new(
        r"^[ABCEGHJKLMNPRSTVXY][0-9][ABCEGHJKLMNPRSTVXY][ -]?[0-9][ABCEGHJKLMNPRSTVXY][0-9]$",
    )
    .unwrap();
    for _ in 0..64 {
        assert!(canadian_postcode.is_match(&canada.postcode()));
        assert!(canada.address().split_once('\n').unwrap().1.contains("  "));
        assert!((1..=3).contains(&australia.building_number().len()));
        assert_eq!(australia.postcode().len(), 4);
        assert_eq!(new_zealand.postcode().len(), 4);
        let city = new_zealand.city();
        assert!(city.ends_with("ville") || city.ends_with("ston"));
        let address = new_zealand.address();
        assert_eq!(address.split(", ").count(), 4);
        assert!(new_zealand.region().is_ok());
    }
    let unique = australia.unique_with_retries(true, 128);
    let mut letters: Vec<_> = (0..4).map(|_| unique.building_letter().unwrap()).collect();
    letters.sort_unstable();
    assert_eq!(letters, ["A", "B", "C", "D"]);
    assert!(unique.building_letter().is_err());
    assert!(australia.random_postcode_letter().is_err());
    assert!(new_zealand.province_abbr().is_err());
}

#[test]
fn british_address_uses_fixed_postcodes_and_both_street_layouts() {
    let british = fake(Some("en-GB")).unwrap();
    let postcode = Regex::new(r"^[A-Z0-9]{2,4} [0-9][A-Z]{2}$").unwrap();
    let secondary = Regex::new(r"^(Flat|Studio) [0-9]{2}[a-z]?$").unwrap();

    for _ in 0..64 {
        let address = british.address();
        let lines: Vec<_> = address.lines().collect();
        assert!(matches!(lines.len(), 3 | 4));
        assert!(postcode.is_match(lines.last().unwrap()));
        if lines.len() == 4 {
            assert!(secondary.is_match(lines[0]));
        } else {
            assert!(lines[0].starts_with(|ch: char| ch.is_ascii_digit()));
        }
    }

    let unique = british.unique_with_retries(true, 20_000);
    let postcodes: std::collections::HashSet<_> =
        (0..497).map(|_| unique.postcode().unwrap()).collect();
    assert_eq!(postcodes.len(), 497);
    assert!(postcodes.contains("DD8 1LR"));
    assert!(unique.postcode().is_err());
    let counties: std::collections::HashSet<_> =
        (0..89).map(|_| unique.county().unwrap()).collect();
    assert_eq!(counties.len(), 89);
    assert!(counties.contains("Hampshire"));
    assert!(unique.county().is_err());
    assert!(fake(Some("en-US")).unwrap().county().is_err());
}

#[test]
fn african_english_address_providers_keep_regional_fields() {
    let south_africa = fake(Some("en-ZA")).unwrap();
    let uganda = fake(Some("en-UG")).unwrap();
    let nigeria = fake(Some("en-NG")).unwrap();
    let south_african_line = Regex::new(r", (EC|FS|GP|KZN|LP|MP|NW|NC|WC) [0-9]{4}$").unwrap();
    let nigerian_postcode = Regex::new(r"^[0-9]{2} ?[0-9]{3}$").unwrap();
    for _ in 0..64 {
        assert!(south_african_line.is_match(&south_africa.address()));
        let postcode = uganda.postcode();
        assert!(
            (1..=5).contains(&postcode.len()) && postcode.bytes().all(|byte| byte.is_ascii_digit())
        );
        assert!(uganda.district().is_ok());
        assert!(nigerian_postcode.is_match(&nigeria.postcode()));
        assert!(nigeria.city().ends_with("Ville"));
        assert!(nigeria.street_name().ends_with(" Street"));
        assert!(nigeria.county().is_ok());
    }
    let unique = uganda.unique_with_retries(true, 128);
    let mut regions: Vec<_> = (0..4).map(|_| unique.region().unwrap()).collect();
    regions.sort_unstable();
    assert_eq!(regions, ["Central", "East", "North", "West"]);
    assert!(unique.region().is_err());
    assert!(nigeria.district().is_err());
}

#[test]
fn singapore_address_preserves_street_blocks_and_literal_unit_patterns() {
    let faker = fake(Some("en-SG")).unwrap();
    let street = Regex::new(r"^[0-9]{2,3} .+\nSINGAPORE [0-9]{6}$").unwrap();
    let block =
        Regex::new(r"^Blk (?:[0-9]{2}|[0-9]{3}[A-H]?) .+\n(?:##|0#) #{2,3}\nSINGAPORE [0-9]{6}$")
            .unwrap();
    let mut formats = [false; 2];
    for _ in 0..256 {
        let address = faker.address();
        if street.is_match(&address) {
            formats[0] = true;
        } else {
            assert!(block.is_match(&address), "{address:?}");
            formats[1] = true;
        }
        assert!(["##", "0#"].contains(&faker.floor_number().unwrap()));
        assert!(["##", "###"].contains(&faker.apartment_number().unwrap()));
    }
    assert_eq!(formats, [true; 2]);
    assert_eq!(faker.country(), "SINGAPORE");
    assert_eq!(faker.town_name().unwrap(), "SINGAPORE");
    assert_eq!(faker.street_prefix().unwrap(), "Jalan");
    assert!(faker.city().ends_with("Ville"));
    assert!(fake(Some("en-US")).unwrap().block_number().is_err());
    let unique = faker.unique(true);
    assert_ne!(
        unique.floor_number().unwrap(),
        unique.floor_number().unwrap()
    );
    assert!(unique.floor_number().is_err());
}

#[test]
fn indian_english_addresses_preserve_locality_and_society_templates() {
    use std::collections::HashSet;

    let faker = fake(Some("en-IN")).unwrap();
    let standalone = Regex::new(r"^[1-9][0-9], [^,]+,$").unwrap();
    let in_society =
        Regex::new(r"^[1-9][0-9], [^,]+ (Society|Apartments|Heights|Villas), [^,]+$").unwrap();
    let postcode = Regex::new(r"^[1-5][0-9]{5}$").unwrap();
    let mut formats = [false; 2];
    for _ in 0..256 {
        let address = faker.address();
        let (location, pin) = address.rsplit_once(" - ").unwrap();
        assert!(postcode.is_match(pin), "{address}");
        let street = faker.street_address();
        if standalone.is_match(&street) {
            formats[0] = true;
        } else {
            assert!(in_society.is_match(&street), "{street}");
            formats[1] = true;
        }
        assert!(location.contains(", "));
        assert!(faker.street_name().ends_with(" Street"));
        assert_eq!(faker.state_abbr().len(), 2);
    }
    assert_eq!(formats, [true; 2]);
    assert!(fake(Some("en-US")).unwrap().locality().is_err());

    let unique = faker.unique(true);
    let suffixes: HashSet<_> = (0..5).map(|_| unique.area_suffix().unwrap()).collect();
    assert_eq!(
        suffixes,
        HashSet::from([" Nagar", "Pur", "Garh", "Gunj", " Chowk"])
    );
    assert!(unique.area_suffix().is_err());
}

#[test]
fn philippine_address_uses_local_routes_and_inherited_us_fields() {
    let faker = fake(Some("en-PH")).unwrap();
    let number = Regex::new(r"^(?:[0-9]{2}A(?:/[0-9]{2})?|[1-9][0-9](?:/[1-9][0-9])?)$").unwrap();
    let address = Regex::new(r"^.+, (?:Poblacion, )?.+ [0-9]{4} .+$").unwrap();
    let secondary = Regex::new(r"^(?:Apt\.|Suite) [0-9]{3}$").unwrap();
    let mut formats = [false; 2];
    for _ in 0..256 {
        let sample = faker.address();
        assert!(address.is_match(&sample), "{sample}");
        formats[usize::from(sample.contains(", Poblacion, "))] = true;
        assert!(number.is_match(&faker.building_number()));
        assert!(secondary.is_match(&faker.secondary_address()));
        assert_eq!(faker.state_abbr().len(), 2);
        assert!(!faker.province().unwrap().is_empty());
    }
    assert_eq!(formats, [true; 2]);
    let unique = faker.unique(true);
    assert_eq!(unique.barangay().unwrap(), "Poblacion");
    assert!(unique.barangay().is_err());
    assert!(fake(Some("en-US")).unwrap().municipality().is_err());
}

#[test]
fn hong_kong_address_preserves_three_lines_and_local_name_templates() {
    let faker = fake(Some("en-HK")).unwrap();
    let city = ["Hong Kong", "Kowloon", "New Territories"];
    let number = Regex::new(r"^(?:Block )?[1-9][0-9][ ,]").unwrap();
    let mut block = false;
    for _ in 0..256 {
        let address = faker.address();
        let lines: Vec<_> = address.lines().collect();
        assert_eq!(lines.len(), 3, "{address}");
        assert!(number.is_match(lines[0]));
        block |= lines[0].starts_with("Block ");
        assert!(!lines[1].is_empty());
        assert!(city.contains(&lines[2]));
        assert!(!address.contains("{{"));
        let estate = faker.estate().unwrap();
        assert!(estate.ends_with(" Estate") || estate.ends_with(" Court"));
        assert_eq!(faker.postcode().len(), 5);
        assert_eq!(faker.country(), "Hong Kong");
    }
    assert!(block);
    let unique = faker.unique(true);
    assert_ne!(
        unique.estate_suffix().unwrap(),
        unique.estate_suffix().unwrap()
    );
    assert!(unique.estate_suffix().is_err());
    assert!(fake(Some("en-PH")).unwrap().town().is_err());
}

#[test]
fn japanese_address_preserves_postcode_components_and_building_boundaries() {
    use std::collections::HashSet;

    let faker = fake(Some("ja-JP")).unwrap();
    let address = Regex::new(
        r"^[1-9][0-9]{2}[1-9][0-9]{3}  \S+市(?:中央|北|東|南|西)区\S+町\S+(?:10|[1-9])-(?:10|[1-9])-(?:10|[1-9])(?: (?:ハイツ|コーポ)\S+(?:10[1-9]|110)号)?$",
    )
    .unwrap();
    let mut formats = [false; 2];
    for _ in 0..128 {
        let sample = faker.address();
        assert!(address.is_match(&sample), "{sample}");
        formats[usize::from(sample.ends_with('号'))] = true;
        assert!((100..=999).contains(&faker.postcode1().unwrap()));
        assert!((1000..=9999).contains(&faker.postcode2().unwrap()));
        assert!((1..=10).contains(&faker.area_number().unwrap()));
        assert!((101..=110).contains(&faker.building_number().parse::<u16>().unwrap()));
    }
    assert_eq!(formats, [true; 2]);
    let unique = faker.unique(true);
    let wards: HashSet<_> = (0..5).map(|_| unique.ward().unwrap()).collect();
    assert_eq!(
        wards,
        HashSet::from(["中央区", "北区", "東区", "南区", "西区"])
    );
    assert!(unique.ward().is_err());
    assert!(fake(Some("en-US")).unwrap().prefecture().is_err());
}

#[test]
fn korean_address_preserves_administrative_order_and_inherited_street_address() {
    let faker = fake(Some("ko-KR")).unwrap();
    let building = Regex::new(r"^[1-9][0-9]{2,3}$").unwrap();
    let postcode = Regex::new(r"^[0-9]{5}$").unwrap();
    let mut formats = [false; 2];
    for _ in 0..128 {
        let address = faker.address();
        let (location, number) = address.rsplit_once(' ').unwrap();
        assert!(building.is_match(number), "{address}");
        let (region, street) = location.rsplit_once(' ').unwrap();
        assert!(street.ends_with('로'));
        let (administration, locality) = region.split_once(' ').unwrap();
        let metropolitan = administration.ends_with('시');
        formats[usize::from(metropolitan)] = true;
        if metropolitan {
            assert!(locality.ends_with('구'));
        } else {
            assert!(administration.ends_with('도'));
        }

        let street_address = faker.street_address();
        let (number, street) = street_address.split_once(' ').unwrap();
        assert!(building.is_match(number) && street.ends_with('로'));
        assert!(postcode.is_match(&faker.postcode()));
    }
    assert_eq!(formats, [true; 2]);
    let unique = faker.unique(true);
    for _ in 0..7 {
        assert!(unique.metropolitan_city().unwrap().ends_with('시'));
    }
    assert!(unique.metropolitan_city().is_err());
    assert!(fake(Some("ja-JP")).unwrap().borough().is_err());
}

#[test]
fn vietnamese_address_preserves_urban_rural_and_inherited_prefix_formats() {
    let faker = fake(Some("vi-VN")).unwrap();
    let cities = ["Cần Thơ", "Đà Nẵng", "Hải Phòng", "Hà Nội", "Hồ Chí Minh"];
    let postcodes = Regex::new(r"^(?:[0-9]{5,6}|[0-9]{5}-[0-9]{4})$").unwrap();
    let mut formats = [false; 3];
    let mut ward_with_hamlet_prefix = false;
    for _ in 0..256 {
        let address = faker.address();
        let (locality, region) = address.split_once('\n').unwrap();
        let parts: Vec<_> = locality.split(", ").collect();
        let city = cities.contains(&region);
        match parts.as_slice() {
            [street, ward, district] => {
                assert!(street.contains(" Phố "));
                assert!(!ward.contains('#') && !district.contains('#'));
                formats[usize::from(!city)] = true;
            }
            [building, hamlet, _, _] => {
                assert!(building.parse::<u16>().is_ok() && !city);
                assert!(hamlet.starts_with("Thôn ") || hamlet.starts_with("Ấp "));
                formats[2] = true;
            }
            _ => panic!("unexpected address {address:?}"),
        }
        let ward = faker.ward_name().unwrap();
        ward_with_hamlet_prefix |= ward.starts_with("Thôn ") || ward.starts_with("Ấp ");
        assert!(!ward.contains('#'));
        assert!(postcodes.is_match(&faker.postcode()));
    }
    assert_eq!(formats, [true; 3]);
    assert!(ward_with_hamlet_prefix);
    let unique = faker.unique(true);
    assert_ne!(
        unique.district_prefix().unwrap(),
        unique.district_prefix().unwrap()
    );
    assert!(unique.district_prefix().is_err());
    assert!(fake(Some("ko-KR")).unwrap().hamlet_name().is_err());
}

#[test]
fn thai_addresses_preserve_road_prefixes_and_postcode_separators() {
    use std::collections::HashSet;

    let faker = fake(Some("th-TH")).unwrap();
    let prefixes = ["ซอย", "ถนน", "สะพาน", "วงเวียน", "แยก"];
    let number = Regex::new(r"^(?:[0-9]{2,5}|[0-9]{2}/[0-9]{3})$").unwrap();
    let postcode = Regex::new(r"^[0-9]{5}$").unwrap();
    let mut slash = false;
    for _ in 0..128 {
        let address = faker.address();
        let (street, locality) = address.split_once('\n').unwrap();
        let (building, road) = street.split_once(' ').unwrap();
        assert!(number.is_match(building));
        slash |= building.contains('/');
        assert!(prefixes.contains(&road.split_once(' ').unwrap().0));
        let (city, postal) = locality.rsplit_once(", ").unwrap();
        assert!(!city.is_empty() && postcode.is_match(postal));
        assert!(!faker.province().unwrap().is_empty());
    }
    assert!(slash);
    let unique = faker.unique(true);
    let prefixes_sampled: HashSet<_> = (0..prefixes.len())
        .map(|_| unique.street_prefix().unwrap())
        .collect();
    assert_eq!(prefixes_sampled, HashSet::from(prefixes));
    assert!(unique.street_prefix().is_err());
}

#[test]
fn indonesian_address_uses_local_streets_and_state_abbreviations() {
    let faker = fake(Some("id-ID")).unwrap();
    let street = Regex::new(r"^.+ No\. [1-9][0-9]{0,2}$").unwrap();
    let postal = Regex::new(r"^.+ [1-9][0-9]{4}$").unwrap();
    for _ in 0..128 {
        let address = faker.address();
        let (first, state) = address.rsplit_once(", ").unwrap();
        let (road, city) = first.rsplit_once(", ").unwrap();
        assert!(street.is_match(road), "{address}");
        assert!(postal.is_match(city), "{address}");
        assert!(!state.is_empty());
        assert!((1..=999).contains(&faker.building_number().parse::<u16>().unwrap()));
        assert!(
            faker.street_prefix().is_ok() && faker.street().is_ok() && faker.city_name().is_ok()
        );
    }
}

#[test]
fn malaysian_address_keeps_state_town_and_postcode_in_one_region() {
    let faker = fake(Some("ms-MY")).unwrap();
    let mut found = false;
    for _ in 0..256 {
        let address = faker.address();
        let town_state = address.splitn(4, ", ").nth(3).unwrap();
        let (town, state) = town_state.rsplit_once(", ").unwrap();
        let (zip, town) = town.split_once(' ').unwrap();
        assert_eq!(zip.len(), 5);
        assert!(!town.is_empty());
        if matches!(state, "Perlis Indera Kayangan" | "Perlis") {
            assert!((1000..=2800).contains(&zip.parse::<u32>().unwrap()));
            found = true;
        }
        let perlis = faker.postcode_with_state("perlis").unwrap();
        assert!(perlis.starts_with('0') && perlis.len() == 5);
        assert!((1000..=2800).contains(&perlis.parse::<u32>().unwrap()));
    }
    assert!(found);
    assert!(faker.postcode_with_state("unknown").is_err());
    assert!(
        fake(Some("id-ID"))
            .unwrap()
            .postcode_with_state("perlis")
            .is_err()
    );
    let unique = faker.unique(true);
    assert_ne!(
        unique.postcode_with_state("perlis").unwrap(),
        unique.postcode_with_state("perlis").unwrap()
    );
}

#[test]
fn bengali_and_nepali_addresses_follow_local_number_and_address_templates() {
    let bengali = fake(Some("bn-BD")).unwrap();
    let nepali = fake(Some("ne-NP")).unwrap();
    let street = Regex::new(r"^(?:[১-৯][০-৯]?|১০০) .+$").unwrap();
    let postcode = Regex::new(r"^[0-9]{5}(?:-[0-9]{4})?$").unwrap();
    let layout = Regex::new(r"^.+-[0-9]{1,2}, .+, .+ [0-9]{5}$").unwrap();
    let mut ward_lengths = [false; 2];
    for _ in 0..128 {
        let address = bengali.address();
        let (prefix, locality) = address.split_once(", ").unwrap();
        assert!(street.is_match(prefix), "{address}");
        assert!(locality.contains(' ') && !locality.bytes().any(|value| value.is_ascii_digit()));
        assert!(postcode.is_match(&bengali.postcode()));
        assert!(layout.is_match(&nepali.address()));
        assert!(
            !nepali
                .street_address()
                .starts_with(|c: char| c.is_ascii_digit())
        );
        let ward = nepali.ward_number().unwrap();
        assert!(ward.bytes().all(|value| value.is_ascii_digit()));
        ward_lengths[ward.len() - 1] = true;
    }
    assert_eq!(ward_lengths, [true; 2]);
    assert!(bengali.ward_number().is_err());
    let unique = bengali.unique(true);
    for _ in 0..3 {
        assert!(unique.bangla_street_name().is_ok());
    }
    assert!(unique.bangla_street_name().is_err());
}

#[test]
fn arabic_addresses_keep_regional_city_and_secondary_address_formats() {
    let eg = fake(Some("ar-EG")).unwrap();
    let jo = fake(Some("ar-JO")).unwrap();
    let sa = fake(Some("ar-SA")).unwrap();
    let number = Regex::new(r"^[1-9][0-9](?:[0-9]{2,3})?$").unwrap();
    let postcode = Regex::new(r"^[0-9]{5}(?:-[0-9]{4})?$").unwrap();
    for faker in [&eg, &jo, &sa] {
        let mut formats = [false; 2];
        for _ in 0..128 {
            let address = faker.address();
            let (street, city) = address.split_once('\n').unwrap();
            let (building, road) = street.split_once(' ').unwrap();
            assert!(number.is_match(building) && !road.is_empty());
            assert!(!city.is_empty() && !city.bytes().any(|value| value.is_ascii_digit()));
            assert!(postcode.is_match(&faker.postcode()));
            formats[usize::from(road.contains("رقم"))] = true;
        }
        assert_eq!(formats, [true; 2]);
    }
    assert_eq!(jo.street_prefix().unwrap(), "شارع");
    assert!(sa.subdivision().is_ok() && jo.governorate().is_err());
    assert!(sa.governorate_id().is_err());

    let unique = eg.unique(true);
    for _ in 0..26 {
        assert_eq!(unique.governorate_id().unwrap().len(), 2);
    }
    assert!(unique.governorate_id().is_err());
}

#[test]
fn persian_and_hebrew_addresses_preserve_optional_segments_and_spaces() {
    let persian = fake(Some("fa-IR")).unwrap();
    let hebrew = fake(Some("he-IL")).unwrap();
    let hebrew_layout =
        Regex::new(r"^.+ [1-9][0-9]{0,2} (?:(?:דירה|קומה) [0-9]{2})?\n.+, (?:[0-9]{5}|[0-9]{7})$")
            .unwrap();
    let persian_zip = Regex::new(r"^[0-9]{10}$").unwrap();
    let mut persian_variants = [false; 2];
    let mut hebrew_variants = [false; 2];
    for _ in 0..128 {
        let address = persian.address();
        assert!(address.contains(" خیابان ") && address.contains(" ساختمان "));
        let has_postcode = address.contains(" کد پستی ");
        persian_variants[usize::from(has_postcode)] = true;
        if has_postcode {
            assert!(persian_zip.is_match(address.rsplit_once(' ').unwrap().1));
        }
        assert!(persian_zip.is_match(&persian.postcode()));

        let address = hebrew.address();
        assert!(hebrew_layout.is_match(&address), "{address:?}");
        let street = address.split_once('\n').unwrap().0;
        hebrew_variants[usize::from(street.ends_with(' '))] = true;
        assert!(matches!(hebrew.postcode().len(), 5 | 7));
    }
    assert_eq!(persian_variants, [true; 2]);
    assert_eq!(hebrew_variants, [true; 2]);
    assert!(hebrew.building().is_err());
    let unique = persian.unique(true);
    assert_eq!(unique.postcode_prefix().unwrap(), "کد پستی");
    assert!(unique.postcode_prefix().is_err());
}

#[test]
fn turkish_addresses_put_the_building_last_and_the_postcode_before_the_city() {
    use std::collections::HashSet;

    let faker = fake(Some("tr-TR")).unwrap();
    let building = Regex::new(r"^[1-9][0-9]{0,2}$").unwrap();
    let postcode = Regex::new(r"^[0-9]{5}$").unwrap();
    let suffixes = ["Sokak", "Caddesi", "Kavşağı", "Durağı", "İş Hanı", "Mevkii"];
    for _ in 0..128 {
        let address = faker.address();
        let (street, place) = address.split_once('\n').unwrap();
        let (name, number) = street.rsplit_once(' ').unwrap();
        assert!(building.is_match(number), "{address}");
        assert!(suffixes.iter().any(|suffix| name.ends_with(suffix)));
        let (postal, city) = place.split_once(' ').unwrap();
        assert!(postcode.is_match(postal) && !city.is_empty());
    }
    let unique = faker.unique(true);
    let cities: HashSet<_> = (0..81).map(|_| unique.city_name().unwrap()).collect();
    assert_eq!(cities.len(), 81);
    assert!(cities.contains("İstanbul") && cities.contains("Şanlıurfa"));
    assert!(unique.city_name().is_err());
}

#[test]
fn russian_ukrainian_and_kazakh_addresses_preserve_regional_layouts() {
    let ru = fake(Some("ru-RU")).unwrap();
    let ua = fake(Some("uk-UA")).unwrap();
    let kz = fake(Some("kk-KZ")).unwrap();
    let russian = Regex::new(r"^[0-9]{6}, .+ область, город .+, .+, [1-9][0-9]$").unwrap();
    let ukrainian = Regex::new(r"^[0-9]{5}, .+ область, місто .+, .+, [1-9][0-9]$").unwrap();
    let kazakh =
        Regex::new(r"^0[0-9]{5}, .+ облысы, .+ қаласы, .+ (?:көшесі|даңғылы), [1-9][0-9]{2}$")
            .unwrap();

    for _ in 0..128 {
        assert!(russian.is_match(&ru.address()));
        assert!(ukrainian.is_match(&ua.address()));
        assert!(kazakh.is_match(&kz.address()));
        assert!(ru.street().is_ok() && kz.street().is_ok());
    }
    assert_eq!(ua.street_suffix(), "");
    assert!(ru.city_and_region().is_err());
    let unique = ua.unique(true);
    let pairs: std::collections::HashSet<_> =
        (0..24).map(|_| unique.city_and_region().unwrap()).collect();
    assert_eq!(pairs.len(), 24);
    assert!(pairs.contains("Закарпатська область, місто Ужгород"));
    assert!(pairs.contains("Львівська область, місто Львів"));
    assert!(unique.city_and_region().is_err());
}

#[test]
fn polish_czech_and_slovak_addresses_preserve_punctuation_and_postcodes() {
    let pl = fake(Some("pl-PL")).unwrap();
    let cz = fake(Some("cs-CZ")).unwrap();
    let sk = fake(Some("sk-SK")).unwrap();
    let polish =
        Regex::new(r"^.+ (?:[0-9]{2}A|[1-9][0-9])(?:/[1-9][0-9])?, [0-9]{2}-[0-9]{3} .+$").unwrap();
    let slovak = Regex::new(r"^.+ [1-9][0-9]{2,4}\\n [0-9]{3} [0-9]{2}\\n .+$").unwrap();
    let czech_postcode = Regex::new(r"^(?:[0-9]{5}|[0-9]{3} [0-9]{2})$").unwrap();
    let mut czech_layouts = [false; 3];
    for _ in 0..192 {
        assert!(polish.is_match(&pl.address()));
        assert!(slovak.is_match(&sk.address()));
        assert!(czech_postcode.is_match(&cz.postcode()));
        let address = cz.address();
        let index = match address.lines().count() {
            2 => 0,
            3 if address.ends_with("\nČeská republika") => 1,
            3 => 2,
            _ => panic!("unexpected Czech address {address:?}"),
        };
        czech_layouts[index] = true;
    }
    assert_eq!(czech_layouts, [true; 3]);
    let regions = cz.unique(true);
    for _ in 0..14 {
        assert!(regions.region().is_ok());
    }
    assert!(regions.region().is_err());
}

#[test]
fn german_address_building_numbers_and_swiss_cantons_use_local_catalogs() {
    let germany = fake(Some("de-DE")).unwrap();
    let austria = fake(Some("de-AT")).unwrap();
    let swiss = fake(Some("de-CH")).unwrap();
    let german_number = Regex::new(r"^(?:[1-9][0-9]{0,2}|[1-9]/[1-9]|[1-9][0-9]?[abc])$").unwrap();
    let austrian_number = Regex::new(r"^(?:[1-9][0-9]{0,2}|[0-9]{1,2}[abc])$").unwrap();
    for _ in 0..128 {
        assert!(german_number.is_match(&germany.building_number()));
        assert!(austrian_number.is_match(&austria.building_number()));
        assert!(austrian_number.is_match(&swiss.building_number()));
        assert_eq!(germany.postcode().len(), 5);
        assert_eq!(austria.postcode().len(), 4);
        let address = swiss.address();
        let (_, place) = address.split_once('\n').unwrap();
        assert_eq!(place.split_once(' ').unwrap().0.len(), 4);
    }
    let cantons = swiss.unique(true);
    let values: std::collections::HashSet<_> = (0..26).map(|_| cantons.canton().unwrap()).collect();
    assert!(values.contains(&("ZH", "Zürich")) && values.contains(&("GE", "Genf")));
    assert!(cantons.canton().is_err());
    assert!(germany.canton().is_err());
    assert!(fake(Some("en-US")).unwrap().street_suffix_short().is_err());
}

#[test]
fn dutch_addresses_keep_country_specific_postcodes_and_whitespace() {
    let nl = fake(Some("nl-NL")).unwrap();
    let be = fake(Some("nl-BE")).unwrap();
    let nl_postcode = Regex::new(r"^[0-9]{4}[A-Z]{2}$").unwrap();
    let be_postcode = Regex::new(r"^[0-9]{4}$").unwrap();
    let nl_number = Regex::new(r"^[1-9][0-9]{0,3}(?:[a-z]|-[a-z]|-[0-9]{1,2})?$").unwrap();
    let mut alphabetic = false;
    for _ in 0..128 {
        let address = nl.address();
        let (street, place) = address.split_once('\n').unwrap();
        let number = street.rsplit_once(' ').unwrap().1;
        assert!(nl_number.is_match(number), "{address}");
        alphabetic |= number.bytes().any(|digit| digit.is_ascii_lowercase());
        assert!(nl_postcode.is_match(place.split_once(' ').unwrap().0));
        let address = be.address();
        let (_, place) = address.split_once("\n ").unwrap();
        assert!(be_postcode.is_match(place.split_once(' ').unwrap().0));
        assert!(be.city_name().is_ok());
    }
    assert!(alphabetic);
    let unique = nl.unique(true);
    for _ in 0..12 {
        assert!(!unique.state().unwrap().is_empty());
    }
    assert!(unique.state().is_err());
}

#[test]
fn norwegian_and_swedish_addresses_keep_postcode_shapes_and_uppercase_buildings() {
    let no = fake(Some("nb-NO")).unwrap();
    let se = fake(Some("sv-SE")).unwrap();
    let building = Regex::new(r"^[1-9](?:[0-9]{0,3}|[0-9]?[A-Z])$").unwrap();
    let no_postcode = Regex::new(r"^[0-9]{4}$").unwrap();
    let se_postcode = Regex::new(r"^[1-9][0-9]{2} ?[0-9]{2}$").unwrap();
    let mut se_spacing = [false; 2];
    let mut no_leading_zero = false;
    for _ in 0..128 {
        for (faker, postcode) in [(&no, &no_postcode), (&se, &se_postcode)] {
            let addr = faker.address();
            let (street, place) = addr.split_once('\n').unwrap();
            assert!(building.is_match(street.rsplit_once(' ').unwrap().1));
            let postal = faker.postcode();
            assert!(postcode.is_match(&postal));
            assert!(!place.starts_with(' '));
            if faker.locale() == "sv-SE" {
                se_spacing[usize::from(postal.contains(' '))] = true;
            } else {
                no_leading_zero |= postal.starts_with('0');
            }
        }
    }
    assert_eq!(se_spacing, [true; 2]);
    assert!(no_leading_zero);
    let unique = se.unique(true);
    for _ in 0..4 {
        assert!(unique.street_suffix_word().is_ok());
    }
    assert!(unique.street_suffix_word().is_err());
    assert!(fake(Some("en-US")).unwrap().street_suffix_word().is_err());
}

#[test]
fn danish_finnish_and_icelandic_addresses_keep_local_formatters() {
    let dk = fake(Some("da-DK")).unwrap();
    let fi = fake(Some("fi-FI")).unwrap();
    let is = fake(Some("is-IS")).unwrap();
    let danish_level = Regex::new(r"^(?:st\.|[1-9]\.(?: sal\.)?)$").unwrap();
    let finnish_unit = Regex::new(r"^[0-9]{3}$").unwrap();
    let icelandic = Regex::new(r"^.+ [1-9](?:[0-9]{0,2}| ?[A-Z])\n[1-9][0-9]{2} .+$").unwrap();
    let mut danish_layouts = [false; 3];
    let mut finnish_regions = [false; 2];
    for _ in 0..128 {
        let address = dk.address();
        let (street, _) = address.split_once('\n').unwrap();
        let layout = match street.split_once(", ") {
            None => 0,
            Some((_, suffix)) if danish_level.is_match(suffix) => 1,
            Some((_, suffix)) => {
                let (level, side) = suffix.rsplit_once(' ').unwrap();
                assert!(danish_level.is_match(level));
                assert!(["tv.", "mf.", "th."].contains(&side));
                2
            }
        };
        danish_layouts[layout] = true;
        assert!(danish_level.is_match(&dk.building_level().unwrap()));
        assert!(dk.kommune().is_ok());
        let address = fi.address();
        finnish_regions[usize::from(address.split_once('\n').unwrap().1.contains(", "))] = true;
        assert!(finnish_unit.is_match(&fi.secondary_address()));
        assert!(icelandic.is_match(&is.address()));
    }
    assert_eq!(danish_layouts, [true; 3]);
    assert_eq!(finnish_regions, [true; 2]);
    let unique = dk.unique(true);
    for _ in 0..3 {
        assert!(unique.building_side().is_ok());
    }
    assert!(unique.building_side().is_err());
    assert!(fi.kommune().is_err());
}

#[test]
fn spanish_address_providers_keep_inherited_fields_and_local_layouts() {
    let es = fake(Some("es-ES")).unwrap();
    let ar = fake(Some("es-AR")).unwrap();
    let pe = fake(Some("es-PE")).unwrap();
    let ve = fake(Some("es-VE")).unwrap();
    let es_unit =
        Regex::new(r"^(?:Bajos|(?:Ático |Entre suelo |Bajo )?[0-9]{1,2}º(?: [A-F]| [0-9]º)?)$")
            .unwrap();
    let ar_address = Regex::new(r"^.+\n.+, AR-[A-Z] [0-9]{4,5}$").unwrap();
    let ve_address = Regex::new(r"^.+, .+ Edo\. [^,]+(?:, [0-9]{4})?$").unwrap();
    let mut peru_layouts = [false; 2];
    let mut venezuela_layouts = [false; 2];
    for _ in 0..128 {
        let address = es.address();
        let fields: Vec<_> = address.split(", ").collect();
        assert_eq!(fields.len(), 5);
        assert!(es_unit.is_match(fields[2]));
        assert_eq!(fields[3].len(), 5);
        assert!(fields[3].bytes().all(|byte| byte.is_ascii_digit()));
        assert!(ar_address.is_match(&ar.address()));
        assert!(ar.street_suffix().is_empty());
        let street = pe.street_address();
        assert!(street.contains(" # "));
        peru_layouts[usize::from(street.ends_with(' '))] = true;
        assert!(pe.postcode().starts_with("LIMA "));
        let address = ve.address();
        assert!(ve_address.is_match(&address));
        venezuela_layouts[usize::from(address.rsplit_once(", ").unwrap().1.len() == 4)] = true;
    }
    assert_eq!(peru_layouts, [true; 2]);
    assert_eq!(venezuela_layouts, [true; 2]);
    for faker in [&es, &ar, &pe, &ve] {
        assert!(faker.street_prefix().is_ok());
        let unique = faker.unique(true);
        for _ in 0..19 {
            assert!(unique.community().is_ok());
        }
        assert!(unique.community().is_err());
    }
    assert!(fake(Some("en-US")).unwrap().community().is_err());
}

#[test]
fn portuguese_address_providers_keep_local_postcodes_and_regions() {
    let br = fake(Some("pt-BR")).unwrap();
    let pt = fake(Some("pt-PT")).unwrap();
    let brazil = Regex::new(
        r"^[0-9]{5}-[0-9]{3}, [^\n]+, [1-9][0-9]{0,4}(?:\. [^\n]+)?\n[^\n]+ - [A-Z]{2}$",
    )
    .unwrap();
    let portugal = Regex::new(r"^[1-9][0-9]{2}[01459](?:-[0-9]{3})?$").unwrap();
    let mut portugal_postcodes = [false; 2];
    let mut portugal_streets = [false; 3];
    for _ in 0..128 {
        assert!(brazil.is_match(&br.address()));
        let postcode = pt.postcode();
        assert!(portugal.is_match(&postcode));
        portugal_postcodes[usize::from(postcode.contains('-'))] = true;
        let street = pt.street_address();
        portugal_streets[street.matches(", ").count()] = true;
    }
    assert_eq!(portugal_postcodes, [true; 2]);
    assert_eq!(portugal_streets, [true; 3]);
    let abbreviations = ["N", "NE", "CO", "SE", "S"];
    let unique = br.unique(true);
    for _ in abbreviations {
        assert!(abbreviations.contains(&unique.region_abbr().unwrap()));
    }
    assert!(unique.region_abbr().is_err());
    assert!(pt.region_abbr().is_err());
}

#[test]
fn italian_address_providers_keep_their_own_layouts_and_swiss_cantons() {
    let italy = fake(Some("it-IT")).unwrap();
    let swiss = fake(Some("it-CH")).unwrap();
    let italian_address = Regex::new(r"^[^\n]+\n[^\n]+, [0-9]{5} .+ \([A-Z]{2}\)$").unwrap();
    let swiss_address =
        Regex::new(r"^[^\n]+ (?:[1-9][0-9]{0,2}|[0-9][abc])\n[0-9]{4} .+$").unwrap();
    let swiss_letter = Regex::new(r"^[0-9][abc]$").unwrap();
    let mut italian_layouts = [false; 2];
    let mut swiss_letter_seen = false;
    for _ in 0..128 {
        assert!(italian_address.is_match(&italy.address()));
        let street = italy.street_address();
        italian_layouts
            [usize::from(street.contains(" Appartamento ") || street.contains(" Piano "))] = true;
        assert!(swiss_address.is_match(&swiss.address()));
        swiss_letter_seen |= swiss_letter.is_match(&swiss.building_number());
    }
    assert_eq!(italian_layouts, [true; 2]);
    assert!(swiss_letter_seen);
    let unique = swiss.unique(true);
    let pairs = (0..26)
        .map(|_| unique.canton().unwrap())
        .collect::<Vec<_>>();
    assert!(pairs.contains(&("TI", "Ticino")));
    assert!(pairs.contains(&("GE", "Ginevra")));
    assert!(unique.canton().is_err());
    assert!(italy.canton().is_err());
}

#[test]
fn greek_addresses_keep_regional_postcodes_and_prefectures() {
    let greece = fake(Some("el-GR")).unwrap();
    let cyprus = fake(Some("el-CY")).unwrap();
    let greek_address = Regex::new(
        r"^(?:Όδος|Λεωφόρος) [^,]+, [1-9][0-9]{0,2}(?:-[0-9]{1,3})?, [0-9]{3} ?[0-9]{2}, .+$",
    )
    .unwrap();
    let cypriot_address = Regex::new(
        r"^(?:Όδος|Οδός|Λεωφόρος) [^,]+, [1-9][0-9]? [0-9]{4} (?:Λευκωσία|Λεμεσός|Λάρνακα|Πάφος)$",
    )
    .unwrap();
    let mut greek_postcodes = [false; 2];
    let mut greek_ranges = [false; 2];
    for _ in 0..128 {
        assert!(greek_address.is_match(&greece.address()));
        assert!(cypriot_address.is_match(&cyprus.address()));
        greek_postcodes[usize::from(greece.postcode().contains(' '))] = true;
        greek_ranges[usize::from(greece.building_number().contains('-'))] = true;
    }
    assert_eq!(greek_postcodes, [true; 2]);
    assert_eq!(greek_ranges, [true; 2]);
    assert!(greece.unique(true).prefecture().is_ok());
    assert!(cyprus.prefecture().is_err());
    assert!(cyprus.street_suffix().is_empty());
}

#[test]
fn romanian_addresses_keep_weighted_layouts_and_local_postcodes() {
    let romanian = fake(Some("ro-RO")).unwrap();
    let moldovan = fake(Some("ro-MD")).unwrap();
    let floor = Regex::new(r"^[0-9]{1,2}$").unwrap();
    for (faker, postal_length) in [(&romanian, 6), (&moldovan, 4)] {
        let mut layouts_seen = [false; 4];
        for _ in 0..128 {
            let address = faker.address();
            let (street_and_city, postcode) = address.rsplit_once(", CP ").unwrap();
            assert_eq!(postcode.len(), postal_length);
            assert!(postcode.bytes().all(|byte| byte.is_ascii_digit()));
            assert_eq!(
                street_and_city.contains(", bl. "),
                address.contains(" nr. ")
            );
            let street = faker.street_address();
            let layout = if street.contains(" sc. ") {
                3
            } else if street.contains(" et. ") {
                2
            } else {
                usize::from(street.contains(" bl. "))
            };
            layouts_seen[layout] = true;
            assert!(floor.is_match(faker.floor().unwrap()));
            assert!(floor.is_match(faker.apartment_number().unwrap()));
        }
        assert_eq!(layouts_seen, [true; 4]);
        let unique = faker.unique(true);
        for _ in 0..4 {
            assert!(["A", "B", "C", "D"].contains(&unique.block_segment().unwrap()));
        }
        assert!(unique.block_segment().is_err());
    }
    assert!(romanian.county().is_ok());
    assert!(moldovan.county().is_err());
    let singapore = fake(Some("en-SG")).unwrap();
    assert!(singapore.apartment_number().is_ok());
    assert!(singapore.floor().is_err());
}

#[test]
fn croatian_and_slovenian_addresses_keep_local_layouts_and_postcodes() {
    let croatian = fake(Some("hr-HR")).unwrap();
    let slovenian = fake(Some("sl-SI")).unwrap();
    let croatian_address = Regex::new(r"^.+ [1-9][0-9], [0-9]{5} .+$").unwrap();
    let slovenian_address = Regex::new(r"^.+ [1-9][0-9]{0,2}\\n [0-9]{3}0\\n .+$").unwrap();
    for _ in 0..128 {
        assert!(croatian_address.is_match(&croatian.address()));
        let address = slovenian.address();
        assert!(slovenian_address.is_match(&address));
        assert!(!address.contains('\n'));
    }
    assert!(croatian.unique(true).street().is_ok());
    assert!(slovenian.street().is_err());
    assert!(slovenian.unique(true).city_name().is_ok());
}

#[test]
fn lithuanian_and_latvian_addresses_keep_local_postcodes_and_region_fields() {
    let lithuanian = fake(Some("lt-LT")).unwrap();
    let latvian = fake(Some("lv-LV")).unwrap();
    let latvian_address =
        Regex::new(r"^LV-[0-9]{4}, .+ reģions, .+ pilsēta, .+, [1-9][0-9]$").unwrap();
    let mut layouts = [false; 3];
    for _ in 0..128 {
        let address = lithuanian.address();
        let (street, city) = address.split_once(", ").unwrap();
        let layout = if city.contains(" LT-") {
            2
        } else {
            usize::from(street.contains('-'))
        };
        layouts[layout] = true;
        assert!(latvian_address.is_match(&latvian.address()));
        assert_eq!(lithuanian.region_suffix().unwrap(), "regionas");
        assert_eq!(latvian.region_suffix().unwrap(), "reģions");
    }
    assert_eq!(layouts, [true; 3]);
    assert!(lithuanian.unique(true).municipality().is_ok());
    assert!(latvian.municipality().is_err());
    assert!(fake(Some("en-PH")).unwrap().municipality().is_ok());
    let unique = latvian.unique(true);
    assert_eq!(unique.region_suffix().unwrap(), "reģions");
    assert!(unique.region_suffix().is_err());
}

#[test]
fn armenian_and_georgian_addresses_keep_local_layouts_and_region_forms() {
    let armenian = fake(Some("hy-AM")).unwrap();
    let georgian = fake(Some("ka-GE")).unwrap();
    let armenian_address =
        Regex::new(r"^.+ մարզ, (?:քաղաք|գյուղ) .+, .+ [1-9][0-9] փողոց, 00[0-9]{2}$").unwrap();
    let georgian_street = Regex::new(r"^.+ #[1-9][0-9]$").unwrap();
    let mut georgian_layouts = std::collections::HashSet::new();
    for _ in 0..256 {
        assert!(armenian_address.is_match(&armenian.address()));
        assert!(georgian_street.is_match(&georgian.street_address()));
        let address = georgian.address();
        georgian_layouts.insert((
            address.starts_with("საქართველო, "),
            address.ends_with(", საქართველო"),
            address.contains("რაიონი"),
            address
                .split(", ")
                .find(|part| *part != "საქართველო")
                .unwrap()
                .contains(" #"),
        ));
    }
    assert_eq!(georgian_layouts.len(), 8);
    let unique = georgian.unique(true);
    for _ in 0..11 {
        assert!(unique.region_genitive_form().is_ok());
    }
    assert!(unique.region_genitive_form().is_err());
    assert!(armenian.region_genitive_form().is_err());
}

#[test]
fn serbian_address_scripts_and_montenegrin_spacing_match_their_providers() {
    let serbian = fake(Some("sr-RS")).unwrap();
    let cyrillic = fake(Some("sr-Cyrl-RS")).unwrap();
    let latin = fake(Some("sr-Latn-RS")).unwrap();
    let montenegrin = fake(Some("me-ME")).unwrap();
    let cyrillic_city = Regex::new(r"^[\p{Cyrillic} -]+$").unwrap();
    let latin_city = Regex::new(r"^[\p{Latin} -]+$").unwrap();
    let address = Regex::new(r"^.+ [1-9][0-9] [0-9]{5} .+$").unwrap();
    for (faker, script, prefix) in [
        (&serbian, &cyrillic_city, "Булевар"),
        (&cyrillic, &cyrillic_city, "Булевар"),
        (&latin, &latin_city, "Bulevar"),
    ] {
        assert_eq!(faker.street_prefix().unwrap(), prefix);
        for _ in 0..32 {
            assert!(script.is_match(faker.city_name().unwrap()));
            assert!(address.is_match(&faker.address()));
        }
    }
    assert!(montenegrin.street_prefix().unwrap().is_empty());
    let mut leading_spaces = [false; 2];
    for _ in 0..128 {
        leading_spaces[usize::from(montenegrin.street_name().starts_with(' '))] = true;
        assert!(address.is_match(&montenegrin.address()));
    }
    assert_eq!(leading_spaces, [true; 2]);
    let coordinates = montenegrin.local_coordinates().unwrap();
    assert!((42.43..=42.45).contains(&coordinates.latitude));
    assert!((19.16..=19.27).contains(&coordinates.longitude));
}

#[test]
fn hungarian_addresses_preserve_city_tiers_and_secondary_addresses() {
    let hungarian = fake(Some("hu-HU")).unwrap();
    let address =
        Regex::new(r"^[0-9]{4} .+, .+ [1-9][0-9]{0,2}\.(?: [0-9]{2}\. (?:emelet|ajtó))?$").unwrap();
    let mut layouts = [false; 2];
    for _ in 0..128 {
        assert!(address.is_match(&hungarian.address()));
        let street_address = hungarian.street_address();
        layouts
            [usize::from(street_address.ends_with("emelet") || street_address.ends_with("ajtó"))] =
            true;
    }
    assert_eq!(layouts, [true; 2]);
    let unique = hungarian.unique(true);
    assert_eq!(unique.capital().unwrap(), "Budapest");
    assert!(unique.capital().is_err());
    assert!(hungarian.big_city().is_ok());
    assert!(hungarian.smaller_city().is_ok());
}

#[test]
fn absent_address_provider_uses_us_layout_without_switching_person_locale() {
    let address = Regex::new(r"^[1-9][0-9]{2,4} .+\n.+, [A-Z]{2} [0-9]{5}(?:-[0-9]{4})?$").unwrap();
    let cyrillic = Regex::new(r"\p{Cyrillic}").unwrap();
    for locale in ["bg-BG", "cy-GB", "zz-ZZ"] {
        let faker = fake(Some(locale)).unwrap();
        let mut postcode_forms = [false; 2];
        for _ in 0..128 {
            let generated = faker.address();
            assert!(address.is_match(&generated));
            postcode_forms[usize::from(faker.postcode().contains('-'))] = true;
            if locale == "bg-BG" {
                assert!(cyrillic.is_match(&generated));
            }
        }
        assert_eq!(postcode_forms, [true; 2]);
        assert!(faker.capital().is_err());
    }
}

#[test]
fn malaysian_jpj_plates_preserve_empty_letters_and_special_prefix_case() {
    let faker = fake(Some("ms-MY")).unwrap();
    let plate = Regex::new(r"^[A-Za-z0-9]+ [1-9][0-9]{0,3}(?: [A-HJ-NP-Y]?)?$").unwrap();
    let mut trailing_space = false;
    for _ in 0..1024 {
        let value = faker.jpj_number_plate().unwrap();
        assert!(plate.is_match(&value), "{value:?}");
        trailing_space |= value.ends_with(' ');
        assert!((1..=9999).contains(&faker.number_sequence().unwrap()));
    }
    assert!(trailing_space);
    let unique = faker.unique(true);
    let alphabets = (0..24)
        .map(|_| unique.valid_alphabet().unwrap())
        .collect::<Vec<_>>();
    assert!(alphabets.contains(&""));
    assert!(!alphabets.contains(&"I") && !alphabets.contains(&"O"));
    assert!(unique.valid_alphabet().is_err());
    let prefixes = (0..41)
        .map(|_| unique.special_prefix().unwrap())
        .collect::<Vec<_>>();
    assert!(prefixes.contains(&"BAMbee"));
    assert!(fake(Some("en-US")).unwrap().jpj_number_plate().is_err());
}

#[test]
fn polish_license_plate_filters_use_the_selected_region_and_fall_back() {
    let faker = fake(Some("pl-PL")).unwrap();
    let wroclaw = Regex::new(r"^[DV][WX] [0-9A-PR-Z][0-9A-PR-Z]{4}$").unwrap();
    let county = Regex::new(r"^[DV]BL [0-9A-PR-Z]{4,5}$").unwrap();
    let police = Regex::new(r"^HP[ABCDEFGHJKLMNPSTUWZ] [0-9A-PR-Z]{4,5}$").unwrap();
    let unique = faker.unique(true);
    for _ in 0..128 {
        assert!(
            wroclaw.is_match(
                &unique
                    .license_plate_with_options(
                        false,
                        Some(&["unknown", "dolnośląskie"]),
                        Some(&["Wrocław", "unknown"]),
                    )
                    .unwrap(),
            )
        );
        assert!(
            county.is_match(
                &faker
                    .license_plate_with_options(
                        false,
                        Some(&["dolnośląskie"]),
                        Some(&["bolesławiecki"])
                    )
                    .unwrap(),
            )
        );
        assert!(
            police.is_match(
                &faker
                    .license_plate_with_options(true, Some(&["services"]), Some(&["Policja"]))
                    .unwrap(),
            )
        );
        let army = faker
            .license_plate_with_options(true, Some(&["army"]), None)
            .unwrap();
        assert!(army.starts_with('U'));
        let fallback = faker
            .license_plate_with_options(false, Some(&["army"]), Some(&[]))
            .unwrap();
        assert!(!fallback.starts_with(['H', 'U']));
        assert!(!faker.license_plate().unwrap().starts_with(['H', 'U']));
    }
    let english = fake(Some("en-US")).unwrap();
    assert!(english.license_plate().is_err());
    assert!(
        english
            .unique(true)
            .license_plate_with_options(true, None, None)
            .is_err()
    );
}

#[test]
fn date_ranges_floor_fractional_seconds_before_the_unix_epoch() {
    let faker = fake(Some("en-US")).unwrap();
    let instant: jiff::Timestamp = "1969-12-31T23:59:59.500Z".parse().unwrap();
    let expected: jiff::Timestamp = "1969-12-31T23:59:59Z".parse().unwrap();
    assert_eq!(faker.date_time_between(instant, instant).unwrap(), expected);
    let unique = faker.unique_with_retries(true, 2);
    assert_eq!(
        unique.date_time_between(instant, instant).unwrap(),
        expected
    );
    assert!(unique.date_time_between(expected, expected).is_err());
}
