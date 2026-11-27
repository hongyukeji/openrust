use futures_util::stream;
use http::header;
use openrust::http::{HttpError, Request, UploadedFile, client::Part};
use openrust_ai::TranscriptionAudio;
use serde_json::{Map, Value};

const MAX_FILE_BYTES: usize = 25_000_000;
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
) -> Result<(Map<String, Value>, Vec<(String, Part)>), HttpError> {
    let invalid = || HttpError::InvalidRequest;
    let boundary = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .and_then(|media| multer::parse_boundary(media).ok())
        .ok_or_else(invalid)?;
    let data = request.body_bytes().await?;
    let mut form = multer::Multipart::new(
        stream::once(async move { Ok::<_, std::io::Error>(data) }),
        boundary,
    );
    let mut fields = Map::new();
    let mut uploads = Vec::new();
    let mut audio = None;
    let mut terms = 0;
    let mut redactions = 0;
    while let Some(field) = form.next_field().await.map_err(|_| invalid())? {
        let name = field.name().ok_or_else(invalid)?.to_owned();
        if name == "file"
            && audio.is_none()
            && field.file_name().is_some_and(|name| !name.is_empty())
        {
            let bytes = field.bytes().await.map_err(|_| invalid())?;
            if bytes.is_empty() || bytes.len() >= MAX_FILE_BYTES {
                return Err(invalid());
            }
            audio = Some(bytes);
            continue;
        }
        if field.file_name().is_some() {
            return Err(invalid());
        }
        let bytes = field.bytes().await.map_err(|_| invalid())?;
        if bytes.len() > 1024 {
            return Err(invalid());
        }
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
    let audio = audio.ok_or_else(invalid)?;
    let (extension, mime_type) =
        if let Some(raw) = fields.get("audio_format").and_then(Value::as_str) {
            if !matches!(
                fields.get("sample_rate").and_then(Value::as_str),
                Some("8000" | "16000" | "22050" | "24000" | "44100" | "48000")
            ) {
                return Err(invalid());
            }
            match raw {
                "pcm" if audio.len().is_multiple_of(2) => ("pcm", "audio/pcm"),
                "mulaw" => ("mulaw", "audio/basic"),
                "alaw" => ("alaw", "audio/alaw"),
                _ => return Err(invalid()),
            }
        } else {
            match TranscriptionAudio::from_bytes(audio.clone()) {
                Ok(file) => {
                    let extension = match file.mime_type() {
                        "audio/mpeg" => "mp3",
                        "audio/mp4" => "m4a",
                        "video/mp4" => "mp4",
                        "video/webm" => "webm",
                        "audio/ogg" => "ogg",
                        "audio/flac" => "flac",
                        "audio/wav" => "wav",
                        _ => return Err(invalid()),
                    };
                    (extension, file.mime_type())
                }
                Err(_)
                    if UploadedFile::from_bytes(String::new(), None, audio.clone()).extension()
                        == Some("aac") =>
                {
                    ("aac", "audio/aac")
                }
                Err(_) => return Err(invalid()),
            }
        };
    let size = audio.len() as u64;
    let file = Part::stream_with_length(audio, size)
        .file_name(format!("audio.{extension}"))
        .mime_str(mime_type)
        .map_err(|_| invalid())?;
    uploads.push(("file".into(), file));
    Ok((fields, uploads))
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
