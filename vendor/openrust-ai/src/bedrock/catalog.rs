use std::{collections::HashSet, time::Duration};

use aws_sdk_bedrock::{
    Client,
    types::{InferenceProfileStatus, ModelModality},
};

use crate::{AiError, AiResult, ModelCatalog, models::ListedModel};

use super::{
    BedrockProvider, BehaviorVersion, HTTP_BEARER_AUTH_SCHEME_ID, RetryConfig, Token,
    converse_error,
};

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);

impl BedrockProvider {
    async fn catalog_client(&self) -> AiResult<&Client> {
        self.catalog_client
            .get_or_try_init(|| async {
                let mut config = if let Some(key) = self.bearer_key()? {
                    aws_sdk_bedrock::config::Builder::new()
                        .behavior_version(BehaviorVersion::latest())
                        .region(self.region())
                        .bearer_token(Token::new(key, None))
                        .auth_scheme_preference([HTTP_BEARER_AUTH_SCHEME_ID])
                } else {
                    aws_sdk_bedrock::config::Builder::from(self.iam_config().await?)
                };
                if let Some(endpoint) = &self.settings.catalog_url {
                    config = config.endpoint_url(endpoint);
                }
                let retry = RetryConfig::standard().with_max_attempts(1);
                Ok(Client::from_conf(config.retry_config(retry).build()))
            })
            .await
    }

    pub(super) async fn discover_models(&self) -> ModelCatalog {
        let client = match tokio::time::timeout(DISCOVERY_TIMEOUT, self.catalog_client()).await {
            Ok(Ok(client)) => client,
            Ok(Err(error)) => return self.connection.catalog(Err(error)),
            Err(_) => return self.connection.catalog(Err(AiError::Timeout)),
        };
        let (models, profiles) = futures_util::future::join(
            tokio::time::timeout(DISCOVERY_TIMEOUT, foundation_models(client)),
            tokio::time::timeout(DISCOVERY_TIMEOUT, inference_profiles(client)),
        )
        .await;
        let models = models.unwrap_or(Err(AiError::Timeout));
        let profiles = profiles.unwrap_or(Err(AiError::Timeout));
        match (models, profiles) {
            (Ok(mut models), Ok(profiles)) => {
                models.extend(profiles);
                self.connection.catalog(Ok(models))
            }
            (Ok(models), Err(error)) | (Err(error), Ok(models)) => {
                let mut catalog = self.connection.catalog(Ok(models));
                catalog.discovery_error = Some(error);
                catalog
            }
            (Err(error), Err(_)) => self.connection.catalog(Err(error)),
        }
    }
}

async fn foundation_models(client: &Client) -> AiResult<Vec<ListedModel>> {
    let output = client
        .list_foundation_models()
        .send()
        .await
        .map_err(converse_error)?;
    Ok(output
        .model_summaries()
        .iter()
        .map(|model| ListedModel {
            id: model.model_id().into(),
            name: model.model_name().map(str::to_owned),
            input_modalities: modalities(model.input_modalities()),
            output_modalities: modalities(model.output_modalities()),
        })
        .collect())
}

async fn inference_profiles(client: &Client) -> AiResult<Vec<ListedModel>> {
    let mut models = Vec::new();
    let mut token = None;
    let mut seen_tokens = HashSet::new();
    loop {
        let output = client
            .list_inference_profiles()
            .set_next_token(token)
            .send()
            .await
            .map_err(converse_error)?;
        models.extend(
            output
                .inference_profile_summaries()
                .iter()
                .filter(|profile| *profile.status() == InferenceProfileStatus::Active)
                .map(|profile| ListedModel {
                    id: profile.inference_profile_id().into(),
                    name: Some(profile.inference_profile_name().into()),
                    input_modalities: None,
                    output_modalities: None,
                }),
        );
        let Some(next) = output.next_token().filter(|token| !token.is_empty()) else {
            return Ok(models);
        };
        if !seen_tokens.insert(next.to_owned()) {
            return Err(AiError::InvalidResponse);
        }
        token = Some(next.to_owned());
    }
}

fn modalities(values: &[ModelModality]) -> Option<Vec<String>> {
    (!values.is_empty()).then(|| {
        values
            .iter()
            .map(|value| value.as_str().to_ascii_lowercase())
            .collect()
    })
}
