use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use openrust::http::HttpFuture;
use serde_json::{Map, Value};

use crate::{AiError, AiResult, FileInfo, FileProvider, FileUpload, Files, ProviderFilePage};

use super::{
    ProviderStoreFilePage, ProviderStorePage, StoreFileCounts, StoreFileInfo, StoreInfo,
    StoreOptions, StoreProvider, Stores, validate_id,
};

#[derive(Clone, Debug)]
pub struct RecordedStoreCreation {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub file_ids: Vec<String>,
    pub expires_after_days: Option<u16>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedStoreDeletion {
    pub provider: String,
    pub id: String,
}

type Resolver = dyn Fn(&str) -> AiResult<String> + Send + Sync;

enum Responses {
    Sequence(Mutex<VecDeque<String>>),
    Callback(Box<Resolver>),
}

#[derive(Default)]
struct State {
    scopes: HashMap<String, Arc<()>>,
    stores: HashMap<String, BTreeMap<String, FakeStore>>,
    creations: Vec<RecordedStoreCreation>,
    deletions: Vec<RecordedStoreDeletion>,
}

struct FakeStore {
    id: String,
    name: String,
    description: Option<String>,
    members: BTreeMap<String, StoreFileInfo>,
}

impl FakeStore {
    fn info(&self) -> StoreInfo {
        let count = self.members.len() as u64;
        StoreInfo {
            id: self.id.clone(),
            name: Some(self.name.clone()),
            status: Some("completed".into()),
            file_counts: StoreFileCounts {
                completed: Some(count),
                pending: Some(0),
                failed: Some(0),
                total: Some(count),
                ..Default::default()
            },
            metadata: self
                .description
                .as_ref()
                .map(|description| BTreeMap::from([("description".into(), description.clone())]))
                .unwrap_or_default(),
            usage_bytes: None,
        }
    }

    fn add(&mut self, file: &str, attributes: Option<Map<String, Value>>) -> StoreFileInfo {
        let id = if file.starts_with("files/") {
            Files::fake_id(file)
        } else {
            file.to_owned()
        };
        let info = StoreFileInfo {
            id: id.clone(),
            file_id: Some(file.into()),
            store_id: self.id.clone(),
            status: Some("completed".into()),
            usage_bytes: None,
            attributes,
        };
        self.members.insert(id, info.clone());
        info
    }
}

/// 向量仓库替身和应用内调用记录。
pub struct FakeStoreGateway {
    responses: Responses,
    state: Mutex<State>,
    prevent_stray: AtomicBool,
}

impl FakeStoreGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = String>) -> Self {
        Self::with_responses(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        )))
    }

    pub(crate) fn with(
        resolver: impl Fn(&str) -> AiResult<String> + Send + Sync + 'static,
    ) -> Self {
        Self::with_responses(Responses::Callback(Box::new(resolver)))
    }

    fn with_responses(responses: Responses) -> Self {
        Self {
            responses,
            state: Mutex::default(),
            prevent_stray: AtomicBool::new(false),
        }
    }

    pub fn prevent_stray_operations(&self, prevent: bool) -> &Self {
        self.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn creations(&self) -> Vec<RecordedStoreCreation> {
        self.state.lock().unwrap().creations.clone()
    }

    pub fn deletions(&self) -> Vec<RecordedStoreDeletion> {
        self.state.lock().unwrap().deletions.clone()
    }

    #[track_caller]
    pub fn assert_created(&self, name: &str) {
        self.assert_created_where(|store| store.name == name);
    }

    #[track_caller]
    pub fn assert_created_where(&self, matches: impl Fn(&RecordedStoreCreation) -> bool) {
        assert!(
            self.creations().iter().any(matches),
            "未创建匹配的 AI 向量仓库"
        );
    }

    #[track_caller]
    pub fn assert_not_created(&self, name: &str) {
        self.assert_not_created_where(|store| store.name == name);
    }

    #[track_caller]
    pub fn assert_not_created_where(&self, matches: impl Fn(&RecordedStoreCreation) -> bool) {
        assert!(
            !self.creations().iter().any(matches),
            "创建了不应存在的 AI 向量仓库"
        );
    }

    #[track_caller]
    pub fn assert_nothing_created(&self) {
        assert!(
            self.state.lock().unwrap().creations.is_empty(),
            "已创建 AI 向量仓库"
        );
    }

    #[track_caller]
    pub fn assert_deleted(&self, id: &str) {
        self.assert_deleted_where(|store| store.id == id);
    }

    #[track_caller]
    pub fn assert_deleted_where(&self, matches: impl Fn(&RecordedStoreDeletion) -> bool) {
        assert!(
            self.deletions().iter().any(matches),
            "未删除匹配的 AI 向量仓库"
        );
    }

    #[track_caller]
    pub fn assert_not_deleted(&self, id: &str) {
        self.assert_not_deleted_where(|store| store.id == id);
    }

    #[track_caller]
    pub fn assert_not_deleted_where(&self, matches: impl Fn(&RecordedStoreDeletion) -> bool) {
        assert!(
            !self.deletions().iter().any(matches),
            "删除了不应移除的 AI 向量仓库"
        );
    }

    #[track_caller]
    pub fn assert_nothing_deleted(&self) {
        assert!(
            self.state.lock().unwrap().deletions.is_empty(),
            "已删除 AI 向量仓库"
        );
    }

    pub(crate) fn provider(
        self: &Arc<Self>,
        name: String,
        files: Option<Arc<dyn FileProvider>>,
    ) -> Arc<dyn StoreProvider> {
        let scope = self
            .state
            .lock()
            .unwrap()
            .scopes
            .entry(name.clone())
            .or_default()
            .clone();
        Arc::new(FakeStoreProvider {
            name,
            scope,
            files,
            gateway: self.clone(),
        })
    }
}

