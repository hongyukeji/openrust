use std::sync::Arc;

use openrust::http::{
    HttpFuture,
    client::{Response, ResponseStream},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Ai, AiError, AiResult, FileInfo, FileUpload, TextProvider};

/// Gemini Developer API 的原生异步批任务操作。
pub enum GeminiBatchRequest<'a> {
    Generate {
        model: &'a str,
        batch: &'a Value,
    },
    Embed {
        model: &'a str,
        batch: &'a Value,
    },
    Get {
        name: &'a str,
    },
    List {
        limit: Option<u16>,
        cursor: Option<&'a str>,
    },
    Cancel {
        name: &'a str,
    },
    Delete {
        name: &'a str,
    },
    Download {
        file: &'a str,
    },
}

pub trait GeminiBatchGateway: Send + Sync {
    /// 用于持久化上游任务归属。更换端点、认证信息或连接头会得到不同的范围。
    fn batch_scope(&self) -> AiResult<String>;

    /// 将批任务输入上传到同一个上游连接。
    fn upload_batch_input(&self, _file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn send_batch<'a>(
        &'a self,
        request: GeminiBatchRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>>;
}

/// 使用同一具名 Gemini 连接创建和管理批任务。
pub struct GeminiBatches {
    provider: Arc<dyn TextProvider>,
}

impl GeminiBatches {
    pub fn for_connection(name: &str) -> AiResult<Self> {
        let provider = Ai::text_provider(Some(name))?;
        provider
            .gemini_batch_gateway()
            .ok_or(AiError::EndpointNotSupported)?;
        Ok(Self { provider })
    }

    pub async fn generate(&self, model: &str, batch: &Value) -> AiResult<GeminiBatch> {
        decode(
            self.send(GeminiBatchRequest::Generate { model, batch })
                .await?,
        )
    }

    pub async fn embed(&self, model: &str, batch: &Value) -> AiResult<GeminiBatch> {
        decode(
            self.send(GeminiBatchRequest::Embed { model, batch })
                .await?,
        )
    }

    pub async fn get(&self, name: &str) -> AiResult<GeminiBatch> {
        let name = batch_name(name)?;
        let job = decode(self.send(GeminiBatchRequest::Get { name }).await?)?;
        if job.name != name {
            return Err(AiError::InvalidResponse);
        }
        Ok(job)
    }

    pub async fn list(
        &self,
        limit: Option<u16>,
        cursor: Option<&str>,
    ) -> AiResult<GeminiBatchPage> {
        let page: GeminiBatchPage = self
            .send(GeminiBatchRequest::List { limit, cursor })
            .await?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
        for job in &page.operations {
            batch_name(&job.name).map_err(|_| AiError::InvalidResponse)?;
        }
        if page
            .next_page_token
            .as_deref()
            .is_some_and(|token| !valid_cursor(token))
        {
            return Err(AiError::InvalidResponse);
        }
        Ok(page)
    }

    pub async fn cancel(&self, name: &str) -> AiResult<()> {
        self.send(GeminiBatchRequest::Cancel { name }).await?;
        Ok(())
    }

    pub async fn delete(&self, name: &str) -> AiResult<()> {
        self.send(GeminiBatchRequest::Delete { name }).await?;
        Ok(())
    }

    /// 重新读取任务并流式下载其 JSONL 结果文件。
    pub async fn download(&self, name: &str) -> AiResult<ResponseStream> {
        let job = self.get(name).await?;
        let file = job.output_file().ok_or(AiError::InvalidFile)?;
        self.send(GeminiBatchRequest::Download { file })
            .await?
            .into_stream()
            .map_err(crate::text::transport_error)
    }

    async fn send(&self, request: GeminiBatchRequest<'_>) -> AiResult<Response> {
        let response = self
            .provider
            .gemini_batch_gateway()
            .ok_or(AiError::EndpointNotSupported)?
            .send_batch(request)
            .await?;
        crate::text::validate(Ok(response))
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct GeminiBatch {
    pub name: String,
    #[serde(default)]
    pub done: bool,
    pub metadata: Option<Value>,
    pub response: Option<Value>,
    pub error: Option<Value>,
}

impl GeminiBatch {
    pub fn state(&self) -> Option<&str> {
        self.metadata.as_ref()?.get("state")?.as_str()
    }

    pub fn output_file(&self) -> Option<&str> {
        if self.error.is_some() || !(self.done || self.state() == Some("JOB_STATE_SUCCEEDED")) {
            return None;
        }
        let response = self
            .response
            .as_ref()
            .and_then(|output| output["responsesFile"].as_str());
        let metadata = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata["output"]["responsesFile"].as_str());
        match (response, metadata) {
            (Some(first), Some(second)) if first != second => None,
            _ => response.or(metadata),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiBatchPage {
    #[serde(default)]
    pub operations: Vec<GeminiBatch>,
    pub next_page_token: Option<String>,
}

fn decode(response: Response) -> AiResult<GeminiBatch> {
    let job: GeminiBatch = response.json().map_err(|_| AiError::InvalidResponse)?;
    batch_name(&job.name).map_err(|_| AiError::InvalidResponse)?;
    Ok(job)
}

pub(crate) fn batch_name(value: &str) -> AiResult<&str> {
    let id = value
        .strip_prefix("batches/")
        .ok_or(AiError::InvalidPrompt)?;
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AiError::InvalidPrompt);
    }
    Ok(value)
}

pub(crate) fn valid_cursor(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && value.bytes().all(|byte| byte.is_ascii_graphic())
}
