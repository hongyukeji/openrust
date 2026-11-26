use std::{collections::BTreeMap, sync::Arc};

use openrust::http::{HttpFuture, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    Ai, AiError, AiResult, FileProvider, FileResponse,
    files::{Deleted, OpenAiFileProvider, OpenAiPage, same_resource_scope, validate_id},
    text::validate,
};

pub struct Stores;

impl Stores {
    pub async fn create(name: &str, provider: Option<&str>) -> AiResult<Store> {
        Self::create_with(name, StoreOptions::default(), provider).await
    }

    pub async fn create_with(
        name: &str,
        options: StoreOptions<'_>,
        provider: Option<&str>,
    ) -> AiResult<Store> {
        if name.trim().is_empty() || options.expires_after_days.is_some_and(|days| days == 0) {
            return Err(AiError::InvalidStore);
        }
        for id in options.file_ids {
            validate_id(id)?;
        }
        let provider = Ai::store_provider(provider)?;
        let result = provider.create_store(name, options).await?;
        Store::new(result, provider)
    }

    pub async fn get(id: &str, provider: Option<&str>) -> AiResult<Store> {
        validate_id(id)?;
        let provider = Ai::store_provider(provider)?;
        let result = provider.get_store(id).await?;
        verify_resource(id, &result.id)?;
        Store::new(result, provider)
    }

    pub async fn delete(id: &str, provider: Option<&str>) -> AiResult<bool> {
        validate_id(id)?;
        Ai::store_provider(provider)?.delete_store(id).await
    }

