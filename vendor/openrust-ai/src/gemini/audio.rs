use std::sync::Arc;

use openrust::{http::HttpFuture, support::Str};
use serde::Deserialize;
use serde_json::{Value, json};

use super::interactions::{interact, output_blocks, reported_model, usage};

use crate::{
    AiError, AiResult, AudioFormat, AudioPrompt, AudioProvider, AudioResponse, AudioVoice,
    TranscriptionPrompt, TranscriptionProvider, TranscriptionResponse, TranscriptionSegment,
    TranscriptionUsage, connection::Connection,
};

pub(super) struct GeminiAudioProvider(pub(super) Arc<Connection>);

impl AudioProvider for GeminiAudioProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_audio_model(&self) -> AiResult<&str> {
        self.0.default_audio_model()
    }

    fn default_audio_format(&self) -> AudioFormat {
        AudioFormat::Wav
    }

    fn generate_audio<'a>(
        &'a self,
        prompt: AudioPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AudioResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            let model = super::validated_model_id(prompt.model)?;
            let voice = match prompt.voice {
                AudioVoice::Default | AudioVoice::Female => "Kore",
                AudioVoice::Male => "Puck",
                AudioVoice::Named(voice) => voice,
            };
            let input = match prompt
                .instructions
                .map(str::trim)
                .filter(|text| !text.is_empty())
            {
                Some(text) => format!("{text}\n\n{}", prompt.text),
                None => prompt.text.into(),
            };
            let format = match prompt.format {
                AudioFormat::Wav => json!({"type":"audio"}),
                AudioFormat::Pcm => json!({"type":"audio","mime_type":"audio/l16"}),
                _ => return Err(AiError::InvalidConfiguration("audio.format")),
            };
            let data = interact(
                &self.0,
                prompt.timeout,
                json!({
                    "model":model,"input":input,"store":false,"response_format":format,
                    "generation_config":{"speech_config":[{"voice":voice}]},
                }),
            )
            .await?;
            let block = output_blocks(&data)
                .find(|block| block["type"] == "audio")
                .ok_or(AiError::InvalidResponse)?;
            let base64 = block["data"].as_str().ok_or(AiError::InvalidResponse)?;
            let mut content = Str::from_base64(base64).map_err(|_| AiError::InvalidResponse)?;
            let is_wav = content.starts_with(b"RIFF") && content.get(8..12) == Some(b"WAVE");
            if prompt.format == AudioFormat::Wav && !is_wav {
                if block["mime_type"]
                    .as_str()
                    .is_some_and(|mime| !is_pcm_mime(mime))
                {
                    return Err(AiError::InvalidResponse);
                }
                let rate = block["sample_rate"].as_u64().unwrap_or(24_000);
                let channels = block["channels"].as_u64().unwrap_or(1);
                content = pcm_to_wav(&content, rate, channels)?;
            } else if prompt.format == AudioFormat::Pcm
                && (is_wav
                    || block["mime_type"]
                        .as_str()
                        .is_some_and(|mime| !is_pcm_mime(mime)))
            {
                return Err(AiError::InvalidResponse);
            }
            let mut response = AudioResponse::from_bytes(
                content,
                prompt.format,
                self.0.name(),
                reported_model(&data, model),
            )
            .map_err(|_| AiError::InvalidResponse)?;
            response.usage = usage(&data);
            Ok(response)
        })
    }
}

impl TranscriptionProvider for GeminiAudioProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_transcription_model(&self) -> AiResult<&str> {
        self.0.default_transcription_model()
    }

    fn generate_transcription<'a>(
        &'a self,
        prompt: TranscriptionPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<TranscriptionResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            let model = super::validated_model_id(prompt.model)?;
            let audio = json!({
                "type":"audio","mime_type":prompt.audio.mime_type(),"data":Str::to_base64(prompt.audio.content())
            });
            let native = model.contains("transcribe");
            let body = if native {
                native_request(model, audio, &prompt)
            } else {
                prompted_request(model, audio, &prompt)
            };
            let response = interact(&self.0, prompt.timeout, body).await?;
            let mut blocks = output_blocks(&response).filter_map(|block| block["text"].as_str());
            let mut text = blocks.next().ok_or(AiError::InvalidResponse)?.to_owned();
            for block in blocks {
                text.push_str(block);
            }
            let (text, segments) = match (native, prompt.diarize) {
                (true, _) => (text, native_segments(&response)?),
                (false, true) => parse_timed_transcript(&text)?,
                (false, false) => (text, Vec::new()),
            };
            Ok(TranscriptionResponse {
                text: text.trim().into(),
                segments,
                language: response["language"].as_str().map(str::to_owned),
                provider: self.0.name().into(),
                model: reported_model(&response, model).into(),
                usage: TranscriptionUsage {
                    tokens: usage(&response),
                    audio_input_tokens: response["usage"]["input_tokens_by_modality"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .find(|detail| {
                            detail["modality"]
                                .as_str()
                                .is_some_and(|kind| kind.eq_ignore_ascii_case("audio"))
                        })
                        .and_then(|detail| detail["tokens"].as_u64()),
                    seconds: None,
                },
            })
        })
    }
}

fn native_request(model: &str, audio: Value, prompt: &TranscriptionPrompt<'_>) -> Value {
    let mut body = json!({"model":model,"input":[audio],"store":false});
    let mut config = serde_json::Map::new();
    if let Some(language) = prompt.language {
        config.insert("language_codes".into(), json!([language]));
    }
    if prompt.diarize {
        config.insert(
            "mode".into(),
            json!({
                "type":"verbatim","diarization_mode":"speaker","timestamp_granularities":["word"]
            }),
        );
    }
    if !config.is_empty() {
        body["generation_config"] = json!({"transcription_config":config});
    }
    body
}

