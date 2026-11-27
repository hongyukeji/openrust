use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openrust::{
    http::{HttpFuture, StatusCode, client::Response},
    support::Str,
};
use serde_json::{Value, json};

use crate::{
    AiError, AiResult, GeneratedImage, ImageFormat, ImagePrompt, ImageProvider, ImageResponse,
    ImageUsage,
};

use super::{
    BedrockProvider,
    relay::{error_response, relay_response},
};

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
            let body = request_body(&prompt, 1)?;
            let timeout = prompt.timeout.unwrap_or(Duration::from_secs(120));
            let (payload, _) = self.invoke_model(prompt.model, timeout, body).await?;
            let images = decode_images(&payload)?;
            Ok(ImageResponse {
                images,
                usage: ImageUsage::default(),
                provider: self.connection.name().into(),
                model: prompt.model.into(),
            })
        })
    }
}

fn decode_images(payload: &Value) -> AiResult<Vec<GeneratedImage>> {
    payload["images"]
        .as_array()
        .filter(|images| !images.is_empty())
        .ok_or(AiError::InvalidResponse)?
        .iter()
        .map(|image| {
            let encoded = image.as_str().ok_or(AiError::InvalidResponse)?;
            let bytes = Str::from_base64(encoded).map_err(|_| AiError::InvalidResponse)?;
            GeneratedImage::from_bytes(bytes).map_err(|_| AiError::InvalidResponse)
        })
        .collect()
}

fn is_stability(model: &str) -> bool {
    model
        .rsplit('/')
        .next()
        .is_some_and(|id| id.starts_with("stability."))
}

fn request_body(prompt: &ImagePrompt<'_>, count: u64) -> AiResult<Value> {
    let model = prompt.model.rsplit('/').next().unwrap_or(prompt.model);
    if is_stability(prompt.model) {
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
        "numberOfImages": count,
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

pub(super) fn relay_image_request(body: &Value) -> Result<(&str, u64, Value), &'static str> {
    let object = body.as_object().ok_or("Bedrock 图片请求需要 JSON 对象")?;
    if object.iter().any(|(field, value)| {
        !value.is_null()
            && !matches!(
                field.as_str(),
                "model"
                    | "prompt"
                    | "size"
                    | "quality"
                    | "n"
                    | "response_format"
                    | "output_format"
                    | "user"
                    | "stream"
            )
    }) || !matches!(body["stream"], Value::Null | Value::Bool(false))
    {
        return Err("Bedrock 图片入口只支持同步文生图及其生成参数");
    }
    if !body["response_format"].is_null() && body["response_format"] != "b64_json"
        || !body["output_format"].is_null() && body["output_format"] != "png"
    {
        return Err("Bedrock 图片入口返回 Base64 编码的 PNG");
    }
    let model = body["model"].as_str().ok_or("必须指定 Bedrock 图片模型")?;
    let prompt = body["prompt"]
        .as_str()
        .filter(|text| !text.trim().is_empty())
        .ok_or("Bedrock 图片提示词不能为空")?;
    let size = match body["size"].as_str() {
        Some("auto") => None,
        Some("1024x1024" | "1:1") => Some("1:1"),
        Some("1024x1536" | "768x1152" | "2:3") => Some("2:3"),
        Some("1536x1024" | "1152x768" | "3:2") => Some("3:2"),
        None if body["size"].is_null() => None,
        _ => return Err("Bedrock 图片入口支持正方形、2:3 竖图或 3:2 横图"),
    };
    let quality = match body["quality"].as_str() {
        Some("auto") => None,
        Some("low" | "medium" | "standard") => Some("standard"),
        Some("high" | "hd" | "premium") => Some("premium"),
        None if body["quality"].is_null() => None,
        _ => return Err("Bedrock 图片质量必须可映射为标准或高级"),
    };
    let count = if body["n"].is_null() {
        1
    } else {
        body["n"]
            .as_u64()
            .filter(|count| (1..=5).contains(count))
            .ok_or("Bedrock 图片数量须为 1 至 5")?
    };
    if is_stability(model) && (count != 1 || quality.is_some()) {
        return Err("此 Stability 图片入口使用模型默认质量且每次返回一张图");
    }
    let native = request_body(
        &ImagePrompt {
            prompt,
            attachments: &[],
            model,
            size,
            quality,
            timeout: None,
        },
        count,
    )
    .map_err(|_| "此 Bedrock 模型没有可用的文生图映射")?;
    Ok((model, count, native))
}

impl BedrockProvider {
    pub(super) async fn relay_image(&self, body: Value) -> AiResult<Response> {
        let (model, count, native) =
            relay_image_request(&body).map_err(|_| AiError::InvalidPrompt)?;
        let output = match self
            .invoke_with_evidence(model, Duration::from_secs(120), native)
            .await
        {
            Ok(output) => output,
            Err(failure) => return error_response(failure),
        };
        let images = decode_images(&output.body).ok().filter(|images| {
            images.len() as u64 <= count
                && images
                    .iter()
                    .all(|image| image.format() == ImageFormat::Png)
        });
        let Some(images) = images else {
            return relay_response(
                StatusCode::BAD_GATEWAY,
                &json!({}),
                output.request_id.as_deref(),
            );
        };
        let data: Vec<_> = images
            .into_iter()
            .map(|image| {
                json!({
                    "b64_json": Str::to_base64(image.content()),
                })
            })
            .collect();
        let created = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        relay_response(
            StatusCode::OK,
            &json!({
                "created":created,"data":data,"output_format":"png",
            }),
            output.request_id.as_deref(),
        )
    }
}
