use std::sync::Arc;

use http::HeaderMap;
use openrust::{
    cache::{Cache, Repository, Ttl, Value},
    contracts::cache::Repository as _,
};
use sha2::{Digest, Sha256};

#[derive(Clone)]
pub(crate) struct SessionAffinity {
    repository: Arc<Repository>,
    cache_key: String,
    seconds: i64,
}

impl SessionAffinity {
    pub(crate) fn for_request(
        headers: &HeaderMap,
        key_id: &str,
        model: &str,
        store: Option<&str>,
        seconds: i64,
    ) -> Result<Option<Self>, ()> {
        let mut values = headers.get_all("x-session-id").iter();
        let Some(session) = values.next() else {
            return Ok(None);
        };
        let id = session.to_str().map_err(|_| ())?;
        if values.next().is_some()
            || id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            return Err(());
        }
        let digest = key(&[key_id, model, id]);
        let Ok(repository) = Cache::store(store) else {
            return Ok(None);
        };
        Ok(Some(Self {
            repository,
            cache_key: format!("ai-gateway:session:{digest}"),
            seconds,
        }))
    }

    pub(crate) async fn preferred(&self) -> Option<String> {
        self.repository
            .get(&self.cache_key)
            .await
            .ok()
            .flatten()
            .as_ref()
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    pub(crate) async fn remember(self, target: String) {
        // 黏性依赖缓存，但缓存失效时仍可按正常路由顺序处理请求。
        let _ = self
            .repository
            .put(
                &self.cache_key,
                &target.as_str().into(),
                Some(Ttl::Seconds(self.seconds)),
            )
            .await;
    }
}

pub(crate) fn key(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}