    pub async fn list(
        after: Option<&str>,
        limit: u16,
        provider: Option<&str>,
    ) -> AiResult<StorePage> {
        validate_page(after, limit)?;
        let provider = Ai::store_provider(provider)?;
        let result = provider.list_stores(after, limit).await?;
        let stores = result
            .data
            .into_iter()
            .map(|info| Store::new(info, provider.clone()))
            .collect::<AiResult<_>>()?;
        Ok(StorePage {
            stores,
            next_cursor: result.next_cursor,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct StoreOptions<'a> {
    pub description: Option<&'a str>,
    pub file_ids: &'a [&'a str],
    pub expires_after_days: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoreInfo {
    pub id: String,
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub file_counts: StoreFileCounts,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub usage_bytes: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct StoreFileCounts {
    pub completed: Option<u64>,
    #[serde(alias = "in_progress")]
    pub pending: Option<u64>,
    pub failed: Option<u64>,
    pub cancelled: Option<u64>,
    pub total: Option<u64>,
}

pub struct Store {
    info: StoreInfo,
    provider: Arc<dyn StoreProvider>,
}

impl Store {
    fn new(info: StoreInfo, provider: Arc<dyn StoreProvider>) -> AiResult<Self> {
        validate_id(&info.id).map_err(|_| AiError::InvalidResponse)?;
        Ok(Self { info, provider })
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn info(&self) -> &StoreInfo {
        &self.info
    }

    pub fn provider(&self) -> &str {
        self.provider.name()
    }

    pub fn is_ready(&self) -> bool {
        self.info.status.as_deref() == Some("completed")
    }

    pub async fn refresh(&self) -> AiResult<Self> {
        let info = self.provider.get_store(self.id()).await?;
        verify_resource(self.id(), &info.id)?;
        Self::new(info, self.provider.clone())
    }

    pub async fn add(
        &self,
        file: &FileResponse,
        attributes: &Map<String, Value>,
    ) -> AiResult<StoreFile> {
        if !file.uses_provider(self.provider.as_ref()) {
            return Err(AiError::ResourceScopeMismatch);
        }
        self.add_file_id(file.id(), attributes).await
    }

    /// 文件 ID 必须来自当前连接，且已通过应用归属检查。
    pub async fn add_file_id(
        &self,
        id: &str,
        attributes: &Map<String, Value>,
    ) -> AiResult<StoreFile> {
        validate_id(id)?;
        validate_attributes(attributes)?;
        let info = self
            .provider
            .add_file_to_store(self.id(), id, attributes)
            .await?;
        verify_resource(id, &info.file_id)?;
        StoreFile::new(info, self.id(), self.provider.clone())
    }

    pub async fn files(&self, after: Option<&str>, limit: u16) -> AiResult<StoreFilePage> {
        validate_page(after, limit)?;
        let page = self
            .provider
            .list_store_files(self.id(), after, limit)
            .await?;
        let files = page
            .data
            .into_iter()
            .map(|info| StoreFile::new(info, self.id(), self.provider.clone()))
            .collect::<AiResult<_>>()?;
        Ok(StoreFilePage {
            files,
            next_cursor: page.next_cursor,
        })
    }

    pub async fn remove(&self, file: &StoreFile, delete_file: bool) -> AiResult<bool> {
        if self.id() != file.info.store_id
            || (!Arc::ptr_eq(&self.provider, &file.provider)
                && !same_resource_scope(self.provider.as_ref(), file.provider.as_ref()))
        {
            return Err(AiError::ResourceScopeMismatch);
        }
        let removed = self
            .provider
            .remove_file_from_store(self.id(), file.id())
            .await?;
        if removed && delete_file {
            let error = match self.provider.delete_file(file.file_id()).await {
                Ok(true) | Err(AiError::ProviderStatus(404)) => return Ok(true),
                Ok(false) => AiError::InvalidResponse,
                Err(error) => error,
            };
            return Err(AiError::DetachedFileDeletionFailed {
                file_id: file.file_id().to_owned(),
                source: Box::new(error),
            });
        }
        Ok(removed)
    }

    pub async fn delete(&self) -> AiResult<bool> {
        self.provider.delete_store(self.id()).await
    }
}

pub struct StorePage {
    pub stores: Vec<Store>,
    pub next_cursor: Option<String>,
}

pub struct ProviderStorePage {
    pub data: Vec<StoreInfo>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StoreFileInfo {
    pub id: String,
    pub file_id: String,
    pub store_id: String,
    pub status: Option<String>,
    pub usage_bytes: Option<u64>,
    pub attributes: Option<Map<String, Value>>,
}

pub struct StoreFile {
    info: StoreFileInfo,
    provider: Arc<dyn StoreProvider>,
}

impl StoreFile {
    fn new(info: StoreFileInfo, store: &str, provider: Arc<dyn StoreProvider>) -> AiResult<Self> {
        verify_resource(store, &info.store_id)?;
        validate_id(&info.id).map_err(|_| AiError::InvalidResponse)?;
        validate_id(&info.file_id).map_err(|_| AiError::InvalidResponse)?;
        Ok(Self { info, provider })
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn file_id(&self) -> &str {
        &self.info.file_id
    }

    pub fn info(&self) -> &StoreFileInfo {
        &self.info
    }
}

pub struct StoreFilePage {
    pub files: Vec<StoreFile>,
    pub next_cursor: Option<String>,
}

pub struct ProviderStoreFilePage {
    pub data: Vec<StoreFileInfo>,
    pub next_cursor: Option<String>,
}

pub trait StoreProvider: FileProvider {
    fn get_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<StoreInfo>>;

    fn create_store<'a>(
        &'a self,
        name: &'a str,
        options: StoreOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StoreInfo>>;

    fn delete_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>>;

    fn add_file_to_store<'a>(
        &'a self,
        _store: &'a str,
        _file: &'a str,
        _attributes: &'a Map<String, Value>,
    ) -> HttpFuture<'a, AiResult<StoreFileInfo>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn remove_file_from_store<'a>(
        &'a self,
        _store: &'a str,
        _document: &'a str,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn list_store_files<'a>(
        &'a self,
        _store: &'a str,
        _after: Option<&'a str>,
        _limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStoreFilePage>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn list_stores<'a>(
        &'a self,
        _after: Option<&'a str>,
        _limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStorePage>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }
}

impl StoreProvider for OpenAiFileProvider {
    fn get_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            let path = store_path(id)?;
            validate(self.0.request(None)?.get(&path).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)
        })
    }

    fn create_store<'a>(
        &'a self,
        name: &'a str,
        options: StoreOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            let mut body = Map::from_iter([("name".into(), Value::String(name.into()))]);
            if let Some(description) = options
                .description
                .filter(|description| !description.trim().is_empty())
            {
                body.insert("metadata".into(), json!({"description":description}));
            }
            if !options.file_ids.is_empty() {
                body.insert("file_ids".into(), json!(options.file_ids));
            }
            if let Some(days) = options.expires_after_days {
                body.insert(
                    "expires_after".into(),
                    json!({"anchor":"last_active_at","days":days}),
                );
            }
            validate(self.0.request(None)?.post("vector_stores", &body).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)
        })
    }

    fn delete_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let path = store_path(id)?;
            let response: Deleted =
                validate(self.0.request(None)?.send(Method::DELETE, &path).await)?
                    .json()
                    .map_err(|_| AiError::InvalidResponse)?;
            verify_resource(id, &response.id)?;
            Ok(response.deleted)
        })
    }

    fn add_file_to_store<'a>(
        &'a self,
        store: &'a str,
        file: &'a str,
        attributes: &'a Map<String, Value>,
    ) -> HttpFuture<'a, AiResult<StoreFileInfo>> {
        Box::pin(async move {
            validate_id(file)?;
            validate_attributes(attributes)?;
            let path = format!("{}/files", store_path(store)?);
            let mut body = json!({"file_id":file});
            if !attributes.is_empty() {
                body["attributes"] = json!(attributes);
            }
            let record: OpenAiStoreFile = validate(self.0.request(None)?.post(&path, &body).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            verify_resource(file, &record.id)?;
            Ok(record.into())
        })
    }

    fn remove_file_from_store<'a>(
        &'a self,
        store: &'a str,
        document: &'a str,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            validate_id(document)?;
            let path = format!("{}/files/{document}", store_path(store)?);
            let record: Deleted =
                validate(self.0.request(None)?.send(Method::DELETE, &path).await)?
                    .json()
                    .map_err(|_| AiError::InvalidResponse)?;
            verify_resource(document, &record.id)?;
            Ok(record.deleted)
        })
    }

    fn list_store_files<'a>(
        &'a self,
        store: &'a str,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStoreFilePage>> {
        Box::pin(async move {
            validate_page(after, limit)?;
            let path = format!("{}/files", store_path(store)?);
            let response = self
                .0
                .request(None)?
                .with_query_parameters(page_query(after, limit))
                .get(&path)
                .await;
            let page: OpenAiPage<OpenAiStoreFile> = validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let (data, next_cursor) = page.decode(after, limit, |file| &file.id)?;
            Ok(ProviderStoreFilePage {
                data: data.into_iter().map(Into::into).collect(),
                next_cursor,
            })
        })
    }

    fn list_stores<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStorePage>> {
        Box::pin(async move {
            validate_page(after, limit)?;
            let response = self
                .0
                .request(None)?
                .with_query_parameters(page_query(after, limit))
                .get("vector_stores")
                .await;
            let page: OpenAiPage<StoreInfo> = validate(response)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let (data, next_cursor) = page.decode(after, limit, |store| &store.id)?;
            Ok(ProviderStorePage { data, next_cursor })
        })
    }
}

