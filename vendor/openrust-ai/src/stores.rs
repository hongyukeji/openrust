use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

use openrust::http::{HttpFuture, Method};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    Ai, AiError, AiResult, FileProvider, FileResponse,
    files::{Deleted, OpenAiFileProvider, OpenAiPage, validate_id},
    text::validate,
};

mod fake;
mod gemini;
pub use fake::{FakeStoreGateway, RecordedStoreCreation, RecordedStoreDeletion};

pub struct Stores;

impl Stores {
    pub fn fake(responses: impl IntoIterator<Item = String>) -> AiResult<Arc<FakeStoreGateway>> {
        Self::fake_with_files(responses, true)
    }

    pub fn fake_with_files(
        responses: impl IntoIterator<Item = String>,
        files: bool,
    ) -> AiResult<Arc<FakeStoreGateway>> {
        let manager = Ai::manager()?;
        if files {
            manager.fake_files([]);
        }
        Ok(manager.fake_stores(responses))
    }

    pub fn fake_with(
        resolve: impl Fn(&str) -> AiResult<String> + Send + Sync + 'static,
    ) -> AiResult<Arc<FakeStoreGateway>> {
        let manager = Ai::manager()?;
        manager.fake_files([]);
        Ok(manager.fake_stores_with(resolve))
    }

    pub fn fake_id(name: &str) -> String {
        use openrust::support::Str;
        format!("fake_store_{}", Str::of(name).hash::<md5::Md5>().value())
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::stores_are_faked()
    }

    #[track_caller]
    pub fn assert_created(name: &str) -> AiResult<()> {
        Self::fake_gateway()?.assert_created(name);
        Ok(())
    }

    #[track_caller]
    pub fn assert_created_where(matches: impl Fn(&RecordedStoreCreation) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_created_where(matches);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_created(name: &str) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_created(name);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_created_where(
        matches: impl Fn(&RecordedStoreCreation) -> bool,
    ) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_created_where(matches);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_created() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_created();
        Ok(())
    }

    #[track_caller]
    pub fn assert_deleted(id: &str) -> AiResult<()> {
        Self::fake_gateway()?.assert_deleted(id);
        Ok(())
    }

    #[track_caller]
    pub fn assert_deleted_where(matches: impl Fn(&RecordedStoreDeletion) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_deleted_where(matches);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_deleted(id: &str) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_deleted(id);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_deleted_where(
        matches: impl Fn(&RecordedStoreDeletion) -> bool,
    ) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_deleted_where(matches);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_deleted() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_deleted();
        Ok(())
    }

    fn fake_gateway() -> AiResult<Arc<FakeStoreGateway>> {
        Ai::manager()?
            .stores_fake()
            .ok_or(AiError::FakeNotInstalled)
    }

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
        let provider = Ai::fakeable_store_provider(provider)?;
        for id in options.file_ids {
            provider.canonical_id(id)?;
        }
        let result = provider.create_store(name, options).await?;
        Store::new(result, provider)
    }

    pub async fn get(id: &str, provider: Option<&str>) -> AiResult<Store> {
        let provider = Ai::fakeable_store_provider(provider)?;
        let id = provider.canonical_store_id(id)?;
        let result = provider.get_store(&id).await?;
        verify_resource(&id, &result.id)?;
        Store::new(result, provider)
    }

    pub async fn delete(id: &str, provider: Option<&str>) -> AiResult<bool> {
        let provider = Ai::fakeable_store_provider(provider)?;
        let id = provider.canonical_store_id(id)?;
        provider.delete_store(&id).await
    }

    pub async fn list(
        after: Option<&str>,
        limit: u16,
        provider: Option<&str>,
    ) -> AiResult<StorePage> {
        let provider = Ai::fakeable_store_provider(provider)?;
        validate_page(provider.as_ref(), after, limit)?;
        let result = provider.list_stores(after, limit).await?;
        validate_next_cursor(provider.as_ref(), after, result.next_cursor.as_deref())?;
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
        let canonical = provider
            .canonical_store_id(&info.id)
            .map_err(|_| AiError::InvalidResponse)?;
        verify_resource(&canonical, &info.id)?;
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
        let id = self.provider.canonical_id(id)?;
        let info = self
            .provider
            .add_file_to_store(self.id(), &id, attributes)
            .await?;
        verify_resource(
            &id,
            info.file_id.as_deref().ok_or(AiError::InvalidResponse)?,
        )?;
        StoreFile::new(info, self.id(), self.provider.clone())
    }

    pub async fn files(&self, after: Option<&str>, limit: u16) -> AiResult<StoreFilePage> {
        validate_page(self.provider.as_ref(), after, limit)?;
        let page = self
            .provider
            .list_store_files(self.id(), after, limit)
            .await?;
        validate_next_cursor(self.provider.as_ref(), after, page.next_cursor.as_deref())?;
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
        let same_scope = self
            .provider
            .store_scope()
            .zip(file.provider.store_scope())
            .is_some_and(|(store, member)| Arc::ptr_eq(store, member));
        if self.id() != file.info.store_id
            || (!Arc::ptr_eq(&self.provider, &file.provider) && !same_scope)
        {
            return Err(AiError::ResourceScopeMismatch);
        }
        let source_file = if delete_file {
            Some(file.file_id().ok_or(AiError::StoreSourceFileUnavailable)?)
        } else {
            None
        };
        let removed = self
            .provider
            .remove_file_from_store(self.id(), file.id())
            .await?;
        if let (true, Some(file_id)) = (removed, source_file) {
            let error = match self.provider.delete_file(file_id).await {
                Ok(true) | Err(AiError::ProviderStatus(404)) => return Ok(true),
                Ok(false) => AiError::InvalidResponse,
                Err(error) => error,
            };
            return Err(AiError::DetachedFileDeletionFailed {
                file_id: file_id.to_owned(),
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
    pub file_id: Option<String>,
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
        if let Some(file_id) = &info.file_id {
            let canonical = provider
                .canonical_id(file_id)
                .map_err(|_| AiError::InvalidResponse)?;
            verify_resource(&canonical, file_id)?;
        }
        Ok(Self { info, provider })
    }

    pub fn id(&self) -> &str {
        &self.info.id
    }

    pub fn file_id(&self) -> Option<&str> {
        self.info.file_id.as_deref()
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
    fn store_scope(&self) -> Option<&Arc<()>> {
        self.resource_scope()
    }

    fn canonical_store_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        validate_id(id)?;
        Ok(Cow::Borrowed(id))
    }

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
            validate_page(self, after, limit)?;
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
            validate_page(self, after, limit)?;
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
            file_id: Some(record.id.clone()),
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

fn validate_page(provider: &dyn StoreProvider, after: Option<&str>, limit: u16) -> AiResult<()> {
    if let Some(cursor) = after {
        provider.validate_cursor(cursor)?;
    }
    if !(1..=100).contains(&limit) {
        return Err(AiError::InvalidConfiguration("stores.limit"));
    }
    Ok(())
}

fn validate_next_cursor(
    provider: &dyn StoreProvider,
    after: Option<&str>,
    next: Option<&str>,
) -> AiResult<()> {
    if let Some(next) = next {
        provider
            .validate_cursor(next)
            .map_err(|_| AiError::InvalidResponse)?;
        if Some(next) == after {
            return Err(AiError::InvalidResponse);
        }
    }
    Ok(())
}

fn verify_resource(requested: &str, returned: &str) -> AiResult<()> {
    if requested != returned {
        return Err(AiError::InvalidResponse);
    }
    Ok(())
}
