use chrono::{Datelike as _, NaiveDate};

use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker, luhn_check_digit};

const TAIWAN_BIRTHPLACES: &[(char, u32)] = &[
    ('A', 10),
    ('B', 11),
    ('C', 12),
    ('D', 13),
    ('E', 14),
    ('F', 15),
    ('G', 16),
    ('H', 17),
    ('I', 34),
    ('J', 18),
    ('K', 19),
    ('M', 21),
    ('N', 22),
    ('O', 35),
    ('P', 23),
    ('Q', 24),
    ('T', 27),
    ('U', 28),
    ('V', 29),
    ('W', 32),
    ('X', 30),
    ('Z', 33),
];

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    /// Samples the selected Person provider's SSN format.
    pub fn ssn(&self) -> DatabaseResult<String> {
        match self.locale() {
            "en_US" | "th_TH" => Ok(us_ssn()),
            "de_AT" => self.ssnWithBirthdate(None),
            "is_IS" => Ok(icelandic_ssn(self.identity_birthdate()?)),
            _ => Err(DatabaseError::InvalidQuery("unsupported Faker SSN locale")),
        }
    }

    /// Samples the Austrian SSN using the supplied civil birth date, if any.
    pub fn ssnWithBirthdate(&self, birthdate: Option<NaiveDate>) -> DatabaseResult<String> {
        if self.locale() != "de_AT" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker SSN birthdate locale",
            ));
        }
        let birthdate = match birthdate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        Ok(austrian_ssn(birthdate))
    }

    fn identity_birthdate(&self) -> DatabaseResult<NaiveDate> {
        let timezone = self
            .getDefaultTimezone()
            .unwrap_or(self.application_timezone);
        Ok(self
            .dateTimeThisCentury(None)?
            .with_timezone(&timezone)
            .date_naive())
    }

    /// Samples the British Person provider's National Insurance number format.
    pub fn nino(&self) -> DatabaseResult<String> {
        if self.locale() != "en_GB" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker NINO locale"));
        }
        const LETTERS: &[u8] = b"ABCEGHJKLMNOPRSTWXYZ";
        let prefix = loop {
            let prefix = [
                LETTERS[fastrand::usize(..LETTERS.len())],
                LETTERS[fastrand::usize(..LETTERS.len())],
            ];
            if prefix[1] != b'O'
                && ![*b"BG", *b"GB", *b"KN", *b"NK", *b"NT", *b"TN", *b"ZZ"].contains(&prefix)
            {
                break prefix.map(char::from);
            }
        };
        let [first, second] = prefix;
        let digits = fastrand::u32(..1_000_000);
        let suffix = char::from(fastrand::u8(b'A'..=b'D'));
        Ok(format!("{first}{second}{digits:06}{suffix}"))
    }

    /// Samples a DNI for Spain or Peru.
    pub fn dni(&self) -> DatabaseResult<String> {
        match self.locale() {
            "es_ES" => {
                const LETTERS: &[u8; 23] = b"TRWAGMYFPDXBNJZSQVHLCKE";
                let number = fastrand::usize(..100_000_000);
                let check = char::from(LETTERS[number % LETTERS.len()]);
                Ok(format!("{number:08}{check}"))
            }
            "es_PE" => Ok(self.numerify(Some("########"))),
            _ => Err(DatabaseError::InvalidQuery("unsupported Faker DNI locale")),
        }
    }

    /// Samples the pinned Peruvian RUC format.
    pub fn ruc(&self, isNaturalPerson: Option<bool>) -> DatabaseResult<String> {
        if self.locale() != "es_PE" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker RUC locale"));
        }
        let mask = if isNaturalPerson.unwrap_or(false) {
            "10#########"
        } else {
            "20#########"
        };
        Ok(self.numerify(Some(mask)))
    }

    /// Samples a driving licence category for Spain or South Africa.
    pub fn licenceCode(&self) -> DatabaseResult<&'static str> {
        let codes: &[&str] = match self.locale() {
            "es_ES" => &[
                "AM", "A1", "A2", "A", "B", "B+E", "C1", "C1+E", "C", "C+E", "D1", "D1+E", "D",
                "D+E",
            ],
            "en_ZA" => &[
                "A", "A1", "B", "C", "C1", "C2", "EB", "EC", "EC1", "I", "L", "L1",
            ],
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker licence code locale",
                ));
            }
        };
        Ok(codes[fastrand::usize(..codes.len())])
    }

    /// Samples a Brazilian CPF, with punctuation by default.
    pub fn cpf(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        let number = self.brazilian_identifier("#########", 2)?;
        if formatted.unwrap_or(true) {
            return Ok(format!(
                "{}.{}.{}-{}",
                &number[..3],
                &number[3..6],
                &number[6..9],
                &number[9..]
            ));
        }
        Ok(number)
    }

    /// Samples an RG using the pinned São Paulo provider.
    pub fn rg(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        let number = self.brazilian_identifier("########", 1)?;
        if formatted.unwrap_or(true) {
            return Ok(format!(
                "{}.{}.{}-{}",
                &number[..2],
                &number[2..5],
                &number[5..8],
                &number[8..]
            ));
        }
        Ok(number)
    }

    /// Samples a Brazilian CNPJ for the default branch (0001).
    pub fn cnpj(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        let number = self.brazilian_identifier("########0001", 2)?;
        if formatted.unwrap_or(true) {
            return Ok(format!(
                "{}.{}.{}/{}-{}",
                &number[..2],
                &number[2..5],
                &number[5..8],
                &number[8..12],
                &number[12..]
            ));
        }
        Ok(number)
    }

    fn brazilian_identifier(&self, mask: &str, check_digits: usize) -> DatabaseResult<String> {
        if self.locale() != "pt_BR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Brazilian identity locale",
            ));
        }
        let mut number = self.numerify(Some(mask));
        for _ in 0..check_digits {
            number.push(brazilian_check_digit(&number));
        }
        Ok(number)
    }

    /// Samples a Polish PESEL, using a generated birth date when omitted.
    pub fn pesel(&self, birthdate: Option<NaiveDate>, sex: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "pl_PL" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker PESEL locale",
            ));
        }
        let birthdate = match birthdate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        let year = birthdate.year();
        if !(1800..=2299).contains(&year) {
            return Err(DatabaseError::InvalidQuery(
                "Faker PESEL year must be between 1800 and 2299",
            ));
        }

        let month = birthdate.month() + ((year as u32 / 100 - 14) % 5) * 20;
        let day = birthdate.day();
        let serial = fastrand::u16(0..=4999) * 2 + u16::from(sex != Some("F"));
        let prefix = format!("{:02}{month:02}{day:02}{serial:04}", year % 100);
        let sum = weighted_digits(&prefix, &[1, 3, 7, 9, 1, 3, 7, 9, 1, 3]);
        let check = (10 - sum % 10) % 10;
        Ok(format!("{prefix}{check}"))
    }

    /// Samples a Polish tax identification number (NIP).
    pub fn taxpayerIdentificationNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "pl_PL" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker taxpayer identification locale",
            ));
        }
        loop {
            let prefix = self.numerify(Some("%%%######"));
            let check = weighted_digits(&prefix, &[6, 5, 7, 2, 3, 4, 5, 6, 7]) % 11;
            if check < 10 {
                return Ok(format!("{prefix}{check}"));
            }
        }
    }

    /// Samples the selected Person provider's personal identity number.
    pub fn personalIdentityNumber(&self, gender: Option<&str>) -> DatabaseResult<String> {
        match self.locale() {
            "zh_TW" => Ok(self.taiwanese_identity(gender)),
            "pl_PL" => Ok(polish_identity()),
            _ => self.personalIdentityNumberWithBirthdate(None, gender),
        }
    }

    /// Samples a European personal identity number using a civil birth date.
    pub fn personalIdentityNumberWithBirthdate(
        &self,
        birthdate: Option<NaiveDate>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        let locale = self.locale();
        if !matches!(locale, "lt_LT" | "lv_LV" | "fi_FI" | "nb_NO" | "sv_SE") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker dated personal identity locale",
            ));
        }
        let birthdate = match birthdate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        match locale {
            "lt_LT" => lithuanian_identity(birthdate, gender, None),
            "lv_LV" => Ok(latvian_identity(birthdate)),
            "fi_FI" => finnish_identity(birthdate, gender),
            "nb_NO" => Ok(norwegian_identity(birthdate, gender)),
            _ => Ok(swedish_identity(birthdate, gender)),
        }
    }

    /// Supplies the optional Lithuanian serial while preserving leading zeroes.
    pub fn personalIdentityNumberWithSerial(
        &self,
        birthdate: Option<NaiveDate>,
        gender: Option<&str>,
        serial: Option<&str>,
    ) -> DatabaseResult<String> {
        if self.locale() != "lt_LT" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker personal identity serial locale",
            ));
        }
        let birthdate = match birthdate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        lithuanian_identity(birthdate, gender, serial)
    }

    fn taiwanese_identity(&self, gender: Option<&str>) -> String {
        let &(birthplace, code) = self.randomElement(TAIWAN_BIRTHPLACES).unwrap();
        let gender = match gender.filter(|value| !value.is_empty()) {
            Some("male") => 1,
            Some(_) => 2,
            None => fastrand::u32(1..=2),
        };
        let serial = fastrand::u32(1_000_000..=9_999_999);
        let mut remaining = serial;
        let mut total = code / 10 + 9 * (code % 10) + 8 * gender;
        for weight in 1..=7 {
            total += weight * (remaining % 10);
            remaining /= 10;
        }
        let check = (10 - total % 10) % 10;
        format!("{birthplace}{gender}{serial}{check}")
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn ssn(&self) -> DatabaseResult<String> {
        self.try_generate("ssn", || self.0.ssn())
    }

    pub fn ssnWithBirthdate(&self, birthdate: Option<NaiveDate>) -> DatabaseResult<String> {
        self.try_generate("ssn", || self.0.ssnWithBirthdate(birthdate))
    }

    pub fn nino(&self) -> DatabaseResult<String> {
        self.try_generate("nino", || self.0.nino())
    }

    pub fn dni(&self) -> DatabaseResult<String> {
        self.try_generate("dni", || self.0.dni())
    }

    pub fn ruc(&self, isNaturalPerson: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("ruc", || self.0.ruc(isNaturalPerson))
    }

    pub fn licenceCode(&self) -> DatabaseResult<&'static str> {
        self.try_generate("licenceCode", || self.0.licenceCode())
    }

    pub fn cpf(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("cpf", || self.0.cpf(formatted))
    }

    pub fn rg(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("rg", || self.0.rg(formatted))
    }

    pub fn cnpj(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("cnpj", || self.0.cnpj(formatted))
    }

    pub fn pesel(&self, birthdate: Option<NaiveDate>, sex: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("pesel", || self.0.pesel(birthdate, sex))
    }

    pub fn taxpayerIdentificationNumber(&self) -> DatabaseResult<String> {
        self.try_generate("taxpayerIdentificationNumber", || {
            self.0.taxpayerIdentificationNumber()
        })
    }

    pub fn personalIdentityNumber(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("personalIdentityNumber", || {
            self.0.personalIdentityNumber(gender)
        })
    }
    pub fn personalIdentityNumberWithBirthdate(
        &self,
        birthdate: Option<NaiveDate>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("personalIdentityNumber", || {
            self.0
                .personalIdentityNumberWithBirthdate(birthdate, gender)
        })
    }

    pub fn personalIdentityNumberWithSerial(
        &self,
        birthdate: Option<NaiveDate>,
        gender: Option<&str>,
        serial: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("personalIdentityNumber", || {
            self.0
                .personalIdentityNumberWithSerial(birthdate, gender, serial)
        })
    }
}

