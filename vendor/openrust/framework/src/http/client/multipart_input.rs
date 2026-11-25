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
        let body = scalar(&value)?;
        let mut headers = HeaderMap::new();
        add_content_length(&body, &mut headers);
        let field = Part::text(body).headers(headers);
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
    add_content_length(&body, &mut headers);
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

fn add_content_length(body: &str, headers: &mut HeaderMap) {
    if !body.is_empty() {
        headers
            .entry(header::CONTENT_LENGTH)
            .or_insert_with(|| HeaderValue::from(body.len() as u64));
    }
}

fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) if value.is_f64() => {
            Ok(php_float(value.as_f64().expect("JSON 浮点值")))
        }
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(true) => Ok("1".into()),
        Value::Bool(false) | Value::Null => Ok(String::new()),
        _ => Err(Error::InvalidRequest),
    }
}

fn php_float(value: f64) -> String {
    const PRECISION: i32 = 14;

    // 固定 PHP 默认精度。先舍入有效位，再用舍入后的指数选择表示法。
    let fractional_digits = (PRECISION - 1) as usize;
    let scientific = format!("{value:.fractional_digits$e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("有限浮点数有指数");
    let exponent: i32 = exponent.parse().expect("浮点指数为整数");
    if (-4..PRECISION).contains(&exponent) {
        let plain = format!("{value:.*}", (PRECISION - 1 - exponent) as usize);
        if plain.contains('.') {
            return plain.trim_end_matches('0').trim_end_matches('.').to_owned();
        }
        return plain;
    }

    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
    let fractional_zero = if mantissa.contains('.') { "" } else { ".0" };
    format!("{mantissa}{fractional_zero}E{exponent:+}")
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
