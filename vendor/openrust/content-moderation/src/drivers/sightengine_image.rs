use std::{collections::BTreeMap, sync::Arc};

use openrust::{
    filesystem::FilesystemManager,
    http::client::{Factory, Part},
};
use serde::Deserialize;
use serde_json::Value;

use super::{
    Endpoint, field, http_status,
    media::VersionedMediaLinks,
    sightengine::endpoint,
    sightengine_models::{MediaFindings, MediaModels},
    video::build_assessment,
};
use crate::{Assessment, Decision, ImageInput, ModerationError, ModerationResult};

pub(super) struct ImageService {
    endpoint: Endpoint,
    models: MediaModels,
    media: Option<VersionedMediaLinks>,
}

impl ImageService {
    pub(super) fn configured(
        http: Arc<Factory>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("image_models").is_none() {
            return Ok(None);
        }
        let models = MediaModels::new(
            profile,
            "image_models",
            "image_review_score",
            "image_text_categories",
        )?;
        let endpoint = endpoint(http, profile, "/1.0/check.json")?;
        let media = match (
            field(profile, "media_disk")?,
            field(profile, "media_origin")?,
        ) {
            (None, None) => None,
            _ => Some(VersionedMediaLinks::new(storage, profile)?),
        };
        Ok(Some(Self {
            endpoint,
            models,
            media,
        }))
    }

    pub(super) async fn assess(
        &self,
        user: &str,
        secret: &str,
        input: &ImageInput,
    ) -> ModerationResult<Assessment> {
        let mut parameters = vec![
            ("api_user", user.to_owned()),
            ("api_secret", secret.to_owned()),
        ];
        parameters.extend(self.models.parameters());
        let headers = http::HeaderMap::new();
        let response = if let Some(bytes) = input.as_bytes() {
            let mut parts: Vec<_> = parameters
                .into_iter()
                .map(|(name, value)| (name, Part::text(value)))
                .collect();
            parts.push(("media", Part::bytes(bytes.to_vec()).file_name("image")));
            self.endpoint.post_parts(headers, parts).await?
        } else if let (Some(media), Some(source)) = (&self.media, input.as_stored()) {
            let url = media.create(source).await?;
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(parameters)
                .append_pair("url", url.as_str())
                .finish();
            self.endpoint
                .post_encoded(headers, body, "application/x-www-form-urlencoded")
                .await?
        } else {
            return Err(ModerationError::InvalidInput("sightengine stored image"));
        };
        http_status(response.status().as_u16())?;
        let output: ImageOutput = response.json().map_err(|_| ModerationError::Protocol)?;
        self.result(output)
    }

    fn result(&self, output: ImageOutput) -> ModerationResult<Assessment> {
        if output.status != "success" {
            return Err(ModerationError::Protocol);
        }
        let frames =
            std::iter::once(&output.models).chain(output.data.iter().flat_map(|data| &data.frames));
        let mut findings = MediaFindings::default();
        for frame in frames {
            self.models.observe(frame, &mut findings)?;
        }
        let coverage = findings.coverage();
        let evidence = findings.evidence(None)?;
        let decision = if evidence.is_empty() {
            Decision::Allow
        } else {
            Decision::Review
        };
        build_assessment(decision, coverage, Some(output.request.id), evidence)
    }
}

#[derive(Deserialize)]
struct ImageOutput {
    status: String,
    request: RequestMetadata,
    data: Option<FrameData>,
    #[serde(flatten)]
    models: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct RequestMetadata {
    id: String,
}

#[derive(Deserialize)]
struct FrameData {
    frames: Vec<BTreeMap<String, Value>>,
}
