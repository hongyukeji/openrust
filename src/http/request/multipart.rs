//! 使用 Axum 官方 multipart 提取器构建 PHP 形态的文本与文件 bags。

use std::{collections::HashMap, sync::Arc};

use axum::{
    body::{Body, Bytes},
    extract::{FromRequest, Multipart},
    http::{Extensions, HeaderValue, StatusCode, header},
};
use serde_json::Value;
use tokio::io::AsyncReadExt;

use super::{
    MimeTypes, UploadedFile, form,
    raw_body::{RawBody, RequestBodyLimits, TempWriter},
};
use crate::{Error, Result, support::DataValue};

pub(super) async fn parse(
    content_type: Option<HeaderValue>,
    extensions: Option<Extensions>,
    raw: &RawBody,
    limits: RequestBodyLimits,
    mime_types: Arc<MimeTypes>,
) -> Result<(Value, DataValue)> {
    let reader = raw.open().await?;
    let body = Body::from_stream(futures_util::stream::unfold(
        Some(reader),
        |reader| async move {
            let mut reader = reader?;
            let mut buffer = vec![0_u8; 64 * 1024];
            match reader.read(&mut buffer).await {
                Ok(0) => None,
                Ok(size) => {
                    buffer.truncate(size);
                    Some((
                        Ok::<Bytes, std::io::Error>(Bytes::from(buffer)),
                        Some(reader),
                    ))
                }
                Err(error) => Some((Err(error), None)),
            }
        },
    ));
    let mut request = axum::extract::Request::new(body);
    if let Some(extensions) = extensions {
        *request.extensions_mut() = extensions;
    }
    if let Some(content_type) = content_type {
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    let mut multipart = Multipart::from_request(request, &())
        .await
        .map_err(|_| invalid_multipart())?;
    let mut fields = Vec::new();
    let mut files = Vec::new();
    let mut received = HashMap::new();
    let mut count = 0usize;
    let mut text_bytes = 0usize;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| invalid_multipart())?
    {
        count += 1;
        if count > limits.max_fields {
            return Err(limit());
        }
        let header_bytes = field
            .headers()
            .iter()
            .try_fold(0_usize, |used, (name, value)| {
                used.checked_add(name.as_str().len())?
                    .checked_add(value.as_bytes().len())
            })
            .ok_or_else(limit)?;
        if header_bytes > limits.max_part_header_bytes {
            return Err(limit());
        }
        let Some(name) = field.name().map(str::to_owned) else {
            continue;
        };
        let client_name = field.file_name().map(str::to_owned);
        let client_mime = field.content_type().map(str::to_owned);
        if let Some(client_name) = client_name {
            let marker = format!("__openrust_uploaded_file_{count}__");
            let file = if client_name.is_empty() {
                // 即使是空文件字段，也要完整消费本 part 才能读取后续字段。
                consume(field, limits.max_file_bytes).await?;
                None
            } else {
                Some(
                    receive_file(
                        field,
                        client_name,
                        client_mime,
                        limits.max_file_bytes,
                        Arc::clone(&mime_types),
                    )
                    .await?,
                )
            };
            received.insert(marker.clone(), file);
            files.push((name, marker));
        } else {
            let bytes = receive_text(field, limits.max_field_bytes).await?;
            text_bytes = text_bytes.checked_add(bytes.len()).ok_or_else(limit)?;
            if text_bytes > limits.max_decoded_bytes {
                return Err(limit());
            }
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| invalid_encoding())?
                .to_owned();
            fields.push((name, text));
        }
    }
    let text = form::parse_form(&encoded(&fields))?;
    let file_shape = form::parse_form(&encoded(&files))?;
    let files = file_tree(file_shape, &received)?;
    files.validate()?;
    Ok((text, files))
}

