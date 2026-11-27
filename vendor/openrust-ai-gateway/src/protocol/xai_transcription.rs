use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::body::Bytes;
use futures_util::{Stream, StreamExt as _};
use http::header;
use openrust::http::{HttpError, Request, UploadedFile, client::Part};
use serde_json::{Map, Value};
use tokio::io::{AsyncSeekExt as _, AsyncWriteExt as _};

const MAX_FILE_BYTES: u64 = 500_000_000;
pub(super) const MAX_BODY_BYTES: usize = 500_000_000 + 256 * 1024;
const MAX_PREFIX_BYTES: usize = 8 * 1024;
const MAX_UNCONSUMED_BYTES: usize = 64 * 1024;
const OPTIONS: &[&str] = &[
    "model",
    "audio_format",
    "sample_rate",
    "language",
    "format",
    "multichannel",
    "channels",
    "diarize",
    "filler_words",
    "vad_threshold",
    "show_redacted_text",
];

pub(super) async fn parse(
    request: &mut Request,
    directory: &Path,
) -> Result<(Map<String, Value>, Vec<(String, Part)>), HttpError> {
    let invalid = || HttpError::InvalidRequest;
    let (mut form, unread) = multipart(request)?;
    let mut fields = Map::new();
    let mut uploads = Vec::new();
    let mut audio = None;
    let mut terms = 0;
    let mut redactions = 0;
    while let Some(field) = form.next_field().await.map_err(multipart_error)? {
        unread.store(0, Ordering::Relaxed);
        let name = field.name().ok_or_else(invalid)?.to_owned();
        if name == "file"
            && audio.is_none()
            && field.file_name().is_some_and(|name| !name.is_empty())
        {
            audio = Some(spool_audio(field, directory, &unread).await?);
            continue;
        }
        if field.file_name().is_some() {
            return Err(invalid());
        }
        let bytes = field.bytes().await.map_err(multipart_error)?;
        let value = std::str::from_utf8(&bytes).map_err(|_| invalid())?;
        match name.as_str() {
            "keyterm" if terms < 100 && (1..=50).contains(&value.chars().count()) => {
                terms += 1;
                uploads.push((name, Part::text(value.to_owned())));
            }
            "redact" if redactions < 100 && !value.is_empty() => {
                redactions += 1;
                uploads.push((name, Part::text(value.to_owned())));
            }
            option if OPTIONS.contains(&option) && !fields.contains_key(option) => {
                fields.insert(name, Value::String(value.to_owned()));
            }
            _ => return Err(invalid()),
        }
    }
    let (file, prefix, length) = audio.ok_or_else(invalid)?;
    let (extension, mime_type) = audio_format(&fields, prefix, length).ok_or_else(invalid)?;
    let file = Part::stream_with_length(file, length)
        .file_name(format!("audio.{extension}"))
        .mime_str(mime_type)
        .map_err(|_| invalid())?;
    uploads.push(("file".into(), file));
    Ok((fields, uploads))
}

pub(super) fn multipart(
    request: &mut Request,
) -> Result<(multer::Multipart<'static>, Arc<AtomicUsize>), HttpError> {
    let boundary = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .and_then(|media| multer::parse_boundary(media).ok())
        .filter(|boundary| (1..=70).contains(&boundary.len()))
        .ok_or(HttpError::InvalidRequest)?;
    let sizes = multer::SizeLimit::new()
        .per_field(1024)
        .for_field("file", MAX_FILE_BYTES);
    let unread = Arc::new(AtomicUsize::new(0));
    let form = multer::Multipart::with_constraints(
        multipart_input(request.take_streaming_body()?, unread.clone()),
        boundary,
        multer::Constraints::new().size_limit(sizes),
    );
    Ok((form, unread))
}

fn multipart_input(
    body: impl Stream<Item = Result<Bytes, HttpError>> + Send,
    unread: Arc<AtomicUsize>,
) -> impl Stream<Item = Result<Bytes, HttpError>> + Send {
    async_stream::try_stream! {
        futures_util::pin_mut!(body);
        while let Some(chunk) = body.next().await {
            let chunk = chunk?;
            for start in (0..chunk.len()).step_by(MAX_PREFIX_BYTES) {
                let part = chunk.slice(start..(start + MAX_PREFIX_BYTES).min(chunk.len()));
                let previous = unread.fetch_add(part.len(), Ordering::Relaxed);
                if previous + part.len() > MAX_UNCONSUMED_BYTES {
                    Err(HttpError::PayloadTooLarge)?;
                }
                yield part;
                // multer 会连续轮询已就绪的输入，分段让出使字段数据先写入磁盘。
                tokio::task::yield_now().await;
            }
        }
    }
}

