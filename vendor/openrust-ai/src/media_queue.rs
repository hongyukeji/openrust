use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::Duration,
};

use openrust::{
    contracts::queue::JobHandler,
    queue::{
        Delay, DispatchOptions, DispatchResult, JobContext, JobOptions, Payload, QueueError,
        QueueFuture, QueueManager, QueueResult,
    },
    support::facades::Queue,
};
use serde::{Deserialize, Serialize};

use crate::{
    Ai, AiError, AudioResponse, ImageResponse, PendingAudioGeneration, PendingImageGeneration,
    RecordedAudio, Transcription, TranscriptionResponse,
};

const GENERATE_MEDIA: &str = "openrust.ai.generate-media";

pub type MediaQueueThen<T> = for<'a> fn(&'a T) -> QueueFuture<'a, QueueResult<()>>;
pub type MediaQueueCatch = for<'a> fn(&'a QueueError) -> QueueFuture<'a, QueueResult<()>>;

/// 当前应用可在 Worker 中通过稳定名称调用的回调。
pub(crate) struct MediaQueueCallbacks<T> {
    then: RwLock<HashMap<String, MediaQueueThen<T>>>,
    catch: RwLock<HashMap<String, MediaQueueCatch>>,
}

impl<T> Default for MediaQueueCallbacks<T> {
    fn default() -> Self {
        Self {
            then: RwLock::default(),
            catch: RwLock::default(),
        }
    }
}

impl<T> MediaQueueCallbacks<T> {
    pub fn register_then(
        &self,
        name: impl Into<String>,
        callback: MediaQueueThen<T>,
    ) -> QueueResult<()> {
        insert_callback(&self.then, name.into(), callback)
    }

    pub fn register_catch(
        &self,
        name: impl Into<String>,
        callback: MediaQueueCatch,
    ) -> QueueResult<()> {
        insert_callback(&self.catch, name.into(), callback)
    }

    fn validate(&self, then: &[String], catch: &[String]) -> QueueResult<()> {
        let completed = self.then.read().unwrap();
        let failed = self.catch.read().unwrap();
        if then.iter().any(|name| !completed.contains_key(name))
            || catch.iter().any(|name| !failed.contains_key(name))
        {
            return Err(QueueError::InvalidConfiguration);
        }
        Ok(())
    }

    async fn completed(&self, names: &[String], response: &T) -> QueueResult<()> {
        for name in names {
            let callback = self
                .then
                .read()
                .unwrap()
                .get(name)
                .copied()
                .ok_or(QueueError::InvalidConfiguration)?;
            callback(response).await?;
        }
        Ok(())
    }

