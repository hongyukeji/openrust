use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use crate::{
    contracts::validation::Validator as Contract, support::MessageBag, translation::Translator,
};

use super::{
    Data, DatabasePresenceVerifier, Rule, RuleContext, RuleFailure, RuleSet, ValidatedInput,
    ValidationError, ValidationException, ValidationFuture, ValidationResult, Value, path,
    rule::{Kind, integer, is_empty, size},
};

/// Rust 使用类型化异常工厂替代运行期异常类名。
pub type ValidationExceptionFactory = fn(MessageBag, Option<&Translator>) -> ValidationException;

type AfterValidation = dyn Fn(&mut Validator) -> ValidationResult<()> + Send + Sync;

#[derive(Clone, Copy)]
struct RuleFlags {
    nullable: bool,
    sometimes: bool,
    bail: bool,
    numeric: bool,
    array: bool,
    implicit: bool,
}
impl RuleFlags {
    fn category(self, file: bool) -> &'static str {
        if self.numeric {
            "numeric"
        } else if self.array {
            "array"
        } else if file {
            "file"
        } else {
            "string"
        }
    }
    fn new(rules: &[Rule], extensions: &super::extensions::Extensions) -> Self {
        Self {
            array: rules.iter().any(|rule| {
                matches!(rule.0, Kind::Array | Kind::ArrayKeys(_))
                    || matches!(&rule.0, Kind::Scalar(rule) if rule.name() == "list")
            }),
            implicit: contains(rules, |rule| implicit_builtin(rule, extensions)),
            nullable: contains(rules, |rule| matches!(rule, Kind::Nullable)),
            sometimes: contains(rules, |rule| matches!(rule, Kind::Sometimes)),
            bail: contains(rules, |rule| matches!(rule, Kind::Bail)),
            numeric: contains(rules, |rule| {
                matches!(
                    rule,
                    Kind::Integer | Kind::StrictInteger | Kind::Numeric | Kind::StrictNumeric
                ) || matches!(rule,Kind::Scalar(rule)if rule.name()=="decimal")
            }),
        }
    }
}

