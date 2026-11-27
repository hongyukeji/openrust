use std::io;

use axum::body::Bytes;
use futures_util::StreamExt as _;
use http::{HeaderValue, header};
use openrust::{
    http::{Response, StatusCode, client},
    support::Str,
};
use openrust_ai::{AudioFormat, AudioResponse, TextEndpoint};
use serde_json::{Map, Value, json};

use super::{HttpResult, Protocol, downstream_headers};
use crate::gateway::{Call, Gateway, Key};

const BUILTIN_VOICES: &[&str] = &[
    "ara", "eve", "leo", "rex", "sal", "altair", "atlas", "aurora", "carina", "castor", "celeste",
    "cosmo", "helios", "helix", "iris", "kepler", "liora", "lumen", "luna", "lux", "naksh",
    "orion", "perseus", "rigel", "sirius", "ursa", "zagan", "zenith",
];

pub(super) fn implicit_model<'a>(
    gateway: &'a Gateway,
    key: &Key,
    endpoint: TextEndpoint,
) -> Result<&'a str, (StatusCode, &'static str, &'static str)> {
    let mut allowed = gateway
        .models
        .iter()
        .filter(|(name, model)| key.allows_model(name) && model.supports_for(endpoint, key));
    let Some((name, _)) = allowed.next() else {
        return Err((StatusCode::NOT_FOUND, "model_not_found", "模型不可用"));
    };
    if allowed.next().is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            "missing_model",
            "多个可用语音模型需要指定公开模型名称",
        ));
    }
    Ok(name)
}

#[derive(Clone, Copy)]
pub(super) struct Reply {
    codec: Codec,
    timestamps: bool,
}

#[derive(Clone, Copy)]
enum Codec {
    Audio(AudioFormat),
    MuLaw,
    ALaw,
}

impl Codec {
    fn mime(self) -> &'static str {
        match self {
            Self::Audio(format) => format.mime_type(),
            Self::MuLaw => "audio/basic",
            Self::ALaw => "audio/alaw",
        }
    }

    fn prefix_len(self) -> usize {
        match self {
            Self::Audio(AudioFormat::Wav) => 12,
            Self::Audio(AudioFormat::Mp3) => 3,
            Self::Audio(AudioFormat::Pcm) => 2,
            _ => 1,
        }
    }

    fn valid(self, bytes: Bytes) -> bool {
        match self {
            Self::Audio(format) => AudioResponse::from_bytes(bytes, format, "", "").is_ok(),
            _ => !bytes.is_empty(),
        }
    }
}

pub(super) fn validate(fields: &Map<String, Value>) -> Result<(Reply, i64), &'static str> {
    if fields.keys().any(|field| {
        !matches!(
            field.as_str(),
            "model"
                | "text"
                | "language"
                | "voice_id"
                | "output_format"
                | "speed"
                | "normalize"
                | "text_normalization"
                | "optimize_streaming_latency"
                | "replace"
                | "with_timestamps"
        )
    }) {
        return Err("语音合成只接收原生合成字段");
    }
    let timestamps = match fields.get("with_timestamps") {
        None => false,
        Some(Value::Bool(enabled)) => *enabled,
        _ => return Err("with_timestamps 须为布尔值"),
    };
    let characters = fields
        .get("text")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .and_then(|text| i64::try_from(text.chars().count()).ok())
        .filter(|characters| *characters <= 60_000)
        .ok_or("语音合成需要有效的正文和语言")?;
    if !fields
        .get("language")
        .and_then(Value::as_str)
        .is_some_and(|language| {
            !language.is_empty()
                && language.len() <= 35
                && language
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || value == b'-')
        })
    {
        return Err("语音合成需要有效的正文和语言");
    }
    // 共享凭证不能读取其他调用方的自定义音色。
    if fields.get("voice_id").is_some_and(|voice| {
        voice.as_str().is_none_or(|voice| {
            !BUILTIN_VOICES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(voice))
        })
    }) {
        return Err("共享语音连接仅支持 xAI 内置音色");
    }
    let Some(format) = fields.get("output_format") else {
        let codec = Codec::Audio(AudioFormat::Mp3);
        return Ok((Reply { codec, timestamps }, characters));
    };
    let format = format
        .as_object()
        .filter(|format| {
            format
                .keys()
                .all(|key| matches!(key.as_str(), "codec" | "sample_rate" | "bit_rate"))
        })
        .ok_or("无效的音频格式配置")?;
    let codec = match format.get("codec") {
        None => Codec::Audio(AudioFormat::Mp3),
        Some(value) => match value.as_str() {
            Some("mp3") => Codec::Audio(AudioFormat::Mp3),
            Some("wav") => Codec::Audio(AudioFormat::Wav),
            Some("pcm") => Codec::Audio(AudioFormat::Pcm),
            Some("mulaw") => Codec::MuLaw,
            Some("alaw") => Codec::ALaw,
            _ => return Err("不支持的语音合成编码"),
        },
    };
    Ok((Reply { codec, timestamps }, characters))
}

