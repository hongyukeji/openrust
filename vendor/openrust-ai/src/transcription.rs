use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use openrust::{
    filesystem::Storage,
    http::{HttpFuture, UploadedFile, client::Part},
    support::facades::Facade,
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Ai, AiError, AiResult, AudioResponse, FakeTranscriptionGateway, FakeTranscriptionResult,
    MediaQueueCatch, MediaQueueThen, QueuedAudioSource, QueuedTranscriptionResponse,
    RecordedQueuedTranscription, RecordedTranscription, Usage,
    connection::Connection,
    media_storage,
    text::{transport_error, validate},
};

const MAX_AUDIO_BYTES: usize = 25_000_000;

pub struct Transcription;

impl Transcription {
    pub fn register_queue_then(
        name: impl Into<String>,
        callback: MediaQueueThen<TranscriptionResponse>,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .transcription_queue_callbacks()
            .register_then(name, callback)
    }

    pub fn register_queue_catch(
        name: impl Into<String>,
        callback: MediaQueueCatch,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .transcription_queue_callbacks()
            .register_catch(name, callback)
    }

    pub fn of(audio: TranscriptionAudio) -> PendingTranscriptionGeneration {
        PendingTranscriptionGeneration {
            audio,
            source: None,
            language: None,
            diarize: false,
            timeout: Duration::from_secs(30),
        }
    }

    pub fn from_bytes(bytes: impl Into<Bytes>) -> AiResult<PendingTranscriptionGeneration> {
        Ok(Self::of(TranscriptionAudio::from_bytes(bytes)?))
    }

    pub fn from_upload(file: &UploadedFile) -> AiResult<PendingTranscriptionGeneration> {
        Self::from_bytes(file.get())
    }

    pub async fn from_path(
        path: impl AsRef<std::path::Path>,
    ) -> AiResult<PendingTranscriptionGeneration> {
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|_| AiError::MediaStorageUnavailable)?;
        let mut pending =
            Self::from_bytes(media_storage::read_path(&path, MAX_AUDIO_BYTES).await?)?;
        pending.source = Some(QueuedAudioSource::Path { path });
        Ok(pending)
    }

    pub async fn from_storage(
        path: &str,
        disk: Option<&str>,
    ) -> AiResult<PendingTranscriptionGeneration> {
        let disk = match disk.filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => Storage::get_facade_root()
                .map_err(|_| AiError::MediaStorageUnavailable)?
                .get_default_driver()
                .map_err(|_| AiError::MediaStorageUnavailable)?,
        };
        let content = media_storage::read(path, Some(&disk), MAX_AUDIO_BYTES)
            .await?
            .ok_or(AiError::InvalidAudio)?;
        let mut pending = Self::from_bytes(content)?;
        pending.source = Some(QueuedAudioSource::Storage {
            disk,
            path: path.into(),
        });
        Ok(pending)
    }

    pub fn fake(
        responses: impl IntoIterator<Item = FakeTranscriptionResult>,
    ) -> AiResult<Arc<FakeTranscriptionGateway>> {
        Ai::fake_transcription(responses)
    }

    pub fn fake_with(
        resolve: impl Fn(&RecordedTranscription) -> AiResult<FakeTranscriptionResult>
        + Send
        + Sync
        + 'static,
    ) -> AiResult<Arc<FakeTranscriptionGateway>> {
        Ai::fake_transcription_with(resolve)
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::transcription_is_faked()
    }

    fn fake_gateway() -> AiResult<Arc<FakeTranscriptionGateway>> {
        Ai::manager()?
            .transcription_fake()
            .ok_or(AiError::FakeNotInstalled)
    }

    #[track_caller]
    pub fn assert_queued(predicate: impl Fn(&RecordedQueuedTranscription) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_queued(
        predicate: impl Fn(&RecordedQueuedTranscription) -> bool,
    ) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_queued() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_queued();
        Ok(())
    }

    #[track_caller]
    pub fn assert_generated(predicate: impl Fn(&RecordedTranscription) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_generated(
        predicate: impl Fn(&RecordedTranscription) -> bool,
    ) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_generated() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_generated();
        Ok(())
    }
}

pub struct TranscriptionAudio {
    content: Bytes,
    extension: &'static str,
    mime_type: &'static str,
}

impl TranscriptionAudio {
    pub fn from_bytes(bytes: impl Into<Bytes>) -> AiResult<Self> {
        let content = bytes.into();
        if content.is_empty() || content.len() >= MAX_AUDIO_BYTES {
            return Err(AiError::InvalidAudio);
        }
        let uploaded = UploadedFile::from_bytes(String::new(), None, content.clone());
        let (extension, mime_type) = match uploaded.extension() {
            Some("flac") => ("flac", "audio/flac"),
            Some("mp3") => ("mp3", "audio/mpeg"),
            Some("mp4") => ("mp4", "video/mp4"),
            Some("m4a") => ("m4a", "audio/mp4"),
            Some("ogg" | "opus") => ("ogg", "audio/ogg"),
            Some("wav") => ("wav", "audio/wav"),
            Some("webm") => ("webm", "video/webm"),
            _ => return Err(AiError::InvalidAudio),
        };
        Ok(Self {
            content,
            extension,
            mime_type,
        })
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub fn mime_type(&self) -> &'static str {
        self.mime_type
    }
}