fn us_ssn() -> String {
    let area = if fastrand::bool() {
        fastrand::u16(1..=665)
    } else {
        fastrand::u16(667..=899)
    };
    let group = fastrand::u8(1..=99);
    let serial = fastrand::u16(1..=9999);
    format!("{area:03}-{group:02}-{serial:04}")
}

fn austrian_ssn(birthdate: NaiveDate) -> String {
    let date = birthdate.format("%d%m%y").to_string();
    let date_sum = weighted_digits(&date, &[5, 8, 4, 2, 1, 6]);
    loop {
        let serial = fastrand::u16(100..=999).to_string();
        let check = (date_sum + weighted_digits(&serial, &[3, 7, 9])) % 11;
        if check < 10 {
            return format!("{serial}{check}{date}");
        }
    }
}

fn icelandic_ssn(birthdate: NaiveDate) -> String {
    let formatted_year = birthdate.format("%Y").to_string();
    let century_digit = formatted_year.as_bytes()[1] as char;
    let date = birthdate.format("%d%m%y").to_string();
    let date_sum = weighted_digits(&date, &[3, 2, 7, 6, 5, 4]);
    loop {
        let serial = fastrand::u8(0..=99);
        let sum = date_sum + 3 * u16::from(serial / 10) + 2 * u16::from(serial % 10);
        let check = (11 - sum % 11) % 11;
        if check < 10 {
            return format!("{date}-{serial:02}{check}{century_digit}");
        }
    }
}

