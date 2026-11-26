use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use md5::Md5;
use openrust::{
    config::Environment,
    contracts::config::Repository,
    http::client::Factory,
    support::{Str, facades::Facade},
};
use serde_json::Value;

use crate::{
    Agent, AgentPrompt, AgentResponse, AiError, AiResult, EmbeddingProvider, FakeTextGateway,
    PromptOptions, StreamableAgentResponse, StructuredOutput, TextPrompt, TextProvider,
    anthropic::AnthropicTextProvider,
    gemini::GeminiTextProvider,
    on_demand::{OnDemandProviders, reserved_name},
    text::{OpenAiProtocol, OpenAiTextProvider},
};

type DriverFactory = dyn Fn(Value) -> AiResult<Arc<dyn TextProvider>> + Send + Sync;
type ProviderSlot = Arc<Mutex<Option<Arc<dyn TextProvider>>>>;

/// 当前应用的 AI 连接、协议驱动及 Fake。
pub struct AiManager {
    config: Arc<dyn Repository>,
    factories: RwLock<HashMap<String, Arc<DriverFactory>>>,
    providers: Mutex<HashMap<String, ProviderSlot>>,
    fakes: RwLock<HashMap<TypeId, Arc<FakeTextGateway>>>,
    on_demand: Arc<OnDemandProviders>,
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
            on_demand: Arc::new(OnDemandProviders::new(Arc::new(()))),
        };
        for (driver, protocol) in [
            ("openai", OpenAiProtocol::Responses),
            ("openai-compatible", OpenAiProtocol::ChatCompletions),
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
        {
            let environment = environment.clone();
            let http = http.clone();
            manager.extend("anthropic", move |config| {
                Ok(Arc::new(AnthropicTextProvider::new(
                    config,
                    environment.clone(),
                    http.clone(),
                )?))
            });
        }
        manager.extend("gemini", move |config| {
            Ok(Arc::new(GeminiTextProvider::new(
                config,
                environment.clone(),
                http.clone(),
            )?))
        });
        manager
    }

    pub(crate) fn with_scope_owner(mut self, owner: Arc<()>) -> Self {
        self.on_demand = Arc::new(OnDemandProviders::new(owner));
        self
    }

    fn on_demand(&self) -> Arc<OnDemandProviders> {
        OnDemandProviders::get_facade_root()
            .ok()
            .filter(|scope| Arc::ptr_eq(&scope.owner, &self.on_demand.owner))
            .unwrap_or_else(|| self.on_demand.clone())
    }

    /// 注册当前执行范围可用的按需 Provider。
    pub fn build(&self, mut config: Value) -> AiResult<Arc<dyn TextProvider>> {
        let name = match config.get("name") {
            None | Some(Value::Null) => {
                let encoded = serde_json::to_string(&config)
                    .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
                format!("ondemand_{}", Str::of(encoded).hash::<Md5>().value())
            }
            Some(Value::String(name)) if !name.trim().is_empty() => name.clone(),
            _ => return Err(AiError::InvalidConfiguration("providers.<name>.name")),
        };
        if reserved_name(&name)
            || self
                .config
                .get("ai.providers")
                .is_some_and(|providers| providers.get(&name).is_some())
        {
            return Err(AiError::ProviderNameTaken);
        }
        let fields = config
            .as_object_mut()
            .ok_or(AiError::InvalidConfiguration("providers.<name>"))?;
        fields.insert("name".into(), name.clone().into());
        fields.insert("ondemand".into(), true.into());
        let provider = self.create_text_provider(config.clone())?;
        self.on_demand().insert(name, config, provider.clone());
        Ok(provider)
    }

    /// 清除当前执行范围的按需 Provider。
    pub fn flush_state(&self) {
        self.on_demand().clear();
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
        if let Some(entry) = self.on_demand().entry(name) {
            return Ok(entry.config.clone());
        }
        self.configured_instance(name)
    }

    fn configured_instance(&self, name: &str) -> AiResult<Value> {
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
        if let Some(entry) = self.on_demand().entry(name) {
            return entry.instance(|config| self.create_text_provider(config));
        }
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
        let provider = self.create_text_provider(self.configured_instance(name)?)?;
        *cached = Some(provider.clone());
        Ok(provider)
    }

    pub fn embedding_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn EmbeddingProvider>> {
        let default = if name.is_none() {
            match self.config.get("ai.default_for_embeddings") {
                Some(Value::String(name)) if !name.trim().is_empty() => Some(name),
                None | Some(Value::Null) => None,
                _ => return Err(AiError::InvalidConfiguration("default_for_embeddings")),
            }
        } else {
            None
        };
        self.text_provider(name.or(default.as_deref()))?
            .embedding_provider()
            .ok_or(AiError::EmbeddingsNotSupported)
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
        if let Some(entry) = self.on_demand().entry(name) {
            entry.forget_instance();
        } else {
            self.providers.lock().unwrap().remove(name);
        }
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
        self.prompt_with_schema(agent, prompt, options, None).await
    }

    pub(crate) async fn prompt_with_schema<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
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
            return fake.respond(
                prompt,
                agent.messages(),
                structured_output.as_ref().map(|output| output.schema),
                provider,
                model,
            );
        }
        let provider = self.text_provider_for(agent, options.provider)?;
        let model = resolve_model(provider.as_ref(), options.model.or_else(|| agent.model()))?;
        provider
            .generate(AgentPrompt {
                text: TextPrompt {
                    instructions: agent.instructions(),
                    prompt,
                    model,
                    timeout: options.timeout,
                },
                messages: agent.messages(),
                structured_output,
            })
            .await
    }

    pub async fn stream<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
    ) -> AiResult<StreamableAgentResponse> {
        self.stream_with_schema(agent, prompt, options, None).await
    }

    pub(crate) async fn stream_with_schema<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
    ) -> AiResult<StreamableAgentResponse> {
        if prompt.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        if self.text_fake::<A>().is_some() {
            return self
                .prompt_with_schema(agent, prompt, options, structured_output)
                .await
                .map(StreamableAgentResponse::from_text);
        }
        let provider = self.text_provider_for(agent, options.provider)?;
        let model = resolve_model(provider.as_ref(), options.model.or_else(|| agent.model()))?;
        provider
            .stream(AgentPrompt {
                text: TextPrompt {
                    instructions: agent.instructions(),
                    prompt,
                    model,
                    timeout: options.timeout,
                },
                messages: agent.messages(),
                structured_output,
            })
            .await
    }
}

fn resolve_model<'a>(provider: &'a dyn TextProvider, model: Option<&'a str>) -> AiResult<&'a str> {
    match model {
        Some(model) if !model.trim().is_empty() => Ok(model),
        Some(_) => Err(AiError::MissingModel),
        None => provider.default_text_model(),
    }
}
