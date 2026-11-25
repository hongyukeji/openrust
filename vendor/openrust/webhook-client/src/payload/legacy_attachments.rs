use base64::{Engine as _, engine::general_purpose::STANDARD};
use openrust::http::{UploadedFile, UploadedFileValue};
use serde::Deserialize;
use serde_json::Value;

use crate::{Error, Result};

const MAX_FILES: usize = 128;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyFile<'a> {
    original_name: &'a str,
    mime_type: Option<&'a str>,
    size: Option<usize>,
    error: u8,
    content: &'a str,
}

pub(crate) fn restore_legacy_attachments(
    attachments: Option<&Value>,
    payload: Option<&Value>,
) -> Result<UploadedFileValue> {
    let value = attachments
        .filter(|value| !value.is_null())
        .or_else(|| payload.and_then(|value| value.get("attachments")));
    let mut remaining = MAX_BYTES;
    let files = match value {
        None | Some(Value::Null) => Default::default(),
        Some(Value::Array(files)) if files.len() <= MAX_FILES => files
            .iter()
            .enumerate()
            .map(|(index, file)| Ok((index.to_string(), restore_file(file, &mut remaining)?)))
            .collect::<Result<_>>()?,
        Some(Value::Object(files)) if files.len() <= MAX_FILES => files
            .iter()
            .map(|(name, file)| {
                if !valid_label(name) {
                    return Err(Error::InvalidRecord);
                }
                Ok((name.clone(), restore_file(file, &mut remaining)?))
            })
            .collect::<Result<_>>()?,
        _ => return Err(Error::InvalidRecord),
    };
    Ok(UploadedFileValue::Files(files))
}

fn restore_file(value: &Value, remaining: &mut usize) -> Result<UploadedFileValue> {
    let file = LegacyFile::deserialize(value).map_err(|_| Error::InvalidRecord)?;
    if !valid_label(file.original_name)
        || file.original_name.contains(['/', '\\'])
        || matches!(file.original_name, "." | "..")
        || file.mime_type.is_some_and(|mime| !valid_label(mime))
        || file.error != 0
        || file.size.is_some_and(|size| size > *remaining)
        || file.content.len() > (*remaining).div_ceil(3) * 4
    {
        return Err(Error::InvalidRecord);
    }

    let bytes = STANDARD
        .decode(file.content)
        .map_err(|_| Error::InvalidRecord)?;
    if file.size.is_some_and(|size| size != bytes.len()) {
        return Err(Error::InvalidRecord);
    }
    *remaining = remaining
        .checked_sub(bytes.len())
        .ok_or(Error::InvalidRecord)?;
    Ok(UploadedFileValue::File(UploadedFile::from_bytes(
        file.original_name.into(),
        file.mime_type.map(str::to_owned),
        bytes.into(),
    )))
}

fn valid_label(value: &str) -> bool {
    !value.is_empty() && value.len() <= 255 && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enforces_limits_and_does_not_read_embedded_paths() {
        let file = json!({
            "originalName": "old.bin", "content": "AA==", "error": 0, "size": 1,
            "path": "/nonexistent-test-upload"
        });
        assert!(
            restore_legacy_attachments(Some(&json!(vec![file.clone(); MAX_FILES])), None).is_ok()
        );
        assert!(matches!(
            restore_legacy_attachments(Some(&json!(vec![file.clone(); MAX_FILES + 1])), None),
            Err(Error::InvalidRecord)
        ));
        for (field, value) in [
            ("originalName", json!("../old.bin")),
            ("error", json!(1)),
            ("size", json!(2)),
            ("mimeType", json!("text/\r\nplain")),
        ] {
            let mut bad = file.clone();
            bad[field] = value;
            assert!(matches!(
                restore_legacy_attachments(Some(&json!([bad])), None),
                Err(Error::InvalidRecord)
            ));
        }

        let mut large = file.clone();
        large["content"] = json!(STANDARD.encode(vec![0; MAX_BYTES]));
        large["size"] = json!(MAX_BYTES);
        assert!(restore_legacy_attachments(Some(&json!([large.clone()])), None).is_ok());
        assert!(matches!(
            restore_legacy_attachments(Some(&json!([large, file])), None),
            Err(Error::InvalidRecord)
        ));
    }
}
