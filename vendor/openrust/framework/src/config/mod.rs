//! Environment snapshot and application configuration repository.

pub(crate) mod cache;
mod environment;
mod environment_file;
mod error;
mod repository;

pub use environment::Environment;
pub(crate) use environment_file::render_environment;
pub use environment_file::{
    EnvironmentDocument, EnvironmentEditor, EnvironmentFile, EnvironmentFileError,
    EnvironmentPosition, EnvironmentWriteOptions,
};
pub use error::ConfigError;
pub(crate) use repository::LocaleOverrides;
pub use repository::Repository;
pub use serde_json::Value;

impl EnvironmentFile {
    /// 使用应用实际绑定的 env.file，不猜测环境名或重新搜索 .env。
    pub fn for_application(
        app: &crate::foundation::Application,
    ) -> Result<Self, EnvironmentFileError> {
        use crate::contracts::{container::Container as _, foundation::Application as _};
        let path = app
            .container()
            .get::<std::path::PathBuf>("env.file")
            .map_err(|_| EnvironmentFileError::Unbound)?;
        Self::open(path.as_ref())
    }
}
