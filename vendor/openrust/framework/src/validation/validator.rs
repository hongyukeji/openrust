use std::{collections::BTreeMap, sync::Arc};

use crate::{
    contracts::validation::Validator as Contract, support::MessageBag, translation::Translator,
};

use super::{
    Data, DatabasePresenceVerifier, Rule, RuleContext, RuleFailure, RuleSet, ValidatedInput,
    ValidationError, ValidationException, ValidationFuture, ValidationResult, Value, path,
    rule::{Kind, integer, is_empty, size},
};

type AfterValidation = dyn Fn(&mut Validator) -> ValidationResult<()> + Send + Sync;

/// Evaluates the configured rules against one input value.
pub struct Validator {
    data: Data,
    rules: RuleSet,
    translator: Option<Arc<Translator>>,
    presence: Option<DatabasePresenceVerifier>,
    user_resolver: Option<Arc<super::current_password::UserResolver>>,
    messages: BTreeMap<String, String>,
    attributes: BTreeMap<String, String>,
    errors: MessageBag,
    result: Option<ValidatedInput>,
    after: Vec<Arc<AfterValidation>>,
    stop_on_first_failure: bool,
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
            user_resolver: None,
            messages,
            attributes,
            errors: MessageBag::default(),
            result: None,
            after: Vec::new(),
            stop_on_first_failure: false,
        }
    }

    pub fn after(
        &mut self,
        callback: impl Fn(&mut Self) -> ValidationResult<()> + Send + Sync + 'static,
    ) -> &mut Self {
        self.after.push(Arc::new(callback));
        self
    }

    #[allow(non_snake_case, reason = "遵循 Laravel Validator 命名")]
    pub fn stopOnFirstFailure(&mut self, stop: bool) -> &mut Self {
        self.stop_on_first_failure = stop;
        self
    }

    pub fn safe(&self) -> ValidationResult<ValidatedInput> {
        self.validated()
    }

    pub fn errors_mut(&mut self) -> &mut MessageBag {
        &mut self.errors
    }

    pub fn sometimes(
        &mut self,
        attribute: &str,
        rules: Vec<Rule>,
        condition: impl Fn(&Data, Option<&Value>) -> bool,
    ) -> ValidationResult<&mut Self> {
        for (path, _) in self.data.attributes(attribute)? {
            let parent = if attribute.contains('*') {
                path.rsplit_once('.')
                    .map(|(parent, _)| parent)
                    .unwrap_or("")
            } else {
                ""
            };
            let item = if parent.is_empty() {
                Some(self.data.as_value())
            } else {
                self.data
                    .attributes(parent)?
                    .into_iter()
                    .next()
                    .and_then(|(_, value)| value)
            };
            if condition(&self.data, item) {
                if let Some((_, existing)) =
                    self.rules.fields.iter_mut().find(|(name, _)| name == &path)
                {
                    existing.extend(rules.clone());
                } else {
                    self.rules = self.rules.clone().field(path, rules.clone())?;
                }
            }
        }
        Ok(self)
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

    /// Supplies identities explicitly when validation runs outside an HTTP request.
    /// The resolver receives the rule's guard argument; None means the application's default.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's authentication resolver naming"
    )]
    pub fn setUserResolver(
        &mut self,
        resolver: impl Fn(
            Option<&str>,
        )
            -> ValidationResult<Option<Arc<dyn crate::contracts::auth::Authenticatable>>>
        + Send
        + Sync
        + 'static,
    ) {
        self.user_resolver = Some(Arc::new(resolver));
    }

    /// Validates using this request's real named authentication guards.
    #[allow(non_snake_case, reason = "Matches Laravel's request validation naming")]
    pub async fn validateRequest(
        &mut self,
        request: &mut crate::http::Request,
    ) -> ValidationResult<ValidatedInput> {
        self.run(Some(request)).await?;
        self.validated()
    }

    async fn run(
        &mut self,
        mut request: Option<&mut crate::http::Request>,
    ) -> ValidationResult<()> {
        self.result = None;
        self.errors = MessageBag::default();
        let mut excluded: Vec<String> = Vec::new();
        let mut errors = MessageBag::default();
        let mut budget = path::Budget::default();
        let mut fields = Vec::new();
        'fields: for (index, (pattern, rules)) in self.rules.fields.iter().enumerate() {
            let nullable = contains(rules, |rule| matches!(rule, Kind::Nullable));
            let sometimes = contains(rules, |rule| matches!(rule, Kind::Sometimes));
            let bail = contains(rules, |rule| matches!(rule, Kind::Bail));
            let numeric = contains(rules, |rule| {
                matches!(rule, Kind::Integer | Kind::StrictInteger | Kind::Numeric)
            });
            let presence_rules = contains(rules, |rule| {
                matches!(rule, Kind::Exists(_) | Kind::Unique(_))
            });
            let segments = path::parse(pattern)?;
            let wildcard = presence_rules
                && segments
                    .iter()
                    .any(|part| matches!(part, path::Segment::Any));
            for (attribute, value) in path::expand(self.data.as_value(), &segments, &mut budget)? {
                if excluded
                    .iter()
                    .any(|field| attribute == *field || attribute.starts_with(&format!("{field}.")))
                {
                    continue;
                }
                if self.stop_on_first_failure && !errors.is_empty() {
                    break 'fields;
                }
                if !self.rules.accepts(index, &unescape(&attribute)) {
                    continue;
                }
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
                        if let Kind::ExcludeIf(other, values) = rule {
                            let other = dependent_path(pattern, &attribute, other)?;
                            if condition_matches(&self.data, &other, values)? {
                                excluded.push(attribute.clone());
                                break;
                            }
                        }
                        if failed && matches!(rule, Kind::Exists(_) | Kind::Unique(_)) {
                            continue;
                        }
                        let failures = if let Kind::RequiredIf(other, values) = rule {
                            let other = dependent_path(pattern, &attribute, other)?;
                            if condition_matches(&self.data, &other, values)?
                                && value.is_none_or(is_empty)
                                && self.data.file(&attribute).is_none()
                            {
                                {
                                    let mut failure = RuleFailure::new("validation.required_if");
                                    let actual = self
                                        .data
                                        .attributes(&other)?
                                        .into_iter()
                                        .next()
                                        .and_then(|(_, value)| value);
                                    failure.replacements.extend([
                                        ("other".into(), self.display_attribute(&other, &other)),
                                        ("value".into(), self.display_value(&other, actual)),
                                    ]);
                                    vec![failure]
                                }
                            } else {
                                Vec::new()
                            }
                        } else if let Kind::CurrentPassword(guard) = rule {
                            super::current_password::failures(
                                value,
                                guard.as_deref(),
                                nullable,
                                request.as_deref_mut(),
                                self.user_resolver.as_deref(),
                            )
                            .await?
                        } else {
                            self.evaluate(
                                rule,
                                value,
                                (pattern, &attribute),
                                attribute_for_query,
                                nullable,
                                numeric,
                            )
                            .await?
                        };
                        failed |= !failures.is_empty();
                        let stop = !failures.is_empty()
                            && (bail
                                || matches!(
                                    rule,
                                    Kind::Required
                                        | Kind::RequiredIf(..)
                                        | Kind::Accepted
                                        | Kind::Present
                                ));
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
                fields.push(attribute);
            }
        }
        for field in &excluded {
            self.data.forget(field)?;
        }
        let retained = |field: &str| {
            !excluded
                .iter()
                .any(|excluded| field == excluded || field.starts_with(&format!("{excluded}.")))
        };
        let mut retained_errors = MessageBag::default();
        for (field, messages) in errors.messages() {
            if retained(&field) {
                for message in messages {
                    retained_errors.add(field.clone(), message);
                }
            }
        }
        errors = retained_errors;
        if errors.is_empty() {
            let fields = fields
                .iter()
                .filter(|field| retained(field))
                .map(|field| {
                    Ok((
                        field.clone(),
                        self.data
                            .attributes(field)?
                            .into_iter()
                            .next()
                            .and_then(|(_, value)| value),
                    ))
                })
                .collect::<ValidationResult<Vec<_>>>()?;
            self.result = Some(ValidatedInput::project(&self.data, &fields)?);
        }
        self.errors = errors;
        for callback in self.after.clone() {
            callback(self)?;
        }
        if !self.errors.is_empty() {
            self.result = None;
        }
        Ok(())
    }

    async fn evaluate(
        &self,
        rule: &Kind,
        value: Option<&Value>,
        attribute: (&str, &str),
        attribute_for_query: &str,
        nullable: bool,
        numeric: bool,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let (pattern, attribute) = attribute;
        if let Kind::Password(password) = rule {
            return Ok(password.failures(value, nullable));
        }
        let file = self.data.file(attribute);
        if let Some(file) = file {
            let observed = bigdecimal::BigDecimal::from(file.getSize() as u64) / 1024;
            let (kind, bounds, passes) = match rule {
                Kind::Min(bound) => ("min", vec![("min", bound)], observed >= *bound),
                Kind::Max(bound) => ("max", vec![("max", bound)], observed <= *bound),
                Kind::Size(bound) => ("size", vec![("size", bound)], observed == *bound),
                Kind::Between(min, max) => (
                    "between",
                    vec![("min", min), ("max", max)],
                    observed >= *min && observed <= *max,
                ),
                _ => ("", Vec::new(), true),
            };
            if !kind.is_empty() {
                return Ok(if passes {
                    Vec::new()
                } else {
                    let mut failure = RuleFailure::new(format!("validation.{kind}.file"));
                    failure.replacements.extend(
                        bounds
                            .into_iter()
                            .map(|(name, value)| (name.into(), value.to_string())),
                    );
                    vec![failure]
                });
            }
        }
        let failure = match (rule, value) {
            (Kind::Required, value) if file.is_none() && value.is_none_or(is_empty) => {
                Some("required")
            }
            (Kind::Accepted, value) if value.is_none_or(is_empty) => Some("accepted"),
            (Kind::Present, None) => Some("present"),
            (_, None) => None,
            (_, Some(Value::Null)) if nullable && file.is_none() => None,
            (_, Some(Value::String(value)))
                if value
                    .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                    .is_empty() =>
            {
                None
            }
            (Kind::String, Some(value)) if !value.is_string() => Some("string"),
            (Kind::Integer, Some(value)) if integer(value).is_none() => Some("integer"),
            (Kind::StrictInteger, Some(value))
                if !value.is_i64()
                    && value
                        .as_u64()
                        .is_none_or(|value| i64::try_from(value).is_err()) =>
            {
                Some("integer")
            }
            (Kind::StrictBoolean, Some(value)) if !value.is_boolean() => Some("boolean"),
            (Kind::Numeric, Some(value)) if !super::standard_rules::numeric(value) => {
                Some("numeric")
            }
            (Kind::Date, Some(value))
                if !matches!(value, Value::String(_) | Value::Number(_))
                    || !super::standard_rules::text(value)
                        .is_some_and(|text| super::standard_rules::date(&text)) =>
            {
                Some("date")
            }
            (Kind::Accepted, Some(value))
                if !matches!(value, Value::Bool(true))
                    && value.as_i64() != Some(1)
                    && !matches!(value.as_str(), Some("yes" | "on" | "1" | "true")) =>
            {
                Some("accepted")
            }
            (Kind::Url, Some(value))
                if !value
                    .as_str()
                    .is_some_and(|value| crate::support::Str::isUrl(value, &[])) =>
            {
                Some("url")
            }
            (Kind::Uuid, Some(value))
                if !value
                    .as_str()
                    .is_some_and(|value| crate::support::Str::isUuid(value, None)) =>
            {
                Some("uuid")
            }
            (Kind::In(values), Some(value))
                if !{
                    let array_rule = self
                        .rules
                        .fields
                        .iter()
                        .find(|(name, _)| name == pattern)
                        .is_some_and(|(_, rules)| {
                            contains(rules, |rule| {
                                matches!(rule, Kind::Array | Kind::ArrayKeys(_))
                            })
                        });
                    match value {
                        Value::Array(items) if array_rule => items.iter().all(|item| {
                            super::standard_rules::text(item)
                                .is_some_and(|item| values.contains(&item))
                        }),
                        Value::Object(items) if array_rule => items.values().all(|item| {
                            super::standard_rules::text(item)
                                .is_some_and(|item| values.contains(&item))
                        }),
                        _ => super::standard_rules::text(value)
                            .is_some_and(|value| values.contains(&value)),
                    }
                } =>
            {
                Some("in")
            }
            (Kind::Regex(regex), Some(value))
                if !super::standard_rules::text(value)
                    .is_some_and(|value| regex.is_match(value.as_bytes()).unwrap_or(false)) =>
            {
                Some("regex")
            }
            (Kind::ArrayKeys(keys), Some(value))
                if match value {
                    Value::Object(value) => value.keys().any(|key| !keys.contains(key)),
                    Value::Array(value) => {
                        (0..value.len()).any(|index| !keys.contains(&index.to_string()))
                    }
                    _ => true,
                } =>
            {
                Some("array")
            }
            (Kind::Boolean, Some(value))
                if !value.is_boolean()
                    && !matches!(value.as_i64(), Some(0 | 1))
                    && !matches!(value.as_str(), Some("0" | "1")) =>
            {
                Some("boolean")
            }
            (Kind::Email, Some(value))
                if !value.as_str().is_some_and(super::rules::Email::passes) =>
            {
                Some("email")
            }
            (Kind::Confirmed(other), Some(value)) => {
                let other = other
                    .clone()
                    .unwrap_or_else(|| format!("{attribute}_confirmation"));
                let other = dependent_path(pattern, attribute, &other)?;
                let other = self
                    .data
                    .attributes(&other)?
                    .into_iter()
                    .next()
                    .and_then(|(_, value)| value);
                (!strict_same(value, other.unwrap_or(&Value::Null))).then_some("confirmed")
            }
            (Kind::Array, Some(value)) if !value.is_array() && !value.is_object() => Some("array"),
            (Kind::Dimensions(rule), Some(_)) => (!rule.passes(file)?).then_some("dimensions"),
            (Kind::Base64, Some(value)) if !value.as_str().is_some_and(canonical_base64) => {
                Some("base64")
            }
            (Kind::Encoding(rule), Some(value)) => {
                let valid = file.map_or_else(|| rule.value(value), |file| rule.bytes(&file.get()));
                return Ok((!valid).then(|| rule.failure()).into_iter().collect());
            }
            (Kind::File(rule), Some(_)) => return rule.failures(file),
            (Kind::Size(bound), Some(value)) => {
                return scalar_size(value, numeric, "size", &[("size", bound)], |n| n == bound);
            }
            (Kind::Between(min, max), Some(value)) => {
                return scalar_size(
                    value,
                    numeric,
                    "between",
                    &[("min", min), ("max", max)],
                    |n| n >= min && n <= max,
                );
            }
            (Kind::Min(bound) | Kind::Max(bound), Some(value)) => {
                let minimum = matches!(rule, Kind::Min(_));
                let kind = if minimum { "min" } else { "max" };
                return scalar_size(value, numeric, kind, &[(kind, bound)], |n| {
                    if minimum { n >= bound } else { n <= bound }
                });
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

    fn display_attribute(&self, pattern: &str, attribute: &str) -> String {
        self.attributes
            .get(attribute)
            .or_else(|| self.attributes.get(pattern))
            .cloned()
            .or_else(|| {
                let translator = self.translator.as_ref()?;
                [attribute, pattern].into_iter().find_map(|field| {
                    let key = format!("validation.attributes.{field}");
                    translator
                        .string(&key, &Default::default(), None)
                        .ok()
                        .filter(|text| text != &key)
                })
            })
            .unwrap_or_else(|| attribute.replace('_', " "))
    }

    fn display_value(&self, field: &str, value: Option<&Value>) -> String {
        if value.is_some_and(|value| value.is_array() || value.is_object()) {
            return "array".into();
        }
        let key_value = value
            .and_then(super::standard_rules::text)
            .unwrap_or_default();
        if let Some(translator) = &self.translator {
            let key = format!("validation.values.{field}.{key_value}");
            if let Ok(text) = translator.string(&key, &Default::default(), None)
                && text != key
            {
                return text;
            }
        }
        match value {
            Some(Value::Bool(value)) => value.to_string(),
            None | Some(Value::Null) => "empty".into(),
            _ => key_value,
        }
    }

    fn format_message(&self, pattern: &str, attribute: &str, mut failure: RuleFailure) -> String {
        let attribute = unescape(attribute);
        let display = self.display_attribute(pattern, &attribute);
        failure
            .replacements
            .entry("attribute".into())
            .or_insert(display);
        let rule_key = failure.message.strip_prefix("validation.");
        let rule = rule_key.and_then(|key| {
            if key.starts_with("password.") {
                Some(key)
            } else {
                key.split('.').next()
            }
        });
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
            self.run(None).await?;
            self.validated()
        })
    }

    fn fails(&mut self) -> ValidationFuture<'_, ValidationResult<bool>> {
        Box::pin(async move {
            self.run(None).await?;
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
    bounds: &[(&str, &bigdecimal::BigDecimal)],
    passes: impl FnOnce(&bigdecimal::BigDecimal) -> bool,
) -> ValidationResult<Vec<RuleFailure>> {
    if numeric {
        super::standard_rules::check_exponent(value)?;
    }
    let observed = size(value, numeric);
    if observed.as_ref().is_some_and(|(number, _)| passes(number)) {
        return Ok(Vec::new());
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
    Ok(vec![failure])
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

fn canonical_base64(value: &str) -> bool {
    use base64::{engine::general_purpose::STANDARD, read::DecoderReader};

    !value.is_empty()
        && std::io::copy(
            &mut DecoderReader::new(value.as_bytes(), &STANDARD),
            &mut std::io::sink(),
        )
        .is_ok()
}

fn fallback(rule: &str) -> &str {
    match rule {
        "current_password" => "The password is incorrect.",
        "confirmed" => "The :attribute field confirmation does not match.",
        "password.mixed" => {
            "The :attribute field must contain at least one uppercase and one lowercase letter."
        }
        "password.letters" => "The :attribute field must contain at least one letter.",
        "password.numbers" => "The :attribute field must contain at least one number.",
        "password.symbols" => "The :attribute field must contain at least one symbol.",
        "numeric" => "The :attribute field must be a number.",
        "date" => "The :attribute field must be a valid date.",
        "accepted" => "The :attribute field must be accepted.",
        "url" => "The :attribute field must be a valid URL.",
        "uuid" => "The :attribute field must be a valid UUID.",
        "in" => "The selected :attribute is invalid.",
        "regex" => "The :attribute field format is invalid.",
        "required_if" => "The :attribute field is required when :other is :value.",
        "required" => "The :attribute field is required.",
        "present" => "The :attribute field must be present.",
        "string" => "The :attribute field must be a string.",
        "integer" => "The :attribute field must be an integer.",
        "boolean" => "The :attribute field must be true or false.",
        "array" => "The :attribute field must be an array.",
        "base64" => "The :attribute field must be a valid Base64 string.",
        "exists" => "The selected :attribute is invalid.",
        "unique" => "The :attribute has already been taken.",
        "dimensions" => "The :attribute field has invalid image dimensions.",
        "encoding" => "The :attribute field must be encoded in :encoding.",
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

// PHP array identity includes key order and the types of every nested value.
fn strict_same(left: &Value, right: &Value) -> bool {
    fn entries(value: &Value) -> Option<Vec<(String, &Value)>> {
        match value {
            Value::Object(values) => Some(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value))
                    .collect(),
            ),
            Value::Array(values) => Some(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| (index.to_string(), value))
                    .collect(),
            ),
            _ => None,
        }
    }
    match (entries(left), entries(right)) {
        (Some(left), Some(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((a, x), (b, y))| a == &b && strict_same(x, y))
        }
        _ => left == right,
    }
}

fn dependent_path(pattern: &str, attribute: &str, other: &str) -> ValidationResult<String> {
    let actual = path::parse(attribute)?;
    let keys: Vec<_> = path::parse(pattern)?
        .into_iter()
        .zip(actual)
        .filter_map(|(pattern, actual)| matches!(pattern, path::Segment::Any).then_some(actual))
        .collect();
    let mut keys = keys.into_iter();
    let mut result = String::new();
    for segment in path::parse(other)? {
        let segment = if matches!(segment, path::Segment::Any) {
            keys.next().unwrap_or(segment)
        } else {
            segment
        };
        match segment {
            path::Segment::Key(key) => result = path::join(&result, &key)?,
            path::Segment::Any => return Err(ValidationError::InvalidPath),
        }
    }
    Ok(result)
}

fn condition_matches(data: &Data, field: &str, values: &[String]) -> ValidationResult<bool> {
    let value = data
        .attributes(field)?
        .into_iter()
        .next()
        .and_then(|(_, value)| value);
    Ok(value.is_some_and(|value| {
        values.iter().any(|expected| match value {
            Value::Bool(value) => expected == if *value { "true" } else { "false" },
            Value::Null => expected == "null",
            Value::Number(_) | Value::String(_) => {
                let actual = super::standard_rules::text(value).unwrap_or_default();
                match (
                    super::standard_rules::number(value),
                    super::standard_rules::number(&Value::String(expected.clone())),
                ) {
                    (Some(actual), Some(expected)) => actual == expected,
                    _ => actual == *expected,
                }
            }
            _ => false,
        })
    }))
}
