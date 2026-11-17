use std::{collections::BTreeMap, sync::Arc};

use crate::{
    contracts::validation::Validator as Contract, support::MessageBag, translation::Translator,
};

use super::{
    Data, DatabasePresenceVerifier, Rule, RuleContext, RuleFailure, RuleSet, ValidatedInput,
    ValidationError, ValidationException, ValidationFuture, ValidationResult, Value, path,
    rule::{Kind, integer, is_empty, size},
    rules::size_failure,
};

/// Evaluates the configured rules against one input value.
pub struct Validator {
    data: Data,
    rules: RuleSet,
    translator: Option<Arc<Translator>>,
    presence: Option<DatabasePresenceVerifier>,
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
        presence: Option<DatabasePresenceVerifier>,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> Self {
        Self {
            data,
            rules,
            translator,
            presence,
            messages,
            attributes,
            errors: MessageBag::default(),
            result: None,
        }
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Validator API")]
    pub async fn validateWithBag(&mut self, error_bag: &str) -> ValidationResult<ValidatedInput> {
        self.validate().await.map_err(|mut error| {
            if let ValidationError::Invalid(exception) = &mut error {
                exception.errorBag(error_bag);
            }
            error
        })
    }

    async fn run(&mut self) -> ValidationResult<()> {
        self.result = None;
        self.errors = MessageBag::default();
        let mut errors = MessageBag::default();
        let mut budget = path::Budget::default();
        let mut fields = Vec::new();
        for (pattern, rules) in &self.rules.0 {
            let nullable = contains(rules, |rule| matches!(rule, Kind::Nullable));
            let sometimes = contains(rules, |rule| matches!(rule, Kind::Sometimes));
            let bail = contains(rules, |rule| matches!(rule, Kind::Bail));
            let numeric = contains(rules, |rule| matches!(rule, Kind::Integer));
            let presence_rules = contains(rules, |rule| {
                matches!(rule, Kind::Exists(_) | Kind::Unique(_))
            });
            let segments = path::parse(pattern)?;
            let wildcard = presence_rules
                && segments
                    .iter()
                    .any(|part| matches!(part, path::Segment::Any));
            for (attribute, value) in path::expand(self.data.as_value(), &segments, &mut budget)? {
                if !sometimes || value.is_some() {
                    let attribute_for_query = if wildcard {
                        attribute
                            .rsplit('.')
                            .next()
                            .filter(|last| last.parse::<f64>().is_err())
                            .unwrap_or(&attribute)
                    } else {
                        &attribute
                    };
                    let mut failed =
                        presence_rules && !errors.get(&unescape(&attribute)).is_empty();
                    for Rule(rule) in rules {
                        if failed && matches!(rule, Kind::Exists(_) | Kind::Unique(_)) {
                            continue;
                        }
                        let failures = self
                            .evaluate(
                                rule,
                                value,
                                &attribute,
                                attribute_for_query,
                                nullable,
                                numeric,
                            )
                            .await?;
                        failed |= !failures.is_empty();
                        let stop = !failures.is_empty()
                            && (bail || matches!(rule, Kind::Required | Kind::Present));
                        for failure in failures {
                            errors.add(
                                unescape(&attribute),
                                self.format_message(pattern, &attribute, failure),
                            );
                        }
                        if stop {
                            break;
                        }
                    }
                }
                fields.push((attribute, value));
            }
        }
        if errors.is_empty() {
            self.result = Some(ValidatedInput::project(&self.data, &fields)?);
        }
        self.errors = errors;
        Ok(())
    }

    async fn evaluate(
        &self,
        rule: &Kind,
        value: Option<&Value>,
        attribute: &str,
        attribute_for_query: &str,
        nullable: bool,
        numeric: bool,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let file = self.data.file(attribute);
        if let Some(file) = file {
            let bounds = match rule {
                Kind::Min(bound) => Some((Some(*bound), None)),
                Kind::Max(bound) => Some((None, Some(*bound))),
                Kind::Size(bound) => Some((Some(*bound), Some(*bound))),
                Kind::Between(min, max) => Some((Some(*min), Some(*max))),
                _ => None,
            };
            if let Some((min, max)) = bounds {
                return Ok(size_failure(file.getSize(), min, max).into_iter().collect());
            }
        }
        let failure = match (rule, value) {
            (Kind::Required, value) if file.is_none() && value.is_none_or(is_empty) => {
                Some("required")
            }
            (Kind::Present, None) => Some("present"),
            (_, None) => None,
            (_, Some(Value::Null)) if nullable && file.is_none() => None,
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
            (Kind::File(rule), Some(_)) => return rule.failures(file),
            (Kind::Size(bound), Some(value)) => {
                return Ok(scalar_size(
                    value,
                    numeric,
                    "size",
                    &[("size", *bound)],
                    |n| n == *bound as i128,
                ));
            }
            (Kind::Between(min, max), Some(value)) => {
                return Ok(scalar_size(
                    value,
                    numeric,
                    "between",
                    &[("min", *min), ("max", *max)],
                    |n| n >= *min as i128 && n <= *max as i128,
                ));
            }
            (Kind::Min(bound) | Kind::Max(bound), Some(value)) => {
                let minimum = matches!(rule, Kind::Min(_));
                let kind = if minimum { "min" } else { "max" };
                return Ok(scalar_size(value, numeric, kind, &[(kind, *bound)], |n| {
                    if minimum {
                        n >= *bound as i128
                    } else {
                        n <= *bound as i128
                    }
                }));
            }
            (Kind::Exists(_), Some(_)) if file.is_some() => Some("exists"),
            (Kind::Unique(_), Some(_)) if file.is_some() => Some("unique"),
            (Kind::Exists(rule), Some(value)) => {
                let column = rule
                    .target
                    .column
                    .clone()
                    .unwrap_or_else(|| unescape(attribute_for_query));
                (!self
                    .presence()?
                    .exists(&rule.target, &column, value)
                    .await?)
                    .then_some("exists")
            }
            (Kind::Unique(rule), Some(value)) => {
                let column = rule
                    .target
                    .column
                    .clone()
                    .unwrap_or_else(|| unescape(attribute_for_query));
                (!self
                    .presence()?
                    .unique(rule, &column, value, &self.data)
                    .await?)
                    .then_some("unique")
            }
            (Kind::Custom(rule), Some(value)) => {
                let context = RuleContext { data: &self.data };
                return rule.validate(&context, attribute, Some(value)).await;
            }
            _ => None,
        };
        Ok(failure
            .into_iter()
            .map(|rule| RuleFailure::new(format!("validation.{rule}")))
            .collect())
    }

    fn presence(&self) -> ValidationResult<&DatabasePresenceVerifier> {
        self.presence
            .as_ref()
            .ok_or_else(|| ValidationError::Execution {
                source: std::io::Error::other("database presence verifier is not configured")
                    .into(),
            })
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

fn scalar_size(
    value: &Value,
    numeric: bool,
    rule: &str,
    bounds: &[(&str, usize)],
    passes: impl FnOnce(i128) -> bool,
) -> Vec<RuleFailure> {
    let observed = size(value, numeric);
    if observed.is_some_and(|(number, _)| passes(number)) {
        return Vec::new();
    }
    let category = observed.map_or(
        if numeric { "numeric" } else { "string" },
        |(_, category)| category,
    );
    let mut failure = RuleFailure::new(format!("validation.{rule}.{category}"));
    failure.replacements.extend(
        bounds
            .iter()
            .map(|(key, bound)| ((*key).into(), bound.to_string())),
    );
    vec![failure]
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
        "exists" => "The selected :attribute is invalid.",
        "unique" => "The :attribute has already been taken.",
        "file" => "The :attribute field must be a file.",
        "image" => "The :attribute field must be an image.",
        "mimes" | "mimetypes" => "The :attribute field must be a file of type: :values.",
        "extensions" => "The :attribute field must have one of the following extensions: :values.",
        "min.file" => "The :attribute field must be at least :min kilobytes.",
        "max.file" => "The :attribute field must not be greater than :max kilobytes.",
        "size.file" => "The :attribute field must be :size kilobytes.",
        "between.file" => "The :attribute field must be between :min and :max kilobytes.",
        "size.string" => "The :attribute field must be :size characters.",
        "size.array" => "The :attribute field must contain :size items.",
        "size.numeric" => "The :attribute field must be :size.",
        "between.string" => "The :attribute field must be between :min and :max characters.",
        "between.array" => "The :attribute field must have between :min and :max items.",
        "between.numeric" => "The :attribute field must be between :min and :max.",
        "min.string" => "The :attribute field must be at least :min characters.",
        "max.string" => "The :attribute field must not be greater than :max characters.",
        "min.array" => "The :attribute field must have at least :min items.",
        "max.array" => "The :attribute field must not have more than :max items.",
        "min.numeric" => "The :attribute field must be at least :min.",
        "max.numeric" => "The :attribute field must not be greater than :max.",
        _ => rule,
    }
}
