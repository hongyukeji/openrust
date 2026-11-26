use std::{sync::Arc, time::Duration};

use openrust::{
    config::Environment,
    http::client::{Factory, PendingRequest, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    AiError, AiResult, ModelCatalog, TextEndpoint, models::ListedModel, text::transport_error,
};

#[derive(Default, Deserialize)]
struct Models {
    #[serde(default)]
    text: TextModels,
    embeddings: Option<EmbeddingModels>,
    reranking: Option<DefaultModel>,
    images: Option<DefaultModel>,
    audio: Option<DefaultModel>,
    transcription: Option<DefaultModel>,
    classification: Option<ClassificationModels>,
}

#[derive(Default, Deserialize)]
struct TextModels {
    default: Option<String>,
    #[serde(default)]
    ids: Vec<String>,
}

#[derive(Deserialize)]
struct EmbeddingModels {
    default: Option<String>,
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct DefaultModel {
    default: Option<String>,
}

#[derive(Deserialize)]
struct ClassificationModels {
    default: Option<String>,
    protocol: Option<ClassificationProtocol>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ClassificationProtocol {
    OpenrouterDecisions,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum FileConfiguration {
    Enabled(bool),
    Protocol(FileProtocol),
}

#[derive(Deserialize)]
enum FileProtocol {
    #[serde(rename = "openrouter")]
    OpenRouter,
}

#[derive(Deserialize)]
struct Configuration {
    name: String,
    url: String,
    #[serde(default)]
    key_env: Option<String>,
    #[serde(default)]
    models: Models,
    #[serde(default)]
    files: Option<FileConfiguration>,
    #[serde(default)]
    stores: bool,
    #[serde(default = "default_store")]
    store: bool,
    #[serde(default = "anthropic_version")]
    version: String,
}

fn default_store() -> bool {
    true
}

fn anthropic_version() -> String {
    "2023-06-01".into()
}

#[derive(Clone, Copy)]
pub(crate) enum Authorization {
    Bearer,
    Anthropic,
    Gemini,
}

pub(crate) struct Connection {
    config: Configuration,
    environment: Arc<Environment>,
    http: Arc<Factory>,
    authorization: Authorization,
    resource_scope: Arc<()>,
}

impl Connection {
    pub(crate) fn new(
        config: Value,
        environment: Arc<Environment>,
        http: Arc<Factory>,
        authorization: Authorization,
    ) -> AiResult<Self> {
        let config: Configuration = serde_json::from_value(config)
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
        if config
            .models
            .embeddings
            .as_ref()
            .and_then(|models| models.dimensions)
            == Some(0)
        {
            return Err(AiError::InvalidConfiguration(
                "providers.<name>.models.embeddings.dimensions",
            ));
        }
        let url = url::Url::parse(&config.url)
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>.url"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(AiError::InvalidConfiguration("providers.<name>.url"));
        }
        Ok(Self {
            authorization,
            resource_scope: Arc::new(()),
            config,
            environment,
            http,
        })
    }

    pub(crate) fn request(&self, timeout: Option<Duration>) -> AiResult<PendingRequest> {
        let mut request = self
            .http
            .pending_request()
            .base_url(&self.config.url)
            .without_redirecting()
            .redact_payload()
            .timeout(timeout.unwrap_or(Duration::from_secs(60)));
        if matches!(self.authorization, Authorization::Anthropic) {
            request = request.with_header(
                "anthropic-version".parse().expect("valid header name"),
                self.config
                    .version
                    .parse()
                    .map_err(|_| AiError::InvalidConfiguration("providers.<name>.version"))?,
            );
        }
        if let Some(token) = self.credential()? {
            request = match self.authorization {
                Authorization::Bearer => request.with_token(token),
                Authorization::Anthropic | Authorization::Gemini => {
                    let name = if matches!(self.authorization, Authorization::Anthropic) {
                        "x-api-key"
                    } else {
                        "x-goog-api-key"
                    };
                    let mut value: http::HeaderValue = token
                        .parse()
                        .map_err(|_| AiError::InvalidConfiguration("providers.<name>.key_env"))?;
                    value.set_sensitive(true);
                    request.with_header(name.parse().expect("valid header name"), value)
                }
            };
        }
        Ok(request)
    }

    fn credential(&self) -> AiResult<Option<&str>> {
        self.config
            .key_env
            .as_ref()
            .map(|key| {
                self.environment
                    .get_utf8(key)
                    .map_err(|_| AiError::InvalidConfiguration("providers.<name>.key_env"))?
                    .filter(|token| !token.trim().is_empty())
                    .ok_or(AiError::MissingCredential)
            })
            .transpose()
    }

    pub(crate) fn embeddings_cache_namespace(&self, protocol: &str) -> AiResult<String> {
        let credential = self
            .credential()?
            .map(|token| format!("{:x}", Sha256::digest(token)));
        let identity = serde_json::to_vec(&(self.name(), &self.config.url, protocol, credential))
            .map_err(|_| AiError::InvalidConfiguration("providers.<name>"))?;
        Ok(format!("{:x}", Sha256::digest(identity)))
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.config.url
    }

    pub(crate) fn name(&self) -> &str {
        &self.config.name
    }

    pub(crate) fn default_text_model(&self) -> AiResult<&str> {
        self.config
            .models
            .text
            .default
            .as_deref()
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn embeddings_configured(&self) -> bool {
        self.config.models.embeddings.is_some()
    }

    pub(crate) fn reranking_configured(&self) -> bool {
        self.config.models.reranking.is_some()
    }

    pub(crate) fn images_configured(&self) -> bool {
        self.config.models.images.is_some()
    }

    pub(crate) fn audio_configured(&self) -> bool {
        self.config.models.audio.is_some()
    }

    pub(crate) fn transcription_configured(&self) -> bool {
        self.config.models.transcription.is_some()
    }

    pub(crate) fn default_audio_model(&self) -> AiResult<&str> {
        self.config
            .models
            .audio
            .as_ref()
            .and_then(|model| model.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn default_transcription_model(&self) -> AiResult<&str> {
        self.config
            .models
            .transcription
            .as_ref()
            .and_then(|model| model.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn classification_configured(&self) -> bool {
        self.config.models.classification.is_some()
    }

    pub(crate) fn uses_openrouter_decisions(&self) -> bool {
        self.config
            .models
            .classification
            .as_ref()
            .and_then(|model| model.protocol.as_ref())
            == Some(&ClassificationProtocol::OpenrouterDecisions)
    }

    pub(crate) fn default_classification_model(&self) -> AiResult<&str> {
        self.config
            .models
            .classification
            .as_ref()
            .and_then(|model| model.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn default_image_model(&self) -> AiResult<&str> {
        self.config
            .models
            .images
            .as_ref()
            .and_then(|models| models.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn default_reranking_model(&self) -> AiResult<&str> {
        self.config
            .models
            .reranking
            .as_ref()
            .and_then(|models| models.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn default_embeddings_model(&self) -> AiResult<&str> {
        self.config
            .models
            .embeddings
            .as_ref()
            .and_then(|models| models.default.as_deref())
            .filter(|model| !model.trim().is_empty())
            .ok_or(AiError::MissingModel)
    }

    pub(crate) fn default_embeddings_dimensions(&self) -> Option<usize> {
        self.config
            .models
            .embeddings
            .as_ref()
            .and_then(|models| models.dimensions)
    }

    pub(crate) fn resource_scope(&self) -> &Arc<()> {
        &self.resource_scope
    }

    pub(crate) fn stores_configured(&self) -> bool {
        self.config.stores
    }

    pub(crate) fn files_configured(&self) -> bool {
        matches!(&self.config.files, Some(FileConfiguration::Enabled(true)))
    }

    pub(crate) fn uses_openrouter_files(&self) -> bool {
        matches!(
            &self.config.files,
            Some(FileConfiguration::Protocol(FileProtocol::OpenRouter))
        )
    }

    pub(crate) fn store(&self) -> bool {
        self.config.store
    }

    pub(crate) fn configured_text_models(&self) -> impl Iterator<Item = &str> {
        let configured = &self.config.models.text;
        configured
            .default
            .iter()
            .chain(&configured.ids)
            .map(String::as_str)
    }

    pub(crate) fn catalog(&self, discovered: AiResult<Vec<ListedModel>>) -> ModelCatalog {
        ModelCatalog::merge(discovered, self.configured_text_models().map(str::to_owned))
    }

    pub(crate) async fn send(
        &self,
        endpoint: TextEndpoint,
        body: &impl Serialize,
        stream: bool,
        timeout: Option<Duration>,
    ) -> AiResult<Response> {
        let mut request = self.request(timeout)?;
        if stream {
            request = request.stream();
        }
        request
            .post(endpoint.path(), body)
            .await
            .map_err(transport_error)
    }
}
