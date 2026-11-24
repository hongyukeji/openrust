use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use crate::{contracts::validation::Factory as Contract, translation::Translator};

use super::extensions::name;
use super::{
    Data, DatabasePresenceVerifier, RuleSet, ValidationExtension, ValidationReplacer,
    ValidationResult, Validator, ValidatorResolver,
};

#[derive(Clone, Default)]
struct Registrations {
    ordinary: BTreeMap<String, ValidationExtension>,
    implicit: BTreeMap<String, ValidationExtension>,
    dependent: BTreeMap<String, ValidationExtension>,
    replacers: BTreeMap<String, ValidationReplacer>,
    fallback_messages: BTreeMap<String, String>,
    resolver: Option<ValidatorResolver>,
    include_unvalidated_array_keys: bool,
    password_defaults: Option<Arc<dyn Fn() -> super::rules::Password + Send + Sync>>,
    uncompromised: Option<Arc<dyn crate::contracts::validation::UncompromisedVerifier>>,
}

/// 扩展按验证器创建时取得注册快照；工厂克隆共享注册表和DNS测试策略。
#[derive(Clone, Default)]
pub struct Factory {
    translator: Option<Arc<Translator>>,
    pub(super) owner: std::sync::Weak<crate::container::Container>,
    presence: Option<DatabasePresenceVerifier>,
    registrations: Arc<RwLock<Registrations>>,
    dns_policy: Arc<std::sync::atomic::AtomicBool>,
}

impl Factory {
    pub fn new(translator: Arc<Translator>) -> Self {
        Self {
            translator: Some(translator),
            ..Default::default()
        }
    }

    /// 默认策略按应用保存；调用回调时不持有注册锁。
    pub fn set_password_defaults(
        &self,
        callback: impl Fn() -> super::rules::Password + Send + Sync + 'static,
    ) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .password_defaults = Some(Arc::new(callback));
    }
    pub fn password_defaults(&self) -> super::rules::Password {
        let callback = self
            .registrations
            .read()
            .expect("验证器注册锁损坏")
            .password_defaults
            .clone();
        callback.map_or_else(|| super::rules::Password::min(8), |callback| callback())
    }
    pub fn set_uncompromised_verifier(
        &self,
        verifier: Arc<dyn crate::contracts::validation::UncompromisedVerifier>,
    ) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .uncompromised = Some(verifier);
    }
    pub(super) async fn verify_uncompromised(
        &self,
        value: &super::Value,
        threshold: i64,
    ) -> ValidationResult<bool> {
        use crate::contracts::container::Container as _;
        let verifier = self
            .registrations
            .read()
            .expect("验证器注册锁损坏")
            .uncompromised
            .clone();
        let verifier = match verifier {
            Some(verifier) => verifier,
            None => {
                let owner =
                    self.owner
                        .upgrade()
                        .ok_or_else(|| super::ValidationError::Execution {
                            source: std::io::Error::other("密码泄漏验证器尚未配置").into(),
                        })?;
                owner
                    .make_async::<dyn crate::contracts::validation::UncompromisedVerifier>()
                    .await
                    .map_err(|source| super::ValidationError::Execution {
                        source: Box::new(source),
                    })?
            }
        };
        verifier.verify(value, threshold).await
    }

    /// 每个应用共享自己的测试策略，已创建验证器也可观察切换，不使用进程全局状态。
    pub fn fake_dns_lookups(&self, value: bool) {
        self.dns_policy
            .store(value, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn set_presence_verifier(&mut self, verifier: DatabasePresenceVerifier) {
        self.presence = Some(verifier);
    }

    pub fn resolver(
        &self,
        resolver: impl Fn(Validator) -> ValidationResult<Validator> + Send + Sync + 'static,
    ) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .resolver = Some(Arc::new(resolver));
    }

    pub fn replacer(
        &self,
        rule: &str,
        replacer: impl Fn(&str, &str, &str, &[String], &Validator) -> String + Send + Sync + 'static,
    ) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .replacers
            .insert(name(rule), Arc::new(replacer));
    }
}

