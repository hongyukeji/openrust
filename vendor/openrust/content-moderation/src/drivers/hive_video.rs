use std::sync::Arc;

use openrust::{filesystem::FilesystemManager, http::client::Factory, support::Str};
use serde::Deserialize;
use serde_json::Value;
use url::Url;

use super::{field, hive::Model, http_status, media::VersionedMediaLinks};
use crate::{
    ModerationError, ModerationResult, TaskId, VideoInput, credentials::DriverCredentials,
};

pub(super) struct VideoService {
    model: Model,
    storage: VersionedMediaLinks,
    callback: Url,
}

impl VideoService {
    pub(super) fn configured(
        http: Arc<Factory>,
        credentials: &DriverCredentials<'_>,
        storage: Arc<FilesystemManager>,
        profile: &Value,
    ) -> ModerationResult<Option<Self>> {
        if profile.get("video_project").is_none() {
            return Ok(None);
        }
        let mut model = Model::new(
            http,
            credentials,
            profile,
            "video_token_env",
            "video_project",
            "CATEGORIZATION",
        )?;
        model.endpoint.url.set_path("/api/v2/task/async");
        let callback = field(profile, "video_callback_url")?
            .filter(|url| url.len() <= 2048)
            .and_then(|url| Url::parse(url).ok())
            .filter(|url| {
                url.scheme() == "https"
                    && url.host().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.fragment().is_none()
                    && url.query().is_none()
            })
            .ok_or(ModerationError::InvalidConfiguration("video_callback_url"))?;
        Ok(Some(Self {
            model,
            storage: VersionedMediaLinks::new(storage, profile)?,
            callback,
        }))
    }

    pub(super) async fn submit(&self, input: &VideoInput) -> ModerationResult<TaskId> {
        let media = self.storage.create(input.source()).await?;
        let payload = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", media.as_str())
            .append_pair("callback_url", self.callback.as_str())
            .finish();
        let response = self
            .model
            .endpoint
            .post_encoded(
                self.model.headers(),
                payload,
                "application/x-www-form-urlencoded",
            )
            .await
            .map_err(uncertain)?;
        submission_status(response.status().as_u16())?;
        let task: Acknowledgement = response
            .json()
            .map_err(|_| ModerationError::SubmissionUnknown)?;
        if let Some(code) = task.code {
            submission_status(code)?;
        }
        if task.project_id != Some(self.model.project)
            || task
                .callback
                .as_ref()
                .is_some_and(|callback| callback != self.callback.as_str())
        {
            return Err(ModerationError::SubmissionUnknown);
        }
        let id = task
            .id
            .filter(|id| Str::is_uuid(id, None) && !Str::is_uuid(id, Some(0)))
            .ok_or(ModerationError::SubmissionUnknown)?;
        TaskId::new(id).map_err(|_| ModerationError::SubmissionUnknown)
    }
}

#[derive(Deserialize)]
struct Acknowledgement {
    id: Option<String>,
    project_id: Option<u64>,
    callback: Option<String>,
    code: Option<u16>,
}

fn submission_status(code: u16) -> ModerationResult<()> {
    http_status(code).map_err(uncertain)
}

fn uncertain(error: ModerationError) -> ModerationError {
    match error {
        ModerationError::Timeout
        | ModerationError::ServiceUnavailable
        | ModerationError::Protocol => ModerationError::SubmissionUnknown,
        error => error,
    }
}