fn prompted_request(model: &str, audio: Value, prompt: &TranscriptionPrompt<'_>) -> Value {
    let text = match (prompt.diarize, prompt.language) {
        (true, Some(language)) => format!(
            "Transcribe this audio with timestamps in {language}. Return the full transcript and timed segments."
        ),
        (true, None) => {
            "Transcribe this audio with timestamps. Return the full transcript and timed segments."
                .into()
        }
        (false, Some(language)) => {
            format!("Transcribe this audio. Output only the transcription in {language}.")
        }
        (false, None) => "Transcribe this audio. Output only the transcription text.".into(),
    };
    let mut body = json!({"model":model,"input":[{"type":"text","text":text},audio],"store":false});
    if prompt.diarize {
        body["input"][0]["text"] = format!(
            "{text} Use MM:SS or HH:MM:SS timestamps, with optional fractional seconds, for start_time and end_time."
        ).into();
        body["response_format"] = json!({
            "type":"text","mime_type":"application/json",
            "schema":{"type":"object","required":["transcript","segments"],"properties":{
                "transcript":{"type":"string"},"segments":{"type":"array","items":{
                    "type":"object","required":["text","start_time","end_time"],"properties":{
                        "text":{"type":"string"},"start_time":{"type":"string"},"end_time":{"type":"string"}
                    }
                }}
            }}
        });
    }
    body
}

fn native_segments(data: &Value) -> AiResult<Vec<TranscriptionSegment>> {
    let mut segments: Vec<TranscriptionSegment> = Vec::new();
    for word in output_blocks(data)
        .filter_map(|block| block["annotations"].as_array())
        .flatten()
        .filter(|annotation| annotation["type"] == "word_info")
    {
        let text = word["text"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .trim();
        let speaker = word["speaker"].as_str().filter(|name| !name.is_empty());
        let start = timestamp(
            word["start_offset"]
                .as_str()
                .ok_or(AiError::InvalidResponse)?,
        )?;
        let end = timestamp(
            word["end_offset"]
                .as_str()
                .ok_or(AiError::InvalidResponse)?,
        )?;
        if end < start {
            return Err(AiError::InvalidResponse);
        }
        if let Some(last) = segments
            .last_mut()
            .filter(|segment| segment.speaker.as_deref() == speaker)
        {
            if !last.text.is_empty() && !text.is_empty() {
                last.text.push(' ');
            }
            last.text.push_str(text);
            last.end = last.end.max(end);
        } else {
            segments.push(TranscriptionSegment {
                text: text.into(),
                speaker: speaker.map(str::to_owned),
                start,
                end,
            });
        }
    }
    Ok(segments)
}

fn parse_timed_transcript(text: &str) -> AiResult<(String, Vec<TranscriptionSegment>)> {
    #[derive(Deserialize)]
    struct Transcript {
        transcript: String,
        segments: Vec<Segment>,
    }
    #[derive(Deserialize)]
    struct Segment {
        text: String,
        start_time: String,
        end_time: String,
    }

    let payload: Transcript = serde_json::from_str(text).map_err(|_| AiError::InvalidResponse)?;
    let segments = payload
        .segments
        .into_iter()
        .map(|segment| {
            let start = timestamp(&segment.start_time)?;
            let end = timestamp(&segment.end_time)?;
            if end < start {
                return Err(AiError::InvalidResponse);
            }
            Ok(TranscriptionSegment {
                start,
                end,
                text: segment.text,
                speaker: None,
            })
        })
        .collect::<AiResult<_>>()?;
    Ok((payload.transcript, segments))
}

fn timestamp(value: &str) -> AiResult<f64> {
    let value = value.trim().trim_end_matches('s').replace(',', ".");
    let mut parts = value.rsplit(':');
    let mut seconds: f64 = parts
        .next()
        .and_then(|part| part.parse().ok())
        .ok_or(AiError::InvalidResponse)?;
    for (offset, part) in parts.enumerate() {
        let unit = [60.0, 3600.0].get(offset).ok_or(AiError::InvalidResponse)?;
        let count: u32 = part.parse().map_err(|_| AiError::InvalidResponse)?;
        seconds += f64::from(count) * unit;
    }
    if seconds.is_finite() && seconds >= 0.0 {
        Ok(seconds)
    } else {
        Err(AiError::InvalidResponse)
    }
}

fn is_pcm_mime(mime: &str) -> bool {
    mime.split(';')
        .next()
        .is_some_and(|name| name.trim().eq_ignore_ascii_case("audio/l16"))
}

fn pcm_to_wav(samples: &[u8], rate: u64, channels: u64) -> AiResult<Vec<u8>> {
    let rate = u32::try_from(rate).map_err(|_| AiError::InvalidResponse)?;
    let channels = u16::try_from(channels).map_err(|_| AiError::InvalidResponse)?;
    let data_len = u32::try_from(samples.len()).map_err(|_| AiError::InvalidResponse)?;
    let block = channels
        .checked_mul(2)
        .filter(|block| *block != 0)
        .ok_or(AiError::InvalidResponse)?;
    let byte_rate = rate
        .checked_mul(u32::from(block))
        .filter(|rate| *rate != 0)
        .ok_or(AiError::InvalidResponse)?;
    let riff_len = data_len.checked_add(36).ok_or(AiError::InvalidResponse)?;
    if samples.is_empty() || !samples.len().is_multiple_of(usize::from(block)) {
        return Err(AiError::InvalidResponse);
    }
    let mut wav = Vec::with_capacity(samples.len() + 44);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&riff_len.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(samples);
    Ok(wav)
}
