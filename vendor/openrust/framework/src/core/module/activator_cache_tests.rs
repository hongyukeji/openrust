//! 默认文件激活器保留实例快照；自身修改和物理文件合并分别验证。

use super::*;
use serde_json::json;

fn persisted(path: &Path) -> Result<Value> {
    serde_json::from_slice(&fs::read(path).map_err(Error::internal)?).map_err(Error::internal)
}

fn failure<T>(result: Result<T>) -> Result<Error> {
    match result {
        Err(error) => Ok(error),
        Ok(_) => Err(Error::custom(
            "test_expected_activation_failure",
            "预期模块状态操作失败",
        )),
    }
}

#[test]
fn construction_captures_file_and_only_a_new_instance_sees_external_changes() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    fs::write(&path, br#"{"Blog":true,"initial":null}"#).map_err(Error::internal)?;
    let original = FileModuleActivator::new(&path)?;
    let before = original.snapshot()?;

    fs::write(&path, br#"{"Blog":false,"External":true}"#).map_err(Error::internal)?;
    assert!(original.has_status("Blog", true)?);
    assert_eq!(original.snapshot()?.value("External"), None);
    assert_eq!(original.snapshot()?.value("initial"), Some(&Value::Null));
    assert!(before.is_enabled("Blog"));
    assert_eq!(original.enabled()?, HashSet::from(["Blog".to_owned()]));

    let restarted = FileModuleActivator::new(&path)?;
    assert!(restarted.has_status("Blog", false)?);
    assert!(restarted.has_status("External", true)?);
    assert_eq!(restarted.snapshot()?.value("initial"), None);
    assert!(original.has_status("Blog", true)?);
    Ok(())
}

#[test]
fn independent_writers_merge_disk_entries_without_absorbing_foreign_cache_changes() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    let unknown = json!({"nested":[null, {"keep":17}]});
    fs::write(&path, json!({"Blog":false,"unknown":unknown}).to_string())
        .map_err(Error::internal)?;
    let first = FileModuleActivator::new(&path)?;
    let second = FileModuleActivator::new(&path)?;

    first.set_active_by_name("Blog", true)?;
    second.set_active_by_name("Other", true)?;
    assert!(first.has_status("Blog", true)?);
    assert_eq!(first.snapshot()?.value("Other"), None);
    assert!(second.has_status("Blog", false)?);
    assert!(second.has_status("Other", true)?);
    assert_eq!(
        persisted(&path)?,
        json!({"Blog":true,"unknown":unknown,"Other":true})
    );

    first.delete("Blog")?;
    assert_eq!(first.snapshot()?.value("Blog"), None);
    assert_eq!(first.snapshot()?.value("Other"), None);
    assert!(second.has_status("Blog", false)?);
    assert_eq!(persisted(&path)?, json!({"unknown":unknown,"Other":true}));
    let restarted = FileModuleActivator::new(&path)?;
    assert_eq!(restarted.snapshot()?.value("Blog"), None);
    assert!(restarted.has_status("Other", true)?);
    assert_eq!(restarted.snapshot()?.value("unknown"), Some(&unknown));
    Ok(())
}

#[test]
fn busy_set_and_delete_keep_memory_changes_for_the_next_successful_write() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    let bytes = br#"{"Delete":true,"Existing":true,"unknown":{"keep":17}}"#;
    fs::write(&path, bytes).map_err(Error::internal)?;
    let activator = FileModuleActivator::new(&path)?;
    let locked = LockedTarget::acquire(&path)?;

    assert_eq!(
        failure(activator.set_active_by_name("Pending", true))?.code(),
        "module_state_busy"
    );
    assert_eq!(
        failure(activator.delete("Delete"))?.code(),
        "module_state_busy"
    );
    assert!(activator.has_status("Pending", true)?);
    assert_eq!(activator.snapshot()?.value("Delete"), None);
    assert_eq!(fs::read(&path).map_err(Error::internal)?, bytes);
    drop(locked);

    let foreign = FileModuleActivator::new(&path)?;
    foreign.set_active_by_name("Foreign", true)?;
    assert_eq!(activator.snapshot()?.value("Foreign"), None);
    activator.set_active_by_name("Later", false)?;
    assert_eq!(
        persisted(&path)?,
        json!({
            "Existing":true,"unknown":{"keep":17},
            "Foreign":true,"Pending":true,"Later":false
        })
    );
    assert_eq!(activator.snapshot()?.value("Foreign"), None);
    assert_eq!(activator.snapshot()?.value("Delete"), None);
    assert!(activator.has_status("Pending", true)?);
    assert!(activator.has_status("Later", false)?);
    Ok(())
}

