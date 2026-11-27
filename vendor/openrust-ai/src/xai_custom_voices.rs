use openrust::http::{
    HttpFuture, Method,
    client::{Part, Response},
};
use serde_json::Value;

use crate::{AiError, AiResult, connection::Connection, text::transport_error};

pub enum XaiCustomVoiceRequest<'a> {
    Create {
        metadata: &'a Value,
        audio: Box<Part>,
    },
    Get {
        id: &'a str,
    },
    Update {
        id: &'a str,
        metadata: &'a Value,
    },
    Audio {
        id: &'a str,
    },
    Delete {
        id: &'a str,
    },
}

/// 供网关复用已配置的 xAI 连接。网关负责校验调用方和资源归属。
pub trait XaiCustomVoiceGateway: Send + Sync {
    fn voice_scope(&self) -> AiResult<String>;

    fn send_voice<'a>(
        &'a self,
        operation: XaiCustomVoiceRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>>;
}

impl XaiCustomVoiceGateway for Connection {
    fn voice_scope(&self) -> AiResult<String> {
        self.request_namespace("xai-custom-voices")
    }

    fn send_voice<'a>(
        &'a self,
        operation: XaiCustomVoiceRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>> {
        Box::pin(async move {
            let request = self.request(None)?;
            let response = match operation {
                XaiCustomVoiceRequest::Create { metadata, audio } => {
                    request
                        .as_multipart()
                        .attach("file", *audio)
                        .post("custom-voices", metadata)
                        .await
                }
                XaiCustomVoiceRequest::Get { id } => request.get(&path(id)?).await,
                XaiCustomVoiceRequest::Update { id, metadata } => {
                    request.patch(&path(id)?, metadata).await
                }
                XaiCustomVoiceRequest::Audio { id } => {
                    request.stream().get(&format!("{}/audio", path(id)?)).await
                }
                XaiCustomVoiceRequest::Delete { id } => {
                    request.send(Method::DELETE, &path(id)?).await
                }
            };
            response.map_err(transport_error)
        })
    }
}

fn path(id: &str) -> AiResult<String> {
    if id.len() != 8
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        return Err(AiError::InvalidPrompt);
    }
    Ok(format!("custom-voices/{id}"))
}