pub(super) async fn spool_audio(
    mut field: multer::Field<'_>,
    directory: &Path,
    unread: &AtomicUsize,
) -> Result<(tokio::fs::File, Vec<u8>, u64), HttpError> {
    tokio::fs::create_dir_all(directory).await?;
    let mut file = tokio::fs::File::from_std(tempfile::tempfile_in(directory)?);
    let mut length = 0_u64;
    let mut prefix = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(multipart_error)? {
        unread.store(0, Ordering::Relaxed);
        let capture = (MAX_PREFIX_BYTES - prefix.len()).min(chunk.len());
        prefix.extend_from_slice(&chunk[..capture]);
        file.write_all(&chunk).await?;
        length += chunk.len() as u64;
    }
    if length == 0 {
        return Err(HttpError::InvalidRequest);
    }
    file.flush().await?;
    file.rewind().await?;
    Ok((file, prefix, length))
}

fn audio_format(
    fields: &Map<String, Value>,
    prefix: Vec<u8>,
    length: u64,
) -> Option<(&'static str, &'static str)> {
    if let Some(raw) = fields.get("audio_format").and_then(Value::as_str) {
        if !matches!(
            fields.get("sample_rate").and_then(Value::as_str),
            Some("8000" | "16000" | "22050" | "24000" | "44100" | "48000")
        ) {
            return None;
        }
        return match raw {
            "pcm" if length.is_multiple_of(2) => Some(("pcm", "audio/pcm")),
            "mulaw" => Some(("mulaw", "audio/basic")),
            "alaw" => Some(("alaw", "audio/alaw")),
            _ => None,
        };
    }
    audio_container(prefix)
}

pub(super) fn audio_container(prefix: Vec<u8>) -> Option<(&'static str, &'static str)> {
    let upload = UploadedFile::from_bytes(String::new(), None, prefix.into());
    match upload.extension()? {
        "aac" => Some(("aac", "audio/aac")),
        "mp3" => Some(("mp3", "audio/mpeg")),
        "m4a" => Some(("m4a", "audio/mp4")),
        "mp4" => Some(("mp4", "video/mp4")),
        "webm" => Some(("webm", "video/webm")),
        "mkv" => Some(("mkv", "video/x-matroska")),
        "mov" => Some(("mov", "video/quicktime")),
        "avi" => Some(("avi", "video/x-msvideo")),
        "ogg" | "opus" => Some(("ogg", "audio/ogg")),
        "flac" => Some(("flac", "audio/flac")),
        "wav" => Some(("wav", "audio/wav")),
        _ => None,
    }
}

pub(super) fn multipart_error(error: multer::Error) -> HttpError {
    match error {
        multer::Error::FieldSizeExceeded { .. } | multer::Error::StreamSizeExceeded { .. } => {
            HttpError::PayloadTooLarge
        }
        multer::Error::StreamReadFailed(error) => error
            .downcast::<HttpError>()
            .map_or(HttpError::InvalidRequest, |error| *error),
        _ => HttpError::InvalidRequest,
    }
}

pub(super) fn validate_response(value: &mut Value) -> Option<i64> {
    let data = value.as_object_mut()?;
    let duration_ms = duration_ms(data.get("duration")?)?;
    if !data.get("text").is_some_and(Value::is_string)
        || !valid_words(data.get("words"))
        || !data.get("channels").is_none_or(|channels| {
            channels.as_array().is_some_and(|channels| {
                channels.iter().all(|channel| {
                    channel["index"].as_u64().is_some()
                        && channel["text"].is_string()
                        && valid_words(channel.get("words"))
                })
            })
        })
    {
        return None;
    }
    data.retain(|key, _| {
        matches!(
            key.as_str(),
            "text" | "language" | "duration" | "words" | "channels" | "model"
        )
    });
    Some(duration_ms)
}

fn duration_ms(value: &Value) -> Option<i64> {
    let number = value.as_number()?.to_string();
    let (whole, fraction) = number.split_once('.').unwrap_or((&number, ""));
    let whole = whole.parse::<u64>().ok()?;
    let fraction = fraction.trim_end_matches('0');
    if fraction.len() > 3 {
        return None;
    }
    let fractional_ms = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()? * 10_u64.pow(3 - fraction.len() as u32)
    };
    whole
        .checked_mul(1000)?
        .checked_add(fractional_ms)?
        .try_into()
        .ok()
}

fn seconds(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .filter(|number| number.is_finite() && *number >= 0.0)
}

fn valid_words(value: Option<&Value>) -> bool {
    value.is_none_or(|words| {
        words.as_array().is_some_and(|words| {
            words.iter().all(|word| {
                word["text"].is_string()
                    && seconds(&word["start"])
                        .zip(seconds(&word["end"]))
                        .is_some_and(|(start, end)| start <= end)
                    && word
                        .get("speaker")
                        .is_none_or(|speaker| speaker.as_u64().is_some())
            })
        })
    })
}
