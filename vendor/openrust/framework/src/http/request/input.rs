use axum::body::Bytes;
use futures_util::{StreamExt as _, stream};
use serde_json::{Map, Value};

use crate::http::{Body, HttpError, Method, Request, UploadedFile, UploadedFileValue};
use crate::support::{
    Arr, BackedEnum, Collection, EnumBacking as _, InputDate, Stringable, calendar, data_get,
    facades::Config, frozen_utc_now, input_scalars,
};

use super::{query, variables::VariableBag};

mod mutation;

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

#[derive(Clone, Copy)]
enum InputSource {
    Json,
    Query,
    Form,
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
    pub(crate) async fn apply_method_override(&mut self) -> Result<(), HttpError> {
        if self.method() != Method::POST {
            return Ok(());
        }

        let header = self
            .headers()
            .get("x-http-method-override")
            .map(|header| header.to_str().map_err(|_| HttpError::InvalidRequest))
            .transpose()?
            .filter(|method| !matches!(*method, "" | "0"));
        let method = if let Some(header) = header {
            header.to_ascii_uppercase()
        } else {
            let media = self
                .headers()
                .get(::http::header::CONTENT_TYPE)
                .and_then(|header| header.to_str().ok())
                .and_then(|header| header.split(';').next())
                .unwrap_or_default()
                .trim();
            let field = if media.eq_ignore_ascii_case("application/x-www-form-urlencoded")
                || media.eq_ignore_ascii_case("multipart/form-data")
            {
                self.payload().await?.fields.get("_method").cloned()
            } else {
                None
            };
            let field = match field {
                Some(field) => Some(field),
                None => self.query(Some("_method"))?,
            };
            let Some(field) = field else {
                return Ok(());
            };
            field
                .as_str()
                .ok_or(HttpError::InvalidRequest)?
                .to_ascii_uppercase()
        };

        if matches!(
            method.as_str(),
            "GET" | "HEAD" | "CONNECT" | "TRACE" | "POST"
        ) {
            return Ok(());
        }
        if method.is_empty() || !method.bytes().all(|byte| byte.is_ascii_uppercase()) {
            return Err(HttpError::InvalidRequest);
        }
        let method =
            Method::from_bytes(method.as_bytes()).map_err(|_| HttpError::InvalidRequest)?;
        self.real_method = Some(std::mem::replace(&mut self.parts.method, method));
        Ok(())
    }

    /// Reads the input source and adds query keys that the source does not supply.
    /// A dotted key can select nested objects or arrays.
    pub async fn input(&mut self, key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let source = self.input_source()?;
        if matches!(source, InputSource::Query) {
            let query = self.query_values()?;
            return Ok(match key {
                Some(key) => lookup(query, &key.split('.').collect::<Vec<_>>()),
                None => Some(query.clone()),
            });
        }
        let query = self.query(None)?.unwrap_or_else(empty);
        let fields = self.source_values(source).await?;
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

    /// Reads a signed 64-bit input, using `default` for missing or invalid strict values.
    pub async fn integer(
        &mut self,
        key: &str,
        default: i64,
        strict: bool,
    ) -> Result<i64, HttpError> {
        match self.input(Some(key)).await? {
            Some(value) if !strict || input_scalars::valid_integer(&value) => {
                Ok(input_scalars::integer(&value))
            }
            _ => Ok(default),
        }
    }

    /// Reads a floating-point input, using `default` for missing or invalid strict values.
    pub async fn float(&mut self, key: &str, default: f64, strict: bool) -> Result<f64, HttpError> {
        match self.input(Some(key)).await? {
            Some(value) if !strict || input_scalars::valid_float(&value) => {
                Ok(input_scalars::float(&value))
            }
            _ => Ok(default),
        }
    }

    /// Reads Laravel's boolean tokens, with a fallback for invalid strict values.
    pub async fn boolean(
        &mut self,
        key: &str,
        default: bool,
        strict: bool,
    ) -> Result<bool, HttpError> {
        match self.input(Some(key)).await? {
            Some(value) => Ok(input_scalars::boolean(&value, default, strict)),
            None => Ok(default),
        }
    }

    /// Reads input as a fluent string.
    pub async fn string(
        &mut self,
        key: &str,
        default: &str,
        strict: bool,
    ) -> Result<Stringable, HttpError> {
        let value = self.input(Some(key)).await?;
        Ok(Stringable::from(match value {
            Some(value) if !strict || value.is_string() || value.is_number() => {
                input_scalars::string(value)
            }
            _ => default.to_owned(),
        }))
    }

    /// Alias for [`Self::string`].
    pub async fn str(
        &mut self,
        key: &str,
        default: &str,
        strict: bool,
    ) -> Result<Stringable, HttpError> {
        self.string(key, default, strict).await
    }

    /// Reads a keyed input as a PHP-style array, or the entire input bag when omitted.
    pub async fn array(&mut self, key: Option<&str>) -> Result<Value, HttpError> {
        Ok(match self.input(key).await? {
            None | Some(Value::Null) => Value::Array(Vec::new()),
            Some(value @ (Value::Array(_) | Value::Object(_))) => value,
            Some(value) => Value::Array(vec![value]),
        })
    }

    /// Wraps array input in a keyed collection.
    pub async fn collect(&mut self, key: Option<&str>) -> Result<Collection, HttpError> {
        Ok(input_collection(self.array(key).await?))
    }

    /// Reads a backed enum, returning `default` for empty or unrecognized values.
    pub async fn r#enum<E: BackedEnum>(
        &mut self,
        key: &str,
        default: Option<E>,
    ) -> Result<Option<E>, HttpError> {
        let Some(value) = self
            .input(Some(key))
            .await?
            .filter(|value| !input_is_empty(value))
        else {
            return Ok(default);
        };
        Ok(E::try_from_backing(E::Backing::from_input(&value)?).or(default))
    }