fn weighted_digits(value: &str, weights: &[u16]) -> u16 {
    value
        .bytes()
        .zip(weights)
        .map(|(digit, weight)| u16::from(digit - b'0') * weight)
        .sum()
}

fn lithuanian_identity(
    birthdate: NaiveDate,
    gender: Option<&str>,
    serial: Option<&str>,
) -> DatabaseResult<String> {
    let first =
        birthdate.year().div_euclid(100) * 2 - 34 - i32::from(gender.unwrap_or("male") == "male");
    if !(1..=9).contains(&first) {
        return Err(DatabaseError::InvalidQuery(
            "Faker Lithuanian personal identity century is out of range",
        ));
    }
    let serial = match serial {
        Some(serial) if serial.len() >= 3 => {
            let digits = serial
                .get(..3)
                .filter(|value| value.bytes().all(|digit| digit.is_ascii_digit()))
                .ok_or(DatabaseError::InvalidQuery(
                    "Faker Lithuanian personal identity serial must start with three digits",
                ))?;
            digits.to_owned()
        }
        _ => format!("{:03}", fastrand::u16(0..=999)),
    };
    let prefix = format!("{first}{}{serial}", birthdate.format("%y%m%d"));
    let mut check = weighted_digits(&prefix, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 1]) % 11;
    if check == 10 {
        check = (weighted_digits(&prefix, &[3, 4, 5, 6, 7, 8, 9, 1, 2, 3]) % 11) % 10;
    }
    Ok(format!("{prefix}{check}"))
}

