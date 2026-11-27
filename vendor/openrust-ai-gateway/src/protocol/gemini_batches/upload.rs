use axum::body::Bytes;
use http::{HeaderName, HeaderValue, header};
use openrust::filesystem::{Storage, Visibility, WriteOptions};
use sha2::{Digest, Sha256};

use super::*;
use crate::usage::gemini_batch_files::{BatchFile, DISK, directory, valid_id};

const LIMIT: usize = 8 * 1024 * 1024;

pub(in crate::protocol) fn upload_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(async move {
        let gateway = Gateway::get_facade_root()?;
        let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
            Ok(key) => key,
            Err(error) => return error,
        };
        let Some(store) = &gateway.usage_store else {
            return storage_error();
        };
        if let Some(response) = admit(&key, PROTOCOL).await? {
            return Ok(response);
        }
        match super::super::single_header(request, "x-goog-upload-command") {
            Some("start") => {
                if !gateway.models.iter().any(|(name, model)| {
                    key.allows_model(name)
                        && [
                            TextEndpoint::GeminiBatchGenerateContent,
                            TextEndpoint::GeminiAsyncBatchEmbedContent,
                        ]
                        .into_iter()
                        .any(|endpoint| model.supports_for(endpoint, &key))
                }) {
                    return not_found();
                }
                start(request, store, &key).await
            }
            Some("upload, finalize" | "upload,finalize") => finish(request, store, &key).await,
            Some("query") => query(request, store, &key).await,
            _ => invalid("批文件上传命令无效"),
        }
    })
}

fn upload_id(request: &Request) -> Option<String> {
    match request.query(Some("upload_id")) {
        Ok(Some(Value::String(id))) if valid_id(&id) => Some(id),
        _ => None,
    }
}

async fn start(request: &mut Request, store: &UsageStore, key: &Key) -> HttpResult {
    if super::super::single_header(request, "x-goog-upload-protocol") != Some("resumable") {
        return invalid("批文件需要 Gemini resumable 上传协议");
    }
    let Some(mime) = super::super::single_header(request, "x-goog-upload-header-content-type")
        .filter(|mime| {
            matches!(
                *mime,
                "application/jsonl" | "application/x-ndjson" | "application/json" | "jsonl"
            )
        })
        .map(str::to_owned)
    else {
        return invalid("批任务文件须为 JSONL");
    };
    let Some(size) = super::super::single_header(request, "x-goog-upload-header-content-length")
        .and_then(|size| size.parse::<usize>().ok())
        .filter(|size| (1..=LIMIT).contains(size))
    else {
        return invalid("批文件长度无效或超出上限");
    };
    let url = request.url()?;
    let meta = match request.body_bytes_with_limit(4096).await {
        Ok(body) => match serde_json::from_slice::<Value>(&body) {
            Ok(meta) if meta["file"].is_object() => meta,
            _ => return invalid("批文件元数据无效"),
        },
        Err(_) => return invalid("无法读取批文件元数据"),
    };
    let display_name = meta["file"]["displayName"]
        .as_str()
        .or_else(|| meta["file"]["display_name"].as_str())
        .unwrap_or("batch.jsonl");
    if display_name.is_empty()
        || display_name.len() > 255
        || display_name.chars().any(char::is_control)
    {
        return invalid("批文件名无效");
    }
    let file = BatchFile {
        id: format!("input-{}", Str::ulid().to_string().to_ascii_lowercase()),
        owner: owner(key),
        display_name: display_name.into(),
        mime,
        size,
        object: None,
        deleted: false,
    };
    let url = format!("{url}?upload_id={}", file.id);
    let Ok(url) = HeaderValue::from_str(&url) else {
        return invalid("上传地址无效");
    };
    if store.create_gemini_batch_file(&file).await.is_err() {
        return storage_error();
    }
    Ok(Response::empty(StatusCode::OK)
        .header(HeaderName::from_static("x-goog-upload-url"), url)
        .header(
            HeaderName::from_static("x-goog-upload-status"),
            HeaderValue::from_static("active"),
        ))
}

