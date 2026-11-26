use std::{borrow::Cow, time::Duration};

use openrust::http::{HttpFuture, Method};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    AiError, AiResult, FileProvider,
    files::{GeminiFileProvider, validate_gemini_id},
    text::validate,
};

use super::{
    ProviderStoreFilePage, ProviderStorePage, StoreFileCounts, StoreFileInfo, StoreInfo,
    StoreOptions, StoreProvider,
};

const STORE_PREFIX: &str = "fileSearchStores/";
const PAGE_LIMIT: u16 = 20;
const IMPORT_TIMEOUT: Duration = Duration::from_secs(300);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

impl StoreProvider for GeminiFileProvider {
    fn canonical_store_id<'a>(&self, id: &'a str) -> AiResult<Cow<'a, str>> {
        let short = id.strip_prefix(STORE_PREFIX).unwrap_or(id);
        validate_gemini_id(short).map_err(|_| AiError::InvalidStore)?;
        Ok(if short.len() == id.len() {
            format!("{STORE_PREFIX}{short}").into()
        } else {
            id.into()
        })
    }

    fn get_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            let id = self.canonical_store_id(id)?;
            let data: Resource = validate(self.0.request(None)?.get(&id).await)?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            if data.name != id {
                return Err(AiError::InvalidResponse);
            }
            data.try_into()
        })
    }

    fn create_store<'a>(
        &'a self,
        name: &'a str,
        options: StoreOptions<'a>,
    ) -> HttpFuture<'a, AiResult<StoreInfo>> {
        Box::pin(async move {
            if name.chars().count() > 512
                || options.description.is_some()
                || options.expires_after_days.is_some()
            {
                return Err(AiError::InvalidStore);
            }
            let data: Resource = validate(
                self.0
                    .request(None)?
                    .post("fileSearchStores", &json!({"displayName":name}))
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            if self
                .canonical_store_id(&data.name)
                .map_err(|_| AiError::InvalidResponse)?
                != data.name
            {
                return Err(AiError::InvalidResponse);
            }
            if options.file_ids.is_empty() {
                return data.try_into();
            }
            let populate = async {
                for file in options.file_ids {
                    self.add_file_to_store(&data.name, file, &Map::new())
                        .await?;
                }
                self.get_store(&data.name).await
            };
            populate
                .await
                .map_err(|source| AiError::StorePopulationFailed {
                    store_id: data.name,
                    source: Box::new(source),
                })
        })
    }

    fn delete_store<'a>(&'a self, id: &'a str) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let id = self.canonical_store_id(id)?;
            delete_resource(self, &id).await
        })
    }

    fn add_file_to_store<'a>(
        &'a self,
        store: &'a str,
        file: &'a str,
        attributes: &'a Map<String, Value>,
    ) -> HttpFuture<'a, AiResult<StoreFileInfo>> {
        Box::pin(async move {
            let store = self.canonical_store_id(store)?;
            let file = self.canonical_id(file)?;
            let metadata = to_metadata(attributes)?;
            let mut body = json!({"fileName":file});
            if !metadata.is_empty() {
                body["customMetadata"] = Value::Array(metadata);
            }
            let url = format!("{store}:importFile");
            let operation = tokio::time::timeout(IMPORT_TIMEOUT, async {
                let mut operation: Operation =
                    validate(self.0.request(None)?.post(&url, &body).await)?
                        .json()
                        .map_err(|_| AiError::InvalidResponse)?;
                if !operation.done {
                    let name = operation.name.take().ok_or(AiError::InvalidResponse)?;
                    let prefix = format!("{store}/operations/");
                    let id = name.strip_prefix(&prefix).ok_or(AiError::InvalidResponse)?;
                    super::validate_id(id).map_err(|_| AiError::InvalidResponse)?;
                    while !operation.done {
                        tokio::time::sleep(POLL_INTERVAL).await;
                        operation = validate(self.0.request(None)?.get(&name).await)?
                            .json()
                            .map_err(|_| AiError::InvalidResponse)?;
                        if operation
                            .name
                            .as_deref()
                            .is_some_and(|returned| returned != name)
                        {
                            return Err(AiError::InvalidResponse);
                        }
                    }
                }
                Ok(operation)
            })
            .await
            .map_err(|_| AiError::Timeout)??;
            if let Some(error) = operation.error {
                return Err(AiError::StoreImportFailed(error.code));
            }
            let response = operation.response.ok_or(AiError::InvalidResponse)?;
            let id = document_id(&store, &response.document_name)?;
            Ok(StoreFileInfo {
                id: id.to_owned(),
                file_id: Some(file.into_owned()),
                store_id: store.into_owned(),
                status: None,
                usage_bytes: None,
                attributes: Some(attributes.clone()),
            })
        })
    }

    fn remove_file_from_store<'a>(
        &'a self,
        store: &'a str,
        document: &'a str,
    ) -> HttpFuture<'a, AiResult<bool>> {
        Box::pin(async move {
            let store = self.canonical_store_id(store)?;
            validate_gemini_id(document)?;
            delete_resource(self, &format!("{store}/documents/{document}")).await
        })
    }

    fn list_stores<'a>(
        &'a self,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStorePage>> {
        Box::pin(async move {
            let query = page_query(after, limit);
            let page: StoresPage = validate(
                self.0
                    .request(None)?
                    .with_query_parameters(query)
                    .get("fileSearchStores")
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            if page.file_search_stores.len() > usize::from(limit.min(PAGE_LIMIT)) {
                return Err(AiError::InvalidResponse);
            }
            Ok(ProviderStorePage {
                data: page
                    .file_search_stores
                    .into_iter()
                    .map(TryInto::try_into)
                    .collect::<AiResult<_>>()?,
                next_cursor: page.next_page_token.filter(|token| !token.is_empty()),
            })
        })
    }

    fn list_store_files<'a>(
        &'a self,
        store: &'a str,
        after: Option<&'a str>,
        limit: u16,
    ) -> HttpFuture<'a, AiResult<ProviderStoreFilePage>> {
        Box::pin(async move {
            let store = self.canonical_store_id(store)?;
            let query = page_query(after, limit);
            let page: DocumentsPage = validate(
                self.0
                    .request(None)?
                    .with_query_parameters(query)
                    .get(&format!("{store}/documents"))
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            if page.documents.len() > usize::from(limit.min(PAGE_LIMIT)) {
                return Err(AiError::InvalidResponse);
            }
            let data = page
                .documents
                .into_iter()
                .map(|document| {
                    Ok(StoreFileInfo {
                        id: document_id(&store, &document.name)?.to_owned(),
                        store_id: store.to_string(),
                        file_id: None,
                        status: document.state,
                        usage_bytes: document.size_bytes.map(|size| size.get()).transpose()?,
                        attributes: document.custom_metadata.map(from_metadata).transpose()?,
                    })
                })
                .collect::<AiResult<_>>()?;
            Ok(ProviderStoreFilePage {
                data,
                next_cursor: page.next_page_token.filter(|token| !token.is_empty()),
            })
        })
    }
}

