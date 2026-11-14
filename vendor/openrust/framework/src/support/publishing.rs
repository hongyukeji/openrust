use std::{
    io,
    path::{Component, Path, PathBuf},
};

use crate::{
    contracts::foundation::Application as _, foundation::Application, support::ProviderResult,
};

/// A compiled package asset registered for the application's Artisan publisher.
#[derive(Clone)]
pub(crate) struct PublishableResource {
    pub(crate) provider: &'static str,
    pub(crate) tag: String,
    pub(crate) path: PathBuf,
    pub(crate) source: PublishSource,
    pub(crate) migration: bool,
}

#[derive(Clone)]
pub(crate) enum PublishSource {
    Embedded(&'static [u8]),
    Path(PathBuf),
}

impl PublishableResource {
    pub(crate) fn source_path(&self) -> PathBuf {
        match &self.source {
            PublishSource::Embedded(_) => PathBuf::from("embedded")
                .join(self.provider)
                .join(&self.path),
            PublishSource::Path(path) => path.clone(),
        }
    }
}

pub(crate) fn register(
    app: &Application,
    provider: &'static str,
    files: &[(&str, &'static [u8])],
    tag: &str,
    migration: bool,
) -> ProviderResult {
    register_sources(
        app,
        provider,
        files
            .iter()
            .map(|(path, bytes)| (Path::new(path), PublishSource::Embedded(bytes))),
        tag,
        migration,
    )
}

pub(crate) fn register_paths(
    app: &Application,
    provider: &'static str,
    paths: &[(&Path, &Path)],
    tag: &str,
) -> ProviderResult {
    register_sources(
        app,
        provider,
        paths
            .iter()
            .map(|(source, target)| (*target, PublishSource::Path(app.base_path().join(source)))),
        tag,
        false,
    )
}

fn register_sources<'a>(
    app: &Application,
    provider: &'static str,
    sources: impl IntoIterator<Item = (&'a Path, PublishSource)>,
    tag: &str,
    migration: bool,
) -> ProviderResult {
    let resources = sources
        .into_iter()
        .map(|(path, source)| {
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
                source,
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
