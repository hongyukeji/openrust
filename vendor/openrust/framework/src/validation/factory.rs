use std::{collections::BTreeMap, sync::Arc};

use crate::{contracts::validation::Factory as Contract, translation::Translator};

use super::{Data, RuleSet, ValidationResult, Validator};

/// Builds validators with the application's translator.
#[derive(Default)]
pub struct Factory {
    translator: Option<Arc<Translator>>,
}

impl Factory {
    pub fn new(translator: Arc<Translator>) -> Self {
        Self {
            translator: Some(translator),
        }
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
            messages,
            attributes,
        ))
    }
}
