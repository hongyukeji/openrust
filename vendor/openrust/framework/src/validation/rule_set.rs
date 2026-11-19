use super::{Rule, ValidationError, ValidationResult, path};

/// Field rules in declaration order.
#[derive(Clone, Default)]
pub struct RuleSet {
    pub(super) fields: Vec<(String, Vec<Rule>)>,
    precognitive: Vec<(usize, regex::bytes::RegexSet)>,
}

impl RuleSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn field(
        mut self,
        attribute: impl Into<String>,
        rules: impl IntoIterator<Item = Rule>,
    ) -> ValidationResult<Self> {
        let attribute = attribute.into();
        path::parse(&attribute)?;
        if self.fields.len() >= 1024 || self.fields.iter().any(|(field, _)| field == &attribute) {
            return Err(ValidationError::InvalidRule);
        }
        let rules: Vec<_> = rules.into_iter().take(65).collect();
        if rules.is_empty() || rules.len() > 64 {
            return Err(ValidationError::InvalidRule);
        }
        self.fields.push((attribute, rules));
        Ok(self)
    }

    pub fn parse<'a>(
        declarations: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> ValidationResult<Self> {
        declarations
            .into_iter()
            .try_fold(Self::new(), |rules, (attribute, spec)| {
                if spec.len() > 4096 {
                    return Err(ValidationError::LimitsExceeded);
                }
                let field = spec
                    .split('|')
                    .map(Rule::parse)
                    .collect::<ValidationResult<Vec<_>>>()?;
                rules.field(attribute, field)
            })
    }

    pub(crate) fn precognitive(mut self, header: &[u8]) -> ValidationResult<Self> {
        use std::fmt::Write;
        let patterns = header.split(|byte| *byte == b',').map(|pattern| {
            let mut regex = String::from(r"(?-u:\A(?:");
            for byte in pattern {
                if *byte == b'*' {
                    regex.push_str("[^.]+");
                } else {
                    write!(&mut regex, "\\x{byte:02x}").expect("writing to a String");
                }
            }
            // PHP's $ anchor also matches immediately before a final newline.
            regex.push_str(r")(?:\n)?\z)");
            regex
        });
        let patterns =
            regex::bytes::RegexSet::new(patterns).map_err(|_| ValidationError::LimitsExceeded)?;
        self.precognitive.push((self.fields.len(), patterns));
        Ok(self)
    }

    pub(super) fn accepts(&self, index: usize, attribute: &str) -> bool {
        self.precognitive
            .iter()
            .all(|(end, patterns)| index >= *end || patterns.is_match(attribute.as_bytes()))
    }
}
