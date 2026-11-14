use std::{
    io,
    path::{Component, Path, PathBuf},
};

use crate::{foundation::Application, support::ProviderResult};

/// A compiled package asset registered for the application's Artisan publisher.
#[derive(Clone)]
pub(crate) struct PublishableResource {
    pub(crate) provider: &'static str,
    pub(crate) tag: String,
    pub(crate) path: PathBuf,
    pub(crate) bytes: &'static [u8],
    pub(crate) migration: bool,
}

pub(crate) fn register(
    app: &Application,
    provider: &'static str,
    files: &[(&str, &'static [u8])],
    tag: &str,
    migration: bool,
) -> ProviderResult {
    let resources = files
        .iter()
        .map(|(path, bytes)| {
            let path = Path::new(path);
            if path.as_os_str().is_empty()
                || path
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "publish destination must be relative to the application",
                ));
            }
            Ok(PublishableResource {
                provider,
                tag: tag.into(),
                path: path.into(),
                bytes,
                migration,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    app.publishable()
        .lock()
        .expect("publish registry lock")
        .extend(resources);
    Ok(())
}
