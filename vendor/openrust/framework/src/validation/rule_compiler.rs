use std::{error::Error, fmt, sync::Arc};

use serde_json::{Map, Value};

use super::{
    Data, NestedRuleSet, Rule, RuleSet, ValidationError, ValidationResult, path, rule::Kind,
};

/// 保留原始错误种类；只有不可复制的执行原因通过Arc共享。
#[derive(Clone, Debug)]
pub(super) struct CompilationError(Arc<ValidationError>);
impl From<ValidationError> for CompilationError {
    fn from(error: ValidationError) -> Self {
        Self(Arc::new(error))
    }
}
impl CompilationError {
    pub fn error(&self) -> ValidationError {
        match self.0.as_ref() {
            ValidationError::Invalid(error) => ValidationError::Invalid(error.clone()),
            ValidationError::InvalidPath => ValidationError::InvalidPath,
            ValidationError::InvalidRule => ValidationError::InvalidRule,
            ValidationError::NotValidated => ValidationError::NotValidated,
            ValidationError::LimitsExceeded => ValidationError::LimitsExceeded,
            ValidationError::Execution { .. } => ValidationError::Execution {
                source: Box::new(self.clone()),
            },
        }
    }
}
impl fmt::Display for CompilationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl Error for CompilationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.0.as_ref())
    }
}

pub(super) fn bind(attribute: &str, rules: NestedRuleSet) -> ValidationResult<RuleSet> {
    match rules {
        NestedRuleSet::Value(rules) => {
            path::parse(attribute)?;
            if rules.len() > 64 {
                return Err(ValidationError::LimitsExceeded);
            }
            let mut result = RuleSet::new();
            result.fields.push((attribute.to_owned(), rules));
            Ok(result)
        }
        NestedRuleSet::Fields(rules) => {
            if rules.fields.is_empty() {
                return bind(attribute, NestedRuleSet::Value(Vec::new()));
            }
            let mut bound = RuleSet::new();
            for (field, values) in rules.fields {
                let key = format!("{attribute}.{field}");
                path::parse(&key)?;
                if values.is_empty() {
                    bound.fields.push((key, values));
                } else {
                    bound = bound.field(key, values)?;
                }
            }
            Ok(bound)
        }
    }
}

impl RuleSet {
    pub(super) fn compile_rules(self, data: &Data) -> ValidationResult<Self> {
        self.compile_scoped(data, data)
    }