    /// Reads backed enum cases while preserving their original collection keys.
    pub async fn enums<E: BackedEnum>(&mut self, key: &str) -> Result<Collection<E>, HttpError> {
        let Some(value) = self
            .input(Some(key))
            .await?
            .filter(|value| !input_is_empty(value))
        else {
            return Ok(Collection::new());
        };
        let mut cases = Collection::new();
        for (key, value) in input_collection(value).iter() {
            if let Some(case) = E::try_from_backing(E::Backing::from_input(value)?) {
                cases.put(key.clone(), case);
            }
        }
        Ok(cases)
    }

    /// Parses an absolute or relative date, optionally using a PHP format and explicit time zone.
    pub async fn date(
        &mut self,
        key: &str,
        format: Option<&str>,
        timezone: Option<&str>,
    ) -> Result<Option<InputDate>, HttpError> {
        let Some(value) = self
            .input(Some(key))
            .await?
            .filter(|value| !input_is_empty(value))
        else {
            return Ok(None);
        };
        let configured;
        let timezone = match timezone {
            Some(timezone) => timezone,
            None => {
                configured = Config::get("app.timezone").map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
                match &configured {
                    None => "UTC",
                    Some(value) => value.as_str().ok_or_else(|| HttpError::Internal {
                        source: Box::new(crate::support::DateError::InvalidTimezone),
                    })?,
                }
            }
        };
        let reference = frozen_utc_now().map_or_else(
            || calendar::DateReference::Wall(chrono::Utc::now()),
            calendar::DateReference::Frozen,
        );
        calendar::parse_input_date(value, format, timezone, reference)
            .map(Some)
            .map_err(|error| match error {
                calendar::CalendarError::LimitExceeded => HttpError::PayloadTooLarge,
                _ => HttpError::InvalidRequest,
            })
    }

    /// Reads a calendar interval from an ISO or human expression, or a value in the selected unit.
    pub async fn interval(
        &mut self,
        key: &str,
        unit: Option<&str>,
    ) -> Result<Option<crate::support::InputInterval>, HttpError> {
        let value = self.input(Some(key)).await?;
        crate::support::InputInterval::parse(value.as_ref(), unit).map_err(|error| match error {
            crate::support::IntervalError::Limit => HttpError::PayloadTooLarge,
            _ => HttpError::InvalidRequest,
        })
    }

    /// Selects named input paths, omitting missing keys and preserving explicit null.
    pub async fn only(&mut self, keys: &[&str]) -> Result<Value, HttpError> {
        let input = self.input(None).await?.unwrap_or_else(empty);
        let mut selected = Value::Array(Vec::new());
        for key in keys {
            if let Some(value) = data_get(&input, Some(key)) {
                Arr::set(&mut selected, key, value).map_err(|_| HttpError::InvalidRequest)?;
            }
        }
        Ok(selected)
    }

    /// Removes named paths from a copy of the input bag.
    pub async fn except(&mut self, keys: &[&str]) -> Result<Value, HttpError> {
        let input = self.input(None).await?.unwrap_or_else(empty);
        Arr::except(&input, keys).map_err(|_| HttpError::InvalidRequest)
    }

