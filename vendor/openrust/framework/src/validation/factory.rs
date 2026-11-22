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
}

/// 工厂按创建时的注册快照装配验证器，各实例后续修改互不影响。
#[derive(Default)]
pub struct Factory {
    translator: Option<Arc<Translator>>,
    presence: Option<DatabasePresenceVerifier>,
    registrations: RwLock<Registrations>,
}

impl Factory {
    pub fn new(translator: Arc<Translator>) -> Self {
        Self {
            translator: Some(translator),
            ..Default::default()
        }
    }

    #[allow(non_snake_case)]
    pub fn setPresenceVerifier(&mut self, verifier: DatabasePresenceVerifier) {
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

impl Contract for Factory {
    fn make(
        &self,
        data: Data,
        rules: RuleSet,
        messages: BTreeMap<String, String>,
        attributes: BTreeMap<String, String>,
    ) -> ValidationResult<Validator> {
        // 只在复制注册表时持锁；应用回调可以安全再次使用工厂。
        let registrations = self.registrations.read().expect("验证器注册锁损坏").clone();
        let mut validator = Validator::new(
            data,
            rules,
            self.translator.clone(),
            None,
            messages,
            attributes,
        );
        if let Some(resolver) = registrations.resolver {
            validator = resolver(validator)?;
        }
        validator.presence = self.presence.clone();
        validator.excludeUnvalidatedArrayKeys = !registrations.include_unvalidated_array_keys;
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
        Ok(validator)
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

    #[allow(non_snake_case)]
    fn extendImplicit(&self, rule: &str, extension: ValidationExtension, message: Option<&str>) {
        Factory::extendImplicit(
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

#[allow(non_snake_case)]
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
    pub fn extendImplicit(
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
    pub fn extendDependent(
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

#[allow(non_snake_case)]
impl Factory {
    pub fn includeUnvalidatedArrayKeys(&self) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .include_unvalidated_array_keys = true;
    }
    pub fn excludeUnvalidatedArrayKeys(&self) {
        self.registrations
            .write()
            .expect("验证器注册锁损坏")
            .include_unvalidated_array_keys = false;
    }
}
