//! Provider 清单的持久化复用、身份和文件边界回归。

use super::super::ProviderEntry;
use super::super::deferred::{DeferrableProvider, Definition};
use super::{CachePaths, order, prepare};
use crate::config::EnvironmentSnapshot;
use crate::container::ServiceKey;
use crate::foundation::ProviderId;
use crate::support::ServiceProvider;
use crate::{Error, Result};
use std::fs;
use std::path::Path;

struct First;
struct Second;
impl ServiceProvider for First {}
impl ServiceProvider for Second {}
impl DeferrableProvider for First {
    fn provides() -> Vec<ServiceKey> {
        vec![
            ServiceKey::typed::<First>(),
            ServiceKey::named(ServiceKey::typed::<First>().name()),
            ServiceKey::typed::<Second>(),
        ]
    }
    fn when() -> Vec<&'static str> {
        vec!["manifest.event"]
    }
}

fn temporary() -> Result<tempfile::TempDir> {
    tempfile::tempdir().map_err(Error::internal)
}

fn eager(reverse: bool) -> Vec<ProviderEntry> {
    let mut entries = vec![
        ProviderEntry {
            provider: Box::new(First),
            force: false,
        },
        ProviderEntry {
            provider: Box::new(Second),
            force: false,
        },
    ];
    if reverse {
        entries.reverse();
    }
    entries
}

fn definitions() -> Vec<Definition> {
    vec![Definition::new(|| First)]
}

fn read(path: &Path) -> Result<serde_json::Value> {
    serde_json::from_slice(&fs::read(path).map_err(Error::internal)?).map_err(Error::internal)
}

fn mark(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = fs::read(path).map_err(Error::internal)?;
    bytes.extend_from_slice(b"\n \t\n");
    fs::write(path, &bytes).map_err(Error::internal)?;
    Ok(bytes)
}

#[test]
fn cold_manifest_and_hot_hit_keep_runtime_order_and_do_not_rewrite_bytes() -> Result<()> {
    let directory = temporary()?;
    let path = directory.path().join("services.json");
    let (providers, deferred) = prepare(&path, eager(false), definitions())?;
    assert_eq!(
        providers
            .iter()
            .map(|entry| entry.provider.id())
            .collect::<Vec<_>>(),
        [ProviderId::of::<First>(), ProviderId::of::<Second>()]
    );
    assert_eq!(deferred[0].id, ProviderId::of::<First>());
    let marked = mark(&path)?;
    let (providers, deferred) = prepare(&path, eager(false), definitions())?;
    assert_eq!(fs::read(&path).map_err(Error::internal)?, marked);
    assert_eq!(providers[0].provider.id(), ProviderId::of::<First>());
    assert_eq!(deferred[0].keys, First::provides());
    Ok(())
}

#[test]
fn eager_order_force_service_and_event_changes_recompile() -> Result<()> {
    let directory = temporary()?;
    let path = directory.path().join("services.json");
    let _ = prepare(&path, eager(false), definitions())?;
    let marked = mark(&path)?;
    let (providers, _) = prepare(&path, eager(true), definitions())?;
    assert_ne!(fs::read(&path).map_err(Error::internal)?, marked);
    assert_eq!(providers[0].provider.id(), ProviderId::of::<Second>());
    let marked = mark(&path)?;
    let mut providers = eager(true);
    providers[0].force = true;
    let _ = prepare(&path, providers, definitions())?;
    assert_ne!(fs::read(&path).map_err(Error::internal)?, marked);
    assert_eq!(read(&path)?["providers"][0]["force"], true);

    let marked = mark(&path)?;
    let mut changed = definitions();
    changed[0].keys.push(ServiceKey::named("manifest.new"));
    let mut providers = eager(true);
    providers[0].force = true;
    let _ = prepare(&path, providers, changed)?;
    assert_ne!(fs::read(&path).map_err(Error::internal)?, marked);
    assert_eq!(
        read(&path)?["deferred"][0]["keys"][3]["name"],
        "manifest.new"
    );
    let marked = mark(&path)?;
    let mut changed = definitions();
    changed[0].keys.push(ServiceKey::named("manifest.new"));
    changed[0].events = vec!["manifest.changed", "manifest.*"];
    let mut providers = eager(true);
    providers[0].force = true;
    let (_, selected) = prepare(&path, providers, changed)?;
    assert_ne!(fs::read(&path).map_err(Error::internal)?, marked);
    assert_eq!(selected[0].events, ["manifest.changed", "manifest.*"]);
    assert_eq!(
        read(&path)?["when"][0]["names"],
        serde_json::json!(["manifest.changed", "manifest.*"])
    );
    Ok(())
}

#[test]
fn typed_named_and_distinct_type_ids_survive_cache_round_trip() -> Result<()> {
    let directory = temporary()?;
    let path = directory.path().join("services.json");
    let _ = prepare(&path, Vec::new(), definitions())?;
    let json = read(&path)?;
    let keys = &json["deferred"][0]["keys"];
    assert_eq!(keys[0]["name"], keys[1]["name"]);
    assert_eq!(keys[0]["typed_identity"], 0);
    assert!(keys[1]["typed_identity"].is_null());
    assert_eq!(keys[2]["typed_identity"], 1);
    let (_, loaded) = prepare(&path, Vec::new(), definitions())?;
    assert_eq!(loaded[0].keys, First::provides());
    assert_ne!(loaded[0].keys[0], loaded[0].keys[1]);
    assert_ne!(loaded[0].keys[0].type_id(), loaded[0].keys[2].type_id());
    Ok(())
}