    /// Converts only the selected input paths to an array.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's naming for multi-key input methods"
    )]
    pub async fn arrayOnly(&mut self, keys: &[&str]) -> Result<Value, HttpError> {
        self.only(keys).await
    }

    /// Converts only the selected input paths to a keyed collection.
    #[allow(
        non_snake_case,
        reason = "Matches Laravel's naming for multi-key input methods"
    )]
    pub async fn collectOnly(&mut self, keys: &[&str]) -> Result<Collection, HttpError> {
        Ok(input_collection(self.only(keys).await?))
    }

    /// Reads the JSON body independently of query parameters.
    pub async fn json(&mut self, key: Option<&str>) -> Result<Option<Value>, HttpError> {
        let value = self.json_values().await?;
        Ok(match key {
            Some(key) => lookup(value, &key.split('.').collect::<Vec<_>>()),
            None => Some(value.clone()),
        })
    }

    async fn json_values(&mut self) -> Result<&Value, HttpError> {
        if self.json_input.is_none() {
            let bytes = self.body_bytes().await?;
            self.json_input = Some(parse_json(&bytes));
        }
        let value = self.json_input.as_ref().expect("JSON input initialized");
        value.as_ref().map_err(|error| HttpError::from(*error))
    }

    pub(crate) async fn validation_uploads(
        &mut self,
    ) -> Result<(&Value, &[UploadedFile]), HttpError> {
        let payload = self.payload().await?;
        Ok((&payload.file_indices, &payload.files))
    }

    /// Returns a multipart upload by field name or dotted array index.
    pub async fn file(&mut self, key: &str) -> Result<Option<UploadedFile>, HttpError> {
        let payload = self.payload().await?;
        Ok(data_get(&payload.file_indices, Some(key))
            .as_ref()
            .and_then(|value| payload.upload(value))
            .cloned())
    }

    /// Returns all uploads with their original field keys and groups.
    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub async fn allFiles(&mut self) -> Result<UploadedFileValue, HttpError> {
        let payload = self.payload().await?;
        Ok(payload.upload_value(&payload.file_indices))
    }

    /// Resolves a file or group by dotted path, including wildcard selections.
    #[allow(
        non_snake_case,
        reason = "Typed counterpart to Laravel's grouped file lookup"
    )]
    pub async fn fileValue(&mut self, key: &str) -> Result<Option<UploadedFileValue>, HttpError> {
        let payload = self.payload().await?;
        Ok(data_get(&payload.file_indices, Some(key)).map(|value| payload.upload_value(&value)))
    }

    /// Creates a lazy image from the multipart field, if present.
    pub async fn image(&mut self, key: &str) -> Result<Option<crate::image::Image>, HttpError> {
        self.file(key)
            .await?
            .map(|file| {
                crate::image::ImageFacade::fromUpload(&file).map_err(|error| HttpError::Internal {
                    source: Box::new(error),
                })
            })
            .transpose()
    }

    #[allow(non_snake_case, reason = "Matches Laravel's Request API")]
    pub async fn hasFile(&mut self, key: &str) -> Result<bool, HttpError> {
        let payload = self.payload().await?;
        let Some(value) = data_get(&payload.file_indices, Some(key)) else {
            return Ok(false);
        };
        let is_file = |value| payload.upload(value).is_some();
        Ok(match &value {
            Value::Array(fields) => fields.iter().any(is_file),
            Value::Object(fields) => fields.values().any(is_file),
            _ => is_file(&value),
        })
    }

    /// Returns a bounded copy-on-write view of the original body bytes.
    pub async fn body_bytes(&mut self) -> Result<Bytes, HttpError> {
        match &self.raw_input {
            RawBody::Ready(bytes) => return Ok(bytes.clone()),
            RawBody::Failed(error) => return Err((*error).into()),
            RawBody::Unread => {}
        }

        if self.declared_body_exceeds_limit() {
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

    pub(crate) fn declared_body_exceeds_limit(&self) -> bool {
        self.headers()
            .get(::http::header::CONTENT_LENGTH)
            .and_then(|header| header.to_str().ok())
            .and_then(|header| header.parse::<u64>().ok())
            .is_some_and(|size| size > MAX_BODY_BYTES as u64)
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

    fn input_source(&self) -> Result<InputSource, HttpError> {
        let media = self.media_type()?;
        Ok(if is_json_media(&media) {
            InputSource::Json
        } else if matches!(*self.getRealMethod(), Method::GET | Method::HEAD) {
            InputSource::Query
        } else {
            InputSource::Form
        })
    }

    async fn source_values(&mut self, source: InputSource) -> Result<&Value, HttpError> {
        match source {
            InputSource::Json => self.json_values().await,
            InputSource::Query => self.query_values(),
            InputSource::Form => {
                if self.form_input.is_none() {
                    return Ok(&self.payload().await?.fields);
                }
                Ok(self.form_input.as_ref().expect("form input installed"))
            }
        }
    }

    pub(crate) async fn transform_input(
        &mut self,
        mut transform: impl FnMut(&mut Value) -> Result<(), HttpError>,
    ) -> Result<(), HttpError> {
        let source = self.input_source()?;
        let mut query = self.query_values()?.clone();
        transform(&mut query)?;
        if !matches!(source, InputSource::Query) {
            let mut body = self.source_values(source).await?.clone();
            transform(&mut body)?;
            match source {
                InputSource::Json => self.json_input = Some(Ok(body)),
                InputSource::Form => self.form_input = Some(body),
                InputSource::Query => unreachable!("body input source"),
            }
        }
        self.query_input = Some(query);
        Ok(())
    }

    fn media_type(&self) -> Result<String, HttpError> {
        let content_type = self.header("content-type")?.unwrap_or_default();
        Ok(content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase())
    }

    async fn parse_payload(&mut self) -> Result<Payload, InputFailure> {
        let media = self.media_type()?;
        if is_json_media(&media) {
            self.json_values().await?;
            return Ok(Payload::fields(empty()));
        }
        if matches!(*self.getRealMethod(), Method::GET | Method::HEAD) {
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
                let boundary =
                    multer::parse_boundary(self.header("content-type")?.unwrap_or_default())
                        .map_err(multipart_error)?;
                parse_multipart(bytes, &boundary).await
            }
            _ => Ok(Payload::fields(empty())),
        }
    }
}

fn is_json_media(media: &str) -> bool {
    media.ends_with("/json") || media.ends_with("+json")
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
    let mut has_empty_upload = false;
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
            if filename.is_empty() {
                has_empty_upload = true;
                file_indices.insert(&name, String::new())?;
            } else {
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
    let file_indices = file_indices.into_value();
    let file_indices = if has_empty_upload {
        filter_file_indices(file_indices).unwrap_or_else(empty)
    } else {
        file_indices
    };
    Ok(Payload {
        fields: fields.into_value(),
        file_indices,
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

fn input_is_empty(value: &Value) -> bool {
    value.is_null()
        || value.as_str().is_some_and(|text| {
            text.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0'])
                .is_empty()
        })
}

fn input_collection(value: Value) -> Collection {
    match value {
        Value::Null => Collection::new(),
        Value::Object(entries) => Collection::fromEntries(entries),
        Value::Array(items) => Collection::make(items),
        scalar => Collection::make([scalar]),
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

// Like Laravel's filterFiles, discard empty uploads and groups without reindexing.
fn filter_file_indices(value: Value) -> Option<Value> {
    match value {
        Value::String(index) => (!index.is_empty()).then_some(Value::String(index)),
        Value::Array(fields) => {
            let fields: Vec<_> = fields
                .into_iter()
                .enumerate()
                .filter_map(|(index, field)| Some((index, filter_file_indices(field)?)))
                .collect();
            if fields.is_empty() {
                None
            } else if fields
                .iter()
                .enumerate()
                .all(|(offset, (index, _))| offset == *index)
            {
                Some(Value::Array(
                    fields.into_iter().map(|(_, field)| field).collect(),
                ))
            } else {
                Some(Value::Object(
                    fields
                        .into_iter()
                        .map(|(index, field)| (index.to_string(), field))
                        .collect(),
                ))
            }
        }
        Value::Object(fields) => {
            let fields: Map<_, _> = fields
                .into_iter()
                .filter_map(|(key, field)| Some((key, filter_file_indices(field)?)))
                .collect();
            (!fields.is_empty()).then_some(Value::Object(fields))
        }
        _ => None,
    }
}

impl Payload {
    fn upload(&self, value: &Value) -> Option<&UploadedFile> {
        self.files.get(value.as_str()?.parse::<usize>().ok()?)
    }

    fn upload_value(&self, value: &Value) -> UploadedFileValue {
        match value {
            Value::Array(fields) => UploadedFileValue::Files(
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| (index.to_string(), self.upload_value(field)))
                    .collect(),
            ),
            Value::Object(fields) => UploadedFileValue::Files(
                fields
                    .iter()
                    .map(|(key, field)| (key.clone(), self.upload_value(field)))
                    .collect(),
            ),
            leaf => self
                .upload(leaf)
                .cloned()
                .map_or(UploadedFileValue::Null, UploadedFileValue::File),
        }
    }
}