async fn lookup(
    store: &UsageStore,
    key: &Key,
    id: &str,
    include_deleted: bool,
) -> Result<BatchFile, HttpResult> {
    if !valid_id(id) {
        return Err(not_found());
    }
    match store
        .gemini_batch_file(&owner(key), id, include_deleted)
        .await
    {
        Ok(Some(file)) => Ok(file),
        Ok(None) => Err(not_found()),
        Err(_) => Err(storage_error()),
    }
}

async fn finish(request: &mut Request, store: &UsageStore, key: &Key) -> HttpResult {
    let Some(id) = upload_id(request) else {
        return not_found();
    };
    let file = match lookup(store, key, &id, false).await {
        Ok(file) => file,
        Err(error) => return error,
    };
    if super::super::single_header(request, "x-goog-upload-offset") != Some("0") {
        return invalid("批文件上传偏移须为 0");
    }
    let data = match request.body_bytes_with_limit(file.size).await {
        Ok(data) if data.len() == file.size => data,
        _ => return invalid("批文件长度不匹配"),
    };
    let hash = digest(&data);
    if let Some((_, existing)) = &file.object {
        return if *existing == hash {
            file_result(&file, true)
        } else {
            conflict()
        };
    }
    let Ok(disk) = Storage::disk(Some(DISK)) else {
        return storage_error();
    };
    let path = format!("{}/{}", directory(&file.id), Str::ulid());
    if !matches!(
        disk.put_with_options(
            &path,
            &data,
            WriteOptions {
                visibility: Some(Visibility::Private)
            }
        )
        .await,
        Ok(true)
    ) {
        return storage_error();
    }
    match store.finish_gemini_batch_file(&file, &path, &hash).await {
        Ok(true) => file_result(&file, true),
        Ok(false) => {
            let _ = disk.delete(&path).await;
            match lookup(store, key, &file.id, false).await {
                Ok(winner)
                    if winner
                        .object
                        .as_ref()
                        .is_some_and(|(_, current)| *current == hash) =>
                {
                    file_result(&winner, true)
                }
                Ok(_) => conflict(),
                Err(error) => error,
            }
        }
        // 写入结果未知时保留文件，避免数据库已提交的引用失效。
        Err(_) => storage_error(),
    }
}

async fn query(request: &mut Request, store: &UsageStore, key: &Key) -> HttpResult {
    let Some(id) = upload_id(request) else {
        return not_found();
    };
    let file = match lookup(store, key, &id, false).await {
        Ok(file) => file,
        Err(error) => return error,
    };
    let status = if file.object.is_some() {
        "final"
    } else {
        "active"
    };
    let size = if file.object.is_some() { file.size } else { 0 };
    Ok(Response::empty(StatusCode::OK)
        .header(
            HeaderName::from_static("x-goog-upload-status"),
            HeaderValue::from_static(status),
        )
        .header(
            HeaderName::from_static("x-goog-upload-size-received"),
            HeaderValue::from(size),
        ))
}

pub(super) fn get_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, false))
}

pub(super) fn delete_route(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    Box::pin(manage(request, true))
}

async fn manage(request: &mut Request, delete: bool) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(store) = &gateway.usage_store else {
        return storage_error();
    };
    let Some(id) = request.route("id") else {
        return not_found();
    };
    let file = match lookup(store, &key, id, delete).await {
        Ok(file) => file,
        Err(error) => return error,
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    if !delete {
        return if file.object.is_some() {
            file_result(&file, false)
        } else {
            not_found()
        };
    }
    if !file.deleted {
        match store.delete_gemini_batch_file(&file).await {
            Ok(true) => {}
            Ok(false) => return conflict(),
            Err(_) => return storage_error(),
        }
    }
    if let Some((path, _)) = &file.object {
        let Ok(disk) = Storage::disk(Some(DISK)) else {
            return storage_error();
        };
        if !matches!(disk.delete(path).await, Ok(true)) {
            return storage_error();
        }
    }
    Response::json(StatusCode::OK, json!({}))
}