struct FakeStoreProvider {
    name: String,
    scope: Arc<()>,
    files: Option<Arc<dyn FileProvider>>,
    gateway: Arc<FakeStoreGateway>,
}

impl FakeStoreProvider {
    fn files(&self) -> AiResult<&dyn FileProvider> {
        self.files.as_deref().ok_or(AiError::FilesNotSupported)
    }
}

impl FileProvider for FakeStoreProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn resource_scope(&self) -> Option<&Arc<()>> {
        self.files.as_ref().and_then(|files| files.resource_scope())
    }

    fn canonical_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        if let Some(files) = &self.files {
            return files.canonical_id(id);
        }
        validate_id(id)?;
        Ok(Cow::Borrowed(id))
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>> {
        Box::pin(async move { self.files()?.get_file(id).await })
    }

    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move { self.files()?.put_file(file).await })
    }

    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move { self.files()?.delete_file(id).await })
    }
    fn file_contents<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bytes::Bytes>> {
        Box::pin(async move { self.files()?.file_contents(id).await })
    }

    fn list_files<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async move { self.files()?.list_files(after, limit).await })
    }
}

impl StoreProvider for FakeStoreProvider {
    fn store_scope(&self) -> Option<&Arc<()>> {
        Some(&self.scope)
    }

    fn canonical_store_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        validate_id(id.strip_prefix("fileSearchStores/").unwrap_or(id))?;
        Ok(Cow::Borrowed(id))
    }

    fn get_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            let name = match &self.gateway.responses {
                Responses::Sequence(responses) => responses.lock().unwrap().pop_front(),
                Responses::Callback(resolver) => Some(resolver(id)?),
            };
            if name.is_none() && self.gateway.prevent_stray.load(Ordering::Relaxed) {
                return Err(AiError::FakeExhausted);
            }
            let mut state = self.gateway.state.lock().unwrap();
            let store = state
                .stores
                .entry(self.name.clone())
                .or_default()
                .entry(id.into())
                .or_insert_with(|| FakeStore {
                    id: id.into(),
                    name: "fake-store".into(),
                    description: None,
                    members: BTreeMap::new(),
                });
            let mut info = store.info();
            if name.is_some() {
                info.name = name;
            }
            Ok(info)
        })
    }

    fn create_store<'a>(
        &'a self,
        name: &'a str,
        options: StoreOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            let id = Stores::fake_id(name);
            let mut store = FakeStore {
                id: id.clone(),
                name: name.into(),
                description: options.description.map(str::to_owned),
                members: BTreeMap::new(),
            };
            for file in options.file_ids {
                store.add(&self.canonical_id(file)?, None);
            }
            let info = store.info();
            let mut state = self.gateway.state.lock().unwrap();
            state
                .stores
                .entry(self.name.clone())
                .or_default()
                .insert(id.clone(), store);
            state.creations.push(RecordedStoreCreation {
                provider: self.name.clone(),
                id,
                name: name.into(),
                description: options.description.map(str::to_owned),
                file_ids: options.file_ids.iter().map(|id| (*id).into()).collect(),
                expires_after_days: options.expires_after_days,
            });
            Ok(info)
        })
    }

    fn delete_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let mut state = self.gateway.state.lock().unwrap();
            if let Some(stores) = state.stores.get_mut(&self.name) {
                stores.remove(id);
            }
            state.deletions.push(RecordedStoreDeletion {
                provider: self.name.clone(),
                id: id.into(),
            });
            Ok(true)
        })
    }

    fn add_file_to_store<'a>(
        &'a self,
        store: &'a str,
        file: &'a str,
        attributes: &'a Map<String, Value>,
    ) -> HttpFuture<'a, AiResult<StoreFileInfo>> {
        Box::pin(async move {
            let mut state = self.gateway.state.lock().unwrap();
            let store = state
                .stores
                .get_mut(&self.name)
                .and_then(|stores| stores.get_mut(store))
                .ok_or(AiError::ProviderStatus(404))?;
            Ok(store.add(file, Some(attributes.clone())))
        })
    }

    fn remove_file_from_store<'a>(
        &'a self,
        store: &'a str,
        file: &'a str,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let mut state = self.gateway.state.lock().unwrap();
            let store = state
                .stores
                .get_mut(&self.name)
                .and_then(|stores| stores.get_mut(store))
                .ok_or(AiError::ProviderStatus(404))?;
            store.members.remove(file);
            Ok(true)
        })
    }

    fn list_stores<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStorePage>> {
        Box::pin(async move {
            let state = self.gateway.state.lock().unwrap();
            let page = state
                .stores
                .get(&self.name)
                .into_iter()
                .flatten()
                .filter(|(id, _)| after.is_none_or(|cursor| id.as_str() > cursor))
                .map(|(_, store)| store.info());
            let (data, next_cursor) = paginate(page, limit, |store| &store.id);
            Ok(ProviderStorePage { data, next_cursor })
        })
    }

    fn list_store_files<'a>(
        &'a self,
        store: &'a str,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStoreFilePage>> {
        Box::pin(async move {
            let state = self.gateway.state.lock().unwrap();
            let entry = state
                .stores
                .get(&self.name)
                .and_then(|stores| stores.get(store))
                .ok_or(AiError::ProviderStatus(404))?;
            let page = entry
                .members
                .iter()
                .filter(|(id, _)| after.is_none_or(|cursor| id.as_str() > cursor))
                .map(|(_, file)| file.clone());
            let (data, next_cursor) = paginate(page, limit, |file| &file.id);
            Ok(ProviderStoreFilePage { data, next_cursor })
        })
    }
}

fn paginate<T>(
    mut rows: impl Iterator<Item = T>,
    limit: u16,
    id: impl Fn(&T) -> &str,
) -> (Vec<T>, Option<String>) {
    let data: Vec<_> = rows.by_ref().take(usize::from(limit)).collect();
    let next = rows.next().and(data.last()).map(|item| id(item).to_owned());
    (data, next)
}
