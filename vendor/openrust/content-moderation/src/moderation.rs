use openrust::{container::ServiceKey, support::facades::Facade};
use std::sync::Arc;

use crate::{
    Assessment, ImageInput, ModerationDriver, ModerationError, ModerationManager, ModerationResult,
    ProfileRevision, TextInput,
};

pub struct Moderation;

impl Facade for Moderation {
    type Service = ModerationManager;

    fn get_facade_accessor() -> ServiceKey {
        "moderation".into()
    }
}

impl Moderation {
    pub fn manager() -> ModerationResult<Arc<ModerationManager>> {
        Self::get_facade_root().map_err(|_| ModerationError::Unavailable)
    }

    pub fn driver(name: &str) -> ModerationResult<Arc<dyn ModerationDriver>> {
        Self::manager()?.driver(name)
    }

    pub fn default_driver() -> ModerationResult<Arc<dyn ModerationDriver>> {
        Self::manager()?.default_driver()
    }

    pub fn pin_profile(name: &str, revision: &str) -> ModerationResult<ProfileRevision> {
        Self::manager()?.pin_profile(name, revision)
    }

    pub fn driver_for_revision(
        profile: &ProfileRevision,
    ) -> ModerationResult<Arc<dyn ModerationDriver>> {
        Self::manager()?.driver_for_revision(profile)
    }

    pub async fn text(input: &TextInput) -> ModerationResult<Assessment> {
        Self::default_driver()?.text(input).await
    }

    pub async fn image(input: &ImageInput) -> ModerationResult<Assessment> {
        Self::default_driver()?.image(input).await
    }
}
