//! Allowlist-bound parsing for the API's comma-separated `sort` query convention.

use crate::{Error, Result};

/// Direction of one validated sort field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    /// Sort from the lowest value to the highest value.
    Ascending,
    /// Sort from the highest value to the lowest value.
    Descending,
}

/// One field accepted by a [`SortParams`] allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortField {
    name: Box<str>,
    direction: SortDirection,
}

impl SortField {
    /// Returns the validated public field name. Map it to a fixed ORM column; never interpolate it into SQL.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns this field's requested direction.
    pub const fn direction(&self) -> SortDirection {
        self.direction
    }
}

/// A validated, ordered list of fields from an application-provided allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SortParams(Vec<SortField>);

impl SortParams {
    /// Parses ADR-008 `sort=-created_at,title` syntax against fixed public field names.
    ///
    /// Missing/empty input means no explicit sort. At most 16 unique fields and 256 bytes are
    /// accepted. Unknown or malformed names return a bad-request error without echoing input.
    pub fn parse(value: Option<&str>, allowed: &[&str]) -> Result<Self> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(Self::default());
        };
        if value.len() > 256 {
            return Err(invalid_sort());
        }
        let mut fields = Vec::new();
        for entry in value.split(',') {
            if fields.len() == 16 || entry.is_empty() {
                return Err(invalid_sort());
            }
            let (direction, name) = if let Some(name) = entry.strip_prefix('-') {
                (SortDirection::Descending, name)
            } else {
                (SortDirection::Ascending, entry)
            };
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
                || !allowed.contains(&name)
                || fields.iter().any(|field: &SortField| field.name() == name)
            {
                return Err(invalid_sort());
            }
            fields.push(SortField {
                name: name.into(),
                direction,
            });
        }
        Ok(Self(fields))
    }

    /// Returns the validated fields in request order.
    pub fn fields(&self) -> &[SortField] {
        &self.0
    }
}

fn invalid_sort() -> Error {
    Error::BadRequest("sort contains an unsupported or invalid field".into())
}

#[cfg(test)]
mod tests {
    use super::{SortDirection, SortParams};

    #[test]
    fn parses_only_unique_allowlisted_fields_in_order() {
        let result = SortParams::parse(Some("-created_at,title"), &["created_at", "title"]);
        assert!(result.is_ok());
        if let Ok(parsed) = result {
            assert_eq!(parsed.fields()[0].name(), "created_at");
            assert_eq!(parsed.fields()[0].direction(), SortDirection::Descending);
            assert_eq!(parsed.fields()[1].name(), "title");
            assert_eq!(parsed.fields()[1].direction(), SortDirection::Ascending);
        }
    }

    #[test]
    fn rejects_unknown_duplicate_malformed_and_unbounded_fields() {
        let too_long = "a".repeat(257);
        for value in [
            "password_hash",
            "title,title",
            "-",
            "title,,created_at",
            "title;drop_table",
            "a,b,c",
            &too_long,
        ] {
            assert!(SortParams::parse(Some(value), &["title", "created_at", "a", "b"]).is_err());
        }
    }
}
