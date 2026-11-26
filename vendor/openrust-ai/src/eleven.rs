use std::sync::Arc;

use openrust::http::HttpFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AgentResponse, AiError, AiResult, AudioFormat, AudioPrompt, AudioProvider, AudioResponse,
    AudioVoice, ModelCatalog, TextPrompt, TextProvider, TranscriptionPrompt, TranscriptionProvider,
    TranscriptionResponse, TranscriptionSegment, TranscriptionUsage, connection::Connection,
    models::ListedModel, text::validate,
};

#[derive(Clone)]
pub(crate) struct ElevenLabsProvider(pub(crate) Arc<Connection>);

impl TextProvider for ElevenLabsProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_text_model(&self) -> AiResult<&str> {
        Err(AiError::EndpointNotSupported)
    }

    fn generate_text<'a>(&'a self, _: TextPrompt<'a>) -> HttpFuture<'a, AiResult<AgentResponse>> {
        Box::pin(async { Err(AiError::EndpointNotSupported) })
    }

    fn audio_provider(&self) -> Option<Arc<dyn AudioProvider>> {
        Some(Arc::new(self.clone()))
    }

    fn transcription_provider(&self) -> Option<Arc<dyn TranscriptionProvider>> {
        Some(Arc::new(self.clone()))
    }

    fn models(&self) -> HttpFuture<'_, ModelCatalog> {
        Box::pin(async {
            let configured = [
                self.0.default_audio_model().ok(),
                self.0.default_transcription_model().ok(),
            ];
            ModelCatalog::merge(
                self.list_models().await,
                configured.into_iter().flatten().map(str::to_owned),
            )
        })
    }
}

impl ElevenLabsProvider {
    async fn list_models(&self) -> AiResult<Vec<ListedModel>> {
        #[derive(Deserialize)]
        struct Entry {
            model_id: String,
            name: Option<String>,
            can_do_text_to_speech: Option<bool>,
        }

        let response = validate(self.0.request(None)?.get("models").await)?;
        let entries: Vec<Entry> = response.json().map_err(|_| AiError::InvalidResponse)?;
        Ok(entries
            .into_iter()
            .map(|entry| {
                let speech = entry.can_do_text_to_speech == Some(true);
                ListedModel {
                    id: entry.model_id,
                    name: entry.name,
                    input_modalities: speech.then(|| vec!["text".into()]),
                    output_modalities: speech.then(|| vec!["audio".into()]),
                }
            })
            .collect())
    }
}

impl AudioProvider for ElevenLabsProvider {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn default_audio_model(&self) -> AiResult<&str> {
        self.0.default_audio_model()
    }

    fn generate_audio<'a>(
        &'a self,
        prompt: AudioPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AudioResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            let voice = match prompt.voice {
                AudioVoice::Default | AudioVoice::Female => "XrExE9yKIg1WjnnlVkGX",
                AudioVoice::Male => "onwK4e9ZLuTAKqWW03F9",
                AudioVoice::Named(name) => name,
            };
            if !voice
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                return Err(AiError::InvalidConfiguration("audio.voice"));
            }
            let format = match prompt.format {
                AudioFormat::Mp3 => "mp3_44100_128",
                AudioFormat::Wav => "wav_24000",
                AudioFormat::Pcm => "pcm_24000",
                AudioFormat::Opus => "opus_48000_64",
                AudioFormat::Aac | AudioFormat::Flac => {
                    return Err(AiError::InvalidConfiguration("audio.format"));
                }
            };
            let path = format!("text-to-speech/{voice}");
            let body = json!({"model_id":prompt.model,"text":prompt.text});
            let response = validate(
                self.0
                    .request(Some(prompt.timeout))?
                    .with_query_parameters([("output_format".into(), format.into())])
                    .post(&path, &body)
                    .await,
            )?;
            let content = response.body().map_err(|_| AiError::InvalidResponse)?;
            AudioResponse::from_bytes(content, prompt.format, self.0.name(), prompt.model)
                .map_err(|_| AiError::InvalidResponse)
        })
    }
}

impl TranscriptionProvider for ElevenLabsProvider {
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
            let mut body = json!({
                "model_id":prompt.model,
                "diarize":if prompt.diarize {"true"} else {"false"},
            });
            if let Some(language) = prompt.language {
                body["language_code"] = language.into();
            }
            let response = validate(
                self.0
                    .request(Some(prompt.timeout))?
                    .attach("file", prompt.audio.multipart_part("file".into())?)
                    .post("speech-to-text", &body)
                    .await,
            )?;
            let payload: Value = response.json().map_err(|_| AiError::InvalidResponse)?;
            let text = payload["text"].as_str().ok_or(AiError::InvalidResponse)?;
            let mut segments = Vec::new();
            if prompt.diarize {
                for word in payload["words"]
                    .as_array()
                    .ok_or(AiError::InvalidResponse)?
                {
                    if word["type"] == "word"
                        && let Some(segment) = word_segment(word)?
                    {
                        segments.push(segment);
                    }
                }
            }
            let seconds = payload["audio_duration_secs"].as_f64();
            if seconds.is_some_and(|seconds| !seconds.is_finite() || seconds < 0.0) {
                return Err(AiError::InvalidResponse);
            }
            Ok(TranscriptionResponse {
                text: text.into(),
                segments,
                language: payload["language_code"].as_str().map(str::to_owned),
                usage: TranscriptionUsage {
                    seconds,
                    ..Default::default()
                },
                provider: self.0.name().into(),
                model: payload["model_id"].as_str().unwrap_or(prompt.model).into(),
            })
        })
    }
}

fn word_segment(word: &Value) -> AiResult<Option<TranscriptionSegment>> {
    if word["start"].is_null() || word["end"].is_null() {
        return Ok(None);
    }
    let start = word["start"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .ok_or(AiError::InvalidResponse)?;
    let end = word["end"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= start)
        .ok_or(AiError::InvalidResponse)?;
    Ok(Some(TranscriptionSegment {
        start,
        end,
        text: word["text"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .into(),
        speaker: word["speaker_id"]
            .as_str()
            .filter(|speaker| !speaker.is_empty())
            .map(str::to_owned),
    }))
}
