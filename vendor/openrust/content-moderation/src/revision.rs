use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use crate::{
    CredentialReference, Modality, ModerationError, ModerationResult,
    credentials::{uses_environment, valid_identifier},
    query_policy::QueryPolicy,
};

const FORMAT: u8 = 1;

/// 领域任务持久化的配置定位符。实际参数留在部署的不可变修订表中。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileRevision {
    format: u8,
    name: String,
    revision: String,
    driver: String,
    account: String,
    policy_revision: String,
    credential: CredentialReference,
    required_modalities: BTreeSet<Modality>,
    #[serde(default)]
    pub(crate) query: QueryPolicy,
    digest: [u8; 32],
}

impl ProfileRevision {
    pub(crate) fn configured(
        name: &str,
        revision: &str,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let identifier = |key| {
            profile
                .get(key)
                .and_then(Value::as_str)
                .filter(|value| valid_identifier(value))
                .map(str::to_owned)
                .ok_or(ModerationError::InvalidConfiguration(key))
        };
        let credential = CredentialReference::configured(profile)?
            .filter(|_| !uses_environment(profile))
            .ok_or(ModerationError::InvalidConfiguration("credential_ref"))?;
        let required_modalities = profile
            .get("required_modalities")
            .and_then(|value| serde_json::from_value::<Vec<Modality>>(value.clone()).ok())
            .filter(|value| (1..=4).contains(&value.len()))
            .ok_or(ModerationError::InvalidConfiguration("required_modalities"))?;
        let unique_modalities = required_modalities.iter().copied().collect::<BTreeSet<_>>();
        if unique_modalities.len() != required_modalities.len() {
            return Err(ModerationError::InvalidConfiguration("required_modalities"));
        }
        let mut canonical = profile.clone();
        canonical.sort_all_objects();
        let encoded = serde_json::to_vec(&canonical)
            .ok()
            .filter(|bytes| bytes.len() <= 32 * 1024)
            .ok_or(ModerationError::InvalidConfiguration("profile_revisions"))?;
        Ok(Self {
            format: FORMAT,
            name: name.to_owned(),
            revision: revision.to_owned(),
            driver: identifier("driver")?,
            account: identifier("account")?,
            policy_revision: identifier("policy_revision")?,
            credential,
            required_modalities: unique_modalities,
            query: QueryPolicy::configured(profile)?,
            digest: Sha256::digest(&encoded).into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub fn driver(&self) -> &str {
        &self.driver
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }

    pub fn credential(&self) -> &CredentialReference {
        &self.credential
    }

    pub fn required_modalities(&self) -> &BTreeSet<Modality> {
        &self.required_modalities
    }

    pub(crate) fn supported(&self) -> bool {
        self.format == FORMAT
    }
}