impl Factory {
    pub(super) fn make_shared(
        &self,
        data: Arc<Data>,
        rules: RuleSet,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> ValidationResult<Validator> {
        // 只在复制注册表时持锁；应用回调可以安全再次使用工厂。
        let registrations = self.registrations.read().expect("验证器注册锁损坏").clone();
        let mut validator = Validator::from_shared(
            data,
            rules,
            self.translator.clone(),
            None,
            messages,
            attributes,
        );
        validator.factory = Some(self.clone());
        validator.dns_policy = self.dns_policy.clone();
        validator.ensure_compiled()?;
        if let Some(resolver) = registrations.resolver {
            validator = resolver(validator)?;
        }
        validator.factory = Some(self.clone());
        validator.presence = self.presence.clone();
        validator.dns_policy = self.dns_policy.clone();
        validator.exclude_unvalidated_array_keys = !registrations.include_unvalidated_array_keys;
        validator
            .extensions
            .implicit
            .extend(registrations.implicit.keys().cloned());
        validator
            .extensions
            .dependent
            .extend(registrations.dependent.keys().cloned());
        validator
            .extensions
            .callbacks
            .extend(registrations.ordinary);
        validator
            .extensions
            .callbacks
            .extend(registrations.implicit);
        validator
            .extensions
            .callbacks
            .extend(registrations.dependent);
        validator
            .extensions
            .replacers
            .extend(registrations.replacers);
        validator
            .extensions
            .fallback_messages
            .extend(registrations.fallback_messages);
        validator.ensure_compiled()?;
        Ok(validator)
    }
}

impl Contract for Factory {
    fn make(
        &self,
        data: Data,
        rules: RuleSet,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> ValidationResult<Validator> {
        self.make_shared(Arc::new(data), rules, messages, attributes)
    }

    fn extend(&self, rule: &str, extension: ValidationExtension, message: Option<&str>) {
        Factory::extend(
            self,
            rule,
            move |attribute, value, parameters, validator| {
                extension(attribute, value, parameters, validator)
            },
            message,
        );
    }

    fn extend_implicit(&self, rule: &str, extension: ValidationExtension, message: Option<&str>) {
        Factory::extend_implicit(
            self,
            rule,
            move |attribute, value, parameters, validator| {
                extension(attribute, value, parameters, validator)
            },
            message,
        );
    }

    fn replacer(&self, rule: &str, replacer: ValidationReplacer) {
        Factory::replacer(
            self,
            rule,
            move |message, attribute, rule, parameters, validator| {
                replacer(message, attribute, rule, parameters, validator)
            },
        );
    }
}

impl Factory {
    pub fn extend(
        &self,
        rule: &str,
        extension: impl Fn(&str, Option<&super::Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) {
        let mut registrations = self.registrations.write().expect("验证器注册锁损坏");
        let rule = name(rule);
        registrations
            .ordinary
            .insert(rule.clone(), Arc::new(extension));
        if let Some(message) = message.filter(|message| !message.is_empty() && *message != "0") {
            registrations
                .fallback_messages
                .insert(rule, message.to_owned());
        }
    }
    pub fn extend_implicit(
        &self,
        rule: &str,
        extension: impl Fn(&str, Option<&super::Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) {
        let mut registrations = self.registrations.write().expect("验证器注册锁损坏");
        let rule = name(rule);
        registrations
            .implicit
            .insert(rule.clone(), Arc::new(extension));
        if let Some(message) = message.filter(|message| !message.is_empty() && *message != "0") {
            registrations
                .fallback_messages
                .insert(rule, message.to_owned());
        }
    }
    pub fn extend_dependent(
        &self,
        rule: &str,
        extension: impl Fn(&str, Option<&super::Value>, &[String], &Validator) -> ValidationResult<bool>
        + Send
        + Sync
        + 'static,
        message: Option<&str>,
    ) {
        let mut registrations = self.registrations.write().expect("验证器注册锁损坏");
        let rule = name(rule);
        registrations
            .dependent
            .insert(rule.clone(), Arc::new(extension));
        if let Some(message) = message.filter(|message| !message.is_empty() && *message != "0") {
            registrations
                .fallback_messages
                .insert(rule, message.to_owned());
        }
    }
}

impl Factory {
    pub fn include_unvalidated_array_keys(&self) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .include_unvalidated_array_keys = true;
    }
    pub fn exclude_unvalidated_array_keys(&self) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .include_unvalidated_array_keys = false;
    }
}
