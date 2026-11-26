use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use openrust::{container::ServiceKey, support::facades::Facade};
use serde_json::Value;

use crate::{AiResult, TextProvider};

pub(crate) struct OnDemandProviders {
    pub(crate) owner: Arc<()>,
    entries: Mutex<HashMap<String, Arc<Entry>>>,
}

impl OnDemandProviders {
    pub(crate) fn new(owner: Arc<()>) -> Self {
        Self {
            owner,
            entries: Mutex::default(),
        }
    }

    pub(crate) fn insert(&self, name: String, config: Value, instance: Arc<dyn TextProvider>) {
        self.entries.lock().unwrap().insert(
            name,
            Arc::new(Entry {
                config,
                instance: Mutex::new(Some(instance)),
            }),
        );
    }

    pub(crate) fn entry(&self, name: &str) -> Option<Arc<Entry>> {
        self.entries.lock().unwrap().get(name).cloned()
    }

    pub(crate) fn clear(&self) {
        self.entries.lock().unwrap().clear();
    }
}

impl Facade for OnDemandProviders {
    type Service = Self;

    fn get_facade_accessor() -> ServiceKey {
        ServiceKey::typed::<Self>()
    }
}

pub(crate) struct Entry {
    pub(crate) config: Value,
    instance: Mutex<Option<Arc<dyn TextProvider>>>,
}

impl Entry {
    pub(crate) fn instance(
        &self,
        build: impl FnOnce(Value) -> AiResult<Arc<dyn TextProvider>>,
    ) -> AiResult<Arc<dyn TextProvider>> {
        let mut instance = self.instance.lock().unwrap();
        if let Some(instance) = instance.as_ref() {
            return Ok(instance.clone());
        }
        let created = build(self.config.clone())?;
        *instance = Some(created.clone());
        Ok(created)
    }

    pub(crate) fn forget_instance(&self) {
        self.instance.lock().unwrap().take();
    }
}

pub(crate) fn reserved_name(name: &str) -> bool {
    matches!(
        name,
        "anthropic"
            | "azure"
            | "bedrock"
            | "cohere"
            | "deepseek"
            | "eleven"
            | "gemini"
            | "groq"
            | "jina"
            | "mistral"
            | "ollama"
            | "openai"
            | "openai-compatible"
            | "openrouter"
            | "typesafe"
            | "voyageai"
            | "xai"
    )
}
