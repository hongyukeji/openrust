use std::{sync::Arc, time::Duration};

use openrust::http::HttpFuture;
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, ImageAttachment, ImagePrompt, ImageProvider, ImageResponse,
    connection::Connection,
    image::parse_base64_image_response,
    text::{transport_error, validate},
};

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
            parse_base64_image_response(&data, self.0.name(), prompt.model)
        })
    }
}

fn image_url(image: &ImageAttachment) -> Value {
    json!({"type": "image_url", "url": image.data_url()})
}