#[test]
fn cached_null_and_missing_delete_do_not_lock_or_rewrite_external_values() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    fs::write(&path, br#"{"Null":null,"Own":true}"#).map_err(Error::internal)?;
    let activator = FileModuleActivator::new(&path)?;
    let external = br#"{ "Null":true, "Missing":true, "Own":false }"#;
    fs::write(&path, external).map_err(Error::internal)?;
    let locked = LockedTarget::acquire(&path)?;

    activator.delete("Null")?;
    activator.delete("Missing")?;
    assert_eq!(fs::read(&path).map_err(Error::internal)?, external);
    assert_eq!(activator.snapshot()?.value("Null"), Some(&Value::Null));
    assert_eq!(activator.snapshot()?.value("Missing"), None);
    assert!(activator.has_status("Own", true)?);
    drop(locked);
    Ok(())
}

#[test]
fn delete_rebuilds_remaining_cache_but_reset_only_clears_after_external_file_removal() -> Result<()>
{
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    fs::write(&path, br#"{"Blog":true,"Other":false}"#).map_err(Error::internal)?;
    let activator = FileModuleActivator::new(&path)?;
    fs::remove_file(&path).map_err(Error::internal)?;

    activator.delete("Blog")?;
    assert_eq!(activator.snapshot()?.value("Blog"), None);
    assert!(activator.has_status("Other", false)?);
    assert_eq!(persisted(&path)?, json!({"Other":false}));
    fs::remove_file(&path).map_err(Error::internal)?;
    activator.reset()?;
    assert!(activator.snapshot()?.entries().is_empty());
    assert!(!path.exists());
    activator.set_active_by_name("Restored", true)?;
    assert_eq!(persisted(&path)?, json!({"Restored":true}));
    Ok(())
}

#[test]
fn malformed_file_fails_construction_and_static_reset_allows_manual_recovery() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    for bytes in [b"{broken".as_slice(), b"[true]", b"null", b"false", b"\xff"] {
        fs::write(&path, bytes).map_err(Error::internal)?;
        assert_eq!(
            failure(FileModuleActivator::new(&path))?.code(),
            "module_state_invalid"
        );
        assert_eq!(fs::read(&path).map_err(Error::internal)?, bytes);
        FileModuleActivator::reset_file(&path)?;
        assert!(!path.exists());
        let recovered = FileModuleActivator::new(&path)?;
        assert!(recovered.snapshot()?.entries().is_empty());
        recovered.set_active_by_name("Blog", true)?;
        assert_eq!(persisted(&path)?, json!({"Blog":true}));
    }
    Ok(())
}

#[test]
fn own_numeric_list_write_remains_cached_and_can_be_extended_into_an_object() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    let activator = FileModuleActivator::new(&path)?;
    activator.set_active_by_name("0", true)?;
    assert_eq!(persisted(&path)?, json!([true]));
    assert!(activator.has_status("0", true)?);
    assert_eq!(
        failure(FileModuleActivator::new(&path))?.code(),
        "module_state_invalid"
    );

    activator.set_active_by_name("Other", true)?;
    assert!(activator.has_status("0", true)?);
    assert!(activator.has_status("Other", true)?);
    assert_eq!(persisted(&path)?, json!({"0":true,"Other":true}));
    let restarted = FileModuleActivator::new(&path)?;
    assert!(restarted.has_status("0", true)?);
    assert!(restarted.has_status("Other", true)?);
    Ok(())
}