#[test]
fn corrupt_directory_and_missing_parent_fail_without_overwriting() -> Result<()> {
    let directory = temporary()?;
    let corrupt = directory.path().join("corrupt.json");
    fs::write(&corrupt, b"not-json").map_err(Error::internal)?;
    assert!(
        matches!(prepare(&corrupt, eager(false), definitions()), Err(error) if error.code() == "provider_manifest_invalid")
    );
    assert_eq!(fs::read(&corrupt).map_err(Error::internal)?, b"not-json");
    let non_file = directory.path().join("directory.json");
    fs::create_dir(&non_file).map_err(Error::internal)?;
    assert!(prepare(&non_file, eager(false), definitions()).is_err());
    assert!(non_file.is_dir());
    let absent = directory.path().join("absent/services.json");
    assert!(prepare(&absent, eager(false), definitions()).is_err());
    assert!(!absent.exists());
    Ok(())
}

#[test]
fn concurrent_first_writes_leave_one_valid_manifest_and_no_temporaries() -> Result<()> {
    let directory = temporary()?;
    let path = directory.path().join("services.json");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
    std::thread::scope(|scope| {
        let tasks = (0..4)
            .map(|_| {
                let path = &path;
                let barrier = std::sync::Arc::clone(&barrier);
                scope.spawn(move || {
                    barrier.wait();
                    prepare(path, eager(false), definitions()).map(|_| ())
                })
            })
            .collect::<Vec<_>>();
        for task in tasks {
            task.join()
                .map_err(|_| Error::custom("manifest_test_thread_failed", "清单测试线程失败"))??;
        }
        Ok::<(), Error>(())
    })?;
    assert_eq!(read(&path)?["eager"], serde_json::json!([0, 1]));
    let files = fs::read_dir(directory.path())
        .map_err(Error::internal)?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(Error::internal)?;
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].file_name(), "services.json");
    Ok(())
}

#[test]
fn runtime_order_rejects_duplicate_out_of_range_and_underflow_slots() -> Result<()> {
    assert_eq!(
        order(vec!["first", "second"], [4, 3], 3)?,
        ["second", "first"]
    );
    assert!(order(vec!["first", "second"], [3, 3], 3).is_err());
    assert!(order(vec!["first"], [4], 3).is_err());
    assert!(order(vec!["first"], [2], 3).is_err());
    Ok(())
}

#[test]
fn cache_path_conflicts_are_rejected_and_clear_preserves_configuration() -> Result<()> {
    let directory = temporary()?;
    let cache = directory.path().join("bootstrap/cache");
    fs::create_dir_all(&cache).map_err(Error::internal)?;
    let config = cache.join("config.json");
    fs::write(&config, b"configuration-sentinel").map_err(Error::internal)?;
    let environment = EnvironmentSnapshot::default();
    let paths = CachePaths::select(directory.path(), "TEST_", &environment, &config)?;
    fs::write(&paths.services, b"services").map_err(Error::internal)?;
    fs::write(cache.join("packages.json"), b"packages").map_err(Error::internal)?;
    paths.clear()?;
    paths.clear()?;
    assert_eq!(
        fs::read(&config).map_err(Error::internal)?,
        b"configuration-sentinel"
    );
    assert!(!paths.services.exists());
    assert!(!cache.join("packages.json").exists());
    fs::create_dir(&paths.services).map_err(Error::internal)?;
    assert!(paths.clear().is_err());
    assert!(paths.services.is_dir());
    let dotenv = directory.path().join(".env");
    fs::write(&dotenv, "TEST_SERVICES_CACHE=bootstrap/cache/config.json\n")
        .map_err(Error::internal)?;
    let collision = environment
        .with_dotenv_at(&dotenv)
        .map_err(Error::internal)?;
    assert!(CachePaths::select(directory.path(), "TEST_", &collision, &config).is_err());
    fs::write(&dotenv, "TEST_SERVICES_CACHE=bootstrap/cache/same.json\nTEST_PACKAGES_CACHE=bootstrap/cache/same.json\n").map_err(Error::internal)?;
    let collision = environment
        .with_dotenv_at(&dotenv)
        .map_err(Error::internal)?;
    assert!(CachePaths::select(directory.path(), "TEST_", &collision, &config).is_err());
    assert_eq!(
        fs::read(&config).map_err(Error::internal)?,
        b"configuration-sentinel"
    );
    Ok(())
}

#[test]
fn oversized_manifest_fails_without_replacement() -> Result<()> {
    let directory = temporary()?;
    let path = directory.path().join("services.json");
    let size = super::MAX_BYTES + 1;
    fs::File::create(&path)
        .and_then(|file| file.set_len(size))
        .map_err(Error::internal)?;
    assert!(
        matches!(prepare(&path, eager(false), definitions()), Err(error) if error.code() == "provider_manifest_invalid")
    );
    assert_eq!(fs::metadata(&path).map_err(Error::internal)?.len(), size);
    Ok(())
}

#[cfg(unix)]
#[test]
fn service_cache_symlink_cannot_be_loaded_or_cleared() -> Result<()> {
    let directory = temporary()?;
    let cache = directory.path().join("bootstrap/cache");
    fs::create_dir_all(&cache).map_err(Error::internal)?;
    let config = cache.join("config.json");
    let target = directory.path().join("sentinel");
    fs::write(&target, b"sentinel").map_err(Error::internal)?;
    let paths = CachePaths::select(
        directory.path(),
        "TEST_",
        &EnvironmentSnapshot::default(),
        &config,
    )?;
    std::os::unix::fs::symlink(&target, &paths.services).map_err(Error::internal)?;
    assert!(prepare(&paths.services, eager(false), definitions()).is_err());
    assert!(paths.clear().is_err());
    assert!(
        fs::symlink_metadata(&paths.services)
            .map_err(Error::internal)?
            .is_symlink()
    );
    assert_eq!(fs::read(&target).map_err(Error::internal)?, b"sentinel");
    Ok(())
}
