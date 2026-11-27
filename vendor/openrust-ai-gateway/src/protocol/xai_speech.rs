use http::{HeaderValue, header};
use openrust::http::{Body, Response, StatusCode, client};
use openrust_ai::{AudioFormat, AudioResponse, TextEndpoint};
use serde_json::{Map, Value};

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

pub(super) fn validate(fields: &Map<String, Value>) -> Result<(AudioFormat, i64), &'static str> {
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
                | "replace"
                | "with_timestamps"
        )
    }) || !matches!(
        fields.get("with_timestamps"),
        None | Some(Value::Bool(false))
    ) {
        return Err("语音合成只支持二进制音频响应及原生合成字段");
    }
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
        return Ok((AudioFormat::Mp3, characters));
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
        None => Ok(AudioFormat::Mp3),
        Some(codec) => match codec.as_str() {
            Some("mp3") => Ok(AudioFormat::Mp3),
            Some("wav") => Ok(AudioFormat::Wav),
            Some("pcm") => Ok(AudioFormat::Pcm),
            _ => Err("语音合成只支持 MP3、WAV 和 PCM 音频"),
        },
    }?;
    Ok((codec, characters))
}

pub(super) async fn respond(
    upstream: client::Response,
    format: AudioFormat,
    mut call: Call,
) -> HttpResult {
    let has_expected_type = upstream
        .header("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case(format.mime_type()));
    let audio = if has_expected_type {
        upstream
            .body()
            .ok()
            .filter(|bytes| AudioResponse::from_bytes(bytes.clone(), format, "", "").is_ok())
    } else {
        None
    };
    let Some(audio) = audio else {
        call.outcome("invalid_upstream_response");
        call.settle().await;
        call.release().await;
        return Protocol::OpenAi.error(
            StatusCode::BAD_GATEWAY,
            "invalid_upstream_response",
            "上游未返回预期的音频",
        );
    };
    let completed = call.complete().await;
    call.release().await;
    if !completed {
        return Protocol::OpenAi.error(
            StatusCode::SERVICE_UNAVAILABLE,
            "usage_unavailable",
            "AI 网关无法持久化本次上游用量",
        );
    }
    Ok(Response::new(upstream.status(), Body::from(audio))
        .with_headers(downstream_headers(upstream.headers()))
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static(format.mime_type()),
        ))
}
