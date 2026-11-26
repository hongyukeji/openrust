use std::time::Duration;

use openrust::{http::HttpFuture, support::Str};
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, GeneratedImage, ImagePrompt, ImageProvider, ImageResponse, ImageUsage,
};

use super::BedrockProvider;

impl ImageProvider for BedrockProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_image_model(&self) -> AiResult<&str> {
        self.connection.default_image_model()
    }

    fn generate_image<'a>(
        &'a self,
        prompt: ImagePrompt<'a>,
    ) -> HttpFuture<'a, AiResult<ImageResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            if !prompt.attachments.is_empty() {
                return Err(AiError::InvalidImage);
            }
            let body = request_body(&prompt)?;
            let timeout = prompt.timeout.unwrap_or(Duration::from_secs(120));
            let (payload, _) = self.invoke_model(prompt.model, timeout, body).await?;
            let images = payload["images"]
                .as_array()
                .filter(|images| !images.is_empty())
                .ok_or(AiError::InvalidResponse)?
                .iter()
                .map(|image| {
                    let encoded = image.as_str().ok_or(AiError::InvalidResponse)?;
                    let bytes = Str::from_base64(encoded).map_err(|_| AiError::InvalidResponse)?;
                    GeneratedImage::from_bytes(bytes).map_err(|_| AiError::InvalidResponse)
                })
                .collect::<AiResult<Vec<_>>>()?;
            Ok(ImageResponse {
                images,
                usage: ImageUsage::default(),
                provider: self.connection.name().into(),
                model: prompt.model.into(),
            })
        })
    }
}

fn request_body(prompt: &ImagePrompt<'_>) -> AiResult<Value> {
    let model = prompt.model.rsplit('/').next().unwrap_or(prompt.model);
    if model.starts_with("stability.") {
        let mut body = json!({"prompt": prompt.prompt, "output_format": "png"});
        if let Some(size @ ("1:1" | "2:3" | "3:2")) = prompt.size {
            body["aspect_ratio"] = size.into();
        }
        return Ok(body);
    }
    let titan = model.starts_with("amazon.titan-image");
    if !titan && !model.starts_with("amazon.nova-canvas") {
        return Err(AiError::EndpointNotSupported);
    }
    let (width, height) = match prompt.size {
        Some("2:3") => (768, 1152),
        Some("3:2") => (1152, 768),
        _ => (1024, 1024),
    };
    let quality = match prompt.quality {
        Some("high" | "premium") => "premium",
        None | Some("low" | "medium" | "standard") => "standard",
        Some(quality) => quality,
    };
    let mut config = json!({
        "numberOfImages": 1,
        "quality": quality,
        "width": width,
        "height": height,
    });
    if titan {
        config["cfgScale"] = 7.0.into();
    }
    Ok(json!({
        "taskType": "TEXT_IMAGE",
        "textToImageParams": {"text": prompt.prompt},
        "imageGenerationConfig": config,
    }))
}
