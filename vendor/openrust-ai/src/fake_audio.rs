use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;

use crate::{
    AiError, AiResult, AudioFormat, AudioResponse, AudioVoice, TranscriptionResponse,
    TranscriptionSegment, TranscriptionUsage,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedAudio {
    pub text: String,
    pub voice: AudioVoice,
    pub instructions: Option<String>,
    pub format: AudioFormat,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedTranscription {
    pub audio: Bytes,
    pub mime_type: &'static str,
    pub language: Option<String>,
    pub diarize: bool,
    pub timeout: Duration,
    pub provider: Option<String>,
    pub model: Option<String>,
}

pub enum FakeTranscriptionResult {
    Text(String),
    Response(Box<TranscriptionResponse>),
}

impl From<String> for FakeTranscriptionResult {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&str> for FakeTranscriptionResult {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}

impl From<TranscriptionResponse> for FakeTranscriptionResult {
    fn from(response: TranscriptionResponse) -> Self {
        Self::Response(Box::new(response))
    }
}

type Resolver<Request, Response> = dyn Fn(&Request) -> AiResult<Response> + Send + Sync;

enum Responses<Request, Response> {
    Sequence(Mutex<VecDeque<Response>>),
    Callback(Box<Resolver<Request, Response>>),
}

struct AudioFake<Request, Response> {
    responses: Responses<Request, Response>,
    recorded: Mutex<Vec<Request>>,
    prevent_stray: AtomicBool,
}

impl<Request, Response> AudioFake<Request, Response> {
    fn new(responses: Responses<Request, Response>) -> Self {
        Self {
            responses,
            recorded: Mutex::default(),
            prevent_stray: AtomicBool::new(false),
        }
    }

    fn next(&self, request: &Request) -> AiResult<Option<Response>> {
        match &self.responses {
            Responses::Sequence(responses) => match responses.lock().unwrap().pop_front() {
                None if self.prevent_stray.load(Ordering::Relaxed) => Err(AiError::FakeExhausted),
                response => Ok(response),
            },
            Responses::Callback(resolve) => resolve(request).map(Some),
        }
    }
}

pub struct FakeAudioGateway(AudioFake<RecordedAudio, AudioResponse>);

impl FakeAudioGateway {
    pub(crate) fn respond(&self, request: RecordedAudio) -> AiResult<AudioResponse> {
        let response = self.0.next(&request)?.unwrap_or_else(|| {
            AudioResponse::fake_placeholder(
                request.format,
                request.provider.clone().unwrap_or_default(),
                request.model.clone().unwrap_or_default(),
            )
        });
        self.0.recorded.lock().unwrap().push(request);
        Ok(response)
    }
}

pub struct FakeTranscriptionGateway(AudioFake<RecordedTranscription, FakeTranscriptionResult>);

impl FakeTranscriptionGateway {
    pub(crate) fn respond(
        &self,
        request: RecordedTranscription,
    ) -> AiResult<TranscriptionResponse> {
        let response = match self.0.next(&request)? {
            Some(FakeTranscriptionResult::Response(response)) => *response,
            result => {
                let text = match result {
                    Some(FakeTranscriptionResult::Text(text)) => text,
                    _ => "Fake transcription text.".into(),
                };
                TranscriptionResponse {
                    segments: vec![TranscriptionSegment {
                        start: 0.0,
                        end: 1.0,
                        text: text.clone(),
                        speaker: Some("Speaker 1".into()),
                    }],
                    text,
                    language: request.language.clone(),
                    usage: TranscriptionUsage::default(),
                    provider: request.provider.clone().unwrap_or_default(),
                    model: request.model.clone().unwrap_or_default(),
                }
            }
        };
        self.0.recorded.lock().unwrap().push(request);
        Ok(response)
    }
}

impl FakeAudioGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = AudioResponse>) -> Self {
        Self(AudioFake::new(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        ))))
    }

    pub(crate) fn with(
        resolve: impl Fn(&RecordedAudio) -> AiResult<AudioResponse> + Send + Sync + 'static,
    ) -> Self {
        Self(AudioFake::new(Responses::Callback(Box::new(resolve))))
    }

    pub fn prevent_stray_audio(&self, prevent: bool) -> &Self {
        self.0.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedAudio> {
        self.0.recorded.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_generated(&self, predicate: impl Fn(&RecordedAudio) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的语音生成请求"
        );
    }

    #[track_caller]
    pub fn assert_not_generated(&self, predicate: impl Fn(&RecordedAudio) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发生的语音生成请求"
        );
    }

    #[track_caller]
    pub fn assert_nothing_generated(&self) {
        assert!(
            self.0.recorded.lock().unwrap().is_empty(),
            "发现了语音生成请求"
        );
    }
}

impl FakeTranscriptionGateway {
    pub(crate) fn new(responses: impl IntoIterator<Item = FakeTranscriptionResult>) -> Self {
        Self(AudioFake::new(Responses::Sequence(Mutex::new(
            responses.into_iter().collect(),
        ))))
    }

    pub(crate) fn with(
        resolve: impl Fn(&RecordedTranscription) -> AiResult<FakeTranscriptionResult>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(AudioFake::new(Responses::Callback(Box::new(resolve))))
    }

    pub fn prevent_stray_transcriptions(&self, prevent: bool) -> &Self {
        self.0.prevent_stray.store(prevent, Ordering::Relaxed);
        self
    }

    pub fn recorded(&self) -> Vec<RecordedTranscription> {
        self.0.recorded.lock().unwrap().clone()
    }

    #[track_caller]
    pub fn assert_generated(&self, predicate: impl Fn(&RecordedTranscription) -> bool) {
        assert!(
            self.recorded().iter().any(predicate),
            "未发现匹配的转录请求"
        );
    }

    #[track_caller]
    pub fn assert_not_generated(&self, predicate: impl Fn(&RecordedTranscription) -> bool) {
        assert!(
            !self.recorded().iter().any(predicate),
            "发现了不应发生的转录请求"
        );
    }

    #[track_caller]
    pub fn assert_nothing_generated(&self) {
        assert!(self.0.recorded.lock().unwrap().is_empty(), "发现了转录请求");
    }
}
