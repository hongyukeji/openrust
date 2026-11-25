use http::{HeaderMap, HeaderName, HeaderValue, header};
use serde_json::Value;

use super::{Error, Part, Result};

pub(super) fn parts(data: Value) -> Result<Vec<(String, Part, Value)>> {
    match data {
        Value::Object(fields) => fields
            .into_iter()
            .map(|(name, value)| part(name, value))
            .collect(),
        Value::Array(fields) => fields
            .into_iter()
            .map(|value| match value {
                Value::Object(fields) => descriptor(fields),
                _ => Err(Error::InvalidRequest),
            })
            .collect(),
        _ => Err(Error::InvalidRequest),
    }
}

fn part(name: String, value: Value) -> Result<(String, Part, Value)> {
    let Value::Object(fields) = value else {
        let field = Part::text(scalar(&value)?);
        return Ok((name, field, value));
    };
    descriptor(fields)
}

fn descriptor(mut fields: serde_json::Map<String, Value>) -> Result<(String, Part, Value)> {
    let Some(Value::String(name)) = fields.remove("name") else {
        return Err(Error::InvalidRequest);
    };
    let contents = fields.remove("contents").ok_or(Error::InvalidRequest)?;
    if contents.is_null() {
        return Err(Error::InvalidRequest);
    }
    let body = scalar(&contents)?;
    let filename = match fields.remove("filename") {
        Some(Value::String(name)) if !name.is_empty() => Some(name),
        Some(Value::String(_) | Value::Null) | None => None,
        _ => return Err(Error::InvalidRequest),
    };
    let mut headers = part_headers(fields.remove("headers"))?;
    if !body.is_empty() && !headers.contains_key(header::CONTENT_LENGTH) {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from(body.len() as u64));
    }
    let disposition = headers.remove(header::CONTENT_DISPOSITION);
    let mut field = Part::text(body);
    if let Some(filename) = filename {
        if !headers.contains_key(header::CONTENT_TYPE) {
            let mime = mime_guess::from_path(&filename).first_or_octet_stream();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime.as_ref()).map_err(|_| Error::InvalidRequest)?,
            );
        }
        let filename = filename
            .trim_end_matches(std::path::is_separator)
            .rsplit(std::path::is_separator)
            .next()
            .unwrap_or_default()
            .to_owned();
        field = field.file_name(filename);
    }
    if let Some(disposition) = disposition {
        field = field.with_content_disposition(disposition);
    }
    Ok((name, field.headers(headers), contents))
}

fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(true) => Ok("1".into()),
        Value::Bool(false) | Value::Null => Ok(String::new()),
        _ => Err(Error::InvalidRequest),
    }
}

fn part_headers(value: Option<Value>) -> Result<HeaderMap> {
    match value {
        None | Some(Value::Null) => Ok(HeaderMap::new()),
        Some(Value::Array(values)) if values.is_empty() => Ok(HeaderMap::new()),
        Some(Value::Object(fields)) => fields
            .into_iter()
            .map(|(name, value)| {
                let name = HeaderName::try_from(name).map_err(|_| Error::InvalidRequest)?;
                let value =
                    HeaderValue::from_str(&scalar(&value)?).map_err(|_| Error::InvalidRequest)?;
                Ok((name, value))
            })
            .collect(),
        _ => Err(Error::InvalidRequest),
    }
}
