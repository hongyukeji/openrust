use chrono::{Datelike as _, NaiveDate};

use super::{
    DatabaseError, DatabaseResult, Faker, UniqueFaker, ean_with_checksum, luhn_check_digit,
};

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

const GREEK_DOY: &[&str] = &[
    "Α' Αθήνας",
    "Β' Αθήνας",
    "Γ' Αθήνας",
    "ΣΤ' Αθήνας",
    "Γαλάτσιου",
    "Α' Πειραιά",
    "Β' Πειραιά",
    "Γ' Πειραιά",
    "Α' Θεσσαλονίκης",
    "Β' Θεσσαλονίκης",
    "Γλυφάδας",
    "Ωροπού",
    "Καλιθέας",
    "Αγίου Δημητρίου",
    "Νέας Σμύρνης",
    "Αμαρουσίου",
    "Θήρας",
    "Αμοργού",
    "Πατρών",
    "ΔΟΥ ΠΛΟΙΩΝ",
    "ΦΑΕΕ ΑΘΗΝΩΝ",
];

const GREEK_OBJECT: &[&str] = &[
    "Προγραμματιστής",
    "Δικηγόρος",
    "Γιατρός",
    "Γραφίστας",
    "Αρχαιολόγος",
    "Εκπαιδευτικός",
    "Μεταφραστής",
    "Μηχανολόγος-μηχανικός",
    "Αρχιτέκτονας",
    "Δημοσιογράφος",
    "Υπηρεσίες Διαδικτύου",
    "Ραδιοφωνικές παραγωγές",
    "Καραγκιοζοπαίχτης",
    "Κουλουράς",
    "Κομπάρσος",
    "Καλλιτεχνικός πράκτορας",
    "Εισαγωγαί-εξαγωγαί",
    "Ωρολογοποιός",
    "Καθεκλοποιός",
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

    /// Samples a Danish CPR number using the Person provider's date and serial mask.
    pub fn cpr(&self) -> DatabaseResult<String> {
        if self.locale() != "da_DK" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker CPR locale"));
        }
        Ok(format!(
            "{}-{}",
            self.identity_birthdate()?.format("%d%m%y"),
            self.numerify(Some("%###"))
        ))
    }

    /// Samples a Czech birth number using the supplied age range and slash probability.
    pub fn birthNumber(
        &self,
        gender: Option<&str>,
        minAge: u32,
        maxAge: u32,
        slashProbability: i64,
    ) -> DatabaseResult<String> {
        if self.locale() != "cs_CZ" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Czech birth number locale",
            ));
        }
        if minAge > maxAge {
            return Err(DatabaseError::InvalidQuery(
                "invalid Faker birth number age range",
            ));
        }
        let now = chrono::Utc::now();
        let age_bound = |age: u32| -> DatabaseResult<_> {
            let local = now.with_timezone(&self.application_timezone).naive_local();
            let year = local
                .year()
                .checked_sub(i32::try_from(age).map_err(|_| super::DATE_RANGE_ERROR)?)
                .ok_or(super::DATE_RANGE_ERROR)?;
            // PHP normalizes February 29 to March 1 in a non-leap year.
            let date = NaiveDate::from_ymd_opt(year, local.month(), local.day())
                .or_else(|| {
                    (local.month() == 2 && local.day() == 29)
                        .then(|| NaiveDate::from_ymd_opt(year, 3, 1))
                        .flatten()
                })
                .ok_or(super::DATE_RANGE_ERROR)?;
            super::resolve_local(self.application_timezone, date.and_time(local.time()))
        };
        let date = self
            .dateTimeBetween(age_bound(maxAge)?, age_bound(minAge)?)?
            .with_timezone(&self.application_timezone);
        let female = gender.map_or_else(fastrand::bool, |value| value == "female");
        let year = date.year();
        let capacity_offset = year >= 2004 && self.boolean(Some(10));
        let month = date.month() + 50 * u32::from(female) + 20 * u32::from(capacity_offset);
        let serial = self.numberBetween(0, 999);
        let mut number = format!(
            "{:02}{month:02}{:02}{serial:03}",
            year.rem_euclid(100),
            date.day()
        );
        if year >= 1954 {
            let remainder = number
                .parse::<u32>()
                .expect("fixed Czech birth number digits")
                % 11;
            number.push(char::from(b'0' + (remainder % 10) as u8));
        }
        if self.boolean(Some(slashProbability)) {
            number.insert(6, '/');
        }
        Ok(number)
    }

    pub fn birthNumberMale(&self) -> DatabaseResult<String> {
        self.birthNumber(Some("male"), 0, 100, 50)
    }

    pub fn birthNumberFemale(&self) -> DatabaseResult<String> {
        self.birthNumber(Some("female"), 0, 100, 50)
    }

    /// Samples a Swiss social security number with the German formatter name.
    pub fn ahv13(&self) -> DatabaseResult<String> {
        if self.locale() != "de_CH" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker AHV13 locale",
            ));
        }
        self.avs13()
    }

    /// Samples a Swiss social security number in dotted notation.
    pub fn avs13(&self) -> DatabaseResult<String> {
        if !matches!(self.locale(), "de_CH" | "fr_CH" | "it_CH") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker AVS13 locale",
            ));
        }

        let digits = ean_with_checksum(self.numerify(Some("756#########")));
        Ok([&digits[..3], &digits[3..7], &digits[7..11], &digits[11..]].join("."))
    }

    /// Samples a Kazakh individual identification number.
    pub fn individualIdentificationNumber(
        &self,
        birthDate: Option<NaiveDate>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        if self.locale() != "kk_KZ" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker IIN locale"));
        }
        let female = match gender {
            None | Some("male") => false,
            Some("female") => true,
            _ => return Err(DatabaseError::InvalidQuery("invalid Faker IIN gender")),
        };
        let date = match birthDate {
            Some(date) => date,
            None => self
                .dateTimeBetweenExpressions("-30 years", "now")?
                .with_timezone(
                    &self
                        .getDefaultTimezone()
                        .unwrap_or(self.application_timezone),
                )
                .date_naive(),
        };
        let century = match date.year() {
            1800..=1899 => 1,
            1900..=1999 => 3,
            2000..=2099 => 5,
            _ => {
                return Err(DatabaseError::InvalidQuery(
                    "unsupported Faker IIN birth year",
                ));
            }
        };
        let marker = century + u8::from(female);
        let birth = date.format("%y%m%d");
        loop {
            let serial = self.numberBetween(1000, 2000);
            let prefix = format!("{birth}{marker}{serial}");
            let check = Self::checkSum(&prefix)?;
            if check != 10 {
                return Ok(format!("{prefix}{check}"));
            }
        }
    }

    /// Calculates the Kazakh IIN control digit for an eleven-digit prefix.
    pub fn checkSum(iinValue: &str) -> DatabaseResult<u8> {
        if iinValue.len() != 11 || !iinValue.bytes().all(|digit| digit.is_ascii_digit()) {
            return Err(DatabaseError::InvalidQuery(
                "Faker IIN prefix must have eleven digits",
            ));
        }
        let first = weighted_digits(iinValue, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]) % 11;
        if first != 10 {
            return Ok(first as u8);
        }
        Ok((weighted_digits(iinValue, &[3, 4, 5, 6, 7, 8, 9, 10, 11, 1, 2]) % 11) as u8)
    }

    /// Samples a Venezuelan national ID with an optional separator.
    pub fn nationalId(&self, separator: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "es_VE" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Venezuelan national ID locale",
            ));
        }
        let (prefix, lower) = if fastrand::bool() {
            ('V', 10_000)
        } else {
            ('E', 80_000_000)
        };
        let serial = self.numberBetween(lower, 100_000_000);
        Ok(format!("{prefix}{}{serial}", separator.unwrap_or_default()))
    }

    /// Samples an Iranian national code with its modulo-11 check digit.
    pub fn nationalCode(&self) -> DatabaseResult<String> {
        if self.locale() != "fa_IR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Iranian national code locale",
            ));
        }
        let area = self.numberBetween(1, 999);
        let core = self.numberBetween(0, 999_999);
        let mut number = format!("{area:03}{core:06}");
        let remainder = weighted_digits(&number, &[10, 9, 8, 7, 6, 5, 4, 3, 2]) % 11;
        let check = if remainder < 2 {
            remainder
        } else {
            11 - remainder
        };
        number.push(char::from(b'0' + check as u8));
        Ok(number)
    }

    /// Samples an Indonesian NIK using an optional civil birth date.
    pub fn nik(
        &self,
        gender: Option<&str>,
        birthDate: Option<NaiveDate>,
    ) -> DatabaseResult<String> {
        if self.locale() != "id_ID" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Indonesian NIK locale",
            ));
        }
        let date = match birthDate {
            Some(date) => date,
            None => self
                .dateTimeBetweenExpressions("-30 years", "now")?
                .with_timezone(
                    &self
                        .getDefaultTimezone()
                        .unwrap_or(self.application_timezone),
                )
                .date_naive(),
        };
        let female = match gender {
            None | Some("" | "0") => fastrand::bool(),
            Some(value) => value == "female",
        };
        let places = super::south_southeast_asian::ID_BIRTH_PLACE_CODES;
        let place = places[fastrand::usize(..places.len())];
        let district = self.numberBetween(0, 99);
        let day = date.day() + 40 * u32::from(female);
        let month = date.month();
        let year = date.year().rem_euclid(100);
        let serial = self.numberBetween(0, 9999);
        Ok(format!(
            "{place}{district:02}{day:02}{month:02}{year:02}{serial:04}"
        ))
    }

    /// Samples a French NIR with optional spacing.
    pub fn nir(&self, gender: Option<&str>, formatted: bool) -> DatabaseResult<String> {
        if !matches!(self.locale(), "fr_FR" | "fr_CH") {
            return Err(DatabaseError::InvalidQuery("unsupported Faker NIR locale"));
        }

        const DEPARTMENTS: &[&str] = &[
            "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14",
            "15", "16", "17", "18", "19", "2A", "2B", "21", "22", "23", "24", "25", "26", "27",
            "28", "29", "30", "31", "32", "33", "34", "35", "36", "37", "38", "39", "40", "41",
            "42", "43", "44", "45", "46", "47", "48", "49", "50", "51", "52", "53", "54", "55",
            "56", "57", "58", "59", "60", "61", "62", "63", "64", "65", "66", "67", "68", "69",
            "70", "71", "72", "73", "74", "75", "76", "77", "78", "79", "80", "81", "82", "83",
            "84", "85", "86", "87", "88", "89", "90", "91", "92", "93", "94", "95", "971", "972",
            "973", "974", "976",
        ];
        let sex = match gender {
            Some("male") => 1,
            Some("female") => 2,
            _ => self.numberBetween(1, 2),
        };
        let year = self.numerify(Some("##"));
        let month = self.numberBetween(1, 12);
        let department = DEPARTMENTS[fastrand::usize(..DEPARTMENTS.len())];
        let town = self.numerify(Some(if department.len() == 2 { "###" } else { "##" }));
        let serial = self.numerify(Some("###"));

        let numeric_department = match department {
            "2A" => "19",
            "2B" => "18",
            digits => digits,
        };
        let numeric_body = format!("{sex}{year}{month:02}{numeric_department}{town}{serial}");
        let check = 97 - numeric_body.parse::<u64>().expect("fixed NIR digits") % 97;
        let body = format!("{sex}{year}{month:02}{department}{town}{serial}{check:02}");
        if formatted {
            return Ok([
                &body[..1],
                &body[1..3],
                &body[3..5],
                &body[5..7],
                &body[7..10],
                &body[10..13],
                &body[13..],
            ]
            .join(" "));
        }
        Ok(body)
    }

    /// Samples the uppercase Italian tax ID mask.
    pub fn taxId(&self) -> DatabaseResult<String> {
        if !matches!(self.locale(), "it_IT" | "it_CH") {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Italian tax ID locale",
            ));
        }
        Ok(self.bothify(Some("??????##?##?###?")).to_ascii_uppercase())
    }

    /// Samples a Belgian National Register number.
    pub fn rrn(&self, gender: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "nl_BE" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Belgian identity locale",
            ));
        }

        let mut serial = self.numberBetween(1, 997) as u64;
        match gender {
            Some("male") => serial += 1 - serial % 2,
            Some("female") => serial += serial % 2,
            _ => {}
        }

        let date = self.identity_birthdate()?;
        let birth = u64::from(date.year().rem_euclid(100) as u32) * 10_000
            + u64::from(date.month()) * 100
            + u64::from(date.day());
        let body = birth * 1_000 + serial;
        let century = if date.year() >= 2000 {
            2_000_000_000
        } else {
            0
        };
        let check = 97 - (century + body) % 97;
        Ok(format!("{body:09}{check:02}"))
    }

    /// Samples a Singapore citizen's NRIC.
    pub fn nric(&self, birthDate: Option<NaiveDate>) -> DatabaseResult<String> {
        self.singaporeId(birthDate, false)
    }

    /// Samples a Singapore foreign identification number.
    pub fn fin(&self, issueDate: Option<NaiveDate>) -> DatabaseResult<String> {
        self.singaporeId(issueDate, true)
    }

    pub fn singaporeId(
        &self,
        issueDate: Option<NaiveDate>,
        foreigner: bool,
    ) -> DatabaseResult<String> {
        if self.locale() != "en_SG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Singapore identity locale",
            ));
        }
        let date = match issueDate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        let year = date.year();
        let recent = year >= 2000;
        let (prefix, checksum_letters) = match (foreigner, recent) {
            (true, false) => ('F', b"XWUTRQPNMLK"),
            (true, true) => ('G', b"XWUTRQPNMLK"),
            (false, false) => ('S', b"JZIHGFEDCBA"),
            (false, true) => ('T', b"JZIHGFEDCBA"),
        };
        let digits = if foreigner {
            self.numerify(Some("#######"))
        } else {
            let year_digits = if year < 1968 {
                fastrand::u8(0..=1).to_string()
            } else {
                date.format("%y").to_string()
            };
            format!("{year_digits:0>2}{}", self.numerify(Some("#####")))
        };
        let checksum = weighted_digits(&digits, &[2, 7, 6, 5, 4, 3, 2]) + 4 * u16::from(recent);
        Ok(format!(
            "{prefix}{digits}{}",
            char::from(checksum_letters[(checksum % 11) as usize])
        ))
    }

    /// Samples an identification number for the selected locale.
    pub fn idNumber(&self) -> DatabaseResult<String> {
        match self.locale() {
            "en_ZA" => self.idNumberWithBirthdate(None, None, None),
            "nl_NL" => Ok(dutch_identity()),
            "ar_SA" => Ok(self.saudi_identity(if fastrand::bool() { "1" } else { "2" })),
            "mn_MN" => Ok(format!(
                "{}{}{}",
                self.alphabet()?,
                self.alphabet()?,
                self.numerify(Some("########"))
            )),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker identity number locale",
            )),
        }
    }

    /// Samples a South African ID with optional birth date, citizenship and gender.
    pub fn idNumberWithBirthdate(
        &self,
        birthdate: Option<NaiveDate>,
        citizen: Option<bool>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        if self.locale() != "en_ZA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker dated identity number locale",
            ));
        }
        let birthdate = match birthdate {
            Some(date) => date,
            None => self.identity_birthdate()?,
        };
        let sex = match gender {
            Some(value) if value.eq_ignore_ascii_case("female") => fastrand::u16(0..=4),
            Some(value) if value.eq_ignore_ascii_case("male") => fastrand::u16(5..=9),
            _ => fastrand::u16(0..=9),
        };
        let sequence = fastrand::u16(0..=999);
        // PHP pads on both sides, with the extra zero on the right.
        let sequence = match sequence {
            0..=9 => format!("0{sequence}0"),
            10..=99 => format!("{sequence}0"),
            _ => sequence.to_string(),
        };
        let citizenship = u8::from(!citizen.unwrap_or(true));
        let race = fastrand::u8(8..=9);
        let mut number = format!(
            "{}{sex}{sequence}{citizenship}{race}",
            birthdate.format("%y%m%d")
        );
        number.push(luhn_check_digit(&number));
        Ok(number)
    }

    pub fn nationalIdNumber(&self) -> DatabaseResult<String> {
        if self.locale() == "ar_SA" {
            return Ok(self.saudi_identity("1"));
        }
        self.nationalIdNumberWithGender(None)
    }

    /// Samples an Egyptian national ID with the requested registration sequence parity.
    pub fn nationalIdNumberWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "ar_EG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker gendered national ID locale",
            ));
        }
        let date = self
            .dateTimeBetweenExpressions("1950-01-10", "2005-12-25")?
            .with_timezone(&self.application_timezone);
        let century = if date.year() >= 2000 { 3 } else { 2 };
        let sequence = fastrand::u16(1..=500);
        let sequence = match gender {
            Some("male") => sequence | 1,
            Some("female") => sequence & !1,
            _ => sequence,
        };
        let check = fastrand::u8(1..=9);
        Ok(format!(
            "{century}{}{governorate}{sequence:04}{check}",
            date.format("%y%m%d"),
            governorate = self.governorateId()?
        ))
    }

    pub fn foreignerIdNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "ar_SA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker foreigner ID locale",
            ));
        }
        Ok(self.saudi_identity("2"))
    }

    fn saudi_identity(&self, prefix: &str) -> String {
        let mut number = self.numerify(Some("########"));
        number.insert_str(0, prefix);
        number.push(luhn_check_digit(&number));
        number
    }

    /// Samples the initial used in Mongolian full names.
    pub fn namePrefix(&self) -> DatabaseResult<char> {
        if self.locale() != "mn_MN" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker name prefix locale",
            ));
        }
        const PREFIXES: &[char] = &[
            'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ё', 'Ж', 'З', 'И', 'К', 'Л', 'М', 'Н', 'О', 'Ө', 'П',
            'Р', 'С', 'Т', 'У', 'Ү', 'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Э', 'Ю', 'Я',
        ];
        Ok(PREFIXES[fastrand::usize(..PREFIXES.len())])
    }

    pub fn alphabet(&self) -> DatabaseResult<char> {
        if self.locale() != "mn_MN" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker alphabet locale",
            ));
        }
        const ALPHABET: [char; 35] = [
            'А', 'Л', 'Х', 'Б', 'М', 'Ц', 'В', 'Н', 'Ч', 'Г', 'О', 'Ш', 'Д', 'Ө', 'Щ', 'Е', 'П',
            'Ъ', 'Ё', 'Р', 'Ы', 'Ж', 'С', 'Ь', 'З', 'Т', 'Э', 'И', 'У', 'Ю', 'Й', 'Ү', 'Я', 'К',
            'Ф',
        ];
        Ok(ALPHABET[fastrand::usize(..ALPHABET.len())])
    }

    pub fn governorateId(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "ar_EG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker governorate locale",
            ));
        }
        const CODES: [&str; 26] = [
            "02", "19", "28", "25", "29", "31", "18", "22", "03", "35", "01", "12", "11", "26",
            "04", "13", "34", "16", "23", "14", "27", "15", "33", "17", "24", "32",
        ];
        Ok(CODES[fastrand::usize(..CODES.len())])
    }

    /// Samples a Latvian driving licence number.
    pub fn driverLicence(&self) -> DatabaseResult<String> {
        if self.locale() != "lv_LV" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker driving licence locale",
            ));
        }
        Ok(self.bothify(Some("??######")))
    }

    /// Samples a passport number using the Lithuanian or Latvian provider.
    pub fn passportNumber(&self) -> DatabaseResult<String> {
        match self.locale() {
            "lt_LT" => Ok(self.numerify(Some("########"))),
            "lv_LV" => Ok(self.bothify(Some("??#######"))),
            _ => Err(DatabaseError::InvalidQuery(
                "unsupported Faker passport locale",
            )),
        }
    }

    /// Samples a Turkish identity number using the fixed FakerPHP checksum.
    pub fn tcNo(&self) -> DatabaseResult<String> {
        if self.locale() != "tr_TR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Turkish identity locale",
            ));
        }
        let mut number = self.numerify(Some("#########"));
        number.push_str(&Self::tcNoChecksum(&number)?);
        Ok(number)
    }

    pub fn tcNoChecksum(identityPrefix: &str) -> DatabaseResult<String> {
        if identityPrefix.len() != 9 || !identityPrefix.bytes().all(|digit| digit.is_ascii_digit())
        {
            return Err(DatabaseError::InvalidQuery(
                "Faker Turkish identity prefix must contain nine ASCII digits",
            ));
        }

        let odd = weighted_digits(identityPrefix, &[1, 0, 1, 0, 1, 0, 1, 0, 1]);
        let even = weighted_digits(identityPrefix, &[0, 1, 0, 1, 0, 1, 0, 1, 0]);
        // PHP preserves the sign of a negative remainder.
        let tenth = (7 * odd as i16 - even as i16) % 10;
        let eleventh = (odd as i16 + even as i16 + tenth) % 10;
        Ok(format!("{tenth}{eleventh}"))
    }

    pub fn tcNoIsValid(tcNo: &str) -> DatabaseResult<bool> {
        let split = tcNo
            .len()
            .checked_sub(2)
            .and_then(|offset| tcNo.split_at_checked(offset))
            .ok_or(DatabaseError::InvalidQuery(
                "Faker Turkish identity number has an invalid length",
            ))?;
        Ok(Self::tcNoChecksum(split.0)? == split.1)
    }

    /// Samples the Taiwanese eight-digit VAT identifier.
    pub fn VAT(&self) -> DatabaseResult<i64> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Taiwanese VAT locale",
            ));
        }
        self.randomNumber(Some(8), true)
    }

    /// Samples a Greek tax office.
    pub fn doy(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "el_GR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker tax office locale",
            ));
        }
        Ok(GREEK_DOY[fastrand::usize(..GREEK_DOY.len())])
    }

    /// Samples a Greek company's business activity.
    pub fn object(&self) -> DatabaseResult<&'static str> {
        if self.locale() != "el_GR" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker business activity locale",
            ));
        }
        Ok(GREEK_OBJECT[fastrand::usize(..GREEK_OBJECT.len())])
    }

    /// Samples a Kazakh business ID using the supplied civil registration date.
    pub fn businessIdentificationNumber(
        &self,
        registration_date: Option<NaiveDate>,
    ) -> DatabaseResult<String> {
        if self.locale() != "kk_KZ" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker BIN locale"));
        }
        let date = match registration_date {
            Some(date) => date,
            None => {
                let zone = self
                    .getDefaultTimezone()
                    .unwrap_or(self.application_timezone);
                self.dateTimeThisYear(None)?
                    .with_timezone(&zone)
                    .date_naive()
            }
        };
        let legal_type = fastrand::u8(4..=6);
        let additional_type = fastrand::u8(0..=3);
        let digits = self.numerify(Some("######"));
        Ok(format!(
            "{}{legal_type}{additional_type}{digits}",
            date.format("%y%m")
        ))
    }

    /// Samples a South African company registration number.
    pub fn companyNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "en_ZA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker company registration locale",
            ));
        }
        const ENTITIES: [&str; 23] = [
            "01", "02", "06", "07", "08", "09", "10", "11", "12", "14", "15", "16", "17", "20",
            "21", "22", "23", "24", "25", "26", "30", "31", "80",
        ];
        let zone = self
            .getDefaultTimezone()
            .unwrap_or(self.application_timezone);
        let date = self
            .dateTimeBetweenExpressions("-50 years", "now")?
            .with_timezone(&zone);
        let year = date.format("%Y");
        let number = self.randomNumber(Some(6), true)?;
        let entity = ENTITIES[fastrand::usize(..ENTITIES.len())];
        Ok(format!("{year}/{number}/{entity}"))
    }

    /// Samples a Saudi company identification number.
    pub fn companyIdNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "ar_SA" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Saudi company ID locale",
            ));
        }
        Ok(self.company_number_with_luhn("700######"))
    }

    /// Samples an Egyptian company tax identification number.
    pub fn companyTaxIdNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "ar_EG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Egyptian company tax ID locale",
            ));
        }
        Ok(self.company_number_with_luhn("#########"))
    }

    /// Samples an Egyptian commercial registration number.
    pub fn companyTradeRegisterNumber(&self) -> DatabaseResult<String> {
        if self.locale() != "ar_EG" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker Egyptian commercial registration locale",
            ));
        }
        Ok(self.company_number_with_luhn("######"))
    }

    fn company_number_with_luhn(&self, mask: &str) -> String {
        let mut number = self.numerify(Some(mask));
        number.push(luhn_check_digit(&number));
        number
    }

    /// Samples a Czech business identification number (IČO).
    pub fn ico(&self) -> DatabaseResult<String> {
        if self.locale() != "cs_CZ" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker IČO locale"));
        }
        let mut number = self.numerify(Some("#######"));
        let check = match weighted_digits(&number, &[8, 7, 6, 5, 4, 3, 2]) % 11 {
            0 | 10 => 1,
            1 => 0,
            remainder => 11 - remainder,
        };
        number.push(char::from(b'0' + check as u8));
        Ok(number)
    }

    /// Samples a Danish business registration number (CVR).
    pub fn cvr(&self) -> DatabaseResult<String> {
        if self.locale() != "da_DK" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker CVR locale"));
        }
        Ok(self.numerify(Some("%#######")))
    }

    /// Samples a Danish production unit number.
    pub fn p(&self) -> DatabaseResult<String> {
        if self.locale() != "da_DK" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker P number locale",
            ));
        }
        Ok(self.numerify(Some("%#########")))
    }

    /// Samples an Icelandic VAT registration number (VSK).
    pub fn vsk(&self) -> DatabaseResult<String> {
        if self.locale() != "is_IS" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker VSK locale"));
        }
        Ok(self.numerify(Some("%####")))
    }

    /// Samples a French establishment identifier with a valid SIREN prefix.
    pub fn siret(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        const NIC_FORMATS: [&str; 3] = ["####", "0###", "00#%"];
        let mut number = self.siren(false)?;
        number.push_str(&self.numerify(Some(NIC_FORMATS[fastrand::usize(..NIC_FORMATS.len())])));
        number.push(luhn_check_digit(&number));
        if formatted.unwrap_or(true) {
            Ok(format!(
                "{} {} {} {}",
                &number[..3],
                &number[3..6],
                &number[6..9],
                &number[9..],
            ))
        } else {
            Ok(number)
        }
    }

    /// Samples a Polish national business register number.
    pub fn regon(&self) -> DatabaseResult<String> {
        if self.locale() != "pl_PL" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker REGON locale",
            ));
        }
        let region = fastrand::u8(0..50) * 2 + 1;
        let mut number = format!("{region:02}{:06}", fastrand::u32(..1_000_000));
        number.push(regon_check_digit(&number, &[8, 9, 2, 3, 4, 5, 6, 7]));
        Ok(number)
    }

    /// Samples a Polish local business register number with a valid REGON prefix.
    pub fn regonLocal(&self) -> DatabaseResult<String> {
        let mut number = self.regon()?;
        number.push_str(&self.numerify(Some("####")));
        number.push(regon_check_digit(
            &number,
            &[2, 4, 8, 5, 0, 9, 7, 3, 6, 1, 2, 4, 8],
        ));
        Ok(number)
    }

    /// Samples a US Employer Identification Number.
    pub fn ein(&self) -> DatabaseResult<String> {
        const PREFIXES: &[u8] = &[
            1, 2, 3, 4, 5, 6, 10, 11, 12, 13, 14, 15, 16, 20, 21, 22, 23, 24, 25, 26, 27, 30, 31,
            32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 50, 51, 52, 53, 54,
            55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 71, 72, 73, 74, 75, 76, 77, 80,
            81, 82, 83, 84, 85, 86, 87, 88, 90, 91, 92, 93, 94, 95, 98, 99,
        ];
        if self.locale() != "en_US" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker EIN locale"));
        }

        let prefix = PREFIXES[fastrand::usize(..PREFIXES.len())];
        let suffix = fastrand::u32(..10_000_000);
        Ok(format!("{prefix:02}-{suffix:07}"))
    }

    /// Legacy name for the Russian ten-digit INN formatter.
    pub fn inn(&self, area_code: Option<&str>) -> DatabaseResult<String> {
        self.inn10(area_code)
    }

    /// Samples a Russian ten-digit taxpayer identification number.
    pub fn inn10(&self, area_code: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "ru_RU" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker INN locale"));
        }

        let area = match area_code {
            None | Some("") => 0,
            Some(code) => code
                .parse::<u8>()
                .ok()
                .filter(|area| *area <= 99 && code.bytes().all(|digit| digit.is_ascii_digit()))
                .ok_or(DatabaseError::InvalidQuery(
                    "Faker INN area must be between 0 and 99",
                ))?,
        };
        let area = if area == 0 {
            fastrand::u8(1..=91)
        } else {
            area
        };
        let mut number = format!("{area:02}{:07}", fastrand::u32(..10_000_000));
        number.push_str(&Self::inn10Checksum(&number)?);
        Ok(number)
    }

    /// Samples a Russian tax registration reason code from the first four INN digits.
    pub fn kpp(&self, inn: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "ru_RU" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker KPP locale"));
        }

        let generated;
        let inn = match inn {
            Some(value) if value.len() >= 4 => value,
            _ => {
                generated = self.inn10(None)?;
                &generated
            }
        };
        let prefix = inn
            .get(..4)
            .filter(|value| value.bytes().all(|digit| digit.is_ascii_digit()))
            .ok_or(DatabaseError::InvalidQuery(
                "Faker KPP prefix must contain four ASCII digits",
            ))?;
        Ok(format!("{prefix}01001"))
    }

    /// Calculates the check digit from the first nine digits.
    pub fn inn10Checksum(inn: &str) -> DatabaseResult<String> {
        let prefix = inn
            .get(..9)
            .filter(|value| value.bytes().all(|digit| digit.is_ascii_digit()))
            .ok_or(DatabaseError::InvalidQuery(
                "Faker INN prefix must contain nine ASCII digits",
            ))?;
        Ok(((weighted_digits(prefix, &[2, 4, 10, 3, 5, 9, 4, 6, 8]) % 11) % 10).to_string())
    }

    /// Checks a ten-digit INN against its checksum.
    pub fn inn10IsValid(inn: &str) -> bool {
        inn.len() == 10
            && Self::inn10Checksum(inn).is_ok_and(|check| inn.as_bytes()[9..] == *check.as_bytes())
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

    /// Calculates the Portuguese Person provider's modulo-11 check digit.
    pub fn dvCalcMod11(number: &str) -> DatabaseResult<u8> {
        let mut remainder = 0;
        for (digit, weight) in number.bytes().rev().zip((2..=9).cycle()) {
            if !digit.is_ascii_digit() {
                return Err(DatabaseError::InvalidQuery(
                    "Faker modulo-11 input must contain only ASCII digits",
                ));
            }
            remainder = (remainder + (digit - b'0') * weight) % 11;
        }
        Ok(if remainder < 2 { 0 } else { 11 - remainder })
    }

    /// Samples a Portuguese NIF or Polish NIP.
    pub fn taxpayerIdentificationNumber(&self) -> DatabaseResult<String> {
        if self.locale() == "pt_PT" {
            const MASKS: &[&str] = &[
                "1#######", "2#######", "3#######", "45######", "5#######", "6#######", "70######",
                "71######", "72######", "77######", "79######", "8#######", "90######", "98######",
                "99######",
            ];
            let mask = MASKS[fastrand::usize(..MASKS.len())];
            let mut number = self.numerify(Some(mask));
            let digit = Self::dvCalcMod11(&number)?;
            number.push(char::from(b'0' + digit));
            return Ok(number);
        }
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

    pub fn cpr(&self) -> DatabaseResult<String> {
        self.try_generate("cpr", || self.0.cpr())
    }

    pub fn birthNumber(
        &self,
        gender: Option<&str>,
        minAge: u32,
        maxAge: u32,
        slashProbability: i64,
    ) -> DatabaseResult<String> {
        self.try_generate("birthNumber", || {
            self.0.birthNumber(gender, minAge, maxAge, slashProbability)
        })
    }

    pub fn birthNumberMale(&self) -> DatabaseResult<String> {
        self.try_generate("birthNumberMale", || self.0.birthNumberMale())
    }

    pub fn birthNumberFemale(&self) -> DatabaseResult<String> {
        self.try_generate("birthNumberFemale", || self.0.birthNumberFemale())
    }

    pub fn ahv13(&self) -> DatabaseResult<String> {
        self.try_generate("ahv13", || self.0.ahv13())
    }

    pub fn avs13(&self) -> DatabaseResult<String> {
        self.try_generate("avs13", || self.0.avs13())
    }

    pub fn individualIdentificationNumber(
        &self,
        birthDate: Option<NaiveDate>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("individualIdentificationNumber", || {
            self.0.individualIdentificationNumber(birthDate, gender)
        })
    }

    pub fn nationalId(&self, separator: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("nationalId", || self.0.nationalId(separator))
    }

    pub fn nationalCode(&self) -> DatabaseResult<String> {
        self.try_generate("nationalCode", || self.0.nationalCode())
    }

    pub fn nik(
        &self,
        gender: Option<&str>,
        birthDate: Option<NaiveDate>,
    ) -> DatabaseResult<String> {
        self.try_generate("nik", || self.0.nik(gender, birthDate))
    }

    pub fn nir(&self, gender: Option<&str>, formatted: bool) -> DatabaseResult<String> {
        self.try_generate("nir", || self.0.nir(gender, formatted))
    }

    pub fn taxId(&self) -> DatabaseResult<String> {
        self.try_generate("taxId", || self.0.taxId())
    }

    pub fn rrn(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("rrn", || self.0.rrn(gender))
    }

    pub fn nric(&self, birthDate: Option<NaiveDate>) -> DatabaseResult<String> {
        self.try_generate("nric", || self.0.nric(birthDate))
    }

    pub fn fin(&self, issueDate: Option<NaiveDate>) -> DatabaseResult<String> {
        self.try_generate("fin", || self.0.fin(issueDate))
    }

    pub fn singaporeId(
        &self,
        issueDate: Option<NaiveDate>,
        foreigner: bool,
    ) -> DatabaseResult<String> {
        self.try_generate("singaporeId", || self.0.singaporeId(issueDate, foreigner))
    }

    pub fn idNumber(&self) -> DatabaseResult<String> {
        self.try_generate("idNumber", || self.0.idNumber())
    }

    pub fn idNumberWithBirthdate(
        &self,
        birthdate: Option<NaiveDate>,
        citizen: Option<bool>,
        gender: Option<&str>,
    ) -> DatabaseResult<String> {
        self.try_generate("idNumber", || {
            self.0.idNumberWithBirthdate(birthdate, citizen, gender)
        })
    }

    pub fn nationalIdNumber(&self) -> DatabaseResult<String> {
        self.try_generate("nationalIdNumber", || self.0.nationalIdNumber())
    }

    pub fn nationalIdNumberWithGender(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("nationalIdNumber", || {
            self.0.nationalIdNumberWithGender(gender)
        })
    }

    pub fn foreignerIdNumber(&self) -> DatabaseResult<String> {
        self.try_generate("foreignerIdNumber", || self.0.foreignerIdNumber())
    }

    pub fn namePrefix(&self) -> DatabaseResult<char> {
        self.try_generate("namePrefix", || self.0.namePrefix())
    }

    pub fn alphabet(&self) -> DatabaseResult<char> {
        self.try_generate("alphabet", || self.0.alphabet())
    }

    pub fn governorateId(&self) -> DatabaseResult<&'static str> {
        self.try_generate("governorateId", || self.0.governorateId())
    }

    pub fn VAT(&self) -> DatabaseResult<i64> {
        self.try_generate("VAT", || self.0.VAT())
    }

    pub fn doy(&self) -> DatabaseResult<&'static str> {
        self.try_generate("doy", || self.0.doy())
    }

    pub fn object(&self) -> DatabaseResult<&'static str> {
        self.try_generate("object", || self.0.object())
    }

    pub fn businessIdentificationNumber(
        &self,
        registration_date: Option<NaiveDate>,
    ) -> DatabaseResult<String> {
        self.try_generate("businessIdentificationNumber", || {
            self.0.businessIdentificationNumber(registration_date)
        })
    }

    pub fn companyNumber(&self) -> DatabaseResult<String> {
        self.try_generate("companyNumber", || self.0.companyNumber())
    }

    pub fn companyIdNumber(&self) -> DatabaseResult<String> {
        self.try_generate("companyIdNumber", || self.0.companyIdNumber())
    }

    pub fn companyTaxIdNumber(&self) -> DatabaseResult<String> {
        self.try_generate("companyTaxIdNumber", || self.0.companyTaxIdNumber())
    }

    pub fn companyTradeRegisterNumber(&self) -> DatabaseResult<String> {
        self.try_generate("companyTradeRegisterNumber", || {
            self.0.companyTradeRegisterNumber()
        })
    }

    pub fn ico(&self) -> DatabaseResult<String> {
        self.try_generate("ico", || self.0.ico())
    }

    pub fn cvr(&self) -> DatabaseResult<String> {
        self.try_generate("cvr", || self.0.cvr())
    }

    pub fn p(&self) -> DatabaseResult<String> {
        self.try_generate("p", || self.0.p())
    }

    pub fn vsk(&self) -> DatabaseResult<String> {
        self.try_generate("vsk", || self.0.vsk())
    }

    pub fn siret(&self, formatted: Option<bool>) -> DatabaseResult<String> {
        self.try_generate("siret", || self.0.siret(formatted))
    }

    pub fn regon(&self) -> DatabaseResult<String> {
        self.try_generate("regon", || self.0.regon())
    }

    pub fn regonLocal(&self) -> DatabaseResult<String> {
        self.try_generate("regonLocal", || self.0.regonLocal())
    }

    pub fn ein(&self) -> DatabaseResult<String> {
        self.try_generate("ein", || self.0.ein())
    }

    pub fn inn(&self, area_code: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("inn", || self.0.inn(area_code))
    }

    pub fn inn10(&self, area_code: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("inn10", || self.0.inn10(area_code))
    }

    pub fn kpp(&self, inn: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("kpp", || self.0.kpp(inn))
    }

    pub fn driverLicence(&self) -> DatabaseResult<String> {
        self.try_generate("driverLicence", || self.0.driverLicence())
    }

    pub fn passportNumber(&self) -> DatabaseResult<String> {
        self.try_generate("passportNumber", || self.0.passportNumber())
    }

    pub fn tcNo(&self) -> DatabaseResult<String> {
        self.try_generate("tcNo", || self.0.tcNo())
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

fn regon_check_digit(number: &str, weights: &[u16]) -> char {
    let digit = (weighted_digits(number, weights) % 11) % 10;
    char::from(b'0' + digit as u8)
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

fn dutch_identity() -> String {
    let mut digits = std::array::from_fn::<_, 9, _>(|_| fastrand::u8(0..=9));
    digits[0] = fastrand::u8(0..=6);
    let total: u16 = digits[..8]
        .iter()
        .zip((2..=9).rev())
        .map(|(&digit, weight)| u16::from(digit) * weight)
        .sum();
    digits[8] = (total % 11) as u8;
    if digits[8] == 10 {
        if digits[7] > 0 {
            digits[7] -= 1;
            digits[8] = 8;
        } else {
            digits[7] += 1;
            digits[8] = 1;
        }
    }
    digits
        .into_iter()
        .map(|digit| char::from(b'0' + digit))
        .collect()
}