    async fn failed(&self, names: &[String], error: &QueueError) -> QueueResult<()> {
        let mut failure = None;
        for name in names {
            let callback = self.catch.read().unwrap().get(name).copied();
            let result = match callback {
                Some(callback) => callback(error).await,
                None => Err(QueueError::InvalidConfiguration),
            };
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        failure.map_or(Ok(()), Err)
    }
}

fn insert_callback<T>(
    callbacks: &RwLock<HashMap<String, T>>,
    name: String,
    callback: T,
) -> QueueResult<()> {
    if name.trim().is_empty() {
        return Err(QueueError::InvalidConfiguration);
    }
    let mut callbacks = callbacks.write().unwrap();
    if callbacks.contains_key(&name) {
        return Err(QueueError::InvalidConfiguration);
    }
    callbacks.insert(name, callback);
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum QueuedMediaSource {
    Path { path: PathBuf },
    Storage { disk: String, path: String },
}

pub type QueuedAudioSource = QueuedMediaSource;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedQueuedImage {
    pub prompt: String,
    pub attachments: Vec<QueuedMediaSource>,
    pub size: Option<String>,
    pub quality: Option<String>,
    pub timeout: Option<Duration>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordedQueuedTranscription {
    pub audio: QueuedAudioSource,
    pub language: Option<String>,
    pub diarize: bool,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum MediaPrompt {
    Audio { prompt: RecordedAudio },
    Image { prompt: RecordedQueuedImage },
    Transcription { prompt: RecordedQueuedTranscription },
}

#[derive(Serialize, Deserialize)]
struct Invocation {
    prompt: MediaPrompt,
    then: Vec<String>,
    catch: Vec<String>,
}

/// 调用 `dispatch` 前配置队列、重试及具名回调。
pub struct QueuedMediaResponse {
    work: Invocation,
    dispatch: DispatchOptions,
    job: JobOptions,
}

pub type QueuedAudioResponse = QueuedMediaResponse;
pub type QueuedImageResponse = QueuedMediaResponse;
pub type QueuedTranscriptionResponse = QueuedMediaResponse;

impl QueuedMediaResponse {
    fn new(prompt: MediaPrompt) -> Self {
        Self {
            work: Invocation {
                prompt,
                then: Vec::new(),
                catch: Vec::new(),
            },
            dispatch: DispatchOptions::default(),
            job: JobOptions::default(),
        }
    }

    pub(crate) fn image(prompt: RecordedQueuedImage) -> Self {
        Self::new(MediaPrompt::Image { prompt })
    }

    pub(crate) fn audio(prompt: RecordedAudio) -> Self {
        Self::new(MediaPrompt::Audio { prompt })
    }

    pub(crate) fn transcription(prompt: RecordedQueuedTranscription) -> Self {
        Self::new(MediaPrompt::Transcription { prompt })
    }

    pub fn on_connection(mut self, name: impl Into<String>) -> Self {
        self.dispatch.connection = Some(name.into());
        self
    }

    pub fn on_queue(mut self, name: impl Into<String>) -> Self {
        self.dispatch.queue = Some(name.into());
        self
    }

    pub fn delay(mut self, delay: impl Into<Delay>) -> Self {
        self.dispatch.delay = Some(delay.into());
        self
    }

    pub fn with_job_options(mut self, options: JobOptions) -> Self {
        self.job = options;
        self
    }

    pub fn then(mut self, name: impl Into<String>) -> Self {
        self.work.then.push(name.into());
        self
    }

    pub fn catch(mut self, name: impl Into<String>) -> Self {
        self.work.catch.push(name.into());
        self
    }

    pub async fn dispatch(self) -> QueueResult<DispatchResult> {
        let ai = Ai::manager().map_err(QueueError::execution)?;
        let payload = Payload::new(GENERATE_MEDIA, &self.work)?.with_options(self.job);
        match &self.work.prompt {
            MediaPrompt::Image { prompt } => {
                ai.image_queue_callbacks()
                    .validate(&self.work.then, &self.work.catch)?;
                if let Some(fake) = ai.image_fake() {
                    fake.record_queued(prompt.clone());
                }
            }
            MediaPrompt::Audio { prompt } => {
                ai.audio_queue_callbacks()
                    .validate(&self.work.then, &self.work.catch)?;
                if let Some(fake) = ai.audio_fake() {
                    fake.record_queued(prompt.clone());
                }
            }
            MediaPrompt::Transcription { prompt } => {
                ai.transcription_queue_callbacks()
                    .validate(&self.work.then, &self.work.catch)?;
                if let Some(fake) = ai.transcription_fake() {
                    fake.record_queued(prompt.clone());
                }
            }
        }
        Queue::dispatch(payload, self.dispatch, None).await
    }
}

pub(crate) fn register(queue: &QueueManager) -> QueueResult<()> {
    queue.register(GENERATE_MEDIA, Arc::new(GenerateMedia))
}

struct GenerateMedia;

impl JobHandler for GenerateMedia {
    fn handle<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let work: Invocation = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let ai = Ai::manager().map_err(QueueError::execution)?;
            match work.prompt {
                MediaPrompt::Image { prompt } => {
                    let callbacks = ai.image_queue_callbacks();
                    callbacks.validate(&work.then, &work.catch)?;
                    let pending = PendingImageGeneration::from_queued(&prompt)
                        .await
                        .map_err(QueueError::execution)?;
                    let response = pending
                        .generate_with(prompt.provider.as_deref(), prompt.model.as_deref())
                        .await
                        .map_err(QueueError::execution)?;
                    callbacks.completed(&work.then, &response).await
                }
                MediaPrompt::Audio { prompt } => {
                    let callbacks = ai.audio_queue_callbacks();
                    callbacks.validate(&work.then, &work.catch)?;
                    let (pending, provider, model) = PendingAudioGeneration::from_queued(prompt);
                    let response = pending
                        .generate_with(provider.as_deref(), model.as_deref())
                        .await
                        .map_err(QueueError::execution)?;
                    callbacks.completed(&work.then, &response).await
                }
                MediaPrompt::Transcription { prompt } => {
                    let callbacks = ai.transcription_queue_callbacks();
                    callbacks.validate(&work.then, &work.catch)?;
                    let pending = match &prompt.audio {
                        QueuedAudioSource::Path { path } if path.is_absolute() => {
                            Transcription::from_path(path).await
                        }
                        QueuedAudioSource::Path { .. } => Err(AiError::InvalidAudio),
                        QueuedAudioSource::Storage { disk, path } => {
                            Transcription::from_storage(path, Some(disk)).await
                        }
                    }
                    .map_err(QueueError::execution)?;
                    let response = pending
                        .with_queued_options(&prompt)
                        .generate_with(prompt.provider.as_deref(), prompt.model.as_deref())
                        .await
                        .map_err(QueueError::execution)?;
                    callbacks.completed(&work.then, &response).await
                }
            }
        })
    }

    fn failed<'a>(
        &'a self,
        _: &'a mut JobContext<'_>,
        payload: &'a Payload,
        error: &'a QueueError,
    ) -> QueueFuture<'a, QueueResult<()>> {
        Box::pin(async move {
            let work: Invocation = serde_json::from_value(payload.data().clone())
                .map_err(|_| QueueError::InvalidPayload)?;
            let ai = Ai::manager().map_err(QueueError::execution)?;
            match work.prompt {
                MediaPrompt::Image { .. } => {
                    ai.image_queue_callbacks().failed(&work.catch, error).await
                }
                MediaPrompt::Audio { .. } => {
                    ai.audio_queue_callbacks().failed(&work.catch, error).await
                }
                MediaPrompt::Transcription { .. } => {
                    ai.transcription_queue_callbacks()
                        .failed(&work.catch, error)
                        .await
                }
            }
        })
    }
}

pub(crate) type AudioQueueCallbacks = MediaQueueCallbacks<AudioResponse>;
pub(crate) type TranscriptionQueueCallbacks = MediaQueueCallbacks<TranscriptionResponse>;

pub(crate) type ImageQueueCallbacks = MediaQueueCallbacks<ImageResponse>;
