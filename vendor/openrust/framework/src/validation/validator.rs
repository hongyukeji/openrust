use std::{collections::BTreeMap, sync::Arc};

use crate::{
    contracts::validation::Validator as Contract, support::MessageBag, translation::Translator,
};

use super::{
    Data, Rule, RuleContext, RuleFailure, RuleSet, ValidatedInput, ValidationError,
    ValidationException, ValidationFuture, ValidationResult, Value, path,
    rule::{Kind, integer, is_empty, size},
};

/// Evaluates the configured rules against one input value.
pub struct Validator {
    data: Data,
    rules: RuleSet,
    translator: Option<Arc<Translator>>,
    messages: BTreeMap<String, String>,
    attributes: BTreeMap<String, String>,
    errors: MessageBag,
    result: Option<ValidatedInput>,
}

impl Validator {
    pub(super) fn new(
        data: Data,
        rules: RuleSet,
        translator: Option<Arc<Translator>>,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> Self {
        Self {
            data,
            rules,
            translator,
            messages,
            attributes,
            errors: MessageBag::default(),
            result: None,
        }
    }

    async fn run(&mut self) -> ValidationResult<()> {
        self.result = None;
        self.errors = MessageBag::default();
        let mut errors = MessageBag::default();
        let mut budget = path::Budget::default();
        let mut fields = Vec::new();
        let context = RuleContext { data: &self.data };
        for (pattern, rules) in &self.rules.0 {
            let nullable = contains(rules, |rule| matches!(rule, Kind::Nullable));
            let sometimes = contains(rules, |rule| matches!(rule, Kind::Sometimes));
            let bail = contains(rules, |rule| matches!(rule, Kind::Bail));
            let numeric = contains(rules, |rule| matches!(rule, Kind::Integer));
            for (attribute, value) in self.data.expand(pattern, &mut budget)? {
                if !sometimes || value.is_some() {
                    for Rule(rule) in rules {
                        let failures =
                            evaluate(rule, value, &context, &attribute, nullable, numeric).await?;
                        let failed = !failures.is_empty();
                        for failure in failures {
                            errors.add(
                                unescape(&attribute),
                                self.format_message(pattern, &attribute, failure),
                            );
                        }
                        if failed && (bail || matches!(rule, Kind::Required | Kind::Present)) {
                            break;
                        }
                    }
                }
                fields.push((attribute, value));
            }
        }
        if errors.is_empty() {
            self.result = Some(ValidatedInput::project(self.data.as_value(), &fields)?);
        }
        self.errors = errors;
        Ok(())
    }

    fn format_message(&self, pattern: &str, attribute: &str, mut failure: RuleFailure) -> String {
        let attribute = unescape(attribute);
        let display = self
            .attributes
            .get(&attribute)
            .or_else(|| self.attributes.get(pattern))
            .cloned()
            .or_else(|| {
                let translator = self.translator.as_ref()?;
                [&attribute, pattern].into_iter().find_map(|field| {
                    let key = format!("validation.attributes.{field}");
                    translator
                        .string(&key, &Default::default(), None)
                        .ok()
                        .filter(|text| text != &key)
                })
            })
            .unwrap_or_else(|| attribute.replace('_', " "));
        failure
            .replacements
            .entry("attribute".into())
            .or_insert(display);
        let rule_key = failure.message.strip_prefix("validation.");
        let rule = rule_key.and_then(|key| key.split('.').next());
        if let Some(rule) = rule
            && let Some(message) = self
                .messages
                .get(&format!("{attribute}.{rule}"))
                .or_else(|| self.messages.get(&format!("{pattern}.{rule}")))
                .or_else(|| self.messages.get(rule))
        {
            return replace(message, &failure.replacements);
        }
        if let Some(translator) = &self.translator {
            if let Some(rule) = rule {
                for field in [&attribute, pattern] {
                    let key = format!("validation.custom.{field}.{rule}");
                    if let Ok(message) = translator.string(&key, &failure.replacements, None)
                        && message != key
                    {
                        return message;
                    }
                }
            }
            if let Ok(message) = translator.string(&failure.message, &failure.replacements, None)
                && message != failure.message
            {
                return message;
            }
        }
        replace(
            rule_key.map_or(failure.message.as_str(), fallback),
            &failure.replacements,
        )
    }
}

impl Contract for Validator {
    fn validate(&mut self) -> ValidationFuture<'_, ValidationResult<ValidatedInput>> {
        Box::pin(async move {
            self.run().await?;
            self.validated()
        })
    }

