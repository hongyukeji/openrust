use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use aws_sdk_bedrockruntime::operation::RequestId;
use aws_smithy_runtime_api::{
    box_error::BoxError,
    client::{
        interceptors::{Intercept, context::BeforeDeserializationInterceptorContextRef},
        runtime_components::RuntimeComponents,
    },
};
use serde_json::Value;

use super::{BedrockProvider, Blob, TimeoutConfig, converse_error};
use crate::{AiError, AiResult};

pub(super) struct InvokeOutput {
    pub body: Value,
    pub input_tokens: Option<u64>,
    pub request_id: Option<String>,
}

pub(super) struct InvokeFailure {
    pub error: AiError,
    pub request_id: Option<String>,
}

impl From<AiError> for InvokeFailure {
    fn from(error: AiError) -> Self {
        Self {
            error,
            request_id: None,
        }
    }
}

impl BedrockProvider {
    pub(super) async fn invoke_model(
        &self,
        model: &str,
        timeout: Duration,
        body: Value,
    ) -> AiResult<(Value, Option<u64>)> {
        let output = self
            .invoke_with_evidence(model, timeout, body)
            .await
            .map_err(|failure| failure.error)?;
        Ok((output.body, output.input_tokens))
    }

    pub(super) async fn invoke_with_evidence(
        &self,
        model: &str,
        timeout: Duration,
        body: Value,
    ) -> Result<InvokeOutput, InvokeFailure> {
        let input_tokens = Arc::new(OnceLock::new());
        let timeout = TimeoutConfig::builder().operation_timeout(timeout).build();
        let output = self
            .client()
            .await?
            .invoke_model()
            .model_id(model)
            .content_type("application/json")
            .accept("application/json")
            .body(Blob::new(
                serde_json::to_vec(&body).map_err(|_| AiError::InvalidPrompt)?,
            ))
            .customize()
            .config_override(aws_sdk_bedrockruntime::config::Builder::new().timeout_config(timeout))
            .interceptor(InputTokenHeader(input_tokens.clone()))
            .send()
            .await
            .map_err(|error| {
                let request_id = error
                    .as_service_error()
                    .and_then(RequestId::request_id)
                    .map(str::to_owned);
                InvokeFailure {
                    error: converse_error(error),
                    request_id,
                }
            })?;
        let request_id = output.request_id().map(str::to_owned);
        let body = serde_json::from_slice(output.body().as_ref()).map_err(|_| InvokeFailure {
            error: AiError::InvalidResponse,
            request_id: request_id.clone(),
        })?;
        Ok(InvokeOutput {
            body,
            input_tokens: input_tokens.get().copied(),
            request_id,
        })
    }
}

#[derive(Debug)]
struct InputTokenHeader(Arc<OnceLock<u64>>);

impl Intercept for InputTokenHeader {
    fn name(&self) -> &'static str {
        "BedrockInputTokenHeader"
    }

    fn read_before_deserialization(
        &self,
        context: &BeforeDeserializationInterceptorContextRef<'_>,
        _: &RuntimeComponents,
        _: &mut aws_sdk_bedrockruntime::config::ConfigBag,
    ) -> Result<(), BoxError> {
        if let Some(tokens) = context
            .response()
            .headers()
            .get("x-amzn-bedrock-input-token-count")
            .and_then(|value| value.parse().ok())
        {
            let _ = self.0.set(tokens);
        }
        Ok(())
    }
}
