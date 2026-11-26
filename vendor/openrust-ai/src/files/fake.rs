use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use bytes::Bytes;
use openrust::http::HttpFuture;

use crate::{AiError, AiResult};

use super::{FileInfo, FileProvider, FileUpload, Files, ProviderFilePage};

#[derive(Clone, Debug)]
pub struct RecordedFileUpload {
    pub provider: String,
    pub id: String,
    pub name: String,
    pub mime_type: String,
    pub content: Bytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedFileDeletion {
    pub provider: String,
    pub id: String,
}

type Resolver = dyn Fn(&str) -> AiResult<Bytes> + Send + Sync;

enum Responses {
    Sequence(Mutex<VecDeque<Bytes>>),
    Callback(Box<Resolver>),
}

#[derive(Default)]
struct State {
    scopes: HashMap<String, Arc<()>>,
    files: HashMap<String, BTreeMap<String, FakeFile>>,
    uploads: Vec<RecordedFileUpload>,
    deletions: Vec<RecordedFileDeletion>,
}

#[derive(Clone)]
struct FakeFile {
    info: FileInfo,
    content: Bytes,
}

/// 文件替身和应用内调用记录。
pub struct FakeFileGateway {
    responses: Responses,
    state: Mutex<State>,
    prevent_stray: AtomicBool,
}

impl FakeFileGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = Bytes>) -> Self {
        Self::with_responses(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        )))
    }

    pub(crate) fn with(resolver: impl Fn(&str) -> AiResult<Bytes> + Send + Sync + 'static) -> Self {
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

    pub fn uploads(&self) -> Vec<RecordedFileUpload> {
        self.state.lock().unwrap().uploads.clone()
    }

    pub fn deletions(&self) -> Vec<RecordedFileDeletion> {
        self.state.lock().unwrap().deletions.clone()
    }

    #[track_caller]
    pub fn assert_stored(&self, matches: impl Fn(&RecordedFileUpload) -> bool) {
        assert!(self.uploads().iter().any(matches), "未上传匹配的 AI 文件");
    }

    #[track_caller]
    pub fn assert_not_stored(&self, matches: impl Fn(&RecordedFileUpload) -> bool) {
        assert!(
            !self.uploads().iter().any(matches),
            "上传了不应保存的 AI 文件"
        );
    }

    #[track_caller]
    pub fn assert_nothing_stored(&self) {
        assert!(
            self.state.lock().unwrap().uploads.is_empty(),
            "已上传 AI 文件"
        );
    }

    #[track_caller]
    pub fn assert_deleted(&self, id: &str) {
        self.assert_deleted_where(|file| file.id == id);
    }

    #[track_caller]
    pub fn assert_deleted_where(&self, matches: impl Fn(&RecordedFileDeletion) -> bool) {
        assert!(self.deletions().iter().any(matches), "未删除匹配的 AI 文件");
    }

    #[track_caller]
    pub fn assert_not_deleted(&self, id: &str) {
        self.assert_not_deleted_where(|file| file.id == id);
    }

    #[track_caller]
    pub fn assert_not_deleted_where(&self, matches: impl Fn(&RecordedFileDeletion) -> bool) {
        assert!(
            !self.deletions().iter().any(matches),
            "删除了不应移除的 AI 文件"
        );
    }

    #[track_caller]
    pub fn assert_nothing_deleted(&self) {
        assert!(
            self.state.lock().unwrap().deletions.is_empty(),
            "已删除 AI 文件"
        );
    }

    pub(crate) fn provider(self: &Arc<Self>, name: String) -> Arc<dyn FileProvider> {
        let scope = self
            .state
            .lock()
            .unwrap()
            .scopes
            .entry(name.clone())
            .or_default()
            .clone();
        Arc::new(FakeFileProvider {
            name,
            scope,
            gateway: self.clone(),
        })
    }
}

struct FakeFileProvider {
    name: String,
    scope: Arc<()>,
    gateway: Arc<FakeFileGateway>,
}

