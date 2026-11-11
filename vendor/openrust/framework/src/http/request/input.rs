use axum::body::Bytes;
use futures_util::{StreamExt as _, stream};
use serde_json::{Map, Value};

use crate::http::{Body, HttpError, Method, Request, UploadedFile};

use super::{query, variables::VariableBag};

const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_FIELD_BYTES: usize = 1024 * 1024;
const MAX_FIELDS: usize = 1024;

#[derive(Clone, Copy)]
pub(super) enum InputFailure {
    Invalid,
    Limit,
}

impl From<InputFailure> for HttpError {
    fn from(error: InputFailure) -> Self {
        match error {
            InputFailure::Invalid => Self::InvalidRequest,
            InputFailure::Limit => Self::PayloadTooLarge,
        }
    }
}

impl From<HttpError> for InputFailure {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::PayloadTooLarge => Self::Limit,
            _ => Self::Invalid,
        }
    }
}

#[derive(Default)]
pub(super) enum RawBody {
    #[default]
    Unread,
    Ready(Bytes),
    Failed(InputFailure),
}

pub(super) struct Payload {
    fields: Value,
    file_indices: Value,
    files: Vec<UploadedFile>,
}

impl Payload {
    fn fields(fields: Value) -> Self {
        Self {
            fields,
            file_indices: Value::Array(Vec::new()),
            files: Vec::new(),
        }
    }
}

impl Request {
    /// Reads the input source and adds query keys that the source does not supply.
    /// A dotted key can select nested objects or arrays.
    pub async fn input(&mut self, key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let query = self.query(None)?.unwrap_or_else(empty);
        let fields = &self.payload().await?.fields;
        let Some(key) = key else {
            return Ok(Some(union(fields.clone(), query)));
        };
        let segments: Vec<_> = key.split('.').collect();
        if segments[0] == "*" {
            return Ok(lookup(&union(fields.clone(), query), &segments));
        }
        if child(fields, segments[0]).is_some() {
            Ok(lookup(fields, &segments))
        } else {
            Ok(lookup(&query, &segments))
        }
    }

    /// Reads the JSON body independently of query parameters.
    pub async fn json(&mut self, key: Option<&str>) -> Result<Option<Value>, HttpError> {
        if self.json_input.is_none() {
            let bytes = self.body_bytes().await?;
            self.json_input = Some(parse_json(&bytes));
        }
        let value = self.json_input.as_ref().expect("JSON input initialized");
        let value = value.as_ref().map_err(|error| HttpError::from(*error))?;
        Ok(match key {
            Some(key) => lookup(value, &key.split('.').collect::<Vec<_>>()),
            None => Some(value.clone()),
        })
    }

    /// Returns a multipart upload by field name or dotted array index.
    pub async fn file(&mut self, key: &str) -> Result<Option<UploadedFile>, HttpError> {
        let payload = self.payload().await?;
        let mut value = &payload.file_indices;
        for segment in key.split('.') {
            let Some(field) = child(value, segment) else {
                return Ok(None);
            };
            value = field;
        }
        Ok(value
            .as_str()
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| payload.files.get(index))
            .cloned())
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub async fn hasFile(&mut self, key: &str) -> Result<bool, HttpError> {
        let payload = self.payload().await?;
        let mut value = &payload.file_indices;
        for segment in key.split('.') {
            let Some(field) = child(value, segment) else {
                return Ok(false);
            };
            value = field;
        }
        Ok(contains_file(value))
    }

    /// Returns a bounded copy-on-write view of the original body bytes.
    pub async fn body_bytes(&mut self) -> Result<Bytes, HttpError> {
        match &self.raw_input {
            RawBody::Ready(bytes) => return Ok(bytes.clone()),
            RawBody::Failed(error) => return Err((*error).into()),
            RawBody::Unread => {}
        }

        let declared_size = self
            .headers()
            .get(::http::header::CONTENT_LENGTH)
            .and_then(|header| header.to_str().ok())
            .and_then(|header| header.parse::<u64>().ok());
        if declared_size.is_some_and(|size| size > MAX_BODY_BYTES as u64) {
            self.raw_input = RawBody::Failed(InputFailure::Limit);
            return Err(HttpError::PayloadTooLarge);
        }

        // Leave an explicit error if the collection future is cancelled.
        self.raw_input = RawBody::Failed(InputFailure::Invalid);
        let result = collect(self.take_body()).await;
        match result {
            Ok(bytes) => {
                self.body = Body::from(bytes.clone());
                self.raw_input = RawBody::Ready(bytes.clone());
                Ok(bytes)
            }
            Err(error) => {
                self.raw_input = RawBody::Failed(error);
                Err(error.into())
            }
        }
    }

    async fn payload(&mut self) -> Result<&Payload, HttpError> {
        if self.payload_input.is_none() {
            let parsed = self.parse_payload().await;
            self.payload_input = Some(parsed);
        }
        self.payload_input
            .as_ref()
            .expect("payload initialized")
            .as_ref()
            .map_err(|error| (*error).into())
    }

