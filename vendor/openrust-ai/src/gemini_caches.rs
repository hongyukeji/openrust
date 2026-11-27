use std::sync::Arc;

use openrust::http::{HttpFuture, client::Response};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Ai, AiError, AiResult, TextProvider};

/// Gemini Developer API 的原生上下文缓存操作。
pub enum GeminiCacheRequest<'a> {
    Create {
        model: &'a str,
        content: &'a Value,
    },
    Get {
        name: &'a str,
    },
    List {
        limit: Option<u16>,
        cursor: Option<&'a str>,
    },
    Update {
        name: &'a str,
        expiration: GeminiCacheExpiration<'a>,
    },
    Delete {
        name: &'a str,
    },
}

#[derive(Clone, Copy)]
pub enum GeminiCacheExpiration<'a> {
    Ttl(&'a str),
    ExpireTime(&'a str),
}

impl<'a> GeminiCacheExpiration<'a> {
    pub(crate) fn field(self) -> (&'static str, &'a str) {
        match self {
            Self::Ttl(value) => ("ttl", value),
            Self::ExpireTime(value) => ("expireTime", value),
        }
    }
}

pub trait GeminiCacheGateway: Send + Sync {
    /// 上游端点和凭证变化时更换资源范围。
    fn cache_scope(&self) -> AiResult<String>;

    fn send_cache<'a>(
        &'a self,
        request: GeminiCacheRequest<'a>,
    ) -> HttpFuture<'a, AiResult<Response>>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCachedContent {
    pub name: String,
    pub model: String,
    pub display_name: Option<String>,
    pub create_time: Option<String>,
    pub update_time: Option<String>,
    pub expire_time: String,
    pub usage_metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiCachedContentPage {
    #[serde(default)]
    pub cached_contents: Vec<GeminiCachedContent>,
    pub next_page_token: Option<String>,
}

/// 使用具名 Gemini 连接创建和管理显式上下文缓存。
pub struct GeminiCaches {
    provider: Arc<dyn TextProvider>,
}

impl GeminiCaches {
    pub fn for_connection(name: &str) -> AiResult<Self> {
        let provider = Ai::text_provider(Some(name))?;
        if provider.gemini_cache_gateway().is_none() {
            return Err(AiError::EndpointNotSupported);
        }
        Ok(Self { provider })
    }

    pub async fn create(&self, model: &str, content: &Value) -> AiResult<GeminiCachedContent> {
        self.resource(GeminiCacheRequest::Create { model, content })
            .await
    }

    pub async fn get(&self, name: &str) -> AiResult<GeminiCachedContent> {
        self.named_resource(name, GeminiCacheRequest::Get { name })
            .await
    }

    pub async fn list(
        &self,
        limit: Option<u16>,
        cursor: Option<&str>,
    ) -> AiResult<GeminiCachedContentPage> {
        let page: GeminiCachedContentPage = self
            .send(GeminiCacheRequest::List { limit, cursor })
            .await?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
        for resource in &page.cached_contents {
            validate_resource(resource)?;
        }
        if page
            .next_page_token
            .as_deref()
            .is_some_and(|token| !crate::gemini_batches::valid_cursor(token))
        {
            return Err(AiError::InvalidResponse);
        }
        Ok(page)
    }

    pub async fn update(
        &self,
        name: &str,
        expiration: GeminiCacheExpiration<'_>,
    ) -> AiResult<GeminiCachedContent> {
        self.named_resource(name, GeminiCacheRequest::Update { name, expiration })
            .await
    }

    pub async fn delete(&self, name: &str) -> AiResult<()> {
        self.send(GeminiCacheRequest::Delete { name }).await?;
        Ok(())
    }

    async fn named_resource(
        &self,
        expected: &str,
        request: GeminiCacheRequest<'_>,
    ) -> AiResult<GeminiCachedContent> {
        let resource = self.resource(request).await?;
        if resource.name != expected {
            return Err(AiError::InvalidResponse);
        }
        Ok(resource)
    }

    async fn resource(&self, request: GeminiCacheRequest<'_>) -> AiResult<GeminiCachedContent> {
        let resource = self
            .send(request)
            .await?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
        validate_resource(&resource)?;
        Ok(resource)
    }

    async fn send(&self, request: GeminiCacheRequest<'_>) -> AiResult<Response> {
        let response = self
            .provider
            .gemini_cache_gateway()
            .ok_or(AiError::EndpointNotSupported)?
            .send_cache(request)
            .await?;
        crate::text::validate(Ok(response))
    }
}

fn validate_resource(resource: &GeminiCachedContent) -> AiResult<()> {
    if !valid_name(&resource.name)
        || !resource.model.starts_with("models/")
        || crate::gemini::validated_model_id(&resource.model).is_err()
        || resource.expire_time.is_empty()
    {
        return Err(AiError::InvalidResponse);
    }
    Ok(())
}

pub(crate) fn valid_name(value: &str) -> bool {
    value.strip_prefix("cachedContents/").is_some_and(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    })
}