async fn receive_file(
    mut field: axum::extract::multipart::Field<'_>,
    client_name: String,
    client_mime: Option<String>,
    max: usize,
    mime_types: Arc<MimeTypes>,
) -> Result<UploadedFile> {
    let mut writer = TempWriter::create().await.map_err(|_| temp_error())?;
    let mut total = 0usize;
    while let Some(chunk) = field.chunk().await.map_err(|_| invalid_multipart())? {
        total = total.checked_add(chunk.len()).ok_or_else(limit)?;
        if total > max {
            return Err(limit());
        }
        writer = writer
            .write(chunk.to_vec())
            .await
            .map_err(|_| temp_error())?;
    }
    let path = writer.finish();
    UploadedFile::from_temp(path, total, &client_name, client_mime, mime_types)
}

async fn receive_text(
    mut field: axum::extract::multipart::Field<'_>,
    max: usize,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| invalid_multipart())? {
        let next = output.len().checked_add(chunk.len()).ok_or_else(limit)?;
        if next > max {
            return Err(limit());
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output)
}

async fn consume(mut field: axum::extract::multipart::Field<'_>, max: usize) -> Result<()> {
    let mut count = 0usize;
    while let Some(chunk) = field.chunk().await.map_err(|_| invalid_multipart())? {
        count = count.checked_add(chunk.len()).ok_or_else(limit)?;
        if count > max {
            return Err(limit());
        }
    }
    Ok(())
}

fn temp_error() -> Error {
    Error::custom("uploaded_file_unavailable", "上传文件暂存不可写入")
}

fn encoded(fields: &[(String, String)]) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().map(|(name, value)| (name, value)))
        .finish()
}

fn file_tree(value: Value, files: &HashMap<String, Option<UploadedFile>>) -> Result<DataValue> {
    match value {
        Value::String(marker) => match files.get(&marker) {
            Some(Some(file)) => Ok(DataValue::object(file.clone())),
            Some(None) => Ok(DataValue::Null),
            None => Err(invalid_multipart()),
        },
        Value::Array(values) => Ok(DataValue::Array(
            values
                .into_iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    let child = file_tree(value, files);
                    match child {
                        Ok(child) if is_file_bag_truthy(&child) => Some(
                            i64::try_from(index)
                                .map(|index| (crate::support::ArrayKey::Integer(index), child))
                                .map_err(|_| limit()),
                        ),
                        Ok(_) => None,
                        Err(error) => Some(Err(error)),
                    }
                })
                .collect::<Result<Vec<_>>>()?,
        )),
        Value::Object(values) => Ok(DataValue::Array(
            values
                .into_iter()
                .map(|(key, value)| {
                    Ok((
                        crate::support::ArrayKey::from(key.as_str()),
                        file_tree(value, files)?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?,
        )),
        Value::Null => Ok(DataValue::Null),
        _ => Err(invalid_multipart()),
    }
}

fn is_file_bag_truthy(value: &DataValue) -> bool {
    match value {
        DataValue::Null => false,
        DataValue::Array(entries) => !entries.is_empty(),
        _ => true,
    }
}

fn invalid_multipart() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "multipart_invalid",
        "multipart 正文无法解析",
    )
}
fn invalid_encoding() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "input_encoding_invalid",
        "文本字段必须使用 UTF-8",
    )
}
fn limit() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "multipart 字段数量超过限制",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn aggregate_text_budget_rejects_multiple_small_fields() -> Result<()> {
        let body = concat!(
            "--budget\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nabc\r\n",
            "--budget\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\ndef\r\n",
            "--budget--\r\n"
        );
        let limits = RequestBodyLimits {
            max_decoded_bytes: 4,
            ..RequestBodyLimits::default()
        };
        let raw = RawBody::capture(Body::from(body), limits).await?;
        let Err(error) = parse(
            Some(HeaderValue::from_static(
                "multipart/form-data; boundary=budget",
            )),
            None,
            &raw,
            limits,
            Arc::new(MimeTypes::new()),
        )
        .await
        else {
            return Err(limit());
        };
        assert_eq!(error.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.code(), "input_limits_exceeded");
        Ok(())
    }
}