pub(super) async fn respond(
    upstream: client::Response,
    reply: Reply,
    mut call: Call,
) -> HttpResult {
    let status = upstream.status();
    let headers = downstream_headers(upstream.headers());
    let expected_type = if reply.timestamps {
        "application/json"
    } else {
        reply.codec.mime()
    };
    if !upstream
        .header("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| mime_matches(value, expected_type))
    {
        return reject(call).await;
    }
    if reply.timestamps {
        let Some(data) = upstream
            .json::<Value>()
            .ok()
            .and_then(|data| timed_audio(data, reply.codec))
        else {
            return reject(call).await;
        };
        let complete = call.complete().await;
        call.release().await;
        return if complete {
            Response::json(status, data).map(|response| response.with_headers(headers))
        } else {
            Protocol::OpenAi.error(
                StatusCode::SERVICE_UNAVAILABLE,
                "usage_unavailable",
                "AI 网关无法持久化本次上游用量",
            )
        };
    }

    let Ok(mut upstream) = upstream.into_stream() else {
        return reject(call).await;
    };
    let mut prefix = Vec::new();
    while prefix.len() < reply.codec.prefix_len() {
        let Some(Ok(chunk)) = upstream.next().await else {
            return reject(call).await;
        };
        prefix.extend_from_slice(&chunk);
    }
    let prefix = Bytes::from(prefix);
    if !reply.codec.valid(prefix.slice(..reply.codec.prefix_len())) {
        return reject(call).await;
    }
    let minimum_size = if matches!(reply.codec, Codec::Audio(AudioFormat::Wav)) {
        let size = u32::from_le_bytes([prefix[4], prefix[5], prefix[6], prefix[7]]);
        // 部分流式 WAV 在文件头使用占位长度。
        if !matches!(size, 0 | u32::MAX) {
            u64::from(size) + 8
        } else {
            0
        }
    } else {
        0
    };
    let stream = async_stream::try_stream! {
        let mut size = prefix.len() as u64;
        yield prefix;
        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    call.outcome("incomplete_stream");
                    call.settle().await;
                    Err(io::Error::new(io::ErrorKind::UnexpectedEof, "上游音频流未完成"))?
                }
            };
            size += chunk.len() as u64;
            yield chunk;
        }
        if size < minimum_size
            || (matches!(reply.codec, Codec::Audio(AudioFormat::Pcm)) && !size.is_multiple_of(2))
        {
            call.outcome("incomplete_stream");
            call.settle().await;
            Err(io::Error::new(io::ErrorKind::UnexpectedEof, "上游音频流未完成"))?;
        }
        if !call.complete().await {
            Err(io::Error::other("AI 网关无法持久化本次上游用量"))?;
        }
        call.release().await;
    };
    Ok(Response::stream::<Bytes, io::Error>(status, stream)
        .with_headers(headers)
        .header(
            http::HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(reply.codec.mime()),
        ))
}

fn mime_matches(value: &str, expected: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(expected))
}

fn timed_audio(data: Value, codec: Codec) -> Option<Value> {
    let audio = data["audio"].as_str()?;
    let decoded = Str::from_base64(audio).ok()?;
    if !codec.valid(Bytes::from(decoded))
        || !data["content_type"]
            .as_str()
            .is_some_and(|mime| mime_matches(mime, codec.mime()))
    {
        return None;
    }
    let duration = data["duration"]
        .as_f64()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)?;
    let timing = &data["audio_timestamps"];
    let characters = timing["graph_chars"].as_array()?;
    let times = timing["graph_times"].as_array()?;
    if characters.is_empty()
        || times.len() != characters.len()
        || characters
            .iter()
            .any(|char| char.as_str().is_none_or(str::is_empty))
        || !times.iter().all(|span| {
            let Some([start, end]) = span.as_array().map(Vec::as_slice) else {
                return false;
            };
            start
                .as_f64()
                .zip(end.as_f64())
                .is_some_and(|(start, end)| start >= 0.0 && end >= start && end.is_finite())
        })
    {
        return None;
    }
    Some(json!({
        "audio": audio, "content_type": codec.mime(), "duration": duration,
        "audio_timestamps": {"graph_chars":characters,"graph_times":times}
    }))
}

async fn reject(mut call: Call) -> HttpResult {
    call.outcome("invalid_upstream_response");
    call.settle().await;
    call.release().await;
    Protocol::OpenAi.error(
        StatusCode::BAD_GATEWAY,
        "invalid_upstream_response",
        "上游未返回预期的音频",
    )
}
