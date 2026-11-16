use std::{error::Error, fmt, io, path::PathBuf};

use crate::contracts::container::BindingResolutionError;

/// A configuration failure without the original configuration value.
#[derive(Debug)]
#[non_exhaustive]
pub enum ConfigError {
    Unavailable(BindingResolutionError),
    InvalidKey { key: String },
    InvalidType { key: String, expected: &'static str },
    InvalidEnvironmentName,
    InvalidEnvironmentFile { path: PathBuf },
    InvalidCache { path: PathBuf },
    ReadFailed { path: PathBuf, source: io::Error },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(source) => write!(f, "configuration unavailable: {source}"),
            Self::InvalidKey { key } => write!(f, "invalid configuration key {key}"),
            Self::InvalidType { key, expected } => write!(f, "{key} must be {expected}"),
            Self::InvalidEnvironmentName => f.write_str("invalid environment name"),
            Self::InvalidEnvironmentFile { path } => {
                write!(f, "invalid environment file syntax in {}", path.display())
            }
            Self::InvalidCache { path } => {
                write!(f, "invalid configuration cache in {}", path.display())
            }
            Self::ReadFailed { path, source } => {
                write!(f, "cannot read {}: {source}", path.display())
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unavailable(source) => Some(source),
            Self::ReadFailed { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<BindingResolutionError> for ConfigError {
    fn from(error: BindingResolutionError) -> Self {
        Self::Unavailable(error)
    }
}