    pub(super) fn compile_scoped(self, original: &Data, data: &Data) -> ValidationResult<Self> {
        if !self
            .fields
            .iter()
            .any(|(_, rules)| rules.iter().any(Rule::is_compilable))
        {
            return Ok(self);
        }
        let mut output = Self::new();
        output.expanded = true;
        let mut compiler = Compiler {
            original,
            output,
            budget: path::Budget::default(),
        };
        for (index, (attribute, rules)) in self.fields.iter().enumerate() {
            let mut selections = self.selections.get(attribute).cloned().unwrap_or_default();
            selections.extend(
                self.precognitive
                    .iter()
                    .filter(|(end, _)| index < *end)
                    .map(|(_, filter)| filter.clone()),
            );
            compiler.field(
                attribute,
                self.origin(attribute),
                rules,
                &selections,
                0,
                data,
            )?;
        }
        Ok(compiler.output)
    }
}
struct Compiler<'a> {
    original: &'a Data,
    output: RuleSet,
    budget: path::Budget,
}
impl Compiler<'_> {
    fn emit(
        &mut self,
        attribute: &str,
        origin: &str,
        rules: Vec<Rule>,
        selections: &[regex::bytes::RegexSet],
    ) -> ValidationResult<()> {
        if let Some((_, current)) = self
            .output
            .fields
            .iter_mut()
            .find(|(field, _)| field == attribute)
        {
            if current.len() + rules.len() > 64 {
                return Err(ValidationError::LimitsExceeded);
            }
            current.extend(rules);
        } else {
            path::parse(attribute)?;
            if self.output.fields.len() >= path::MAX_EXPANDED || rules.len() > 64 {
                return Err(ValidationError::LimitsExceeded);
            }
            self.output.fields.push((attribute.to_owned(), rules));
        }
        if attribute != origin {
            self.output
                .origins
                .entry(attribute.to_owned())
                .or_insert_with(|| origin.to_owned());
        }
        if !selections.is_empty() {
            self.output
                .selections
                .entry(attribute.to_owned())
                .or_insert_with(|| selections.to_vec());
        }
        Ok(())
    }
    fn field(
        &mut self,
        attribute: &str,
        origin: &str,
        rules: &[Rule],
        selections: &[regex::bytes::RegexSet],
        depth: usize,
        master: &Data,
    ) -> ValidationResult<()> {
        if depth > 64 {
            return Err(ValidationError::LimitsExceeded);
        }
        if rules
            .iter()
            .any(|rule| matches!(rule.0, Kind::ConditionalRules(_)))
        {
            let mut resolved = Vec::new();
            for rule in rules {
                if let Kind::ConditionalRules(rule) = &rule.0 {
                    resolved.extend_from_slice(rule.resolve(master)?);
                } else {
                    resolved.push(rule.clone());
                }
            }
            return self.field(attribute, origin, &resolved, selections, depth + 1, master);
        }
        if !rules.iter().any(Rule::is_compilable) {
            return self.emit(attribute, origin, rules.to_vec(), selections);
        }
        let segments = path::parse(attribute)?;
        let wildcard = segments.iter().any(|s| matches!(s, path::Segment::Any));
        let matches = path::expand(master.as_value(), &segments, &mut self.budget)?;
        let flat = gather(master, attribute, wildcard, &matches)?;
        for (actual, matched) in matches {
            let parent = if wildcard {
                let parent = path::parent(&actual)?.unwrap_or_else(|| actual.clone());
                let name = parent.as_str();
                master
                    .attributes(name)?
                    .into_iter()
                    .next()
                    .and_then(|(_, value)| value)
            } else {
                Some(master.as_value())
            };
            let value = if wildcard {
                matched
            } else {
                match master.as_value() {
                    Value::Object(fields) => fields.get(&super::validator::unescape(attribute)),
                    Value::Array(fields) => {
                        attribute.parse::<usize>().ok().and_then(|i| fields.get(i))
                    }
                    _ => None,
                }
            };
            for rule in rules {
                let compiled = match &rule.0 {
                    Kind::Chained(rules) => bind(&actual, NestedRuleSet::Value(rules.clone()))?,
                    Kind::ForEach(rule) => bind(
                        &actual,
                        rule.resolve(&super::NestedRuleContext {
                            value,
                            attribute: &actual,
                            data: self.original,
                            flattened: &flat,
                            parent,
                        })?,
                    )?,
                    Kind::Compilable(rule) => rule.compile_context(&super::NestedRuleContext {
                        value,
                        attribute: &actual,
                        data: self.original,
                        flattened: &flat,
                        parent,
                    })?,
                    _ => {
                        self.emit(&actual, origin, vec![rule.clone()], selections)?;
                        continue;
                    }
                };
                for (child, child_rules) in &compiled.fields {
                    let child_origin = compiled.origin(child).strip_prefix(&actual).map_or_else(
                        || compiled.origin(child).to_owned(),
                        |suffix| format!("{origin}{suffix}"),
                    );
                    let nested_data = if child_rules.iter().any(Rule::is_compilable) {
                        Some(Data::new(path::from_flat(&flat)?)?)
                    } else {
                        None
                    };
                    self.field(
                        child,
                        &child_origin,
                        child_rules,
                        selections,
                        depth + 1,
                        nested_data.as_ref().unwrap_or(master),
                    )?;
                }
            }
        }
        Ok(())
    }
}
fn gather(
    data: &Data,
    attribute: &str,
    wildcard: bool,
    matches: &[(String, Option<&Value>)],
) -> ValidationResult<Map<String, Value>> {
    fn dot(value: &Value, prefix: &str, flat: &mut Map<String, Value>) -> ValidationResult<()> {
        match value {
            Value::Object(fields) if !fields.is_empty() => {
                for (key, value) in fields {
                    dot(value, &path::join(prefix, key)?, flat)?;
                }
            }
            Value::Array(fields) if !fields.is_empty() => {
                for (index, value) in fields.iter().enumerate() {
                    dot(value, &path::join(prefix, &index.to_string())?, flat)?;
                }
            }
            _ if !prefix.is_empty() => {
                flat.insert(prefix.to_owned(), value.clone());
            }
            _ => {}
        }
        Ok(())
    }
    let mut flat = Map::new();
    dot(data.as_value(), "", &mut flat)?;
    if wildcard {
        let mut prefix = String::new();
        for segment in path::parse(attribute)? {
            match segment {
                path::Segment::Key(key) => prefix = path::join(&prefix, &key)?,
                path::Segment::Any => break,
            }
        }
        if !prefix.is_empty() {
            let child = format!("{prefix}.");
            flat.retain(|key, _| key == &prefix || key.starts_with(&child));
        }
        for (field, value) in matches {
            flat.insert(field.to_owned(), value.cloned().unwrap_or(Value::Null));
        }
    }
    Ok(flat)
}

impl crate::contracts::validation::CompilableRules for CompilationError {
    fn compile(
        &self,
        _: &str,
        _: Option<&Value>,
        _: &Map<String, Value>,
        _: Option<&Value>,
    ) -> ValidationResult<RuleSet> {
        Err(self.error())
    }
}