/// Evaluates the configured rules against one input value.
pub struct Validator {
    data: Arc<Data>,
    pub(super) factory: Option<super::Factory>,
    pub(super) extensions: super::extensions::Extensions,
    rules: Arc<RuleSet>,
    initial_rules: RuleSet,
    running: bool,
    compilation_error: Option<super::rule_compiler::CompilationError>,
    exception: ValidationExceptionFactory,
    translator: Option<Arc<Translator>>,
    pub(super) presence: Option<DatabasePresenceVerifier>,
    user_resolver: Option<Arc<super::current_password::UserResolver>>,
    messages: BTreeMap<String, String>,
    attributes: BTreeMap<String, String>,
    values: BTreeMap<String, BTreeMap<String, String>>,
    errors: MessageBag,
    result: Option<ValidatedInput>,
    pub exclude_unvalidated_array_keys: bool,
    after: Vec<Arc<AfterValidation>>,
    stop_on_first_failure: bool,
    fake_dns_lookups: Option<bool>,
    pub(super) dns_policy: Arc<std::sync::atomic::AtomicBool>,
    dns_resolver: Option<hickory_resolver::TokioResolver>,
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
        Self::from_shared(
            Arc::new(data),
            rules,
            translator,
            presence,
            messages,
            attributes,
        )
    }

    pub(super) fn from_shared(
        data: Arc<Data>,
        rules: RuleSet,
        translator: Option<Arc<Translator>>,
        presence: Option<DatabasePresenceVerifier>,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> Self {
        let initial_rules = rules.clone();
        let (rules, compilation_error) = match rules.compile_rules(&data) {
            Ok(rules) => (rules, None),
            Err(error) => (RuleSet::new(), Some(error.into())),
        };
        Self {
            data,
            factory: None,
            running: false,
            extensions: Default::default(),
            initial_rules,
            compilation_error,
            exception: ValidationException::localized,
            rules: Arc::new(rules),
            translator,
            presence,
            user_resolver: None,
            messages,
            attributes,
            values: BTreeMap::new(),
            errors: MessageBag::default(),
            result: None,
            exclude_unvalidated_array_keys: true,
            after: Vec::new(),
            stop_on_first_failure: false,
            fake_dns_lookups: None,
            dns_policy: Arc::default(),
            dns_resolver: None,
        }
    }

    pub(super) fn ensure_compiled(&self) -> ValidationResult<()> {
        self.compilation_error
            .as_ref()
            .map_or(Ok(()), |error| Err(error.error()))
    }

    fn compile_rules(&mut self, rules: RuleSet) {
        match rules.compile_rules(&self.data) {
            Ok(rules) => {
                self.rules = Arc::new(rules);
                self.compilation_error = None;
            }
            Err(error) => {
                self.compilation_error = Some(error.into());
            }
        }
    }

    pub(super) fn invalidate(&mut self) {
        self.result = None;
        if !self.running {
            self.errors = MessageBag::default();
        }
    }

    fn active_flags(&self, attribute: &str) -> ValidationResult<RuleFlags> {
        Ok(self
            .get_rules()?
            .into_iter()
            .find(|(field, _)| field == attribute)
            .map_or_else(
                || RuleFlags::new(&[], &self.extensions),
                |(_, rules)| RuleFlags::new(&rules, &self.extensions),
            ))
    }

    pub(super) fn aware_data(&self) -> Arc<Data> {
        self.data.clone()
    }

    pub(super) fn aware_snapshot(&self) -> super::aware_rule::Snapshot {
        super::aware_rule::Snapshot {
            data: self.data.clone(),
            translator: self.translator.clone(),
            messages: self.messages.clone(),
            attributes: self.attributes.clone(),
        }
    }

    pub fn get_data(&self) -> &Data {
        &self.data
    }

    /// 替换数据后恢复初始规则，移除运行期 addRules/sometimes 追加项。
    pub fn set_data(&mut self, data: Data) -> &mut Self {
        self.data = Arc::new(data);
        self.compile_rules(self.initial_rules.clone());
        self.invalidate();
        self
    }

    pub fn get_value(&self, attribute: &str) -> ValidationResult<Option<&Value>> {
        if path::parse(attribute)?
            .iter()
            .any(|part| matches!(part, path::Segment::Any))
        {
            return Err(ValidationError::InvalidPath);
        }
        Ok(self
            .data
            .attributes(attribute)?
            .into_iter()
            .next()
            .and_then(|(_, value)| value))
    }

    /// 原子更新单值；替换上传字段时同步移除旧文件引用。
    pub fn set_value(&mut self, attribute: &str, value: Value) -> ValidationResult<()> {
        self.get_value(attribute)?;
        let mut data = self.data.as_ref().clone();
        data.forget_files(attribute)?;
        let mut input = data.as_value().clone();
        path::set(&mut input, &path::parse(attribute)?, value)?;
        data.replace(input)?;
        self.data = Arc::new(data);
        self.invalidate();
        Ok(())
    }

    /// 返回当前数据展开后的规则，具体键仍使用原生转义路径。
    pub fn get_rules(&self) -> ValidationResult<Vec<(String, Vec<Rule>)>> {
        self.ensure_compiled()?;
        let mut expanded: Vec<(String, Vec<Rule>)> = Vec::new();
        let mut budget = path::Budget::default();
        for (index, (pattern, rules)) in self.rules.fields.iter().enumerate() {
            for (attribute, _) in
                path::expand(self.data.as_value(), &path::parse(pattern)?, &mut budget)?
            {
                if !self.rules.accepts(index, &unescape(&attribute)) {
                    continue;
                }
                if let Some((_, current)) =
                    expanded.iter_mut().find(|(field, _)| field == &attribute)
                {
                    current.extend(rules.clone());
                } else {
                    expanded.push((attribute, rules.clone()));
                }
            }
        }
        Ok(expanded)
    }

    pub fn set_rules(&mut self, rules: RuleSet) -> &mut Self {
        self.initial_rules = rules.clone();
        self.compile_rules(rules);
        self.invalidate();
        self
    }

    pub fn add_rules(&mut self, rules: RuleSet) -> ValidationResult<()> {
        self.ensure_compiled()?;
        let merged = self.rules.merged(rules)?.compile_rules(&self.data)?;
        self.rules = Arc::new(merged);
        self.invalidate();
        Ok(())
    }

    pub fn append_rules(&mut self, rules: RuleSet) -> ValidationResult<&mut Self> {
        let merged = self.rules.merged(rules)?;
        Ok(self.set_rules(merged))
    }

    pub fn set_custom_messages(&mut self, messages: BTreeMap<String, String>) -> &mut Self {
        self.messages.extend(messages);
        self.invalidate();
        self
    }

    pub fn set_attribute_names(&mut self, attributes: BTreeMap<String, String>) -> &mut Self {
        self.attributes = attributes;
        self.invalidate();
        self
    }

    pub fn add_custom_attributes(&mut self, attributes: BTreeMap<String, String>) -> &mut Self {
        self.attributes.extend(attributes);
        self.invalidate();
        self
    }

    pub fn set_value_names(
        &mut self,
        values: BTreeMap<String, BTreeMap<String, String>>,
    ) -> &mut Self {
        self.values = values;
        self.invalidate();
        self
    }

    pub fn add_custom_values(
        &mut self,
        values: BTreeMap<String, BTreeMap<String, String>>,
    ) -> &mut Self {
        self.values.extend(values);
        self.invalidate();
        self
    }

    pub fn get_translator(&self) -> Option<&Arc<Translator>> {
        self.translator.as_ref()
    }

    pub fn set_translator(&mut self, translator: Arc<Translator>) {
        self.translator = Some(translator);
        self.invalidate();
    }

    pub fn get_exception(&self) -> ValidationExceptionFactory {
        self.exception
    }

    pub fn set_exception(&mut self, exception: ValidationExceptionFactory) -> &mut Self {
        self.exception = exception;
        self.invalidate();
        self
    }

    /// 仅作用于当前验证器，避免异步请求共享测试开关。
    pub fn fake_dns_lookups(&mut self, value: bool) -> &mut Self {
        self.fake_dns_lookups = Some(value);
        self.invalidate();
        self
    }

    /// 允许应用或隔离测试指定DNS 解析器的服务器、超时及重试配置；查询绕过 hosts 和合成记录。
    pub fn set_dns_resolver(&mut self, resolver: hickory_resolver::TokioResolver) -> &mut Self {
        self.dns_resolver = Some(resolver);
        self.invalidate();
        self
    }

    pub fn after(
        &mut self,
        callback: impl Fn(&mut Self) -> ValidationResult<()> + Send + Sync + 'static,
    ) -> &mut Self {
        self.after.push(Arc::new(callback));
        self
    }

    pub fn stop_on_first_failure(&mut self, stop: bool) -> &mut Self {
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
        self.ensure_compiled()?;
        let candidate = if rules.is_empty() {
            path::parse(attribute)?;
            let mut rules = RuleSet::new();
            rules.fields.push((attribute.to_owned(), Vec::new()));
            rules
        } else {
            RuleSet::new().field(attribute, rules)?
        }
        .compile_rules(&self.data)?;
        let mut additions = RuleSet::new();
        for (declaration, rules) in &candidate.fields {
            for (path, _) in self.data.attributes(declaration)? {
                let parent = if attribute.ends_with(".*") {
                    path.clone()
                } else {
                    path::parent(&path)?.unwrap_or_else(|| path.clone())
                };
                let item = self
                    .data
                    .attributes(&parent)?
                    .into_iter()
                    .next()
                    .and_then(|(_, value)| value);
                if condition(&self.data, item) && !rules.is_empty() {
                    additions =
                        additions.merged(RuleSet::new().field(path.clone(), rules.clone())?)?;
                    if path != candidate.origin(declaration) {
                        additions
                            .origins
                            .insert(path, candidate.origin(declaration).to_owned());
                    }
                }
            }
        }
        self.add_rules(additions)?;
        Ok(self)
    }

    pub async fn validate_with_bag(&mut self, error_bag: &str) -> ValidationResult<ValidatedInput> {
        self.validate().await.map_err(|mut error| {
            if let ValidationError::Invalid(exception) = &mut error {
                exception.error_bag(error_bag);
            }
            error
        })
    }

    /// Supplies identities explicitly when validation runs outside an HTTP request.
    /// The resolver receives the rule's guard argument; None means the application's default.
    pub fn set_user_resolver(
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
    pub async fn validate_request(
        &mut self,
        request: &mut crate::http::Request,
    ) -> ValidationResult<ValidatedInput> {
        self.run(Some(request)).await?;
        self.validated()
    }

    async fn run(&mut self, request: Option<&mut crate::http::Request>) -> ValidationResult<()> {
        if self.running {
            return Err(ValidationError::Execution {
                source: std::io::Error::other("不能在规则执行期间递归验证同一验证器").into(),
            });
        }
        struct Running<'a>(&'a mut Validator);
        impl Drop for Running<'_> {
            fn drop(&mut self) {
                self.0.running = false;
            }
        }
        self.running = true;
        let running = Running(self);
        running.0.run_inner(request).await
    }

    async fn run_inner(
        &mut self,
        mut request: Option<&mut crate::http::Request>,
    ) -> ValidationResult<()> {
        self.ensure_compiled()?;
        self.result = None;
        self.errors = MessageBag::default();
        let mut excluded: Vec<String> = Vec::new();
        let mut errors = MessageBag::default();
        let mut budget = path::Budget::default();
        let all_rules = self.rules.clone();
        let original_data = self.data.clone();
        let has_aware = all_rules
            .fields
            .iter()
            .any(|(_, rules)| rules.iter().any(|rule| matches!(rule.0, Kind::Aware(_))));
        'fields: for (index, (declaration, rules)) in all_rules.fields.iter().enumerate() {
            let pattern = all_rules.origin(declaration);
            let flags = RuleFlags::new(rules, &self.extensions);
            let sometimes = flags.sometimes;
            let presence_rules = contains(rules, |rule| {
                matches!(rule, Kind::Exists(_) | Kind::Unique(_))
            });
            let segments = path::parse(declaration)?;
            let wildcard = presence_rules
                && (path::parse(pattern)?
                    .iter()
                    .any(|part| matches!(part, path::Segment::Any))
                    || segments
                        .iter()
                        .any(|part| matches!(part, path::Segment::Any)));
            for (attribute, value) in
                path::expand(original_data.as_value(), &segments, &mut budget)?
            {
                if excluded
                    .iter()
                    .any(|field| attribute == *field || attribute.starts_with(&format!("{field}.")))
                {
                    continue;
                }
                if self.stop_on_first_failure && !errors.is_empty() {
                    break 'fields;
                }
                if !all_rules.accepts(index, &unescape(&attribute)) {
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
                    for Rule(rule, metadata) in rules {
                        let rules_before = has_aware.then(|| self.rules.clone());
                        let flags = if has_aware && !Arc::ptr_eq(&all_rules, &self.rules) {
                            match self.active_flags(&attribute) {
                                Ok(flags) => flags,
                                Err(error) => {
                                    self.errors = errors;
                                    return Err(error);
                                }
                            }
                        } else {
                            flags
                        };
                        let nullable = flags.nullable;
                        let numeric = flags.numeric;
                        let current_data = has_aware.then(|| self.data.clone());
                        let value = if let Some(current_data) = &current_data {
                            current_data
                                .attributes(&attribute)?
                                .into_iter()
                                .next()
                                .and_then(|(_, value)| value)
                        } else {
                            value
                        };
                        if flags.sometimes && value.is_none() {
                            continue;
                        }
                        let extension_parameters = if let Kind::Extension(name, parameters) = rule {
                            self.extension_parameters(name, parameters, pattern, &attribute)?
                        } else {
                            metadata
                                .as_ref()
                                .map_or_else(Vec::new, |(_, parameters)| parameters.clone())
                        };
                        if matches!(rule, Kind::Exclude) {
                            excluded.push(attribute.clone());
                            break;
                        }
                        if let Kind::ExcludeIf(other, values) = rule {
                            let other = dependent_path(pattern, &attribute, other)?;
                            if self.condition_matches(&other, values)? {
                                excluded.push(attribute.clone());
                                break;
                            }
                        }
                        if matches!(rule, Kind::Exists(_) | Kind::Unique(_))
                            && errors.has(unescape(&attribute).as_str())
                        {
                            continue;
                        }
                        // 上游每条规则执行前重置数值分类，只将当前数值比较临时视为 numeric。
                        let numeric = numeric
                            || matches!(rule, Kind::SizeComparison { .. })
                                && value.is_some_and(super::standard_rules::numeric);
                        let failures = if let Kind::Conditional { name, parameters } = rule {
                            let (exclude, failures) = self.evaluate_conditional(
                                name,
                                parameters,
                                value,
                                (pattern, &attribute),
                                nullable,
                            )?;
                            if exclude {
                                excluded.push(attribute.clone());
                                break;
                            }
                            failures
                        } else if let Kind::RequiredIf(other_pattern, values) = rule {
                            let other = dependent_path(pattern, &attribute, other_pattern)?;
                            if self.condition_matches(&other, values)?
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
                                    failure.replacements.extend(message_cases(
                                        "other",
                                        &[self.display_attribute(other_pattern, &other)],
                                        "",
                                    ));
                                    failure.replacements.extend(message_cases(
                                        "value",
                                        &[self.display_value(&other, actual)],
                                        "",
                                    ));
                                    vec![failure]
                                }
                            } else {
                                Vec::new()
                            }
                        } else if let Kind::Extension(name, _) = rule {
                            self.evaluate_extension(
                                name,
                                &extension_parameters,
                                &attribute,
                                value,
                                nullable,
                            )?
                        } else if let Kind::Aware(rule) = rule {
                            let execute = rule.is_implicit()
                                || value.is_some_and(|value| {
                                    !(nullable
                                        && value.is_null()
                                        && self.data.file(&attribute).is_none()
                                        || value.as_str().is_some_and(|value| {
                                            value
                                                .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                                                .is_empty()
                                        }))
                                });
                            if execute {
                                self.errors = std::mem::take(&mut errors);
                                let result = rule.validate(self, &attribute, value).await;
                                errors = std::mem::take(&mut self.errors);
                                match result {
                                    Ok(failures) => failures,
                                    Err(error) => {
                                        self.errors = errors;
                                        return Err(error);
                                    }
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
                                RuleFlags { numeric, ..flags },
                            )
                            .await?
                        };
                        if has_aware && let Err(error) = self.ensure_compiled() {
                            self.errors = errors;
                            return Err(error);
                        }
                        let after_flags = if rules_before
                            .as_ref()
                            .is_some_and(|before| !Arc::ptr_eq(before, &self.rules))
                        {
                            match self.active_flags(&attribute) {
                                Ok(flags) => flags,
                                Err(error) => {
                                    self.errors = errors;
                                    return Err(error);
                                }
                            }
                        } else {
                            flags
                        };
                        let stop = after_flags.bail && errors.has(unescape(&attribute).as_str())
                            || !failures.is_empty()
                                && (after_flags.bail
                                    || after_flags.implicit
                                        && implicit_builtin(rule, &self.extensions));
                        for mut failure in failures {
                            if metadata.as_ref().is_some_and(|(name, _)| {
                                self.extensions.replacers.contains_key(name)
                            }) {
                                // 自定义替换器接管规则参数替换，不能先执行内置 :min 等替换。
                                failure.replacements.clear();
                            }
                            let message = self.format_message(pattern, &attribute, failure);
                            let message = if let Some((name, _)) = metadata {
                                self.extensions.replacers.get(name).map_or_else(
                                    || message.clone(),
                                    |replacer| {
                                        replacer(
                                            &message,
                                            &unescape(&attribute),
                                            name,
                                            &extension_parameters,
                                            self,
                                        )
                                    },
                                )
                            } else {
                                message
                            };
                            errors.add(unescape(&attribute), message);
                        }
                        if stop {
                            break;
                        }
                    }
                }
            }
        }
        drop(original_data);
        for field in &excluded {
            Arc::make_mut(&mut self.data).forget(field)?;
        }
        let retained = |field: &str| {
            !excluded
                .iter()
                .any(|excluded| field == excluded || field.starts_with(&format!("{excluded}.")))
        };
        let mut retained_errors = MessageBag::default();
        for (field, messages) in errors.messages() {
            if retained(field) {
                for message in messages {
                    retained_errors.add(field.clone(), message);
                }
            }
        }
        errors = retained_errors;
        if errors.is_empty() {
            self.result = Some(self.project_validated()?);
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

    fn project_validated(&self) -> ValidationResult<ValidatedInput> {
        let rules = self.get_rules()?;
        let array_fields = rules
            .iter()
            .filter(|(_, rules)| {
                self.exclude_unvalidated_array_keys
                    && contains(rules, |rule| {
                        matches!(rule, Kind::Array)
                            || matches!(rule, Kind::Scalar(rule) if rule.name() == "list")
                    })
            })
            .map(|(field, _)| field.clone())
            .collect();
        let fields = rules
            .iter()
            .map(|(field, _)| {
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
        ValidatedInput::project(&self.data, &fields, &array_fields)
    }

    fn has_boolean_rule(&self, attribute: &str) -> ValidationResult<bool> {
        for (pattern, rules) in &self.rules.fields {
            if contains(rules, |rule| matches!(rule, Kind::Boolean))
                && self
                    .data
                    .attributes(pattern)?
                    .iter()
                    .any(|(field, _)| field == attribute)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn condition_matches(&self, field: &str, values: &[String]) -> ValidationResult<bool> {
        let value = self
            .data
            .attributes(field)?
            .into_iter()
            .next()
            .and_then(|(_, value)| value);
        Ok(value.is_some() && dependent_matches(value, values, self.has_boolean_rule(field)?))
    }

    // 条件规则仍在同一字段循环中执行，共用通配路径、错误格式与排除收尾。
    fn evaluate_conditional(
        &self,
        name: &str,
        parameters: &[String],
        value: Option<&Value>,
        attribute: (&str, &str),
        nullable: bool,
    ) -> ValidationResult<(bool, Vec<RuleFailure>)> {
        let (pattern, attribute) = attribute;
        let exclusion = name.starts_with("exclude_");
        if !implicit_condition(name)
            && !exclusion
            && (value.is_none()
                || value.is_some_and(|value| {
                    (nullable && value.is_null() && self.data.file(attribute).is_none())
                        || value.as_str().is_some_and(|text| {
                            text.trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                                .is_empty()
                        })
                }))
        {
            return Ok((false, Vec::new()));
        }
        let equality = name.ends_with("_if") || name.ends_with("_unless");
        let fields: Vec<_> = parameters
            .iter()
            .take(if equality { 1 } else { parameters.len() })
            .map(|field| dependent_path(pattern, attribute, field))
            .collect::<ValidationResult<_>>()?;
        let lookup = |field: &str| -> ValidationResult<Option<&Value>> {
            Ok(self
                .data
                .attributes(field)?
                .into_iter()
                .next()
                .and_then(|(_, value)| value))
        };
        let populated = |field: &str| -> ValidationResult<bool> {
            Ok(self.data.file(field).is_some()
                || lookup(field)?.is_some_and(|value| !is_empty(value)))
        };
        let first = fields.first().map(String::as_str).unwrap_or("");
        let other = if fields.is_empty() {
            None
        } else {
            lookup(first)?
        };
        let trigger = if equality {
            let boolean_rule = self.has_boolean_rule(first)?;
            let matched = dependent_matches(other, &parameters[1..], boolean_rule);
            if name.ends_with("_unless") {
                !matched
            } else {
                matched
            }
        } else if name.ends_with("_accepted") {
            accepted(other)
        } else if name.ends_with("_declined") {
            declined(other)
        } else {
            let states = fields
                .iter()
                .map(|field| {
                    if name.starts_with("required_")
                        || name == "exclude_without"
                        || name == "prohibits"
                    {
                        populated(field)
                    } else {
                        Ok(lookup(field)?.is_some())
                    }
                })
                .collect::<ValidationResult<Vec<_>>>()?;
            match name {
                "required_with_all" | "missing_with_all" | "present_with_all" => {
                    states.iter().all(|value| *value)
                }
                "required_without_all" => states.iter().all(|value| !value),
                "required_without" | "exclude_without" => states.iter().any(|value| !value),
                "exclude_with" => states.first().copied().unwrap_or(false),
                _ => states.iter().any(|value| *value),
            }
        };
        if exclusion {
            return Ok((trigger, Vec::new()));
        }
        let populated =
            self.data.file(attribute).is_some() || value.is_some_and(|value| !is_empty(value));
        let fails = trigger
            && if name.starts_with("missing_") {
                value.is_some()
            } else if name.starts_with("present_") {
                value.is_none()
            } else if name.starts_with("required_") {
                !populated
            } else if name == "accepted_if" {
                !accepted(value)
            } else if name == "declined_if" {
                !declined(value)
            } else {
                populated
            };
        if !fails {
            return Ok((false, Vec::new()));
        }
        let mut failure = RuleFailure::new(format!("validation.{name}"));
        let display = fields
            .iter()
            .zip(parameters)
            .map(|(field, pattern)| self.display_attribute(pattern, field))
            .collect::<Vec<_>>();
        failure.replacements.insert(
            "other".into(),
            if name == "prohibits" {
                display.join(" / ")
            } else {
                display.first().cloned().unwrap_or_default()
            },
        );
        let expected_values = parameters
            .iter()
            .skip(1)
            .map(|value| self.display_value(first, Some(&Value::String(value.clone()))))
            .collect::<Vec<_>>();
        failure.replacements.insert(
            "value".into(),
            if matches!(name, "missing_unless" | "present_unless") {
                expected_values.first().cloned().unwrap_or_default()
            } else {
                self.display_value(first, other)
            },
        );
        failure.replacements.insert(
            "values".into(),
            if equality {
                expected_values.join(", ")
            } else {
                display.join(" / ")
            },
        );
        if !matches!(name, "missing_unless" | "present_unless") {
            let others = if name == "prohibits" {
                display.clone()
            } else {
                display.iter().take(1).cloned().collect()
            };
            failure.replacements.extend(message_cases(
                "other",
                &others,
                if name == "prohibits" { " / " } else { "" },
            ));
            let values = if equality { &expected_values } else { &display };
            failure.replacements.extend(message_cases(
                "values",
                values,
                if equality { ", " } else { " / " },
            ));
            let actual = failure
                .replacements
                .get("value")
                .cloned()
                .unwrap_or_default();
            failure
                .replacements
                .extend(message_cases("value", &[actual], ""));
        }
        Ok((false, vec![failure]))
    }

    fn compare_size(
        &self,
        name: &str,
        parameter: &str,
        value: &Value,
        (pattern, attribute): (&str, &str),
        flags: RuleFlags,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let numeric = flags.numeric;
        let other = dependent_path(pattern, attribute, parameter)?;
        let compared = self
            .data
            .attributes(&other)?
            .into_iter()
            .next()
            .and_then(|(_, value)| value);
        let file = self.data.file(attribute);
        let other_file = self.data.file(&other);
        let compared_null = other_file.is_none() && compared.is_none_or(Value::is_null);
        let parameter_value = Value::String(other.clone());
        let parameter_numeric = super::standard_rules::numeric(&parameter_value);
        let observed = validation_size(value, file, numeric)?;
        let target = if compared_null && super::standard_rules::numeric(value) && parameter_numeric
        {
            super::standard_rules::check_exponent(&parameter_value)?;
            super::standard_rules::number(&parameter_value)
        } else if parameter_numeric {
            None
        } else if numeric
            && super::standard_rules::numeric(value)
            && compared.is_some_and(super::standard_rules::numeric)
        {
            validation_size(compared.unwrap(), None, true)?
        } else if file.is_some() && other_file.is_some()
            || file.is_none()
                && other_file.is_none()
                && same_size_type(value, compared.unwrap_or(&Value::Null))
        {
            validation_size(compared.unwrap_or(&Value::Null), other_file, numeric)?
        } else {
            None
        };
        let passes =
            observed
                .as_ref()
                .zip(target.as_ref())
                .is_some_and(|(first, second)| match name {
                    "gt" => first > second,
                    "lt" => first < second,
                    "gte" => first >= second,
                    _ => first <= second,
                });
        if passes {
            return Ok(Vec::new());
        }
        let category = flags.category(file.is_some());
        let replacement = if compared_null {
            self.display_attribute(parameter, &other)
        } else {
            let value = compared.unwrap_or(&Value::Null);
            if numeric && super::standard_rules::numeric(value) {
                match value {
                    Value::String(value) => value
                        .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                        .to_owned(),
                    Value::Number(value) if value.is_f64() => {
                        crate::support::php_float_string(value.as_f64().unwrap())
                    }
                    _ => value.to_string(),
                }
            } else {
                validation_size(value, other_file, numeric)?
                    .map(|value| value.normalized().to_string())
                    .unwrap_or_default()
            }
        };
        let mut failure = RuleFailure::new(format!("validation.{name}.{category}"));
        failure.replacements.insert("value".into(), replacement);
        Ok(vec![failure])
    }

    async fn evaluate(
        &self,
        rule: &Kind,
        value: Option<&Value>,
        attribute: (&str, &str),
        attribute_for_query: &str,
        flags: RuleFlags,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let nullable = flags.nullable;
        let (pattern, attribute) = attribute;
        if let Kind::Custom(rule) = rule
            && rule.is_implicit()
        {
            let context = RuleContext {
                data: &self.data,
                original_data: None,
                translator: self.translator.as_deref(),
                attribute,
                messages: &self.messages,
                attributes: &self.attributes,
            };
            return rule
                .validate(&context, &custom_rule_attribute(attribute), value)
                .await;
        }

        if let Kind::Email(email) = rule
            && !email.custom_rules.is_empty()
        {
            let mut base = email.clone();
            base.custom_rules.clear();
            let mut rules = vec![base.into()];
            rules.extend(email.custom_rules.iter().cloned());
            return self.evaluate_nested(attribute, rules).await;
        }
        if let Kind::Password(password) = rule {
            return Ok(password.failures(value, nullable));
        }
        let file = self.data.file(attribute);
        if let Some(file) = file {
            let observed = bigdecimal::BigDecimal::from(file.get_size() as u64) / 1024;
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
                    let mut failure =
                        RuleFailure::new(format!("validation.{kind}.{}", flags.category(true)));
                    failure.replacements.extend(
                        bounds
                            .into_iter()
                            .map(|(name, value)| (name.into(), value.to_string())),
                    );
                    vec![failure]
                });
            }
        }
        if let Kind::SizeComparison { name, parameter } = rule
            && let Some(value) = value
            && (file.is_some()
                || !(nullable && value.is_null()
                    || value.as_str().is_some_and(|value| {
                        value
                            .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                            .is_empty()
                    })))
        {
            return self.compare_size(name, parameter, value, (pattern, attribute), flags);
        }
        let failure = match (rule, value) {
            (Kind::Required, value) if file.is_none() && value.is_none_or(is_empty) => {
                Some("required")
            }
            (Kind::Accepted, value) if value.is_none_or(is_empty) => Some("accepted"),
            (Kind::Present, None) => Some("present"),
            (Kind::Missing, Some(_)) => Some("missing"),
            (Kind::Filled, Some(value)) if file.is_none() && is_empty(value) => Some("filled"),
            (Kind::Declined, value)
                if !value.is_some_and(|value| {
                    matches!(value, Value::Bool(false))
                        || value.as_i64() == Some(0)
                        || matches!(value.as_str(), Some("no" | "off" | "0" | "false"))
                }) =>
            {
                Some("declined")
            }
            (_, None) => None,
            (_, Some(Value::Null)) if nullable && file.is_none() => None,
            (_, Some(Value::String(value)))
                if value
                    .trim_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
                    .is_empty() =>
            {
                None
            }
            (Kind::Scalar(rule), Some(value)) => {
                return Ok(if rule.passes(value)? {
                    Vec::new()
                } else {
                    let mut failure = RuleFailure::new(format!("validation.{}", rule.name()));
                    failure.replacements.extend(rule.replacements());
                    if let super::scalar_rules::ScalarRule::Affix { parameters, .. }
                    | super::scalar_rules::ScalarRule::Array { parameters, .. } = rule
                    {
                        if rule.name() != "contains" {
                            let values: Vec<_> = parameters
                                .iter()
                                .map(|value| {
                                    self.display_value(
                                        attribute,
                                        Some(&Value::String(value.clone())),
                                    )
                                })
                                .collect();
                            failure
                                .replacements
                                .extend(message_cases("values", &values, ", "));
                        }
                        if rule.name() == "array_keys" {
                            let keys: Vec<_> = match value {
                                Value::Object(items) => items.keys().cloned().collect(),
                                Value::Array(items) => {
                                    (0..items.len()).map(|key| key.to_string()).collect()
                                }
                                _ => Vec::new(),
                            };
                            let unexpected: Vec<_> = keys
                                .into_iter()
                                .filter(|key| !parameters.contains(key))
                                .map(|key| self.display_value(attribute, Some(&Value::String(key))))
                                .collect();
                            failure.replacements.extend(message_cases(
                                "unexpected",
                                &[unexpected.join(", ")],
                                "",
                            ));
                        }
                    }
                    vec![failure]
                });
            }
            (Kind::Prohibited, Some(value)) if file.is_some() || !is_empty(value) => {
                Some("prohibited")
            }
            (Kind::Relation { same, fields }, Some(value)) => {
                let mut failed = false;
                let mut display = String::new();
                for field in fields.iter().take(if *same { 1 } else { fields.len() }) {
                    let other = dependent_path(pattern, attribute, field)?;
                    let actual = self
                        .data
                        .attributes(&other)?
                        .into_iter()
                        .next()
                        .and_then(|(_, value)| value);
                    if (*same && !strict_same(value, actual.unwrap_or(&Value::Null)))
                        || (!*same && actual.is_some_and(|other| strict_same(value, other)))
                    {
                        failed = true;
                        let first = dependent_path(pattern, attribute, &fields[0])?;
                        display = self.display_attribute(&fields[0], &first);
                        break;
                    }
                }
                return Ok(if failed {
                    let mut failure = RuleFailure::new(if *same {
                        "validation.same"
                    } else {
                        "validation.different"
                    });
                    failure
                        .replacements
                        .extend(message_cases("other", &[display], ""));
                    vec![failure]
                } else {
                    Vec::new()
                });
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
            (Kind::StrictNumeric, Some(value)) if !value.is_number() => Some("numeric"),
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
            (Kind::ActiveUrl, Some(value)) => {
                let passes = if let Some(value) = value.as_str() {
                    super::active_url::passes(
                        value,
                        self.dns_is_faked(),
                        self.dns_resolver.as_ref(),
                    )
                    .await
                } else {
                    false
                };
                return Ok((!passes)
                    .then(|| RuleFailure::new("validation.active_url"))
                    .into_iter()
                    .collect());
            }
            (Kind::UrlProtocols(protocols), Some(value))
                if !value.as_str().is_some_and(|value| {
                    crate::support::Str::is_url(
                        value,
                        &protocols.iter().map(String::as_str).collect::<Vec<_>>(),
                    )
                }) =>
            {
                Some("url")
            }
            (Kind::UuidVersion(version), Some(value))
                if !value
                    .as_str()
                    .is_some_and(|value| uuid_version(value, version)) =>
            {
                Some("uuid")
            }
            (Kind::Url, Some(value))
                if !value
                    .as_str()
                    .is_some_and(|value| crate::support::Str::is_url(value, &[])) =>
            {
                Some("url")
            }
            (Kind::Uuid, Some(value))
                if !value
                    .as_str()
                    .is_some_and(|value| crate::support::Str::is_uuid(value, None)) =>
            {
                Some("uuid")
            }
            (Kind::DateComparison { name, parameter }, Some(value)) => {
                let context = super::date_rules::DateContext::new()?;
                let other = dependent_path(pattern, attribute, parameter)?;
                let get_format = |field: &str| -> ValidationResult<Option<&str>> {
                    for (pattern, rules) in &self.rules.fields {
                        if self
                            .data
                            .attributes(pattern)?
                            .iter()
                            .any(|(actual, _)| actual == field)
                        {
                            for Rule(rule, _) in rules {
                                if let Kind::Scalar(super::scalar_rules::ScalarRule::DateFormat(
                                    formats,
                                )) = rule
                                {
                                    return Ok(formats.first().map(String::as_str));
                                }
                            }
                        }
                    }
                    Ok(None)
                };
                let format = get_format(attribute)?;
                let first = context.parse(value, format);
                let literal = Value::String(other.clone());
                let compared = self
                    .data
                    .attributes(&other)?
                    .into_iter()
                    .next()
                    .and_then(|(_, value)| value);
                let second_format = if format.is_some() {
                    get_format(&other)?.or(format)
                } else {
                    None
                };
                let mut second = context.parse(&literal, second_format);
                let passes = if format.is_some()
                    && second.is_none()
                    && compared.is_none_or(Value::is_null)
                {
                    true
                } else {
                    if second.is_none() {
                        second = compared.and_then(|value| context.parse(value, second_format));
                    }
                    let order = match (&first, &second) {
                        (Some(first), Some(second)) => {
                            if format.is_some() {
                                Some(
                                    (first.timestamp(), first.microseconds())
                                        .cmp(&(second.timestamp(), second.microseconds())),
                                )
                            } else {
                                Some(first.timestamp().cmp(&second.timestamp()))
                            }
                        }
                        (None, None) if format.is_none() => Some(std::cmp::Ordering::Equal),
                        (None, Some(second)) if format.is_none() => {
                            Some(false.cmp(&(second.timestamp() != 0)))
                        }
                        (Some(first), None) if format.is_none() => {
                            Some((first.timestamp() != 0).cmp(&false))
                        }
                        _ => None,
                    };
                    order.is_some_and(|order| match *name {
                        "before" => order.is_lt(),
                        "before_or_equal" => !order.is_gt(),
                        "after" => order.is_gt(),
                        "after_or_equal" => !order.is_lt(),
                        _ => order.is_eq() && (first.is_some() == second.is_some()),
                    })
                };
                let passes = passes && (value.is_string() || value.is_number());
                return Ok(if passes {
                    Vec::new()
                } else {
                    let mut failure = RuleFailure::new(format!("validation.{name}"));
                    let date = if context.parse(&literal, None).is_some() {
                        self.display_value(attribute, Some(&literal))
                    } else {
                        self.display_attribute(parameter, &other)
                    };
                    failure.replacements.insert("date".into(), date);
                    vec![failure]
                });
            }
            (Kind::InArray(other), Some(value)) => {
                let other = dependent_path_for_values(pattern, attribute, other)?;
                let entries = crate::support::Arr::dot(self.data.as_value())
                    .map_err(|_| ValidationError::InvalidPath)?;
                let matches = entries.iter().any(|(key, item)| {
                    crate::support::Str::is(&[&other], key, false) && strict_same(value, item)
                });
                return Ok(if matches {
                    Vec::new()
                } else {
                    let mut failure = RuleFailure::new("validation.in_array");
                    failure.replacements.extend(message_cases(
                        "other",
                        &[self.display_attribute(&other, &other)],
                        "",
                    ));
                    vec![failure]
                });
            }
            (
                Kind::Distinct {
                    strict,
                    ignore_case,
                },
                Some(value),
            ) => {
                let entries = crate::support::Arr::dot(self.data.as_value())
                    .map_err(|_| ValidationError::InvalidPath)?;
                let pattern = path::parse(pattern)?
                    .into_iter()
                    .map(|segment| match segment {
                        path::Segment::Any => "[^.]+".into(),
                        path::Segment::Key(key) => regex::escape(&key),
                    })
                    .collect::<Vec<_>>()
                    .join(r"\.");
                let field_pattern = regex::Regex::new(&format!(r"\A{pattern}\z"))
                    .map_err(|_| ValidationError::InvalidRule)?;
                let case_pattern = if *ignore_case {
                    value
                        .as_str()
                        .map(|text| {
                            super::standard_rules::regex(&format!(
                                "/^{}$/iu",
                                regex::escape(text).replace('/', r"\/")
                            ))
                        })
                        .transpose()?
                } else {
                    None
                };
                let mut duplicate = false;
                let actual = unescape(attribute);
                for (_, other) in entries
                    .iter()
                    .filter(|(field, _)| *field != &actual && field_pattern.is_match(field))
                {
                    let equal = if let Some(expression) = &case_pattern {
                        let text =
                            super::standard_rules::text(other).unwrap_or_else(|| "Array".into());
                        expression.is_match(text.as_bytes()).map_err(|source| {
                            ValidationError::Execution {
                                source: source.into(),
                            }
                        })?
                    } else if *strict {
                        strict_same(value, other)
                    } else {
                        crate::support::php_loose_value_equal(value, other)
                    };
                    if equal {
                        duplicate = true;
                        break;
                    }
                }
                duplicate.then_some("distinct")
            }
            (Kind::In(values) | Kind::NotIn(values), Some(value)) => {
                let matched = {
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
                };
                let fails = matched == matches!(rule, Kind::NotIn(_));
                return Ok(if fails {
                    let name = if matches!(rule, Kind::NotIn(_)) {
                        "not_in"
                    } else {
                        "in"
                    };
                    let mut failure = RuleFailure::new(format!("validation.{name}"));
                    let values: Vec<_> = values
                        .iter()
                        .map(|value| {
                            self.display_value(attribute, Some(&Value::String(value.clone())))
                        })
                        .collect();
                    failure
                        .replacements
                        .extend(message_cases("values", &values, ", "));
                    vec![failure]
                } else {
                    Vec::new()
                });
            }
            (Kind::Regex(regex), Some(value))
                if !(matches!(value, Value::String(_) | Value::Number(_))
                    && super::standard_rules::text(value).is_some_and(|value| {
                        regex.is_match(value.as_bytes()).unwrap_or(false)
                    })) =>
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
            (Kind::Email(rule), Some(value)) => {
                let passes = if let Some(value) = value.as_str() {
                    rule.passes(value, self.dns_is_faked(), self.dns_resolver.as_ref())
                        .await
                } else {
                    false
                };
                (!passes).then_some("email")
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
                return scalar_size(value, flags, "size", &[("size", bound)], |n| n == bound);
            }
            (Kind::Between(min, max), Some(value)) => {
                return scalar_size(
                    value,
                    flags,
                    "between",
                    &[("min", min), ("max", max)],
                    |n| n >= min && n <= max,
                );
            }
            (Kind::Min(bound) | Kind::Max(bound), Some(value)) => {
                let minimum = matches!(rule, Kind::Min(_));
                let kind = if minimum { "min" } else { "max" };
                return scalar_size(value, flags, kind, &[(kind, bound)], |n| {
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
                let context = RuleContext {
                    data: &self.data,
                    original_data: None,
                    translator: self.translator.as_deref(),
                    attribute,
                    messages: &self.messages,
                    attributes: &self.attributes,
                };
                return rule
                    .validate(&context, &custom_rule_attribute(attribute), Some(value))
                    .await;
            }
            _ => None,
        };
        Ok(failure
            .into_iter()
            .map(|rule| RuleFailure::new(format!("validation.{rule}")))
            .collect())
    }

    async fn evaluate_nested(
        &self,
        attribute: &str,
        rules: Vec<Rule>,
    ) -> ValidationResult<Vec<RuleFailure>> {
        let factory = self
            .factory
            .as_ref()
            .ok_or_else(|| ValidationError::Execution {
                source: std::io::Error::other("嵌套验证器缺少原工厂").into(),
            })?;
        let mut validator = factory.make_shared(
            self.data.clone(),
            RuleSet::new().field(attribute, rules)?,
            self.messages.clone(),
            self.attributes.clone(),
        )?;
        // 请求级认证和显式DNS覆盖仍属于当前请求，不读取全局状态。
        if self.user_resolver.is_some() {
            validator.user_resolver = self.user_resolver.clone();
        }
        if self.fake_dns_lookups.is_some() {
            validator.fake_dns_lookups = self.fake_dns_lookups;
        }
        if self.dns_resolver.is_some() {
            validator.dns_resolver = self.dns_resolver.clone();
        }
        match validator.validate().await {
            Ok(_) => Ok(Vec::new()),
            Err(ValidationError::Invalid(_)) => Ok(validator
                .errors
                .all(None)
                .into_iter()
                .map(|message| {
                    let mut failure = RuleFailure::new(message);
                    failure.translated = true;
                    failure
                })
                .collect()),
            Err(error) => Err(error),
        }
    }

    fn dns_is_faked(&self) -> bool {
        self.fake_dns_lookups
            .unwrap_or_else(|| self.dns_policy.load(std::sync::atomic::Ordering::Relaxed))
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
        let translated = self
            .translator
            .as_ref()
            .map(|translator| super::message_catalog::lines(translator, "validation.attributes"));
        for name in [attribute, pattern] {
            if let Some(name) = super::message_catalog::attribute(&self.attributes, name) {
                return name.into();
            }
            if let Some(name) = translated
                .as_ref()
                .and_then(|names| super::message_catalog::attribute(names, name))
            {
                return name.into();
            }
        }
        if path::parse(pattern).is_ok_and(|segments| {
            segments
                .iter()
                .any(|segment| matches!(segment, path::Segment::Any))
        }) {
            attribute.to_owned()
        } else {
            crate::support::Str::snake(attribute, "_").replace('_', " ")
        }
    }

    fn display_value(&self, field: &str, value: Option<&Value>) -> String {
        if value.is_some_and(|value| value.is_array() || value.is_object()) {
            return "array".into();
        }
        let key_value = value
            .and_then(super::standard_rules::text)
            .unwrap_or_default();
        if let Some(value) = self
            .values
            .get(field)
            .and_then(|values| values.get(&key_value))
        {
            return value.clone();
        }
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
        let canonical_attribute = attribute;
        let attribute = unescape(attribute);
        let display = self.display_attribute(pattern, &attribute);
        let attributes = message_cases("attribute", &[display], "");
        if failure.translated {
            for (key, value) in attributes {
                failure.replacements.entry(key).or_insert(value);
            }
        } else {
            failure.replacements.extend(attributes);
        }
        let attributes: BTreeMap<_, _> = ["attribute", "ATTRIBUTE", "Attribute"]
            .into_iter()
            .filter_map(|key| {
                failure
                    .replacements
                    .remove(key)
                    .map(|value| (key.to_owned(), value))
            })
            .collect();
        let render = |template: &str| {
            let mut message = replace(template, &attributes);
            if message.contains(":input") {
                let actual = self
                    .data
                    .attributes(canonical_attribute)
                    .ok()
                    .and_then(|values| values.first().and_then(|(_, value)| *value));
                if actual.is_none_or(|value| !value.is_array() && !value.is_object()) {
                    message = message.replace(":input", &self.display_value(&attribute, actual));
                }
            }
            let message = super::message_positions::replace(&message, canonical_attribute);
            replace(&message, &failure.replacements)
        };
        if failure.translated {
            return render(&failure.message);
        }
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
                .or_else(|| {
                    self.messages.iter().find_map(|(key, value)| {
                        (key.contains('*')
                            && super::message_positions::matches(
                                key,
                                &format!("{attribute}.{rule}"),
                            ))
                        .then_some(value)
                    })
                })
                .or_else(|| self.messages.get(rule))
                .or_else(|| self.messages.get(&attribute))
        {
            return render(message);
        }
        if let Some(translator) = &self.translator {
            if let Some(rule) = rule {
                let category = rule_key
                    .and_then(|key| key.strip_prefix(rule)?.strip_prefix('.'))
                    .filter(|category| {
                        matches!(*category, "numeric" | "array" | "file" | "string")
                    });
                if let Some(message) =
                    super::message_catalog::custom(translator, &attribute, rule, category)
                {
                    return render(&message);
                }
            }
            if let Ok(message) = translator.string(&failure.message, &Default::default(), None)
                && message != failure.message
            {
                return render(&message);
            }
        }
        if let Some(rule) = rule
            && let Some(message) = self.extensions.fallback_messages.get(rule)
        {
            return render(message);
        }
        let fallback_message =
            if rule.is_some_and(|rule| self.extensions.callbacks.contains_key(rule)) {
                failure.message.as_str()
            } else {
                rule_key.map_or(failure.message.as_str(), fallback)
            };
        render(fallback_message)
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
            return Err(ValidationError::Invalid(Box::new((self.exception)(
                self.errors.clone(),
                self.translator.as_deref(),
            ))));
        }
        self.result.as_ref().ok_or(ValidationError::NotValidated)?;
        self.project_validated()
    }

    fn errors(&self) -> &MessageBag {
        &self.errors
    }
}

fn contains(rules: &[Rule], test: impl Fn(&Kind) -> bool) -> bool {
    rules.iter().any(|Rule(rule, _)| test(rule))
}

fn same_size_type(first: &Value, second: &Value) -> bool {
    match (first, second) {
        (Value::Array(_) | Value::Object(_), Value::Array(_) | Value::Object(_)) => true,
        (Value::Null, Value::Null)
        | (Value::Bool(_), Value::Bool(_))
        | (Value::String(_), Value::String(_)) => true,
        (Value::Number(first), Value::Number(second)) => first.is_f64() == second.is_f64(),
        _ => false,
    }
}

fn validation_size(
    value: &Value,
    file: Option<&crate::http::UploadedFile>,
    numeric: bool,
) -> ValidationResult<Option<bigdecimal::BigDecimal>> {
    if let Some(file) = file {
        return Ok(Some(
            bigdecimal::BigDecimal::from(file.get_size() as u64) / 1024,
        ));
    }
    if numeric {
        super::standard_rules::check_exponent(value)?;
    }
    Ok(Some(size(value, numeric)))
}

fn scalar_size(
    value: &Value,
    flags: RuleFlags,
    rule: &str,
    bounds: &[(&str, &bigdecimal::BigDecimal)],
    passes: impl FnOnce(&bigdecimal::BigDecimal) -> bool,
) -> ValidationResult<Vec<RuleFailure>> {
    let observed = validation_size(value, None, flags.numeric)?;
    if observed.as_ref().is_some_and(passes) {
        return Ok(Vec::new());
    }
    let category = flags.category(false);
    let mut failure = RuleFailure::new(format!("validation.{rule}.{category}"));
    failure.replacements.extend(
        bounds
            .iter()
            .map(|(key, bound)| ((*key).into(), bound.to_string())),
    );
    Ok(vec![failure])
}

pub(super) fn custom_rule_attribute(attribute: &str) -> Cow<'_, str> {
    if attribute.contains('\\') {
        Cow::Owned(unescape(attribute))
    } else {
        Cow::Borrowed(attribute)
    }
}

pub(super) fn unescape(attribute: &str) -> String {
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

pub(super) fn replace(template: &str, replacements: &BTreeMap<String, String>) -> String {
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
        "ascii" => {
            "The :attribute field must only contain single-byte alphanumeric characters and symbols."
        }
        "alpha" => "The :attribute field must only contain letters.",
        "alpha_dash" => {
            "The :attribute field must only contain letters, numbers, dashes, and underscores."
        }
        "alpha_num" => "The :attribute field must only contain letters and numbers.",
        "array_keys" => "The :attribute field must only contain the following keys: :values.",
        "required_array_keys" => "The :attribute field must contain entries for: :values.",
        "in_array_keys" => {
            "The :attribute field must contain at least one of the following keys: :values."
        }
        "contains" => "The :attribute field is missing a required value.",
        "doesnt_contain" => "The :attribute field must not contain any of the following: :values.",
        "not_in" => "The selected :attribute is invalid.",
        "list" => "The :attribute field must be a list.",
        "ip" => "The :attribute field must be a valid IP address.",
        "ipv4" => "The :attribute field must be a valid IPv4 address.",
        "ipv6" => "The :attribute field must be a valid IPv6 address.",
        "mac_address" => "The :attribute field must be a valid MAC address.",
        "json" => "The :attribute field must be a valid JSON string.",
        "hex_color" => "The :attribute field must be a valid hexadecimal color.",
        "digits" => "The :attribute field must be :digits digits.",
        "digits_between" => "The :attribute field must be between :min and :max digits.",
        "max_digits" => "The :attribute field must not have more than :max digits.",
        "min_digits" => "The :attribute field must have at least :min digits.",
        "decimal" => "The :attribute field must have :decimal decimal places.",
        "lowercase" => "The :attribute field must be lowercase.",
        "uppercase" => "The :attribute field must be uppercase.",
        "not_regex" => "The :attribute field format is invalid.",
        "starts_with" => "The :attribute field must start with one of the following: :values.",
        "doesnt_start_with" => {
            "The :attribute field must not start with one of the following: :values."
        }
        "ends_with" => "The :attribute field must end with one of the following: :values.",
        "doesnt_end_with" => {
            "The :attribute field must not end with one of the following: :values."
        }
        "numeric" => "The :attribute field must be a number.",
        "before" => "The :attribute field must be a date before :date.",
        "before_or_equal" => "The :attribute field must be a date before or equal to :date.",
        "after" => "The :attribute field must be a date after :date.",
        "after_or_equal" => "The :attribute field must be a date after or equal to :date.",
        "date_equals" => "The :attribute field must be a date equal to :date.",
        "date" => "The :attribute field must be a valid date.",
        "date_format" => "The :attribute field must match the format :format.",
        "accepted_if" => "The :attribute field must be accepted when :other is :value.",
        "declined_if" => "The :attribute field must be declined when :other is :value.",
        "missing_if" => "The :attribute field must be missing when :other is :value.",
        "missing_unless" => "The :attribute field must be missing unless :other is :value.",
        "missing_with" => "The :attribute field must be missing when :values is present.",
        "missing_with_all" => "The :attribute field must be missing when :values are present.",
        "present_if" => "The :attribute field must be present when :other is :value.",
        "present_unless" => "The :attribute field must be present unless :other is :value.",
        "present_with" => "The :attribute field must be present when :values is present.",
        "present_with_all" => "The :attribute field must be present when :values are present.",
        "required_if_accepted" => "The :attribute field is required when :other is accepted.",
        "required_if_declined" => "The :attribute field is required when :other is declined.",
        "required_unless" => "The :attribute field is required unless :other is in :values.",
        "required_with" => "The :attribute field is required when :values is present.",
        "required_with_all" => "The :attribute field is required when :values are present.",
        "required_without" => "The :attribute field is required when :values is not present.",
        "required_without_all" => {
            "The :attribute field is required when none of :values are present."
        }
        "prohibited_if" => "The :attribute field is prohibited when :other is :value.",
        "prohibited_if_accepted" => "The :attribute field is prohibited when :other is accepted.",
        "prohibited_if_declined" => "The :attribute field is prohibited when :other is declined.",
        "prohibited_unless" => "The :attribute field is prohibited unless :other is in :values.",
        "prohibits" => "The :attribute field prohibits :other from being present.",
        "missing" => "The :attribute field must be missing.",
        "filled" => "The :attribute field must have a value.",
        "declined" => "The :attribute field must be declined.",
        "prohibited" => "The :attribute field is prohibited.",
        "same" => "The :attribute field must match :other.",
        "different" => "The :attribute field and :other must be different.",
        "accepted" => "The :attribute field must be accepted.",
        "active_url" => "The :attribute field must be a valid URL.",
        "url" => "The :attribute field must be a valid URL.",
        "uuid" => "The :attribute field must be a valid UUID.",
        "timezone" => "The :attribute field must be a valid timezone.",
        "ulid" => "The :attribute field must be a valid ULID.",
        "multiple_of" => "The :attribute field must be a multiple of :value.",
        "in" => "The selected :attribute is invalid.",
        "in_array" => "The :attribute field must exist in :other.",
        "distinct" => "The :attribute field has a duplicate value.",
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
        "gt.array" => "The :attribute field must have more than :value items.",
        "gt.file" => "The :attribute field must be greater than :value kilobytes.",
        "gt.numeric" => "The :attribute field must be greater than :value.",
        "gt.string" => "The :attribute field must be greater than :value characters.",
        "lt.array" => "The :attribute field must have less than :value items.",
        "lt.file" => "The :attribute field must be less than :value kilobytes.",
        "lt.numeric" => "The :attribute field must be less than :value.",
        "lt.string" => "The :attribute field must be less than :value characters.",
        "gte.array" => "The :attribute field must have :value items or more.",
        "gte.file" => "The :attribute field must be greater than or equal to :value kilobytes.",
        "gte.numeric" => "The :attribute field must be greater than or equal to :value.",
        "gte.string" => "The :attribute field must be greater than or equal to :value characters.",
        "lte.array" => "The :attribute field must not have more than :value items.",
        "lte.file" => "The :attribute field must be less than or equal to :value kilobytes.",
        "lte.numeric" => "The :attribute field must be less than or equal to :value.",
        "lte.string" => "The :attribute field must be less than or equal to :value characters.",
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

pub(super) fn dependent_path(
    pattern: &str,
    attribute: &str,
    other: &str,
) -> ValidationResult<String> {
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

fn implicit_condition(name: &str) -> bool {
    name.starts_with("required_")
        || name.starts_with("missing_")
        || name.starts_with("present_")
        || matches!(name, "accepted_if" | "declined_if")
}

fn accepted(value: Option<&Value>) -> bool {
    value.is_some_and(|value| {
        matches!(value, Value::Bool(true))
            || value.as_i64() == Some(1)
            || matches!(value.as_str(), Some("yes" | "on" | "1" | "true"))
    })
}

fn declined(value: Option<&Value>) -> bool {
    value.is_some_and(|value| {
        matches!(value, Value::Bool(false))
            || value.as_i64() == Some(0)
            || matches!(value.as_str(), Some("no" | "off" | "0" | "false"))
    })
}

fn dependent_matches(value: Option<&Value>, parameters: &[String], boolean_rule: bool) -> bool {
    let value = value.unwrap_or(&Value::Null);
    parameters.iter().any(|parameter| {
        if let Value::Bool(value) = value {
            return parameter == if *value { "true" } else { "false" };
        }
        if value.is_null() {
            return parameter.eq_ignore_ascii_case("null");
        }
        if boolean_rule && matches!(parameter.as_str(), "true" | "false") {
            let truth = match value {
                Value::Number(value) => value.as_f64().is_some_and(|value| value != 0.0),
                Value::String(value) => !value.is_empty() && value != "0",
                Value::Array(value) => !value.is_empty(),
                Value::Object(value) => !value.is_empty(),
                _ => false,
            };
            return truth == (parameter == "true");
        }
        crate::support::php_loose_string_value_equal(parameter, value)
    })
}

fn message_cases(name: &str, values: &[String], separator: &str) -> [(String, String); 3] {
    [
        (name.into(), values.join(separator)),
        (
            crate::support::Str::upper(name),
            values
                .iter()
                .map(|value| crate::support::Str::upper(value))
                .collect::<Vec<_>>()
                .join(separator),
        ),
        (
            crate::support::Str::ucfirst(name),
            values
                .iter()
                .map(|value| crate::support::Str::ucfirst(value))
                .collect::<Vec<_>>()
                .join(separator),
        ),
    ]
}

// 根字段的候选通配集合原样保留；数组内字段按上游依赖参数替换索引。
fn dependent_path_for_values(
    pattern: &str,
    attribute: &str,
    other: &str,
) -> ValidationResult<String> {
    if path::parse(pattern)?
        .iter()
        .any(|part| matches!(part, path::Segment::Any))
    {
        dependent_path(pattern, attribute, other)
    } else {
        path::parse(other)?;
        Ok(other.to_owned())
    }
}

fn uuid_version(value: &str, parameters: &str) -> bool {
    let parameters = super::rule_parameters::csv(parameters);
    if parameters.len() != 1 {
        return crate::support::Str::is_uuid(value, None);
    }
    if parameters[0] == "max" {
        return value.len() == 36 && uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.is_max());
    }
    let version = crate::support::input_scalars::integer_string(&parameters[0]);
    u8::try_from(version).is_ok_and(|version| crate::support::Str::is_uuid(value, Some(version)))
}

fn implicit_builtin(rule: &Kind, extensions: &super::extensions::Extensions) -> bool {
    matches!(
        rule,
        Kind::Required
            | Kind::RequiredIf(..)
            | Kind::Missing
            | Kind::Filled
            | Kind::Declined
            | Kind::Accepted
            | Kind::Present
    ) || matches!(rule,Kind::Conditional{name,..}if implicit_condition(name))
        || matches!(rule,Kind::Extension(name,_)if extensions.implicit.contains(name))
}