#[derive(Deserialize)]
struct OpenAiStoreFile {
    id: String,
    vector_store_id: String,
    status: Option<String>,
    usage_bytes: Option<u64>,
    attributes: Option<Map<String, Value>>,
}

impl From<OpenAiStoreFile> for StoreFileInfo {
    fn from(record: OpenAiStoreFile) -> Self {
        Self {
            file_id: record.id.clone(),
            id: record.id,
            store_id: record.vector_store_id,
            status: record.status,
            usage_bytes: record.usage_bytes,
            attributes: record.attributes,
        }
    }
}

fn page_query(after: Option<&str>, limit: u16) -> Vec<(String, String)> {
    let mut query = vec![("limit".into(), limit.to_string())];
    if let Some(after) = after {
        query.push(("after".into(), after.into()));
    }
    query
}

fn validate_attributes(attributes: &Map<String, Value>) -> AiResult<()> {
    if attributes.len() > 16
        || attributes.iter().any(|(key, value)| {
            key.is_empty()
                || key.chars().count() > 64
                || match value {
                    Value::Bool(_) | Value::Number(_) => false,
                    Value::String(value) => value.chars().count() > 512,
                    _ => true,
                }
        })
    {
        return Err(AiError::InvalidStore);
    }
    Ok(())
}

fn store_path(id: &str) -> AiResult<String> {
    validate_id(id)?;
    Ok(format!("vector_stores/{id}"))
}

fn validate_page(after: Option<&str>, limit: u16) -> AiResult<()> {
    if let Some(cursor) = after {
        validate_id(cursor)?;
    }
    if !(1..=100).contains(&limit) {
        return Err(AiError::InvalidConfiguration("stores.limit"));
    }
    Ok(())
}

fn verify_resource(requested: &str, returned: &str) -> AiResult<()> {
    if requested != returned {
        return Err(AiError::InvalidResponse);
    }
    Ok(())
}
