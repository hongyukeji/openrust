use std::{sync::Arc, time::Duration};

use openrust::{http::HttpFuture, support::Str};
use serde_json::{Value, json};

use super::interactions::{interact, output_blocks, reported_model, usage};

use crate::{
    AiError, AiResult, GeneratedImage, ImagePrompt, ImageProvider, ImageResponse, ImageUsage,
    connection::Connection,
};

pub(super) struct GeminiImageProvider(pub(super) Arc<Connection>);

impl ImageProvider for GeminiImageProvider {
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
            let model = super::validated_model_id(prompt.model)?;
            let quality = match prompt.quality {
                Some("medium" | "2K") => "2K",
                Some("high" | "4K") => "4K",
                _ => "1K",
            };
            let mut format = json!({"type":"image","image_size":quality});
            if let Some(size) = prompt.size {
                format["aspect_ratio"] = size.into();
            }
            let mut input = vec![json!({"type":"text","text":prompt.prompt})];
            input.extend(prompt.attachments.iter().map(|image| {
                json!({"type":"image","mime_type":image.mime(),"data":Str::to_base64(image.content())})
            }));
            let body = json!({"model":model,"store":false,"input":input,"response_format":format});
            let timeout = prompt.timeout.unwrap_or(Duration::from_secs(120));
            let data = interact(&self.0, timeout, body).await?;
            parse_response(&data, self.name(), model)
        })
    }
}

fn parse_response(data: &Value, provider: &str, model: &str) -> AiResult<ImageResponse> {
    let images = output_blocks(data)
        .filter(|block| block["type"] == "image")
        .map(parse_image)
        .collect::<AiResult<Vec<_>>>()?;
    if images.is_empty() {
        return Err(AiError::InvalidResponse);
    }
    let details = &data["usage"];
    let tokens = usage(data);
    Ok(ImageResponse {
        images,
        usage: ImageUsage {
            tokens,
            image_input_tokens: image_tokens(&details["input_tokens_by_modality"]),
            image_output_tokens: image_tokens(&details["output_tokens_by_modality"]),
        },
        provider: provider.into(),
        model: reported_model(data, model).into(),
    })
}

fn parse_image(block: &Value) -> AiResult<GeneratedImage> {
    let encoded = block["data"].as_str().ok_or(AiError::InvalidResponse)?;
    let bytes = Str::from_base64(encoded).map_err(|_| AiError::InvalidResponse)?;
    let image = GeneratedImage::from_bytes(bytes).map_err(|_| AiError::InvalidResponse)?;
    if block["mime_type"]
        .as_str()
        .is_some_and(|mime| !mime.eq_ignore_ascii_case(image.mime()))
    {
        return Err(AiError::InvalidResponse);
    }
    Ok(image)
}

fn image_tokens(details: &Value) -> Option<u64> {
    details.as_array()?.iter().find(|detail| {
        detail["modality"]
            .as_str()
            .is_some_and(|modality| modality.eq_ignore_ascii_case("image"))
    })?["tokens"]
        .as_u64()
}
