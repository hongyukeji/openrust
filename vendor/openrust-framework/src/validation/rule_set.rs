use super::{Rule, ValidationError, ValidationResult, path};

/// Field rules in declaration order.
#[derive(Clone, Default)]
pub struct RuleSet {
    pub(super) fields: Vec<(String, Vec<Rule>)>,
    pub(super) expanded: bool,
    pub(super) origins: std::collections::BTreeMap<String, String>,
    pub(super) selections: std::collections::BTreeMap<String, Vec<regex::bytes::RegexSet>>,
    pub(super) precognitive: Vec<(usize, regex::bytes::RegexSet)>,
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
        if self.fields.len()
            >= if self.expanded {
                path::MAX_EXPANDED
            } else {
                path::MAX_FIELDS
            }
            || self.fields.iter().any(|(field, _)| field == &attribute)
        {
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

    pub(super) fn merged(&self, other: Self) -> ValidationResult<Self> {
        let mut merged = self.clone();
        merged.expanded |= other.expanded;
        // 过滤器作用于接收它的声明集合，不能静默丢弃传入过滤条件。
        if !other.precognitive.is_empty() {
            return Err(ValidationError::InvalidRule);
        }
        for (attribute, origin) in other.origins {
            merged.origins.entry(attribute).or_insert(origin);
        }
        for (attribute, selections) in other.selections {
            merged.selections.entry(attribute).or_insert(selections);
        }
        for (attribute, rules) in other.fields {
            if let Some((_, existing)) = merged
                .fields
                .iter_mut()
                .find(|(field, _)| field == &attribute)
            {
                if existing.len() + rules.len() > 64 {
                    return Err(ValidationError::LimitsExceeded);
                }
                existing.extend(rules);
            } else if rules.is_empty() {
                path::parse(&attribute)?;
                if merged.fields.len() >= path::MAX_EXPANDED {
                    return Err(ValidationError::LimitsExceeded);
                }
                merged.fields.push((attribute, rules));
            } else {
                merged = merged.field(attribute, rules)?;
            }
        }
        Ok(merged)
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

    pub(super) fn origin<'a>(&'a self, attribute: &'a str) -> &'a str {
        self.origins
            .get(attribute)
            .map_or(attribute, String::as_str)
    }

    pub(super) fn accepts(&self, index: usize, attribute: &str) -> bool {
        self.selections
            .get(&self.fields[index].0)
            .is_none_or(|filters| {
                filters
                    .iter()
                    .all(|filter| filter.is_match(attribute.as_bytes()))
            })
            && self
                .precognitive
                .iter()
                .all(|(end, patterns)| index >= *end || patterns.is_match(attribute.as_bytes()))
    }
}
