use std::{sync::Arc, time::Duration};

use openrust::http::HttpFuture;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, AudioFormat, AudioPrompt, AudioProvider, AudioResponse, AudioVoice,
    ImageAttachment, ImagePrompt, ImageProvider, ImageResponse, ProviderOptions, ProviderTool,
    connection::Connection,
    image::parse_base64_image_response,
    provider_options::apply_provider_options,
    text::{transport_error, validate},
};

pub(crate) struct XaiAudioProvider(pub(crate) Arc<Connection>);

impl AudioProvider for XaiAudioProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_audio_model(&self) -> AiResult<&str> {
        // 原生合成端点没有模型字段，SDK 使用操作名标识该能力。
        Ok("tts")
    }

    fn generate_audio<'a>(
        &'a self,
        prompt: AudioPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AudioResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            if prompt.model != self.default_audio_model()? {
                return Err(AiError::InvalidConfiguration("audio.xai.model"));
            }
            if !matches!(
                prompt.format,
                AudioFormat::Mp3 | AudioFormat::Wav | AudioFormat::Pcm
            ) {
                return Err(AiError::InvalidConfiguration("audio.xai.format"));
            }
            if prompt
                .instructions
                .is_some_and(|text| !text.trim().is_empty())
            {
                return Err(AiError::InvalidConfiguration("audio.xai.instructions"));
            }
            if prompt.text.chars().count() > 60_000 {
                return Err(AiError::InvalidPrompt);
            }
            let voice = match prompt.voice {
                AudioVoice::Default => "eve",
                AudioVoice::Male => "rex",
                AudioVoice::Female => "ara",
                AudioVoice::Named(name) => name,
            };
            let body = json!({
                "text": prompt.text,
                "voice_id": voice,
                "language": "auto",
                "output_format": {"codec": prompt.format.extension()},
            });
            let response = validate(
                self.0
                    .request(Some(prompt.timeout))?
                    .post("tts", &body)
                    .await,
            )?;
            if !response
                .header("content-type")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.split(';').next())
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(prompt.format.mime_type()))
            {
                return Err(AiError::InvalidResponse);
            }
            let content = response.body().map_err(|_| AiError::InvalidResponse)?;
            AudioResponse::from_bytes(content, prompt.format, self.name(), prompt.model)
                .map_err(|_| AiError::InvalidResponse)
        })
    }
}

pub(crate) struct XaiImageProvider(pub(crate) Arc<Connection>);

impl ImageProvider for XaiImageProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_image_model(&self) -> AiResult<&str> {
        self.0.default_image_model()
    }

    fn generate_image<'a>(
        &'a self,
        prompt: ImagePrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ImageResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            if prompt.attachments.len() > 5 {
                return Err(AiError::InvalidImage);
            }

            let mut body = json!({
                "model": prompt.model,
                "prompt": prompt.prompt,
                "response_format": "b64_json",
            });
            if let Some(size) = prompt.size {
                body["aspect_ratio"] = size.into();
            }
            let resolution = match prompt.quality {
                Some("low" | "1K") => Some("1k"),
                Some("medium" | "2K" | "high" | "4K") => Some("2k"),
                _ => None,
            };
            if let Some(resolution) = resolution {
                body["resolution"] = resolution.into();
            }

            let endpoint = match prompt.attachments {
                [] => "images/generations",
                [image] => {
                    body["image"] = image_url(image);
                    "images/edits"
                }
                images => {
                    body["images"] = Value::Array(images.iter().map(image_url).collect());
                    "images/edits"
                }
            };
            let timeout = prompt.timeout.unwrap_or(Duration::from_secs(120));
            let response = self
                .0
                .request(Some(timeout))?
                .post(endpoint, &body)
                .await
                .map_err(transport_error)?;
            let data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let mut response = parse_base64_image_response(&data, self.0.name(), prompt.model)?;
            response.usage.tokens.cost_in_usd_ticks = data["usage"]["cost_in_usd_ticks"].as_u64();
            Ok(response)
        })
    }
}

fn image_url(image: &ImageAttachment) -> Value {
    json!({"type": "image_url", "url": image.data_url()})
}

pub(crate) fn apply_options(body: &mut Value, options: Option<&ProviderOptions>) -> AiResult<()> {
    let Some(options) = options.filter(|options| options.contains_key("tools")) else {
        return apply_provider_options(body, options, false);
    };
    let mut options = options.clone();
    let definitions = options
        .remove("tools")
        .ok_or(AiError::InvalidProviderOptions)?;
    let tools: Vec<ProviderTool> =
        serde_json::from_value(definitions).map_err(|_| AiError::InvalidProviderOptions)?;
    if tools.iter().any(|tool| !tool.is_valid()) {
        return Err(AiError::InvalidProviderOptions);
    }
    apply_provider_options(body, Some(&options), false)?;
    if !tools.is_empty() {
        body["tools"] = serde_json::to_value(tools).map_err(|_| AiError::InvalidProviderOptions)?;
    }
    Ok(())
}

pub(crate) fn is_tool_result(item: &Value) -> bool {
    matches!(
        item["type"].as_str(),
        Some("web_search_call" | "code_interpreter_call" | "file_search_call")
    )
}
