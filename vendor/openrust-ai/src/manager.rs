use std::{
    any::TypeId,
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use md5::Md5;
use openrust::{
    config::Environment,
    contracts::config::Repository,
    http::{HttpFuture, client::Factory},
    support::{Str, facades::Facade},
};
use serde_json::Value;

use crate::{
    Agent, AgentPrompt, AgentResponse, AiError, AiResult, EmbeddingProvider, FakeEmbeddingGateway,
    FakeRerankingGateway, FakeRerankingResult, FakeTextGateway, PromptOptions, RecordedEmbeddings,
    RecordedReranking, RerankingProvider, StreamableAgentResponse, StructuredOutput, TextModelTier,
    TextPrompt, TextProvider,
    anthropic::AnthropicTextProvider,
    classification::{
        DecisionsProtocol, FakeClassificationGateway, FakeClassificationResult,
        HttpDecisionsProvider, RecordedClassification,
    },
    connection::{Authorization, Connection},
    error::ProviderAttemptError,
    gemini::GeminiTextProvider,
    on_demand::{OnDemandProviders, reserved_name},
    reranking::{HttpRerankingProvider, RerankingProtocol},
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
    embeddings_fake: RwLock<Option<Arc<FakeEmbeddingGateway>>>,
    store_fake: RwLock<Option<Arc<crate::stores::FakeStoreGateway>>>,
    file_fake: RwLock<Option<Arc<crate::files::FakeFileGateway>>>,
    classification_fake: RwLock<Option<Arc<FakeClassificationGateway>>>,
    reranking_fake: RwLock<Option<Arc<FakeRerankingGateway>>>,
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
            embeddings_fake: RwLock::default(),
            file_fake: RwLock::default(),
            store_fake: RwLock::default(),
            classification_fake: RwLock::default(),
            reranking_fake: RwLock::default(),
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
        for (driver, protocol) in [
            ("cohere", RerankingProtocol::TopN),
            ("jina", RerankingProtocol::TopN),
            ("voyageai", RerankingProtocol::Voyage),
        ] {
            let environment = environment.clone();
            let http = http.clone();
            manager.extend(driver, move |config| {
                let connection = Arc::new(Connection::new(
                    config,
                    environment.clone(),
                    http.clone(),
                    Authorization::Bearer,
                )?);
                Ok(Arc::new(HttpRerankingProvider {
                    connection,
                    protocol,
                }))
            });
        }
        {
            let environment = environment.clone();
            let http = http.clone();
            manager.extend("system-one", move |config| {
                let connection = Arc::new(Connection::new(
                    config,
                    environment.clone(),
                    http.clone(),
                    Authorization::Bearer,
                )?);
                Ok(Arc::new(HttpDecisionsProvider {
                    connection,
                    protocol: DecisionsProtocol::SystemOne,
                }))
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

    pub(crate) fn conversation_store(&self) -> AiResult<(crate::DatabaseConversationStore, u64)> {
        let connection = match self.config.get("ai.conversations.connection") {
            Some(Value::String(name)) if !name.trim().is_empty() => Some(name),
            Some(Value::Null) | None => None,
            _ => return Err(AiError::InvalidConfiguration("conversations.connection")),
        };
        let history = match self.config.get("ai.conversations.max_messages") {
            Some(value) => value
                .as_u64()
                .ok_or(AiError::InvalidConfiguration("conversations.max_messages"))?,
            None => 100,
        };
        let database = openrust::database::Db::connection(connection.as_deref())
            .map_err(|_| AiError::ConversationStorageUnavailable)?;
        Ok((crate::DatabaseConversationStore::new(database), history))
    }

    pub(crate) fn embedding_cache_config(&self) -> Option<Value> {
        self.config.get("ai.caching.embeddings")
    }

    pub fn embedding_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn EmbeddingProvider>> {
        let name = self.embedding_provider_name(name)?;
        self.text_provider(Some(&name))?
            .embedding_provider()
            .ok_or(AiError::EmbeddingsNotSupported)
    }

    pub(crate) fn embedding_provider_name(&self, name: Option<&str>) -> AiResult<String> {
        match name {
            Some(name) => Ok(name.into()),
            None => match self.config.get("ai.default_for_embeddings") {
                Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
                None | Some(Value::Null) => self.get_default_instance(),
                _ => Err(AiError::InvalidConfiguration("default_for_embeddings")),
            },
        }
    }

    pub fn fake_embeddings(
        &self,
        responses: impl IntoIterator<Item = Vec<Vec<f32>>>,
    ) -> Arc<FakeEmbeddingGateway> {
        self.install_embeddings_fake(FakeEmbeddingGateway::new(responses))
    }

    pub fn fake_embeddings_with(
        &self,
        resolve: impl Fn(&RecordedEmbeddings) -> AiResult<Vec<Vec<f32>>> + Send + Sync + 'static,
    ) -> Arc<FakeEmbeddingGateway> {
        self.install_embeddings_fake(FakeEmbeddingGateway::with(resolve))
    }

    fn install_embeddings_fake(&self, fake: FakeEmbeddingGateway) -> Arc<FakeEmbeddingGateway> {
        let fake = Arc::new(fake);
        *self.embeddings_fake.write().unwrap() = Some(fake.clone());
        fake
    }

    pub fn embeddings_fake(&self) -> Option<Arc<FakeEmbeddingGateway>> {
        self.embeddings_fake.read().unwrap().clone()
    }

    fn provider_name_for(&self, name: Option<&str>, field: &'static str) -> AiResult<String> {
        if let Some(name) = name {
            return Ok(name.into());
        }
        match self.config.get(&format!("ai.{field}")) {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            None | Some(Value::Null) => self.get_default_instance(),
            _ => Err(AiError::InvalidConfiguration(field)),
        }
    }

    pub fn image_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn crate::ImageProvider>> {
        let name = self.provider_name_for(name, "default_for_images")?;
        self.text_provider(Some(&name))?
            .image_provider()
            .ok_or(AiError::ImagesNotSupported)
    }

    pub fn audio_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn crate::AudioProvider>> {
        let name = self.provider_name_for(name, "default_for_audio")?;
        self.text_provider(Some(&name))?
            .audio_provider()
            .ok_or(AiError::AudioNotSupported)
    }

    pub fn transcription_provider(
        &self,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::TranscriptionProvider>> {
        let name = self.provider_name_for(name, "default_for_transcription")?;
        self.text_provider(Some(&name))?
            .transcription_provider()
            .ok_or(AiError::TranscriptionNotSupported)
    }

    pub fn file_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn crate::FileProvider>> {
        let name = self.provider_name_for(name, "default_for_files")?;
        self.text_provider(Some(&name))?
            .file_provider()
            .ok_or(AiError::FilesNotSupported)
    }

    pub fn fakeable_file_provider(
        &self,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::FileProvider>> {
        let Some(fake) = self.files_fake() else {
            return self.file_provider(name);
        };
        let name = self.fake_provider_name(name, "default_for_files")?;
        Ok(fake.provider(name))
    }

    pub fn fake_files(
        &self,
        responses: impl IntoIterator<Item = bytes::Bytes>,
    ) -> Arc<crate::files::FakeFileGateway> {
        self.install_file_fake(crate::files::FakeFileGateway::new(responses))
    }

    pub fn fake_files_with(
        &self,
        resolve: impl Fn(&str) -> AiResult<bytes::Bytes> + Send + Sync + 'static,
    ) -> Arc<crate::files::FakeFileGateway> {
        self.install_file_fake(crate::files::FakeFileGateway::with(resolve))
    }

    fn install_file_fake(
        &self,
        fake: crate::files::FakeFileGateway,
    ) -> Arc<crate::files::FakeFileGateway> {
        let fake = Arc::new(fake);
        *self.file_fake.write().unwrap() = Some(fake.clone());
        fake
    }

    pub fn files_fake(&self) -> Option<Arc<crate::files::FakeFileGateway>> {
        self.file_fake.read().unwrap().clone()
    }

    pub fn store_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn crate::StoreProvider>> {
        let name = self.provider_name_for(name, "default_for_stores")?;
        self.text_provider(Some(&name))?
            .store_provider()
            .ok_or(AiError::StoresNotSupported)
    }

    pub fn fakeable_store_provider(
        &self,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::StoreProvider>> {
        let Some(fake) = self.stores_fake() else {
            return self.store_provider(name);
        };
        let name = self.fake_provider_name(name, "default_for_stores")?;
        let files = match self.fakeable_file_provider(Some(&name)) {
            Ok(provider) => Some(provider),
            Err(AiError::ProviderNotConfigured | AiError::FilesNotSupported) => None,
            Err(error) => return Err(error),
        };
        Ok(fake.provider(name, files))
    }

    fn fake_provider_name(&self, name: Option<&str>, default: &'static str) -> AiResult<String> {
        match self.provider_name_for(name, default) {
            Ok(name) => Ok(name),
            Err(AiError::DefaultNotConfigured) => Ok("fake".into()),
            Err(error) => Err(error),
        }
    }

    pub fn fake_stores(
        &self,
        responses: impl IntoIterator<Item = String>,
    ) -> Arc<crate::stores::FakeStoreGateway> {
        self.install_store_fake(crate::stores::FakeStoreGateway::new(responses))
    }

    pub fn fake_stores_with(
        &self,
        resolve: impl Fn(&str) -> AiResult<String> + Send + Sync + 'static,
    ) -> Arc<crate::stores::FakeStoreGateway> {
        self.install_store_fake(crate::stores::FakeStoreGateway::with(resolve))
    }

    fn install_store_fake(
        &self,
        fake: crate::stores::FakeStoreGateway,
    ) -> Arc<crate::stores::FakeStoreGateway> {
        let fake = Arc::new(fake);
        *self.store_fake.write().unwrap() = Some(fake.clone());
        fake
    }

    pub fn stores_fake(&self) -> Option<Arc<crate::stores::FakeStoreGateway>> {
        self.store_fake.read().unwrap().clone()
    }

    pub fn classification_provider(
        &self,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn crate::ClassificationProvider>> {
        let name = self.classification_provider_name(name)?;
        self.text_provider(Some(&name))?
            .classification_provider()
            .ok_or(AiError::ClassificationNotSupported)
    }

    pub(crate) fn classification_provider_name(&self, name: Option<&str>) -> AiResult<String> {
        self.provider_name_for(name, "default_for_classification")
    }

    pub fn fake_classification(
        &self,
        responses: impl IntoIterator<Item = FakeClassificationResult>,
    ) -> Arc<FakeClassificationGateway> {
        self.install_classification_fake(FakeClassificationGateway::new(responses))
    }

    pub fn fake_classification_with(
        &self,
        resolve: impl Fn(&RecordedClassification) -> AiResult<FakeClassificationResult>
        + Send
        + Sync
        + 'static,
    ) -> Arc<FakeClassificationGateway> {
        self.install_classification_fake(FakeClassificationGateway::with(resolve))
    }

    fn install_classification_fake(
        &self,
        fake: FakeClassificationGateway,
    ) -> Arc<FakeClassificationGateway> {
        let fake = Arc::new(fake);
        *self.classification_fake.write().unwrap() = Some(fake.clone());
        fake
    }

    pub fn classification_fake(&self) -> Option<Arc<FakeClassificationGateway>> {
        self.classification_fake.read().unwrap().clone()
    }

    pub fn reranking_provider(&self, name: Option<&str>) -> AiResult<Arc<dyn RerankingProvider>> {
        let name = self.reranking_provider_name(name)?;
        self.text_provider(Some(&name))?
            .reranking_provider()
            .ok_or(AiError::RerankingNotSupported)
    }

    pub(crate) fn reranking_provider_name(&self, name: Option<&str>) -> AiResult<String> {
        if let Some(name) = name {
            return Ok(name.into());
        }
        match self.config.get("ai.default_for_reranking") {
            Some(Value::String(name)) if !name.trim().is_empty() => Ok(name),
            Some(Value::Null) | None => self.get_default_instance(),
            _ => Err(AiError::InvalidConfiguration("default_for_reranking")),
        }
    }

    pub fn fake_reranking(
        &self,
        responses: impl IntoIterator<Item = FakeRerankingResult>,
    ) -> Arc<FakeRerankingGateway> {
        self.install_reranking_fake(FakeRerankingGateway::new(responses))
    }

    pub fn fake_reranking_with(
        &self,
        resolve: impl Fn(&RecordedReranking) -> AiResult<FakeRerankingResult> + Send + Sync + 'static,
    ) -> Arc<FakeRerankingGateway> {
        self.install_reranking_fake(FakeRerankingGateway::with(resolve))
    }

    fn install_reranking_fake(&self, fake: FakeRerankingGateway) -> Arc<FakeRerankingGateway> {
        let fake = Arc::new(fake);
        *self.reranking_fake.write().unwrap() = Some(fake.clone());
        fake
    }

    pub fn reranking_fake(&self) -> Option<Arc<FakeRerankingGateway>> {
        self.reranking_fake.read().unwrap().clone()
    }

    pub fn text_provider_for(
        &self,
        agent: &impl Agent,
        name: Option<&str>,
    ) -> AiResult<Arc<dyn TextProvider>> {
        self.text_provider(name.or_else(|| primary_provider(agent)))
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
        self.prompt_with_history(agent, prompt, options, structured_output, agent.messages())
            .await
    }

    pub(crate) async fn prompt_with_history<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
        messages: &[crate::Message],
    ) -> AiResult<AgentResponse> {
        self.prompt_in_session(agent, prompt, options, structured_output, messages, None)
            .await
    }

    pub(crate) async fn prompt_in_session<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
        messages: &[crate::Message],
        approvals: Option<&crate::approval_turn::ApprovalSession>,
    ) -> AiResult<AgentResponse> {
        let attachments = agent.attachments();
        let generation = agent.generation_options();
        generation.validate()?;
        let choice = agent.tool_choice();
        if prompt.trim().is_empty() && attachments.is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        crate::attachments::validate_prompt_attachments(attachments)?;
        if let Some(fake) = self.text_fake::<A>() {
            let provider = options
                .provider
                .or_else(|| primary_provider(agent))
                .map(str::to_owned)
                .or_else(|| self.get_default_instance().ok());
            let model = primary_model(agent, options)
                .map(str::to_owned)
                .or_else(|| {
                    self.text_provider(provider.as_deref())
                        .ok()
                        .and_then(|resolved| {
                            resolved
                                .text_model(agent.model_tier())
                                .ok()
                                .map(str::to_owned)
                        })
                });
            return fake.respond(crate::RecordedPrompt {
                text: prompt.into(),
                options: generation,
                provider_options: provider
                    .as_deref()
                    .map(|provider| agent.provider_options(provider))
                    .unwrap_or_default(),
                tool_choice: choice,
                messages: messages.to_vec(),
                attachments: attachments.to_vec(),
                schema: structured_output
                    .as_ref()
                    .map(|output| output.schema.clone()),
                provider,
                model,
            });
        }
        let request = AgentPrompt {
            text: TextPrompt {
                instructions: agent.instructions(),
                prompt,
                model: "",
                timeout: options.timeout,
            },
            options: generation,
            provider_options: None,
            tool_choice: choice.as_ref(),
            messages,
            attachments,
            structured_output,
        };
        let tools = crate::load_skill::tools_for(agent)?;
        if tools.is_empty()
            && let Some(choice) = &choice
        {
            choice.validate(&[])?;
        }
        let middleware = agent.middleware();
        let agent_type = std::any::type_name::<A>();
        if tools.is_empty() && middleware.is_empty() {
            self.with_text_failover(agent, options, request, None, |provider, request, _, _| {
                Box::pin(crate::events::observe_prompt(
                    agent_type,
                    provider.name(),
                    request,
                    move |request| async move {
                        let choice = request.tool_choice;
                        let observation = crate::events::StepObservation::start(
                            provider.name(),
                            &request,
                            &[],
                            0,
                            true,
                        )
                        .await;
                        let outcome = provider.generate(request).await;
                        if let Some(observation) = observation {
                            match &outcome {
                                Ok(response) => {
                                    observation
                                        .complete(|_| {
                                            response.steps.last().cloned().unwrap_or_else(|| {
                                                response.public_step(Default::default())
                                            })
                                        })
                                        .await;
                                }
                                Err(error) => observation.fail(error).await,
                            }
                        }
                        let response = outcome.map_err(ProviderAttemptError::BeforeCommit)?;
                        if let Some(choice) = choice {
                            choice.validate_calls(&response.tool_calls)?;
                        }
                        Ok(response)
                    },
                ))
            })
            .await
        } else {
            let max_steps = agent.max_steps();
            self.with_text_failover(
                agent,
                options,
                request,
                approvals,
                |provider, request, approvals, _| {
                    let tools = tools.clone();
                    let middleware = middleware.clone();
                    Box::pin(crate::events::observe_prompt(
                        agent_type,
                        provider.name(),
                        request,
                        move |request| {
                            crate::tools::run(
                                provider.as_ref(),
                                request,
                                tools,
                                max_steps,
                                middleware,
                                approvals,
                            )
                        },
                    ))
                },
            )
            .await
        }
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
        self.stream_with_history(agent, prompt, options, structured_output, agent.messages())
            .await
    }

    pub(crate) async fn stream_with_history<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
        messages: &[crate::Message],
    ) -> AiResult<StreamableAgentResponse> {
        self.stream_in_session(agent, prompt, options, structured_output, messages, None)
            .await
    }

    pub(crate) async fn stream_in_session<A: Agent>(
        &self,
        agent: &A,
        prompt: &str,
        options: PromptOptions<'_>,
        structured_output: Option<StructuredOutput<'_>>,
        messages: &[crate::Message],
        approvals: Option<crate::approval_turn::ApprovalSession>,
    ) -> AiResult<StreamableAgentResponse> {
        let attachments = agent.attachments();
        let generation = agent.generation_options();
        generation.validate()?;
        let choice = agent.tool_choice();
        if prompt.trim().is_empty() && attachments.is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        crate::attachments::validate_prompt_attachments(attachments)?;
        if self.text_fake::<A>().is_some() {
            return self
                .prompt_with_history(agent, prompt, options, structured_output, messages)
                .await
                .map(StreamableAgentResponse::from_text);
        }
        if !agent.middleware().is_empty() {
            return Err(AiError::StreamingMiddlewareNotSupported);
        }
        let tools = crate::load_skill::tools_for(agent)?;
        if tools.is_empty()
            && let Some(choice) = &choice
        {
            choice.validate(&[])?;
        }
        let request = AgentPrompt {
            text: TextPrompt {
                instructions: agent.instructions(),
                prompt,
                model: "",
                timeout: options.timeout,
            },
            options: generation,
            provider_options: None,
            tool_choice: choice.as_ref(),
            messages,
            attachments,
            structured_output,
        };
        let agent_type = std::any::type_name::<A>();
        if tools.is_empty() {
            let (response, attempt) = self
                .with_text_failover(
                    agent,
                    options,
                    request,
                    None,
                    |provider, request, _, probe| {
                        Box::pin(crate::events::observe_stream_start(
                            agent_type,
                            provider.name(),
                            request,
                            move |request| async move {
                                let response = provider
                                    .stream(request)
                                    .await
                                    .map_err(ProviderAttemptError::BeforeCommit)?;
                                if probe {
                                    response
                                        .probe()
                                        .await
                                        .map_err(ProviderAttemptError::BeforeCommit)
                                } else {
                                    Ok(response)
                                }
                            },
                        ))
                    },
                )
                .await?;
            Ok(response.observe_attempt(attempt))
        } else {
            let max_steps = agent.max_steps();
            let (prepared, attempt) = self
                .with_text_failover(
                    agent,
                    options,
                    request,
                    None,
                    |provider, request, _, probe| {
                        let tools = tools.clone();
                        Box::pin(crate::events::observe_stream_start(
                            agent_type,
                            provider.name(),
                            request,
                            move |request| async move {
                                crate::tool_stream::prepare(
                                    provider.clone(),
                                    request,
                                    tools,
                                    max_steps,
                                    probe,
                                )
                                .await
                                .map_err(ProviderAttemptError::BeforeCommit)
                            },
                        ))
                    },
                )
                .await?;
            Ok(crate::tool_stream::response(prepared, approvals).observe_attempt(attempt))
        }
    }

    async fn with_text_failover<A: Agent, T>(
        &self,
        agent: &A,
        options: PromptOptions<'_>,
        request: AgentPrompt<'_>,
        approvals: Option<&crate::approval_turn::ApprovalSession>,
        send: impl for<'a> Fn(
            &'a Arc<dyn TextProvider>,
            AgentPrompt<'a>,
            Option<&'a crate::approval_turn::ApprovalSession>,
            bool,
        ) -> HttpFuture<'a, Result<T, ProviderAttemptError>>,
    ) -> AiResult<T> {
        let invocation_id = Str::ulid().to_string();
        let targets = agent.providers();
        let mut next = targets.iter().skip(1).peekable();
        let mut name = options.provider.or_else(|| primary_provider(agent));
        let mut requested_model = primary_model(agent, options);
        loop {
            let provider = self.text_provider(name)?;
            let model = resolve_model(provider.as_ref(), requested_model, agent.model_tier())?;
            let provider_options = agent.provider_options(provider.name());
            let attempt = AgentPrompt {
                text: TextPrompt {
                    model,
                    ..request.text
                },
                provider_options: Some(&provider_options),
                ..request
            };
            let can_fail_over = options.provider.is_none() && next.peek().is_some();
            match crate::request_context::in_agent_invocation(
                &invocation_id,
                0,
                Some(std::any::type_name::<A>()),
                send(&provider, attempt, approvals, can_fail_over),
            )
            .await
            {
                Err(ProviderAttemptError::BeforeCommit(error))
                    if can_fail_over && error.allows_provider_failover() =>
                {
                    let target = next.next().expect("已确认存在备用连接");
                    crate::events::dispatch_agent_failover(crate::events::AgentFailedOver {
                        invocation_id: invocation_id.clone(),
                        agent: std::any::type_name::<A>(),
                        provider: provider.name().to_owned(),
                        model: model.to_owned(),
                        error,
                    })
                    .await;
                    name = Some(&target.provider);
                    requested_model = target.model.as_deref();
                }
                result => return result.map_err(ProviderAttemptError::into_error),
            }
        }
    }
}

fn primary_provider(agent: &impl Agent) -> Option<&str> {
    agent
        .providers()
        .first()
        .map(|target| target.provider.as_str())
        .or_else(|| agent.provider())
}

fn primary_model<'a>(agent: &'a impl Agent, options: PromptOptions<'a>) -> Option<&'a str> {
    if options.provider.is_none()
        && let Some(first) = agent.providers().first()
    {
        first.model.as_deref()
    } else {
        options.model.or_else(|| agent.model())
    }
}

fn resolve_model<'a>(
    provider: &'a dyn TextProvider,
    model: Option<&'a str>,
    tier: TextModelTier,
) -> AiResult<&'a str> {
    match model {
        Some(model) if !model.trim().is_empty() => Ok(model),
        Some(_) => Err(AiError::MissingModel),
        None => provider.text_model(tier),
    }
}