fn file_result(file: &BatchFile, wrapped: bool) -> HttpResult {
    let metadata = json!({
        "name":format!("files/{}",file.id), "displayName":file.display_name,
        "mimeType":file.mime, "sizeBytes":file.size.to_string(), "state":"ACTIVE"
    });
    Response::json(
        StatusCode::OK,
        if wrapped {
            json!({"file":metadata})
        } else {
            metadata
        },
    )
    .map(|response| {
        response.header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        )
    })
}

fn conflict() -> HttpResult {
    PROTOCOL.error(StatusCode::CONFLICT, "upload_conflict", "批文件状态已变更")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(super) async fn contents(
    store: &UsageStore,
    key: &Key,
    name: &str,
) -> Result<Bytes, HttpResult> {
    let id = name.strip_prefix("files/").ok_or_else(not_found)?;
    let file = lookup(store, key, id, false).await?;
    let (path, hash) = file.object.ok_or_else(not_found)?;
    let disk = Storage::disk(Some(DISK)).map_err(|_| storage_error())?;
    let data = disk
        .get(&path)
        .await
        .map_err(|_| storage_error())?
        .ok_or_else(storage_error)?;
    if data.len() != file.size || digest(&data) != hash {
        return Err(storage_error());
    }
    Ok(data.into())
}

pub(super) async fn resolve_source(
    store: &UsageStore,
    key: &Key,
    payload: &mut Value,
) -> Result<Option<Vec<Option<String>>>, HttpResult> {
    let Value::Object(batch) = payload else {
        return Ok(None);
    };
    let input = match (batch.get("inputConfig"), batch.get("input_config")) {
        (Some(_), Some(_)) => return Err(invalid("批任务请只设置一种输入字段拼写")),
        (Some(_), None) => "inputConfig",
        (None, Some(_)) => "input_config",
        _ => return Ok(None),
    };
    let Some(config) = batch[input].as_object() else {
        return Ok(None);
    };
    let Some(file) = config.get("fileName").or_else(|| config.get("file_name")) else {
        return Ok(None);
    };
    let name = file
        .as_str()
        .filter(|name| name.strip_prefix("files/").is_some_and(valid_id))
        .filter(|_| config.len() == 1)
        .ok_or_else(|| invalid("批文件输入须使用网关文件名"))?;
    let bytes = contents(store, key, name).await?;
    let mut requests = Vec::new();
    let mut keys = Vec::new();
    let mut unique = std::collections::HashSet::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let mut row = serde_json::from_slice::<Map<String, Value>>(line)
            .map_err(|_| invalid("批文件存在无效 JSONL 行"))?;
        let entry_key = match row.remove("key") {
            Some(Value::String(key))
                if !key.is_empty() && key.len() <= 255 && unique.insert(key.clone()) =>
            {
                Some(key)
            }
            None => None,
            _ => return Err(invalid("批文件条目 Key 无效或重复")),
        };
        let request = if let Some(request) = row.remove("request") {
            if !row.is_empty() {
                return Err(invalid("批文件条目含有不支持的字段"));
            }
            request
        } else {
            Value::Object(row)
        };
        requests.push(json!({"request":request}));
        keys.push(entry_key);
    }
    batch.insert(input.into(), json!({"requests":{"requests":requests}}));
    Ok(Some(keys))
}

pub(super) fn prepared_source(
    payload: &Value,
    keys: &[Option<String>],
) -> Result<FileUpload, &'static str> {
    let requests = gemini_batch_input::requests(payload).ok_or("批文件内容无效")?;
    if requests.len() != keys.len() {
        return Err("批文件条目数不一致");
    }
    let mut encoded = Vec::new();
    for (item, key) in requests.iter().zip(keys) {
        let row = match key {
            Some(key) => json!({"key":key,"request":item["request"]}),
            None => item["request"].clone(),
        };
        serde_json::to_writer(&mut encoded, &row).map_err(|_| "无法编码批文件")?;
        encoded.push(b'\n');
        if encoded.len() > LIMIT {
            return Err("提交的批文件超出上限");
        }
    }
    FileUpload::from_bytes(encoded, "batch.jsonl")
        .and_then(|file| file.with_mime_type("application/jsonl"))
        .map_err(|_| "无法构造上游批文件")
}
