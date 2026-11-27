use std::io::Error;

use axum::body::Bytes;
use futures_util::{Stream, StreamExt};
use http::{HeaderValue, header};
use serde_json::Value;

use super::*;

const MAX_LINE_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn local_file(batch: &Batch) -> String {
    format!("files/{}", batch.id.to_ascii_lowercase())
}

pub(super) fn upstream_file(job: &Value) -> Option<&str> {
    let successful = match job["metadata"]["state"].as_str() {
        Some("JOB_STATE_SUCCEEDED" | "JOB_STATE_PARTIALLY_SUCCEEDED" | "BATCH_STATE_SUCCEEDED") => {
            true
        }
        None => job["done"] == true,
        _ => false,
    };
    if !job["error"].is_null() || !successful {
        return None;
    }
    let mut name = None;
    for path in ["/response", "/metadata/output"] {
        for field in ["responsesFile", "responses_file"] {
            if let Some(value) = job.pointer(path).and_then(|output| output.get(field)) {
                let value = value.as_str()?;
                let id = value.strip_prefix("files/")?;
                if id.is_empty()
                    || id.len() > 40
                    || id.starts_with('-')
                    || id.ends_with('-')
                    || !id.bytes().all(|byte| {
                        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                    })
                    || name.is_some_and(|name| name != value)
                {
                    return None;
                }
                name = Some(value);
            }
        }
    }
    name
}

pub(super) async fn serve(request: &mut Request) -> HttpResult {
    let gateway = Gateway::get_facade_root()?;
    let key = match authorize(&gateway, PROTOCOL.token(request), PROTOCOL).await {
        Ok(key) => key,
        Err(error) => return error,
    };
    let Some(id) = request
        .route("id")
        .and_then(|id| id.strip_suffix(":download"))
    else {
        return not_found();
    };
    let normalized = id.to_ascii_uppercase();
    if !valid_id(&normalized) || id != normalized.to_ascii_lowercase() {
        return not_found();
    }
    if !matches!(request.query(Some("alt")), Ok(Some(Value::String(alt))) if alt == "media") {
        return invalid("批任务结果下载需要 alt=media");
    }
    let Some(store) = &gateway.usage_store else {
        return storage_error();
    };
    let batch = match store.gemini_batch(&owner(&key), &normalized).await {
        Ok(Some(batch)) => batch,
        Ok(None) => return not_found(),
        Err(_) => return storage_error(),
    };
    let Some(target) = target_for(&gateway, &key, &batch) else {
        return not_found();
    };
    if let Some(response) = admit(&key, PROTOCOL).await? {
        return Ok(response);
    }
    let job = match fetch(&batch, target).await {
        Ok(job) => job,
        Err(error) => return error,
    };
    let Some(file) = upstream_file(&job) else {
        return not_found();
    };
    let stream = match result_stream(store, &batch, target, file).await {
        Ok(stream) => stream,
        Err(error) => return error,
    };
    Ok(Response::stream(StatusCode::OK, stream)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/jsonl"),
        )
        .header(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-store"),
        ))
}

pub(super) async fn result_stream(
    store: &UsageStore,
    batch: &Batch,
    target: &Target,
    file: &str,
) -> Result<impl Stream<Item = Result<Bytes, Error>> + use<>, HttpResult> {
    let provider = target
        .provider
        .gemini_batch_gateway()
        .expect("已验证批任务连接");
    let response = provider
        .send_batch(GeminiBatchRequest::Download { file })
        .await
        .map_err(driver_error)?;
    if !response.ok() {
        return Err(upstream_status(response.status()));
    }
    let mut body = response.into_stream().map_err(|_| malformed())?;
    let batch = batch.clone();
    let store = store.clone();
    Ok(async_stream::try_stream! {
        let mut rows = JsonLines::new();
        let mut buffer = Vec::new();
        let mut last = None;
        while let Some(chunk) = body.next().await {
            let chunk = chunk.map_err(|_| invalid_file())?;
            for segment in chunk.split_inclusive(|byte| *byte == b'\n') {
                if segment.len() > MAX_LINE_BYTES.saturating_sub(buffer.len()) {
                    Err(invalid_file())?;
                }
                buffer.extend_from_slice(segment);
                if segment.ends_with(b"\n") {
                    let row = rows.read(&batch, &buffer)?;
                    buffer.clear();
                    if let Some(previous) = last.replace(row) {
                        yield previous;
                    }
                }
            }
        }
        if !buffer.is_empty() {
            let row = rows.read(&batch, &buffer)?;
            if let Some(previous) = last.replace(row) {
                yield previous;
            }
        }
        if rows.count != batch.item_count {
            Err(invalid_file())?;
        }
        if let Some(settlement) = rows.usage.settlement(&batch) {
            store.settle_gemini_batch(&batch, settlement, false).await
                .map_err(|_| Error::other("批任务结果用量写入失败"))?;
        }
        if let Some(last) = last {
            yield last;
        }
    })
}

struct JsonLines {
    count: i32,
    usage: BatchUsage,
}

impl JsonLines {
    fn new() -> Self {
        Self {
            count: 0,
            usage: BatchUsage::new(),
        }
    }

    fn read(&mut self, batch: &Batch, line: &[u8]) -> Result<Bytes, Error> {
        if self.count >= batch.item_count {
            return Err(invalid_file());
        }
        let mut row: Value = serde_json::from_slice(line).map_err(|_| invalid_file())?;
        if row["response"].is_object() && row["error"].is_null() && row["status"].is_null() {
            self.usage.observe(batch, &row["response"]);
            rename_model(&mut row["response"], &batch.model);
        } else if row["response"].is_null() && row["error"].is_object() && row["status"].is_null() {
            self.usage.partial = true;
            redact_error(&mut row["error"]);
        } else if row["response"].is_null() && row["error"].is_null() && row["status"].is_object() {
            self.usage.partial = true;
            redact_error(&mut row["status"]);
        } else if row["candidates"].is_array()
            || row["embedding"].is_object()
            || row["usageMetadata"].is_object()
        {
            self.usage.observe(batch, &row);
            rename_model(&mut row, &batch.model);
        } else if row["code"].is_i64() {
            self.usage.partial = true;
            let key = row.get("key").cloned();
            redact_error(&mut row);
            if let Some(key) = key {
                row["key"] = key;
            }
        } else {
            return Err(invalid_file());
        }
        self.count += 1;
        let mut row = serde_json::to_vec(&row).map_err(|_| invalid_file())?;
        row.push(b'\n');
        Ok(Bytes::from(row))
    }
}

fn invalid_file() -> Error {
    Error::other("批任务结果文件无效")
}
