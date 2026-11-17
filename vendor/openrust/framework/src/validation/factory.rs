use std::{collections::BTreeMap, sync::Arc};

use crate::{contracts::validation::Factory as Contract, translation::Translator};

use super::{Data, DatabasePresenceVerifier, RuleSet, ValidationResult, Validator};

/// Builds validators with the application's translator.
#[derive(Default)]
pub struct Factory {
    translator: Option<Arc<Translator>>,
    presence: Option<DatabasePresenceVerifier>,
}

impl Factory {
    pub fn new(translator: Arc<Translator>) -> Self {
        Self {
            translator: Some(translator),
            presence: None,
        }
    }

    #[allow(non_snake_case, reason = "Matches Laravel's validator factory API")]
    pub fn setPresenceVerifier(&mut self, verifier: DatabasePresenceVerifier) {
        self.presence = Some(verifier);
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
        Ok(Validator::new(
            data,
            rules,
            self.translator.clone(),
            self.presence.clone(),
            messages,
            attributes,
        ))
    }
}
