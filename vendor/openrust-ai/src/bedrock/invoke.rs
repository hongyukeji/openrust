use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

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

impl BedrockProvider {
    pub(super) async fn invoke_model(
        &self,
        model: &str,
        timeout: Duration,
        body: Value,
    ) -> AiResult<(Value, Option<u64>)> {
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
            .map_err(converse_error)?;
        let body =
            serde_json::from_slice(output.body().as_ref()).map_err(|_| AiError::InvalidResponse)?;
        Ok((body, input_tokens.get().copied()))
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