impl TryFrom<&AudioResponse> for TranscriptionAudio {
    type Error = AiError;

    fn try_from(response: &AudioResponse) -> AiResult<Self> {
        Self::from_bytes(response.owned_content())
    }
}

pub struct PendingTranscriptionGeneration {
    audio: TranscriptionAudio,
    source: Option<QueuedAudioSource>,
    language: Option<String>,
    diarize: bool,
    timeout: Duration,
}

impl PendingTranscriptionGeneration {
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    pub fn diarize(mut self, enabled: bool) -> Self {
        self.diarize = enabled;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn queue(self) -> AiResult<QueuedTranscriptionResponse> {
        self.queue_with(None, None)
    }

    pub fn queue_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<QueuedTranscriptionResponse> {
        TranscriptionPrompt {
            audio: &self.audio,
            language: self.language.as_deref(),
            diarize: self.diarize,
            model: model.unwrap_or("queued"),
            timeout: self.timeout,
        }
        .validate()?;
        let audio = self
            .source
            .ok_or(AiError::InvalidConfiguration("transcription.queue_source"))?;
        Ok(QueuedTranscriptionResponse::transcription(
            RecordedQueuedTranscription {
                audio,
                language: self.language,
                diarize: self.diarize,
                timeout: self.timeout,
                provider: provider.map(str::to_owned),
                model: model.map(str::to_owned),
            },
        ))
    }

    pub(crate) fn with_queued_options(mut self, prompt: &RecordedQueuedTranscription) -> Self {
        self.language.clone_from(&prompt.language);
        self.diarize = prompt.diarize;
        self.timeout = prompt.timeout;
        self
    }

    pub async fn generate(self) -> AiResult<TranscriptionResponse> {
        self.generate_with(None, None).await
    }

    pub async fn generate_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<TranscriptionResponse> {
        let manager = Ai::manager()?;
        if let Some(fake) = manager.transcription_fake() {
            let selected = model
                .is_none()
                .then(|| manager.transcription_provider(provider).ok())
                .flatten();
            let resolved_model = model.or_else(|| {
                selected
                    .as_ref()
                    .and_then(|provider| provider.default_transcription_model().ok())
            });
            TranscriptionPrompt {
                audio: &self.audio,
                language: self.language.as_deref(),
                diarize: self.diarize,
                model: resolved_model.unwrap_or("fake"),
                timeout: self.timeout,
            }
            .validate()?;
            return fake.respond(RecordedTranscription {
                mime_type: self.audio.mime_type(),
                audio: self.audio.content,
                language: self.language,
                diarize: self.diarize,
                timeout: self.timeout,
                provider: manager
                    .provider_name_for(provider, "default_for_transcription")
                    .ok(),
                model: resolved_model.map(str::to_owned),
            });
        }
        let provider = manager.transcription_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_transcription_model()?,
        };
        let prompt = TranscriptionPrompt {
            audio: &self.audio,
            language: self.language.as_deref(),
            diarize: self.diarize,
            model,
            timeout: self.timeout,
        };
        prompt.validate()?;
        provider.generate_transcription(prompt).await
    }
}

pub struct TranscriptionPrompt<'a> {
    pub audio: &'a TranscriptionAudio,
    pub language: Option<&'a str>,
    pub diarize: bool,
    pub model: &'a str,
    pub timeout: Duration,
}

impl TranscriptionPrompt<'_> {
    fn validate(&self) -> AiResult<()> {
        if self.model.trim().is_empty() {
            return Err(AiError::MissingModel);
        }
        if self.timeout.is_zero()
            || self
                .language
                .is_some_and(|language| language.trim().is_empty())
        {
            return Err(AiError::InvalidConfiguration("transcription.generation"));
        }
        Ok(())
    }
}