async fn delete_resource(provider: &GeminiFileProvider, path: &str) -> AiResult<bool> {
    validate(
        provider
            .0
            .request(None)?
            .with_query_parameters([("force".into(), "true".into())])
            .send(Method::DELETE, path)
            .await,
    )?;
    Ok(true)
}

fn document_id<'a>(store: &str, resource: &'a str) -> AiResult<&'a str> {
    let id = resource
        .strip_prefix(&format!("{store}/documents/"))
        .ok_or(AiError::InvalidResponse)?;
    validate_gemini_id(id).map_err(|_| AiError::InvalidResponse)?;
    Ok(id)
}

fn page_query(after: Option<&str>, limit: u16) -> Vec<(String, String)> {
    let mut query = vec![("pageSize".into(), limit.min(PAGE_LIMIT).to_string())];
    if let Some(after) = after {
        query.push(("pageToken".into(), after.into()));
    }
    query
}

fn to_metadata(values: &Map<String, Value>) -> AiResult<Vec<Value>> {
    if values.len() > 20 {
        return Err(AiError::InvalidStore);
    }
    values
        .iter()
        .map(|(key, value)| {
            if key.trim().is_empty() {
                return Err(AiError::InvalidStore);
            }
            Ok(match value {
                Value::String(text) => json!({"key":key,"stringValue":text}),
                Value::Number(value) => json!({"key":key,"numericValue":value}),
                Value::Bool(value) => json!({"key":key,"stringValue":value.to_string()}),
                Value::Array(values) if values.iter().all(Value::is_string) => {
                    json!({"key":key,"stringListValue":{"values":values}})
                }
                _ => return Err(AiError::InvalidStore),
            })
        })
        .collect()
}

fn from_metadata(values: Vec<Metadata>) -> AiResult<Map<String, Value>> {
    let mut attributes = Map::new();
    for entry in values {
        let value = match (
            entry.string_value,
            entry.numeric_value,
            entry.string_list_value,
        ) {
            (Some(value), None, None) => Value::String(value),
            (None, Some(value), None) => Value::Number(value),
            (None, None, Some(list)) => json!(list.values),
            _ => return Err(AiError::InvalidResponse),
        };
        if entry.key.is_empty() || attributes.insert(entry.key, value).is_some() {
            return Err(AiError::InvalidResponse);
        }
    }
    Ok(attributes)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Resource {
    name: String,
    display_name: Option<String>,
    size_bytes: Option<Unsigned>,
    active_documents_count: Option<Unsigned>,
    pending_documents_count: Option<Unsigned>,
    failed_documents_count: Option<Unsigned>,
}

impl TryFrom<Resource> for StoreInfo {
    type Error = AiError;

    fn try_from(data: Resource) -> AiResult<Self> {
        Ok(Self {
            id: data.name,
            name: data.display_name,
            status: Some("completed".into()),
            file_counts: StoreFileCounts {
                completed: data
                    .active_documents_count
                    .map(|count| count.get())
                    .transpose()?,
                pending: data
                    .pending_documents_count
                    .map(|count| count.get())
                    .transpose()?,
                failed: data
                    .failed_documents_count
                    .map(|count| count.get())
                    .transpose()?,
                ..Default::default()
            },
            usage_bytes: data.size_bytes.map(|bytes| bytes.get()).transpose()?,
            metadata: Default::default(),
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoresPage {
    #[serde(default)]
    file_search_stores: Vec<Resource>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentsPage {
    #[serde(default)]
    documents: Vec<Document>,
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    name: String,
    state: Option<String>,
    size_bytes: Option<Unsigned>,
    custom_metadata: Option<Vec<Metadata>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    key: String,
    string_value: Option<String>,
    numeric_value: Option<serde_json::Number>,
    string_list_value: Option<StringList>,
}

#[derive(Deserialize)]
struct StringList {
    #[serde(default)]
    values: Vec<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Unsigned {
    Number(u64),
    String(String),
}

impl Unsigned {
    fn get(self) -> AiResult<u64> {
        match self {
            Self::Number(n) => Ok(n),
            Self::String(value) => value.parse().map_err(|_| AiError::InvalidResponse),
        }
    }
}

#[derive(Deserialize)]
struct Operation {
    name: Option<String>,
    #[serde(default)]
    done: bool,
    error: Option<OperationError>,
    response: Option<ImportResponse>,
}

#[derive(Deserialize)]
struct OperationError {
    code: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportResponse {
    document_name: String,
}