    fn fails(&mut self) -> ValidationFuture<'_, ValidationResult<bool>> {
        Box::pin(async move {
            self.run().await?;
            Ok(!self.errors.is_empty())
        })
    }

    fn validated(&self) -> ValidationResult<ValidatedInput> {
        if !self.errors.is_empty() {
            return Err(ValidationError::Invalid(Box::new(
                ValidationException::localized(self.errors.clone(), self.translator.as_deref()),
            )));
        }
        self.result.clone().ok_or(ValidationError::NotValidated)
    }

    fn errors(&self) -> &MessageBag {
        &self.errors
    }
}

fn contains(rules: &[Rule], test: impl Fn(&Kind) -> bool) -> bool {
    rules.iter().any(|Rule(rule)| test(rule))
}

async fn evaluate(
    rule: &Kind,
    value: Option<&Value>,
    context: &RuleContext<'_>,
    attribute: &str,
    nullable: bool,
    numeric: bool,
) -> ValidationResult<Vec<RuleFailure>> {
    let failure = match (rule, value) {
        (Kind::Required, value) if value.is_none_or(is_empty) => Some("required"),
        (Kind::Present, None) => Some("present"),
        (_, None) => None,
        (_, Some(Value::Null)) if nullable => None,
        (_, Some(Value::String(value))) if value.trim().is_empty() => None,
        (Kind::String, Some(value)) if !value.is_string() => Some("string"),
        (Kind::Integer, Some(value)) if integer(value).is_none() => Some("integer"),
        (Kind::Boolean, Some(value))
            if !value.is_boolean()
                && !matches!(value.as_i64(), Some(0 | 1))
                && !matches!(value.as_str(), Some("0" | "1")) =>
        {
            Some("boolean")
        }
        (Kind::Array, Some(value)) if !value.is_array() && !value.is_object() => Some("array"),
        (Kind::Min(bound) | Kind::Max(bound), Some(value)) => {
            let minimum = matches!(rule, Kind::Min(_));
            let kind = if minimum { "min" } else { "max" };
            let observed = size(value, numeric);
            if observed.is_some_and(|(number, _)| {
                if minimum {
                    number >= *bound as i128
                } else {
                    number <= *bound as i128
                }
            }) {
                return Ok(Vec::new());
            }
            let category = observed.map_or(
                if numeric { "numeric" } else { "string" },
                |(_, category)| category,
            );
            let mut failure = RuleFailure::new(format!("validation.{kind}.{category}"));
            failure.replacements.insert(kind.into(), bound.to_string());
            return Ok(vec![failure]);
        }
        (Kind::Custom(rule), Some(value)) => {
            return rule.validate(context, attribute, Some(value)).await;
        }
        _ => None,
    };
    Ok(failure
        .into_iter()
        .map(|rule| RuleFailure::new(format!("validation.{rule}")))
        .collect())
}

fn unescape(attribute: &str) -> String {
    let mut escaped = false;
    attribute
        .chars()
        .filter(|character| {
            if escaped {
                escaped = false;
                return true;
            }
            escaped = *character == '\\';
            !escaped
        })
        .collect()
}

fn replace(template: &str, replacements: &BTreeMap<String, String>) -> String {
    let mut output = String::new();
    let mut tail = template;
    while let Some((prefix, value)) = tail.split_once(':') {
        output.push_str(prefix);
        let end = value
            .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .unwrap_or(value.len());
        if let Some(replacement) = replacements.get(&value[..end]) {
            output.push_str(replacement);
        } else {
            output.push(':');
            output.push_str(&value[..end]);
        }
        tail = &value[end..];
    }
    output.push_str(tail);
    output
}

fn fallback(rule: &str) -> &str {
    match rule {
        "required" => "The :attribute field is required.",
        "present" => "The :attribute field must be present.",
        "string" => "The :attribute field must be a string.",
        "integer" => "The :attribute field must be an integer.",
        "boolean" => "The :attribute field must be true or false.",
        "array" => "The :attribute field must be an array.",
        "min.string" => "The :attribute field must be at least :min characters.",
        "max.string" => "The :attribute field must not be greater than :max characters.",
        "min.array" => "The :attribute field must have at least :min items.",
        "max.array" => "The :attribute field must not have more than :max items.",
        "min.numeric" => "The :attribute field must be at least :min.",
        "max.numeric" => "The :attribute field must not be greater than :max.",
        _ => rule,
    }
}
