use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use http::HeaderMap;
use openrust::{filesystem::FilesystemManager, http::client::Factory};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Endpoint, http_status,
    media::VersionedMediaLinks,
    sightengine::{endpoint, options},
    sightengine_video::media_id,
    video::{VideoEvidence, build_assessment},
};
use crate::{
    Coverage, Decision, EvidenceSpan, Finding, Modality, ModerationError, ModerationResult, TaskId,
    VideoInput, VideoInspection,
};

const CATEGORIES: &[&str] = &[
    "sexual",
    "insult",
    "discriminatory",
    "inappropriate",
    "email",
    "phone_number",
    "link",
    "extremism",
    "weapon",
    "medical",
    "drug",
    "self_harm",
    "violence",
    "platform_evasion",
    "off_platform_mention",
    "content_trade",
    "money_transaction",
    "minor",
];
const MAX_FINDINGS: usize = 256;

pub(super) struct SpeechService {
    endpoint: Endpoint,
    storage: VersionedMediaLinks,
    categories: Vec<String>,
}

impl SpeechService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("video_speech_categories").is_none() {
            return Ok(None);
        }
        if profile.get("video_models").is_some() {
            return Err(ModerationError::InvalidConfiguration(
                "video_speech_categories",
            ));
        }
        let mut endpoint = endpoint(http, profile, "/1.0/video/check-sync.json")?;
        if profile.get("timeout_ms").is_none() {
            endpoint.timeout = Duration::from_secs(75);
        }
        Ok(Some(Self {
            endpoint,
            storage: VersionedMediaLinks::new(storage, profile)?,
            categories: options(profile, "video_speech_categories", CATEGORIES)?,
        }))
    }

    pub(super) async fn inspect(
        &self,
        user: &str,
        secret: &str,
        input: &VideoInput,
    ) -> ModerationResult<VideoInspection> {
        let duration = input
            .source()
            .video_duration_ms()
            .filter(|&ms| ms < 60_000)
            .ok_or(ModerationError::InvalidInput(
                "sightengine speech video duration",
            ))?;
        let url = self.storage.create(input.source()).await?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("api_user", user),
                ("api_secret", secret),
                ("stream_url", url.as_str()),
                ("models", "audio-moderation"),
                ("audio_categories", self.categories.join(",").as_str()),
            ])
            .finish();
        let response = self
            .endpoint
            .post_encoded(HeaderMap::new(), body, "application/x-www-form-urlencoded")
            .await?;
        http_status(response.status().as_u16())?;
        let response: Response = response.json().map_err(|_| ModerationError::Protocol)?;
        if response.status != "success" || !media_id(&response.media.id) {
            return Err(ModerationError::Protocol);
        }
        let (coverage, evidence) = match response.data.audio {
            Some(audio) => (Coverage::Complete, self.read(audio, u64::from(duration))?),
            None => (Coverage::Unavailable, Vec::new()),
        };
        let decision = if coverage == Coverage::Complete && evidence.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        let assessment = build_assessment(
            decision,
            BTreeMap::from([(Modality::Speech, coverage)]),
            Some(response.request.id),
            evidence,
        )?;
        Ok(VideoInspection::new(
            TaskId::new(response.media.id).map_err(|_| ModerationError::Protocol)?,
            assessment,
        ))
    }

    fn read(&self, audio: Audio, duration: u64) -> ModerationResult<Vec<VideoEvidence>> {
        let detected = audio.detected_categories.iter().collect::<BTreeSet<_>>();
        if detected.len() != audio.detected_categories.len()
            || detected.len() != audio.detections.len()
            || detected.iter().any(|name| !self.categories.contains(name))
        {
            return Err(ModerationError::Protocol);
        }
        let mut evidence = Vec::new();
        for (name, details) in audio.detections {
            if !detected.contains(&name)
                || details.details.is_empty()
                || details.details.len() > MAX_FINDINGS.saturating_sub(evidence.len())
            {
                return Err(ModerationError::Protocol);
            }
            for item in details.details {
                if item.end_ms > duration {
                    return Err(ModerationError::Protocol);
                }
                let provider_label = match item.severity.or(details.severity) {
                    Some(severity) => format!("{}/{}", name, severity.name()),
                    None => name.clone(),
                };
                let category = match name.as_str() {
                    "insult" | "inappropriate" => "profanity",
                    "discriminatory" => "hate",
                    "email" | "phone_number" => "personal",
                    category => category,
                };
                evidence.push(VideoEvidence {
                    finding: Finding {
                        modality: Modality::Speech,
                        category: category.to_owned(),
                        provider_label,
                    },
                    span: Some(
                        EvidenceSpan::new(item.start_ms, item.end_ms)
                            .map_err(|_| ModerationError::Protocol)?,
                    ),
                    score: None,
                });
            }
        }
        Ok(evidence)
    }
}

#[derive(Deserialize)]
struct Response {
    status: String,
    request: Reference,
    media: Reference,
    data: Data,
}

#[derive(Deserialize)]
struct Reference {
    id: String,
}

#[derive(Deserialize)]
struct Data {
    audio: Option<Audio>,
}

#[derive(Deserialize)]
struct Audio {
    detected_categories: Vec<String>,
    #[serde(default)]
    detections: BTreeMap<String, Detection>,
}

#[derive(Deserialize)]
struct Detection {
    severity: Option<Severity>,
    details: Vec<Detail>,
}

#[derive(Deserialize)]
struct Detail {
    severity: Option<Severity>,
    start_ms: u64,
    end_ms: u64,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