fn latvian_identity(birthdate: NaiveDate) -> String {
    let century = match birthdate.year() {
        2000..=2099 => 2,
        1900..=1999 => 1,
        _ => 0,
    };
    let date = birthdate.format("%d%m%y").to_string();
    let serial = format!("{century}{:03}", fastrand::u16(0..=999));
    let total =
        weighted_digits(&date, &[1, 6, 3, 7, 9, 10]) + weighted_digits(&serial, &[5, 8, 4, 2]);
    let check = (1101 - total) % 11 % 10;
    format!("{date}-{serial}{check}")
}

fn finnish_identity(birthdate: NaiveDate, gender: Option<&str>) -> DatabaseResult<String> {
    let century = match birthdate.year() {
        1800..=1899 => '+',
        1900..=1999 => '-',
        2000..=2099 => 'A',
        _ => {
            return Err(DatabaseError::InvalidQuery(
                "Faker Finnish personal identity year must be between 1800 and 2099",
            ));
        }
    };
    let date = birthdate.format("%d%m%y").to_string();
    let base = fastrand::u16(0..=89) * 10;
    let tail = if base == 0 {
        loop {
            let digit = gender_digit(gender);
            if digit >= 2 {
                break digit;
            }
        }
    } else {
        gender_digit(gender)
    };
    let serial = base + tail;
    let checksum_chars = b"0123456789ABCDEFHJKLMNPRSTUVWXY";
    let number = date.parse::<usize>().unwrap() * 1000 + usize::from(serial);
    let check = char::from(checksum_chars[number % checksum_chars.len()]);
    Ok(format!("{date}{century}{serial:03}{check}"))
}

fn norwegian_identity(birthdate: NaiveDate, gender: Option<&str>) -> String {
    let date = birthdate.format("%d%m%y");
    let serial = fastrand::u8(0..=99);
    let gender = gender_digit(gender);
    // The pinned provider uses two random digits here.
    let trailing = fastrand::u8(0..=99);
    format!("{date}{serial:02}{gender}{trailing:02}")
}

fn swedish_identity(birthdate: NaiveDate, gender: Option<&str>) -> String {
    let serial = loop {
        let candidate = fastrand::u16(0..=99) * 10 + gender_digit(gender);
        if candidate != 0 {
            break candidate;
        }
    };
    let date = birthdate.format("%y%m%d");
    let check = luhn_check_digit(&format!("{date}{serial:03}"));
    format!("{date}-{serial:03}{check}")
}

fn polish_identity() -> String {
    const LETTERS: &[u8] = b"ABCDEFGHIJKLMNPRSTUVWXYZ";
    let letters = [
        b'A',
        LETTERS[fastrand::usize(..LETTERS.len())],
        LETTERS[fastrand::usize(..LETTERS.len())],
    ];
    let serial = format!("{:05}", fastrand::u32(0..=99_999));
    let total: u16 = letters
        .into_iter()
        .zip([7, 3, 1])
        .map(|(letter, weight)| u16::from(letter - 55) * weight)
        .sum();
    let check = (total + weighted_digits(&serial, &[7, 3, 1, 7, 3])) % 10;
    let [first, second, third] = letters.map(char::from);
    format!("{first}{second}{third}{check}{serial}")
}

fn gender_digit(gender: Option<&str>) -> u16 {
    match gender {
        Some("male") => fastrand::u16(0..=4) * 2 + 1,
        Some("female") => fastrand::u16(0..=4) * 2,
        _ => fastrand::u16(0..=9),
    }
}

fn brazilian_check_digit(number: &str) -> char {
    let sum: u16 = number
        .bytes()
        .rev()
        .zip(2_u16..)
        .map(|(digit, weight)| {
            let weight = if number.len() >= 12 && weight >= 10 {
                weight - 8
            } else {
                weight
            };
            u16::from(digit - b'0') * weight
        })
        .sum();
    let check = 11 - sum % 11;
    char::from(b'0' + if check < 10 { check as u8 } else { 0 })
}