pub trait TranscriptionProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_transcription_model(&self) -> AiResult<&str>;
    fn generate_transcription<'a>(
        &'a self,
        prompt: TranscriptionPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<TranscriptionResponse>>;
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct TranscriptionUsage {
    #[serde(flatten)]
    pub tokens: Usage,
    pub audio_input_tokens: Option<u64>,
    pub seconds: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TranscriptionSegment {
    pub start: f64,
    pub end: f64,
    pub text: String,
    pub speaker: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TranscriptionResponse {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
    pub language: Option<String>,
    pub usage: TranscriptionUsage,
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Copy)]
pub(crate) enum TranscriptionProtocol {
    OpenAi,
    Mistral,
}

pub(crate) struct HttpTranscriptionProvider {
    pub(crate) connection: Arc<Connection>,
    pub(crate) protocol: TranscriptionProtocol,
}

impl TranscriptionProvider for HttpTranscriptionProvider {
    fn name(&self) -> &str {
        self.connection.name()
    }

    fn default_transcription_model(&self) -> AiResult<&str> {
        self.connection.default_transcription_model()
    }

    fn generate_transcription<'a>(
        &'a self,
        prompt: TranscriptionPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<TranscriptionResponse>> {
        Box::pin(async move {
            prompt.validate()?;
            let mistral = matches!(self.protocol, TranscriptionProtocol::Mistral);
            let body = if mistral {
                let mut fields = vec![json!({"name":"model","contents":prompt.model})];
                if prompt.diarize {
                    fields.push(json!({"name":"diarize","contents":true}));
                    fields.push(json!({"name":"timestamp_granularities","contents":"segment"}));
                } else if let Some(language) = prompt.language {
                    fields.push(json!({"name":"language","contents":language}));
                }
                Value::Array(fields)
            } else {
                let mut fields = json!({"model":prompt.model,"response_format":"json"});
                if let Some(language) = prompt.language {
                    fields["language"] = language.into();
                }
                if prompt.diarize {
                    fields["response_format"] = "diarized_json".into();
                    fields["chunking_strategy"] = "auto".into();
                }
                fields
            };
            let file = Part::stream_with_length(
                prompt.audio.content.clone(),
                prompt.audio.content.len() as u64,
            )
            .file_name(format!("audio.{}", prompt.audio.extension))
            .mime_str(prompt.audio.mime_type())
            .map_err(|_| AiError::InvalidAudio)?;
            let response = self
                .connection
                .request(Some(prompt.timeout))?
                .attach("file", file)
                .post("audio/transcriptions", &body)
                .await
                .map_err(transport_error)?;
            let data: Value = validate(Ok(response))?
                .json()
                .map_err(|_| AiError::InvalidResponse)?;
            let text = data["text"]
                .as_str()
                .ok_or(AiError::InvalidResponse)?
                .into();
            let segments = match data.get("segments") {
                Some(Value::Array(segments)) => segments
                    .iter()
                    .map(|value| segment(value, self.protocol))
                    .collect::<AiResult<_>>()?,
                None | Some(Value::Null) if !prompt.diarize => Vec::new(),
                _ => return Err(AiError::InvalidResponse),
            };
            if prompt.diarize
                && !mistral
                && segments.iter().any(|segment: &TranscriptionSegment| {
                    segment
                        .speaker
                        .as_deref()
                        .is_none_or(|speaker| speaker.trim().is_empty())
                })
            {
                return Err(AiError::InvalidResponse);
            }
            let usage = &data["usage"];
            let (tokens, audio_input_tokens, seconds) = match self.protocol {
                TranscriptionProtocol::Mistral => (
                    Usage::from_openai(usage),
                    usage["prompt_tokens_details"]["audio_tokens"].as_u64(),
                    usage["prompt_audio_seconds"].as_f64(),
                ),
                TranscriptionProtocol::OpenAi => (
                    Usage {
                        prompt_tokens: usage["input_tokens"].as_u64(),
                        completion_tokens: usage["output_tokens"].as_u64(),
                        ..Usage::default()
                    },
                    usage["input_token_details"]["audio_tokens"].as_u64(),
                    usage["seconds"]
                        .as_f64()
                        .or_else(|| data["duration"].as_f64()),
                ),
            };
            if seconds.is_some_and(|duration| duration < 0.0 || !duration.is_finite()) {
                return Err(AiError::InvalidResponse);
            }
            Ok(TranscriptionResponse {
                text,
                segments,
                language: data["language"]
                    .as_str()
                    .or_else(|| data["languages"][0]["code"].as_str())
                    .map(str::to_owned),
                provider: self.name().into(),
                model: data["model"].as_str().unwrap_or(prompt.model).into(),
                usage: TranscriptionUsage {
                    tokens,
                    audio_input_tokens,
                    seconds,
                },
            })
        })
    }
}

fn segment(value: &Value, protocol: TranscriptionProtocol) -> AiResult<TranscriptionSegment> {
    let mistral = matches!(protocol, TranscriptionProtocol::Mistral);
    let start = value["start"]
        .as_f64()
        .or_else(|| (mistral && value["start"].is_null()).then_some(0.0))
        .filter(|start| start.is_finite() && *start >= 0.0)
        .ok_or(AiError::InvalidResponse)?;
    let end = value["end"]
        .as_f64()
        .or_else(|| (mistral && value["end"].is_null()).then_some(start))
        .filter(|end| end.is_finite() && *end >= start)
        .ok_or(AiError::InvalidResponse)?;
    Ok(TranscriptionSegment {
        start,
        end,
        text: value["text"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .into(),
        speaker: value[if mistral { "speaker_id" } else { "speaker" }]
            .as_str()
            .map(str::to_owned),
    })
}
