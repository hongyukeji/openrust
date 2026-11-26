use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use openrust::{config::Environment, contracts::config::Repository, http::client::Factory};
use serde_json::Value;

use crate::{
    Agent, AgentResponse, AiError, AiResult, FakeTextGateway, PromptOptions, TextPrompt,
    TextProvider,
    text::{OpenAiTextProvider, TextProtocol},
};

type DriverFactory = dyn Fn(Value) -> AiResult<Arc<dyn TextProvider>> + Send + Sync;
type ProviderSlot = Arc<Mutex<Option<Arc<dyn TextProvider>>>>;

/// 当前应用的 AI 连接、协议驱动及 Fake。
pub struct AiManager {
    config: Arc<dyn Repository>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
    providers: Mutex<HashMap<String, ProviderSlot>>,
    fakes: RwLock<HashMap<TypeId, Arc<FakeTextGateway>>>,
}

impl AiManager {
    pub fn with_http(
        config: Arc<dyn Repository>,
        environment: Arc<Environment>,
        http: Arc<Factory>,
    ) -> Self {
        let manager = Self {
            config,
            factories: RwLock::default(),
            providers: Mutex::default(),
            fakes: RwLock::default(),
        };
        for (driver, protocol) in [
            ("openai", TextProtocol::Responses),
            ("openai-compatible", TextProtocol::ChatCompletions),
        ] {
            let environment = environment.clone();
            let http = http.clone();
            manager.extend(driver, move |config| {
                Ok(Arc::new(OpenAiTextProvider::new(
                    protocol,
                    config,
                    environment.clone(),
                    http.clone(),
                )?))
            });
        }
        manager
    }

    pub fn extend(
        &self,
        driver: impl Into<String>,
        factory: impl Fn(Value) -> AiResult<Arc<dyn TextProvider>> + Send + Sync + 'static,
    ) -> &Self {
        self.factories
            .write()
            .unwrap()
            .insert(driver.into(), Arc::new(factory));
        self
    }

    pub fn get_default_instance(&self) -> AiResult<String> {
        match self.config.get("ai.default") {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            None | Some(Value::Null) => Err(AiError::DefaultNotConfigured),
            _ => Err(AiError::InvalidConfiguration("default")),
        }
    }

    pub fn set_default_instance(&self, name: &str) -> AiResult<()> {
        if name.trim().is_empty() {
            return Err(AiError::InvalidConfiguration("default"));
        }
        self.config
            .set("ai.default", Value::String(name.into()))
            .map_err(|_| AiError::InvalidConfiguration("default"))
    }

    pub fn get_instance_config(&self, name: &str) -> AiResult<Value> {
        let mut profile = self
            .config
            .get("ai.providers")
            .and_then(|profiles| profiles.get(name).cloned())
            .ok_or(AiError::ProviderNotConfigured)?;
        profile
            .as_object_mut()
            .ok_or(AiError::InvalidConfiguration("providers.<name>"))?
            .insert("name".into(), name.into());
        Ok(profile)
    }

    /// 从具名配置解析文本驱动。
    pub fn text_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn TextProvider>> {
        let default;
        let name = match name {
            Some(name) => name,
            None => {
                default = self.get_default_instance()?;
                &default
            }
        };
        let slot = self
            .providers
            .lock()
            .unwrap()
            .entry(name.into())
            .or_default()
            .clone();
        let mut cached = slot.lock().unwrap();
        if let Some(provider) = cached.as_ref() {
            return Ok(provider.clone());
        }
        let provider = self.create_text_provider(self.get_instance_config(name)?)?;
        *cached = Some(provider.clone());
        Ok(provider)
    }

    pub fn text_provider_for(
        &self,
        agent: &impl Agent,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn TextProvider>> {
        self.text_provider(name.or_else(|| agent.provider()))
    }

    fn create_text_provider(&self, config: Value) -> AiResult<Arc<dyn TextProvider>> {
        let driver = config
            .get("driver")
            .and_then(Value::as_str)
            .filter(|driver| !driver.trim().is_empty())
            .ok_or(AiError::InvalidConfiguration("providers.<name>.driver"))?;
        let factory = self
            .factories
            .read()
            .unwrap()
            .get(driver)
            .cloned()
            .ok_or(AiError::DriverNotRegistered)?;
        factory(config)
    }

    pub fn forget_instance(&self, name: &str) {
        self.providers.lock().unwrap().remove(name);
    }

    pub fn fake<A: Agent>(
        &self,
        responses: impl IntoIterator<Item = String>,
    ) -> Arc<FakeTextGateway> {
        let fake = Arc::new(FakeTextGateway::new(responses));
        self.fakes
            .write()
            .unwrap()
            .insert(TypeId::of::<A>(), fake.clone());
        fake
    }

    pub fn text_fake<A: Agent>(&self) -> Option<Arc<FakeTextGateway>> {
        self.fakes.read().unwrap().get(&TypeId::of::<A>()).cloned()
    }

    pub async fn prompt<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<AgentResponse> {
        if prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        if let Some(fake) = self.text_fake::<A>() {
            let provider = options
                .provider
                .or_else(|| agent.provider())
                .map(str::to_owned)
                .or_else(|| self.get_default_instance().ok());
            let model = options
                .model
                .or_else(|| agent.model())
                .map(str::to_owned)
                .or_else(|| {
                    self.text_provider(provider.as_deref())
                        .ok()
                        .and_then(|resolved| resolved.default_text_model().ok().map(str::to_owned))
                });
            return fake.respond(prompt, provider, model);
        }
        let provider = self.text_provider_for(agent, options.provider)?;
        let model = match options.model.or_else(|| agent.model()) {
            Some(model) if !model.trim().is_empty() => model,
            Some(_) => return Err(AiError::MissingModel),
            None => provider.default_text_model()?,
        };
        provider
            .generate_text(TextPrompt {
                instructions: agent.instructions(),
                prompt,
                model,
                timeout: options.timeout,
            })
            .await
    }
}
