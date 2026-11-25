use std::{collections::BTreeMap, io::Read as _, path::Path, sync::Arc};

use cap_std::{ambient_authority, fs::Dir};
use openrust::{config::Environment, contracts::config::Repository};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{ModerationError, ModerationResult, drivers::field};

const MAX_CREDENTIAL_BYTES: u64 = 16 * 1024;
const REFERENCE: &str = "credential_ref";
const REVISION: &str = "credential_revision";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CredentialReference {
    name: String,
    revision: String,
}

impl CredentialReference {
    pub fn new(name: &str, revision: &str) -> ModerationResult<Self> {
        fn valid(value: &str) -> bool {
            !value.is_empty()
                && value.len() <= 64
                && value.bytes().enumerate().all(|(index, byte)| {
                    byte.is_ascii_alphanumeric() || index > 0 && matches!(byte, b'_' | b'-')
                })
        }
        if !valid(name) || !valid(revision) {
            return Err(ModerationError::InvalidConfiguration(REFERENCE));
        }
        Ok(Self {
            name: name.into(),
            revision: revision.into(),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    pub(crate) fn configured(profile: &Value) -> ModerationResult<Option<Self>> {
        match (field(profile, REFERENCE)?, field(profile, REVISION)?) {
            (Some(name), Some(revision)) => Self::new(name, revision).map(Some),
            (None, None) => Ok(None),
            _ => Err(ModerationError::InvalidConfiguration(REFERENCE)),
        }
    }
}

#[derive(Default)]
pub struct ModerationCredentials(BTreeMap<String, Secret>);

impl ModerationCredentials {
    pub fn insert(&mut self, field: impl Into<String>, value: Zeroizing<String>) {
        self.0.insert(field.into(), Secret(value));
    }

    pub fn get(&self, field: &str) -> Option<&str> {
        self.0.get(field).map(|value| value.0.as_str())
    }
}

struct Secret(Zeroizing<String>);

impl<'de> Deserialize<'de> for Secret {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer).map(|value| Self(Zeroizing::new(value)))
    }
}

/// 按受控引用解析一个不可变的供应商凭据版本。
pub trait ModerationCredentialProvider: Send + Sync + 'static {
    fn resolve(&self, reference: &CredentialReference) -> ModerationResult<ModerationCredentials>;
}

pub(crate) struct FileCredentials(pub Arc<dyn Repository>);

impl ModerationCredentialProvider for FileCredentials {
    fn resolve(&self, reference: &CredentialReference) -> ModerationResult<ModerationCredentials> {
        let directory = self.0.get("moderation.credential_directory");
        let path = directory
            .as_ref()
            .and_then(Value::as_str)
            .map(Path::new)
            .filter(|path| path.is_absolute())
            .ok_or(ModerationError::InvalidConfiguration(
                "credential_directory",
            ))?;
        let root = Dir::open_ambient_dir(path, ambient_authority()).map_err(|_| unavailable())?;
        let filename = format!("{}.{}.json", reference.name(), reference.revision());
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt as _;
            use rustix::fs::OFlags;
            options.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
        }
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt as _;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
        }
        let file = root
            .open_with(filename, &options)
            .map_err(|_| unavailable())?
            .into_std();
        let metadata = file.metadata().map_err(|_| unavailable())?;
        if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_BYTES {
            return Err(unavailable());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            if metadata.permissions().mode() & 0o037 != 0 {
                return Err(unavailable());
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
            if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(unavailable());
            }
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(metadata.len() as usize + 1));
        file.take(MAX_CREDENTIAL_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unavailable())?;
        if bytes.len() as u64 > MAX_CREDENTIAL_BYTES {
            return Err(unavailable());
        }
        serde_json::from_slice(&bytes)
            .map(ModerationCredentials)
            .map_err(|_| unavailable())
    }
}

fn unavailable() -> ModerationError {
    ModerationError::InvalidConfiguration("credential file")
}

pub(crate) enum DriverCredentials<'a> {
    Environment(&'a Environment),
    Referenced(ModerationCredentials),
}

impl<'a> DriverCredentials<'a> {
    pub(crate) fn resolve(
        env: &'a Environment,
        provider: &dyn ModerationCredentialProvider,
        profile: &Value,
    ) -> ModerationResult<Self> {
        let Some(reference) = CredentialReference::configured(profile)? else {
            return Ok(Self::Environment(env));
        };
        if profile.as_object().is_some_and(|profile| {
            profile
                .iter()
                .any(|(key, value)| key.ends_with("_env") && !value.is_null())
        }) {
            return Err(ModerationError::InvalidConfiguration(REFERENCE));
        }
        provider.resolve(&reference).map(Self::Referenced)
    }

    pub(crate) fn secret(
        &self,
        profile: &Value,
        environment_field: &'static str,
    ) -> ModerationResult<Option<Zeroizing<String>>> {
        match self {
            Self::Environment(env) => field(profile, environment_field)?
                .map(|variable| {
                    env.get_utf8(variable)
                        .ok()
                        .flatten()
                        .filter(|value| !value.trim().is_empty())
                        .map(|value| Zeroizing::new(value.to_owned()))
                        .ok_or(ModerationError::InvalidConfiguration(environment_field))
                })
                .transpose(),
            Self::Referenced(values) => values
                .get(environment_field.trim_end_matches("_env"))
                .map(|value| {
                    if value.trim().is_empty() {
                        return Err(ModerationError::InvalidConfiguration(REFERENCE));
                    }
                    Ok(Zeroizing::new(value.to_owned()))
                })
                .transpose(),
        }
    }
}
