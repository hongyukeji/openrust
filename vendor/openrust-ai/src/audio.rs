use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use openrust::{
    filesystem::Visibility,
    http::{HttpFuture, UploadedFile},
    support::Str,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    Ai, AiError, AiResult, FakeAudioGateway, MediaQueueCatch, MediaQueueThen, QueuedAudioResponse,
    RecordedAudio,
    connection::Connection,
    media_storage,
    text::{transport_error, validate},
};

pub struct Audio;

impl Audio {
    pub fn register_queue_then(
        name: impl Into<String>,
        callback: MediaQueueThen<AudioResponse>,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .audio_queue_callbacks()
            .register_then(name, callback)
    }

    pub fn register_queue_catch(
        name: impl Into<String>,
        callback: MediaQueueCatch,
    ) -> openrust::queue::QueueResult<()> {
        Ai::manager()
            .map_err(openrust::queue::QueueError::execution)?
            .audio_queue_callbacks()
            .register_catch(name, callback)
    }

    pub fn of(text: impl Into<String>) -> AiResult<PendingAudioGeneration> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        Ok(PendingAudioGeneration {
            text,
            voice: AudioVoice::Default,
            instructions: None,
            format: AudioFormat::Mp3,
            timeout: Duration::from_secs(30),
        })
    }

    pub fn fake(
        responses: impl IntoIterator<Item = AudioResponse>,
    ) -> AiResult<Arc<FakeAudioGateway>> {
        Ai::fake_audio(responses)
    }

    pub fn fake_with(
        resolve: impl Fn(&RecordedAudio) -> AiResult<AudioResponse> + Send + Sync + 'static,
    ) -> AiResult<Arc<FakeAudioGateway>> {
        Ai::fake_audio_with(resolve)
    }

    pub fn is_faked() -> AiResult<bool> {
        Ai::audio_is_faked()
    }

    fn fake_gateway() -> AiResult<Arc<FakeAudioGateway>> {
        Ai::manager()?.audio_fake().ok_or(AiError::FakeNotInstalled)
    }

    #[track_caller]
    pub fn assert_queued(predicate: impl Fn(&RecordedAudio) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_queued(predicate: impl Fn(&RecordedAudio) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_queued(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_queued() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_queued();
        Ok(())
    }

    #[track_caller]
    pub fn assert_generated(predicate: impl Fn(&RecordedAudio) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_not_generated(predicate: impl Fn(&RecordedAudio) -> bool) -> AiResult<()> {
        Self::fake_gateway()?.assert_not_generated(predicate);
        Ok(())
    }

    #[track_caller]
    pub fn assert_nothing_generated() -> AiResult<()> {
        Self::fake_gateway()?.assert_nothing_generated();
        Ok(())
    }
}

pub trait ToAudio {
    fn to_audio(&self) -> AiResult<PendingAudioGeneration>;
}

impl<T: AsRef<str> + ?Sized> ToAudio for T {
    fn to_audio(&self) -> AiResult<PendingAudioGeneration> {
        Audio::of(self.as_ref())
    }
}

pub struct PendingAudioGeneration {
    text: String,
    voice: AudioVoice,
    instructions: Option<String>,
    format: AudioFormat,
    timeout: Duration,
}

impl PendingAudioGeneration {
    pub fn voice(mut self, name: impl Into<String>) -> Self {
        self.voice = AudioVoice::Named(name.into());
        self
    }

    pub fn male(mut self) -> Self {
        self.voice = AudioVoice::Male;
        self
    }

    pub fn female(mut self) -> Self {
        self.voice = AudioVoice::Female;
        self
    }

    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    pub fn format(mut self, format: AudioFormat) -> Self {
        self.format = format;
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn queue(self) -> AiResult<QueuedAudioResponse> {
        self.queue_with(None, None)
    }

    pub fn queue_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<QueuedAudioResponse> {
        AudioPrompt {
            text: &self.text,
            voice: &self.voice,
            instructions: self.instructions.as_deref(),
            format: self.format,
            model: model.unwrap_or("queued"),
            timeout: self.timeout,
        }
        .validate()?;
        Ok(QueuedAudioResponse::audio(RecordedAudio {
            text: self.text,
            voice: self.voice,
            instructions: self.instructions,
            format: self.format,
            timeout: self.timeout,
            provider: provider.map(str::to_owned),
            model: model.map(str::to_owned),
        }))
    }

    pub(crate) fn from_queued(prompt: RecordedAudio) -> (Self, Option<String>, Option<String>) {
        (
            Self {
                text: prompt.text,
                voice: prompt.voice,
                instructions: prompt.instructions,
                format: prompt.format,
                timeout: prompt.timeout,
            },
            prompt.provider,
            prompt.model,
        )
    }

    pub async fn generate(self) -> AiResult<AudioResponse> {
        self.generate_with(None, None).await
    }

    pub async fn generate_with(
        self,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> AiResult<AudioResponse> {
        let manager = Ai::manager()?;
        if let Some(fake) = manager.audio_fake() {
            let selected = model
                .is_none()
                .then(|| manager.audio_provider(provider).ok())
                .flatten();
            let resolved_model = model.or_else(|| {
                selected
                    .as_ref()
                    .and_then(|provider| provider.default_audio_model().ok())
            });
            AudioPrompt {
                text: &self.text,
                voice: &self.voice,
                instructions: self.instructions.as_deref(),
                format: self.format,
                model: resolved_model.unwrap_or("fake"),
                timeout: self.timeout,
            }
            .validate()?;
            return fake.respond(RecordedAudio {
                text: self.text,
                voice: self.voice,
                instructions: self.instructions,
                format: self.format,
                timeout: self.timeout,
                provider: manager
                    .provider_name_for(provider, "default_for_audio")
                    .ok(),
                model: resolved_model.map(str::to_owned),
            });
        }
        let provider = manager.audio_provider(provider)?;
        let model = match model {
            Some(model) => model,
            None => provider.default_audio_model()?,
        };
        let prompt = AudioPrompt {
            text: &self.text,
            voice: &self.voice,
            instructions: self.instructions.as_deref(),
            format: self.format,
            model,
            timeout: self.timeout,
        };
        prompt.validate()?;
        provider.generate_audio(prompt).await
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioVoice {
    #[default]
    Default,
    Named(String),
    Male,
    Female,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioFormat {
    Mp3,
    Opus,
    Aac,
    Flac,
    Wav,
    Pcm,
}

impl AudioFormat {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Mp3 => "audio/mpeg",
            Self::Opus => "audio/ogg",
            Self::Aac => "audio/aac",
            Self::Flac => "audio/flac",
            Self::Wav => "audio/wav",
            Self::Pcm => "audio/pcm",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Opus => "opus",
            Self::Aac => "aac",
            Self::Flac => "flac",
            Self::Wav => "wav",
            Self::Pcm => "pcm",
        }
    }

    fn matches(self, content: &Bytes) -> bool {
        if self == Self::Pcm {
            return !content.is_empty() && content.len().is_multiple_of(2);
        }
        let uploaded = UploadedFile::from_bytes(String::new(), None, content.clone());
        matches!((self, uploaded.extension()), (Self::Opus, Some("ogg")))
            || uploaded.extension() == Some(self.extension())
    }
}

pub struct AudioPrompt<'a> {
    pub text: &'a str,
    pub voice: &'a AudioVoice,
    pub instructions: Option<&'a str>,
    pub format: AudioFormat,
    pub model: &'a str,
    pub timeout: Duration,
}

impl AudioPrompt<'_> {
    fn validate(&self) -> AiResult<()> {
        if self.text.trim().is_empty() {
            return Err(AiError::InvalidPrompt);
        }
        if self.model.trim().is_empty() {
            return Err(AiError::MissingModel);
        }
        if self.timeout.is_zero()
            || matches!(self.voice, AudioVoice::Named(name) if name.trim().is_empty())
        {
            return Err(AiError::InvalidConfiguration("audio.generation"));
        }
        Ok(())
    }
}

pub trait AudioProvider: Send + Sync {
    fn name(&self) -> &str;
    fn default_audio_model(&self) -> AiResult<&str>;
    fn generate_audio<'a>(
        &'a self,
        prompt: AudioPrompt<'a>,
    ) -> HttpFuture<'a, AiResult<AudioResponse>>;
}

pub struct AudioResponse {
    content: Bytes,
    format: AudioFormat,
    pub provider: String,
    pub model: String,
}

impl AudioResponse {
    pub(crate) fn fake_placeholder(format: AudioFormat, provider: String, model: String) -> Self {
        Self {
            content: Bytes::from_static(b"fake-audio-content"),
            format,
            provider,
            model,
        }
    }

    pub fn from_bytes(
        bytes: impl Into<Bytes>,
        format: AudioFormat,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> AiResult<Self> {
        let content = bytes.into();
        if !format.matches(&content) {
            return Err(AiError::InvalidAudio);
        }
        Ok(Self {
            content,
            format,
            provider: provider.into(),
            model: model.into(),
        })
    }

    pub fn content(&self) -> &[u8] {
        &self.content
    }

    pub(crate) fn owned_content(&self) -> Bytes {
        self.content.clone()
    }

    pub fn format(&self) -> AudioFormat {
        self.format
    }

    pub fn mime_type(&self) -> &'static str {
        self.format.mime_type()
    }

    pub async fn store(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.write(directory, None, disk, None).await
    }

    pub async fn store_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.write(directory, Some(name), disk, None).await
    }

    pub async fn store_publicly(&self, directory: &str, disk: Option<&str>) -> AiResult<String> {
        self.write(directory, None, disk, Some(Visibility::Public))
            .await
    }

    pub async fn store_publicly_as(
        &self,
        directory: &str,
        name: &str,
        disk: Option<&str>,
    ) -> AiResult<String> {
        self.write(directory, Some(name), disk, Some(Visibility::Public))
            .await
    }

    async fn write(
        &self,
        directory: &str,
        name: Option<&str>,
        disk: Option<&str>,
        visibility: Option<Visibility>,
    ) -> AiResult<String> {
        media_storage::write(
            &self.content,
            self.format.extension(),
            directory,
            name,
            disk,
            visibility,
        )
        .await
    }
}

pub(crate) struct OpenAiAudioProvider(pub(crate) Arc<Connection>);

impl AudioProvider for OpenAiAudioProvider {
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
                AudioVoice::Default => "alloy",
                AudioVoice::Male => "onyx",
                AudioVoice::Female => "nova",
                AudioVoice::Named(name) => name,
            };
            let mut body = json!({
                "model": prompt.model,
                "input": prompt.text,
                "voice": voice,
                "response_format": prompt.format.extension(),
            });
            if let Some(instructions) = prompt.instructions.filter(|text| !text.trim().is_empty()) {
                body["instructions"] = instructions.into();
            }
            let response = self
                .0
                .request(Some(prompt.timeout))?
                .post("audio/speech", &body)
                .await
                .map_err(transport_error)?;
            let response = validate(Ok(response))?;
            let content = response.body().map_err(|_| AiError::InvalidResponse)?;
            AudioResponse::from_bytes(content, prompt.format, self.name(), prompt.model)
                .map_err(|_| AiError::InvalidResponse)
        })
    }
}

pub(crate) struct MistralAudioProvider(pub(crate) Arc<Connection>);

impl AudioProvider for MistralAudioProvider {
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
            if prompt.format == AudioFormat::Aac {
                return Err(AiError::InvalidConfiguration("audio.format"));
            }
            let voice_id = match prompt.voice {
                AudioVoice::Default | AudioVoice::Female => "gb_jane_neutral",
                AudioVoice::Male => "en_paul_neutral",
                AudioVoice::Named(name) => name,
            };
            let body = json!({
                "model": prompt.model,
                "input": prompt.text,
                "voice_id": voice_id,
                "response_format": prompt.format.extension(),
            });
            #[derive(serde::Deserialize)]
            struct Speech {
                audio_data: String,
            }
            let response: Speech = validate(
                self.0
                    .request(Some(prompt.timeout))?
                    .post("audio/speech", &body)
                    .await,
            )?
            .json()
            .map_err(|_| AiError::InvalidResponse)?;
            let content =
                Str::from_base64(&response.audio_data).map_err(|_| AiError::InvalidResponse)?;
            AudioResponse::from_bytes(content, prompt.format, self.name(), prompt.model)
                .map_err(|_| AiError::InvalidResponse)
        })
    }
}
