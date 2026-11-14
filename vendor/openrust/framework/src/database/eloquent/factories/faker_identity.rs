use super::{DatabaseError, DatabaseResult, Faker, UniqueFaker};

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
    /// Samples the US Person provider's Social Security number format.
    pub fn ssn(&self) -> DatabaseResult<String> {
        if self.locale() != "en_US" {
            return Err(DatabaseError::InvalidQuery("unsupported Faker SSN locale"));
        }
        let area = if fastrand::bool() {
            fastrand::u16(1..=665)
        } else {
            fastrand::u16(667..=899)
        };
        let group = fastrand::u8(1..=99);
        let serial = fastrand::u16(1..=9999);
        Ok(format!("{area:03}-{group:02}-{serial:04}"))
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

    /// Samples a Taiwanese personal identity number with its check digit.
    pub fn personalIdentityNumber(&self, gender: Option<&str>) -> DatabaseResult<String> {
        if self.locale() != "zh_TW" {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker personal identity locale",
            ));
        }
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
        Ok(format!("{birthplace}{gender}{serial}{check}"))
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl UniqueFaker<'_> {
    pub fn ssn(&self) -> DatabaseResult<String> {
        self.try_generate("ssn", || self.0.ssn())
    }

    pub fn nino(&self) -> DatabaseResult<String> {
        self.try_generate("nino", || self.0.nino())
    }

    pub fn personalIdentityNumber(&self, gender: Option<&str>) -> DatabaseResult<String> {
        self.try_generate("personalIdentityNumber", || {
            self.0.personalIdentityNumber(gender)
        })
    }
}