#[test]
fn externally_reencoded_list_does_not_receive_the_own_last_write_exception() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    let activator = FileModuleActivator::new(&path)?;
    activator.set_active_by_name("0", true)?;
    let mut external = fs::read(&path).map_err(Error::internal)?;
    external.push(b' ');
    fs::write(&path, &external).map_err(Error::internal)?;

    assert_eq!(
        failure(activator.set_active_by_name("Other", true))?.code(),
        "module_state_invalid"
    );
    assert!(activator.has_status("0", true)?);
    assert!(activator.has_status("Other", true)?);
    assert_eq!(fs::read(&path).map_err(Error::internal)?, external);
    Ok(())
}

#[test]
fn explicit_live_reads_observe_external_changes_while_default_cache_stays_stable() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    fs::write(&path, br#"{"Blog":false}"#).map_err(Error::internal)?;
    let cached = FileModuleActivator::new(&path)?;
    let live = FileModuleActivator::new(&path)?.with_live_reads();
    fs::write(&path, br#"{"Blog":true,"External":true}"#).map_err(Error::internal)?;

    assert!(cached.has_status("Blog", false)?);
    assert_eq!(cached.snapshot()?.value("External"), None);
    assert!(live.has_status("Blog", true)?);
    assert!(live.snapshot()?.is_enabled("External"));
    fs::remove_file(&path).map_err(Error::internal)?;
    assert!(live.snapshot()?.entries().is_empty());
    assert!(cached.has_status("Blog", false)?);
    Ok(())
}

#[cfg(unix)]
#[test]
fn identical_foreign_list_at_retargeted_symlink_is_not_an_own_commit() -> Result<()> {
    use std::os::unix::fs::symlink;
    for live in [false, true] {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let original = root.path().join("original.json");
        let foreign = root.path().join("foreign.json");
        let alias = root.path().join("state.json");
        fs::write(&original, b"{}").map_err(Error::internal)?;
        symlink(&original, &alias).map_err(Error::internal)?;
        let driver = FileModuleActivator::new(&alias)?;
        let driver = if live {
            driver.with_live_reads()
        } else {
            driver
        };
        driver.set_active_by_name("0", true)?;
        let bytes = fs::read(&original).map_err(Error::internal)?;
        fs::write(&foreign, &bytes).map_err(Error::internal)?;
        fs::remove_file(&alias).map_err(Error::internal)?;
        symlink(&foreign, &alias).map_err(Error::internal)?;
        if live {
            assert_eq!(failure(driver.snapshot())?.code(), "module_state_invalid");
            assert_eq!(
                failure(driver.has_status("0", true))?.code(),
                "module_state_invalid"
            );
        }
        assert_eq!(
            failure(driver.set_active_by_name("Other", true))?.code(),
            "module_state_invalid"
        );
        assert_eq!(fs::read(&foreign).map_err(Error::internal)?, bytes);
        assert_eq!(fs::read(&original).map_err(Error::internal)?, bytes);
        if !live {
            assert!(driver.has_status("Other", true)?);
        }
    }
    Ok(())
}

#[test]
fn live_driver_reads_its_own_numeric_write_but_new_driver_still_rejects_list() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let path = root.path().join("state.json");
    let live = FileModuleActivator::new(&path)?.with_live_reads();
    live.set_active_by_name("0", true)?;
    let own_list = fs::read(&path).map_err(Error::internal)?;
    assert!(live.has_status("0", true)?);
    assert_eq!(
        failure(FileModuleActivator::new(&path))?.code(),
        "module_state_invalid"
    );
    live.set_active_by_name("Other", true)?;
    assert!(live.has_status("0", true)?);
    assert!(live.has_status("Other", true)?);
    let mut foreign = own_list;
    foreign.push(b' ');
    fs::write(&path, &foreign).map_err(Error::internal)?;
    assert_eq!(failure(live.snapshot())?.code(), "module_state_invalid");
    assert_eq!(
        failure(live.set_active_by_name("Next", true))?.code(),
        "module_state_invalid"
    );
    assert_eq!(fs::read(&path).map_err(Error::internal)?, foreign);
    Ok(())
}
