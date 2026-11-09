//! 正式引导的 Provider 清单缓存。编译身份和工厂始终由当前进程提供。

use std::fs::{self, File};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{ProviderEntry, deferred::Definition};
use crate::config::{EnvironmentSnapshot, compiled};
use crate::{Error, Result};

const FORMAT: u32 = 1;
const MAX_BYTES: u64 = 1024 * 1024;

pub(crate) struct CachePaths {
    pub(crate) services: PathBuf,
    packages: PathBuf,
}

impl CachePaths {
    pub(crate) fn select(
        root: &Path,
        namespace: &str,
        environment: &EnvironmentSnapshot,
        config: &Path,
    ) -> Result<Self> {
        let services = compiled::cache_path(
            root,
            namespace,
            environment,
            "SERVICES_CACHE",
            "bootstrap/cache/services.json",
        )?;
        let packages = compiled::cache_path(
            root,
            namespace,
            environment,
            "PACKAGES_CACHE",
            "bootstrap/cache/packages.json",
        )?;
        let paths: [&Path; 3] = [&services, &packages, config];
        let identities = paths.map(path_identity);
        let [services_id, packages_id, config_id] = identities;
        let (services_id, packages_id, config_id) = (services_id?, packages_id?, config_id?);
        if services_id == packages_id || services_id == config_id || packages_id == config_id {
            return Err(failure("配置、服务和包缓存必须使用不同文件"));
        }
        Ok(Self { services, packages })
    }

    pub(crate) fn clear(&self) -> Result<()> {
        for path in [&self.services, &self.packages] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Ok(metadata) if metadata.is_file() => match fs::remove_file(path) {
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    result => result.map_err(|_| failure("无法清除编译清单"))?,
                },
                _ => return Err(failure("编译清单路径不是可用的普通文件")),
            }
        }
        Ok(())
    }
}

fn path_identity(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path).map_err(|_| failure("无法解析缓存路径"))?;
    let Some((parent, filename)) = path.parent().zip(path.file_name()) else {
        return Err(failure("缓存路径必须指向文件"));
    };
    Ok(parent
        .canonicalize()
        .unwrap_or_else(|_| parent.to_path_buf())
        .join(filename))
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    providers: Vec<Provider>,
    eager: Vec<usize>,
    deferred: Vec<Services>,
    #[serde(default)]
    when: Vec<Events>,
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Provider {
    name: String,
    force: bool,
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Services {
    provider: usize,
    keys: Vec<Key>,
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Key {
    name: String,
    typed_identity: Option<usize>,
}

#[derive(Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Events {
    provider: usize,
    names: Vec<String>,
}

impl Manifest {
    fn compile(eager: &[ProviderEntry], deferred: &[Definition]) -> Self {
        let mut providers: Vec<_> = eager
            .iter()
            .map(|entry| Provider {
                name: entry.provider.id().name().to_owned(),
                force: entry.force,
            })
            .collect();
        let eager_slots = (0..providers.len()).collect();
        let mut services = Vec::with_capacity(deferred.len());
        let mut when = Vec::with_capacity(deferred.len());
        let mut typed = indexmap::IndexSet::new();
        for definition in deferred {
            let provider = providers.len();
            providers.push(Provider {
                name: definition.id.name().to_owned(),
                force: false,
            });
            let keys = definition
                .keys
                .iter()
                .map(|key| Key {
                    name: key.name().to_owned(),
                    typed_identity: key.type_id().map(|id| typed.insert_full(id).0),
                })
                .collect();
            services.push(Services { provider, keys });
            when.push(Events {
                provider,
                names: definition
                    .events
                    .iter()
                    .map(|name| (*name).to_owned())
                    .collect(),
            });
        }
        Self {
            format: FORMAT,
            providers,
            eager: eager_slots,
            deferred: services,
            when,
        }
    }

    fn load(path: &Path) -> Result<Option<Self>> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Ok(metadata) if metadata.is_file() => {}
            _ => return Err(failure("服务清单路径不是可用的普通文件")),
        }
        let file = File::open(path).map_err(|_| failure("无法读取服务清单"))?;
        if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return Err(failure("服务清单路径不是可用的普通文件"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| failure("无法读取服务清单"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(failure("服务清单超过大小上限"));
        }
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| failure("服务清单格式无效"))
    }

    fn write(&self, path: &Path) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|_| failure("无法编码服务清单"))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(failure("服务清单超过大小上限"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| failure("服务清单路径缺少父目录"))?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)
            .map_err(|_| failure("服务清单父目录必须存在且可写"))?;
        temp.write_all(&bytes)
            .and_then(|()| temp.as_file().sync_all())
            .map_err(|_| failure("无法写入服务清单"))?;
        temp.persist(path)
            .map_err(|_| failure("无法替换服务清单"))?;
        Ok(())
    }
}

pub(super) fn prepare(
    path: &Path,
    eager: Vec<ProviderEntry>,
    deferred: Vec<Definition>,
) -> Result<(Vec<ProviderEntry>, Vec<Definition>)> {
    let compiled = Manifest::compile(&eager, &deferred);
    let manifest = if let Some(cached) = Manifest::load(path)?
        && cached == compiled
    {
        cached
    } else {
        compiled.write(path)?;
        compiled
    };
    let eager_count = eager.len();
    let eager = order(eager, manifest.eager, 0)?;
    let deferred = order(
        deferred,
        manifest.deferred.into_iter().map(|entry| entry.provider),
        eager_count,
    )?;
    Ok((eager, deferred))
}

fn order<T>(
    entries: Vec<T>,
    slots: impl IntoIterator<Item = usize>,
    offset: usize,
) -> Result<Vec<T>> {
    let mut entries: Vec<_> = entries.into_iter().map(Some).collect();
    slots
        .into_iter()
        .map(|slot| {
            slot.checked_sub(offset)
                .and_then(|index| entries.get_mut(index))
                .and_then(Option::take)
                .ok_or_else(|| failure("服务清单引用了无效的编译条目"))
        })
        .collect()
}

fn failure(message: &str) -> Error {
    Error::custom_with_status(
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        "provider_manifest_invalid",
        message,
    )
}

#[cfg(test)]
#[path = "manifest_tests.rs"]
mod tests;