    async fn parse_payload(&mut self) -> Result<Payload, InputFailure> {
        let content_type = self
            .headers()
            .get(::http::header::CONTENT_TYPE)
            .map(|header| header.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| InputFailure::Invalid)?
            .unwrap_or_default();
        let media = content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if media.ends_with("/json") || media.ends_with("+json") {
            return self
                .json(None)
                .await
                .map(|value| Payload::fields(value.unwrap_or_else(empty)))
                .map_err(Into::into);
        }
        if matches!(*self.method(), Method::GET | Method::HEAD) {
            return Ok(Payload::fields(empty()));
        }
        match media.as_str() {
            "application/x-www-form-urlencoded" => {
                let bytes = self.body_bytes().await?;
                let raw = std::str::from_utf8(&bytes).map_err(|_| InputFailure::Invalid)?;
                let fields = query::parse_form(raw)?;
                Ok(Payload::fields(fields))
            }
            "multipart/form-data" => {
                let bytes = self.body_bytes().await?;
                let boundary = multer::parse_boundary(&content_type).map_err(multipart_error)?;
                parse_multipart(bytes, &boundary).await
            }
            _ => Ok(Payload::fields(empty())),
        }
    }
}

async fn collect(body: Body) -> Result<Bytes, InputFailure> {
    let mut stream = body.into_data_stream();
    let mut first = Bytes::new();
    let mut combined = None;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| InputFailure::Invalid)?;
        let current = combined.as_ref().map_or(first.len(), Vec::len);
        if chunk.len() > MAX_BODY_BYTES.saturating_sub(current) {
            return Err(InputFailure::Limit);
        }
        if first.is_empty() && combined.is_none() {
            first = chunk;
        } else {
            combined
                .get_or_insert_with(|| first.to_vec())
                .extend_from_slice(&chunk);
        }
    }
    Ok(combined.map_or(first, Bytes::from))
}

fn parse_json(raw: &[u8]) -> Result<Value, InputFailure> {
    if raw.iter().all(u8::is_ascii_whitespace) {
        return Ok(empty());
    }
    match serde_json::from_slice(raw).map_err(|_| InputFailure::Invalid)? {
        value @ (Value::Array(_) | Value::Object(_)) => Ok(value),
        Value::Null => Ok(empty()),
        _ => Err(InputFailure::Invalid),
    }
}

async fn parse_multipart(bytes: Bytes, boundary: &str) -> Result<Payload, InputFailure> {
    let stream = stream::once(async move { Ok::<_, std::io::Error>(bytes) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut fields = VariableBag::default();
    let mut file_indices = VariableBag::default();
    let mut files = Vec::new();
    let mut count = 0;
    while let Some(field) = multipart.next_field().await.map_err(multipart_error)? {
        count += 1;
        if count > MAX_FIELDS {
            return Err(InputFailure::Limit);
        }
        let name = field.name().ok_or(InputFailure::Invalid)?.to_owned();
        let filename = field.file_name().map(str::to_owned);
        let media = field.content_type().map(ToString::to_string);
        let bytes = field.bytes().await.map_err(multipart_error)?;
        if let Some(filename) = filename {
            if !filename.is_empty() {
                file_indices.insert(&name, files.len().to_string())?;
                files.push(UploadedFile::new(filename, media, bytes));
            }
        } else {
            if bytes.len() > MAX_TEXT_FIELD_BYTES {
                return Err(InputFailure::Limit);
            }
            let value = String::from_utf8(bytes.to_vec()).map_err(|_| InputFailure::Invalid)?;
            fields.insert(&name, value)?;
        }
    }
    Ok(Payload {
        fields: fields.into_value(),
        file_indices: file_indices.into_value(),
        files,
    })
}

fn multipart_error(error: multer::Error) -> InputFailure {
    match error {
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => {
            InputFailure::Limit
        }
        _ => InputFailure::Invalid,
    }
}

fn empty() -> Value {
    Value::Array(Vec::new())
}

fn child<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    match value {
        Value::Object(fields) => fields.get(key),
        Value::Array(fields) => key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == key)
            .and_then(|index| fields.get(index)),
        _ => None,
    }
}

fn lookup(value: &Value, path: &[&str]) -> Option<Value> {
    let Some((key, tail)) = path.split_first() else {
        return Some(value.clone());
    };
    if *key != "*" {
        return lookup(child(value, key)?, tail);
    }
    let values = value
        .as_array()
        .into_iter()
        .flatten()
        .chain(value.as_object().into_iter().flat_map(Map::values));
    if !(value.is_array() || value.is_object()) {
        return None;
    }
    let mut output = Vec::new();
    for value in values {
        match lookup(value, tail) {
            Some(Value::Array(values)) if tail.contains(&"*") => output.extend(values),
            Some(value) => output.push(value),
            None if !tail.contains(&"*") => output.push(Value::Null),
            _ => {}
        }
    }
    Some(Value::Array(output))
}

fn union(body: Value, query: Value) -> Value {
    match (body, query) {
        (Value::Array(mut body), Value::Array(mut query)) => {
            if query.len() > body.len() {
                body.extend(query.drain(body.len()..));
            }
            Value::Array(body)
        }
        (body, query) => {
            let mut merged = as_fields(body);
            for (key, value) in as_fields(query) {
                merged.entry(key).or_insert(value);
            }
            Value::Object(merged)
        }
    }
}

fn as_fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        Value::Array(fields) => fields
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        _ => Map::new(),
    }
}

fn contains_file(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Array(fields) => fields.iter().any(contains_file),
        Value::Object(fields) => fields.values().any(contains_file),
        _ => false,
    }
}
