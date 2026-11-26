use http::header;
use openrust::http::{HttpError, Request, UploadedFile, UploadedFileValue, client::Part};
use openrust_ai::ImageAttachment;
use serde_json::{Map, Value};

const MAX_IMAGES: usize = 16;

pub(super) fn is_multipart(request: &Request) -> bool {
    request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case("multipart/form-data"))
}

pub(super) async fn parse(
    request: &mut Request,
) -> Result<(Map<String, Value>, Vec<(String, Part)>), HttpError> {
    let Some(fields) = request.raw_form_fields().await?.as_object().cloned() else {
        return Err(HttpError::InvalidRequest);
    };
    let UploadedFileValue::Files(mut uploads) = request.all_files().await? else {
        return Err(HttpError::InvalidRequest);
    };
    let images = match uploads.shift_remove("image") {
        Some(UploadedFileValue::File(file)) => vec![file],
        Some(UploadedFileValue::Files(files)) if (1..=MAX_IMAGES).contains(&files.len()) => files
            .into_values()
            .map(|file| match file {
                UploadedFileValue::File(file) => Ok(file),
                _ => Err(HttpError::InvalidRequest),
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(HttpError::InvalidRequest),
    };
    let mask = match uploads.shift_remove("mask") {
        Some(UploadedFileValue::File(file)) => Some(file),
        None => None,
        _ => return Err(HttpError::InvalidRequest),
    };
    if !uploads.is_empty()
        || ["image", "images", "mask"]
            .iter()
            .any(|name| fields.contains_key(*name))
        || fields.values().any(|value| !value.is_string())
    {
        return Err(HttpError::InvalidRequest);
    }
    let key = if images.len() == 1 {
        "image"
    } else {
        "image[]"
    };
    let mut parts = Vec::with_capacity(images.len() + usize::from(mask.is_some()));
    for (index, image) in images.into_iter().enumerate() {
        parts.push((key.into(), image_part(&image, &format!("image-{index}"))?));
    }
    if let Some(mask) = mask {
        parts.push(("mask".into(), image_part(&mask, "mask")?));
    }
    Ok((fields, parts))
}

fn image_part(file: &UploadedFile, stem: &str) -> Result<Part, HttpError> {
    let image = ImageAttachment::from_upload(file).map_err(|_| HttpError::InvalidRequest)?;
    Part::stream_with_length(file.get(), file.get_size() as u64)
        .file_name(format!("{stem}.{}", image.format().extension()))
        .mime_str(image.mime())
        .map_err(|_| HttpError::InvalidRequest)
}