impl FileProvider for FakeFileProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn resource_scope(&self) -> Option<&Arc<()>> {
        Some(&self.scope)
    }

    fn canonical_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        super::validate_id(id.strip_prefix("files/").unwrap_or(id))?;
        Ok(Cow::Borrowed(id))
    }

    fn get_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<FileInfo>> {
        Box::pin(async move {
            let response = match &self.gateway.responses {
                Responses::Sequence(responses) => responses.lock().unwrap().pop_front(),
                Responses::Callback(resolver) => Some(resolver(id)?),
            };
            let mut state = self.gateway.state.lock().unwrap();
            let files = state.files.entry(self.name.clone()).or_default();
            if let Some(content) = response {
                let info = FakeFile::metadata(id, None, "text/plain", &content, None);
                files.insert(
                    id.into(),
                    FakeFile {
                        info: info.clone(),
                        content,
                    },
                );
                return Ok(info);
            }
            if self.gateway.prevent_stray.load(Ordering::Relaxed) {
                return Err(AiError::FakeExhausted);
            }
            let file = files.entry(id.into()).or_insert_with(|| {
                let content = Bytes::from_static(b"fake-content");
                FakeFile {
                    info: FakeFile::metadata(id, None, "text/plain", &content, None),
                    content,
                }
            });
            Ok(file.info.clone())
        })
    }

    fn put_file(&self, file: FileUpload) -> HttpFuture<'_, AiResult<FileInfo>> {
        Box::pin(async move {
            let id = Files::fake_id(&file.name);
            let info = FakeFile::metadata(
                &id,
                Some(file.name.clone()),
                &file.mime_type,
                &file.content,
                file.purpose.clone(),
            );
            let mut state = self.gateway.state.lock().unwrap();
            state.files.entry(self.name.clone()).or_default().insert(
                id.clone(),
                FakeFile {
                    info: info.clone(),
                    content: file.content.clone(),
                },
            );
            state.uploads.push(RecordedFileUpload {
                provider: self.name.clone(),
                id,
                name: file.name,
                mime_type: file.mime_type,
                content: file.content,
            });
            Ok(info)
        })
    }

    fn delete_file<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let mut state = self.gateway.state.lock().unwrap();
            if let Some(files) = state.files.get_mut(&self.name) {
                files.remove(id);
            }
            state.deletions.push(RecordedFileDeletion {
                provider: self.name.clone(),
                id: id.into(),
            });
            Ok(true)
        })
    }

    fn file_contents<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<Bytes>> {
        Box::pin(async move {
            self.gateway
                .state
                .lock()
                .unwrap()
                .files
                .get(&self.name)
                .and_then(|files| files.get(id))
                .map(|file| file.content.clone())
                .ok_or(AiError::ProviderStatus(404))
        })
    }

    fn list_files<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderFilePage>> {
        Box::pin(async move {
            let state = self.gateway.state.lock().unwrap();
            let mut files = state
                .files
                .get(&self.name)
                .into_iter()
                .flatten()
                .filter(|(id, _)| after.is_none_or(|cursor| id.as_str() > cursor))
                .map(|(_, file)| &file.info);
            let data: Vec<_> = files.by_ref().take(usize::from(limit)).cloned().collect();
            let next_cursor = files.next().and(data.last()).map(|file| file.id.clone());
            Ok(ProviderFilePage { data, next_cursor })
        })
    }
}

impl FakeFile {
    fn metadata(
        id: &str,
        filename: Option<String>,
        mime_type: &str,
        content: &Bytes,
        purpose: Option<String>,
    ) -> FileInfo {
        FileInfo {
            id: id.into(),
            filename,
            mime_type: Some(mime_type.into()),
            downloadable: Some(true),
            bytes: Some(content.len() as u64),
            purpose,
            created_at: None,
            expires_at: None,
            status: None,
        }
    }
}
