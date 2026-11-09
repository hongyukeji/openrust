//! 真实独立文件锁、持久状态与坏记录拒绝；不连接任何数据库。

use super::*;

#[cfg(unix)]
fn journal() -> Result<(tempfile::TempDir, OwnershipJournal)> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let journal = OwnershipJournal::new(&directory.path().join("state"))?;
    Ok((directory, journal))
}

#[cfg(unix)]
fn target(journal: &OwnershipJournal) -> Result<LockedOwnership> {
    journal.create(
        Backend::Mysql,
        "mysql:server-123",
        "mysql",
        "openrust_test_owned",
        ResourceKind::Target,
    )
}

#[cfg(unix)]
#[test]
fn independently_opened_files_and_scanners_cannot_take_a_live_record() -> Result<()> {
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    owner.persist(OwnershipState::Owned)?;
    let paths = journal.list_paths()?;
    assert_eq!(paths.len(), 1);
    let independent = OpenOptions::new()
        .read(true)
        .write(true)
        .open(paths[0].with_extension("lock"))
        .map_err(Error::internal)?;
    assert!(matches!(
        independent.try_lock(),
        Err(TryLockError::WouldBlock)
    ));
    assert!(journal.open_try_locked(&paths[0])?.is_none());
    assert!(paths[0].exists());
    drop(independent);
    drop(owner);
    let recovered = journal
        .open_try_locked(&paths[0])?
        .ok_or_else(|| failure("test_lock_not_released"))?;
    assert_eq!(recovered.load()?.state, OwnershipState::Owned);
    Ok(())
}

#[cfg(unix)]
#[test]
fn transitions_are_durable_and_done_keeps_the_original_lock_inode() -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    let path = journal.list_paths()?.remove(0);
    let lock_path = path.with_extension("lock");
    let initial_inode = fs::metadata(&lock_path).map_err(Error::internal)?.ino();
    let mut previous_data_inode = fs::metadata(&path).map_err(Error::internal)?.ino();
    let initial_token = owner.record().token.clone();
    for state in [
        OwnershipState::Intent,
        OwnershipState::Owned,
        OwnershipState::DropPending,
        OwnershipState::Ambiguous,
        OwnershipState::Done,
    ] {
        owner.persist(state)?;
        assert_eq!(owner.load()?.state, state);
        assert_eq!(
            fs::metadata(&lock_path).map_err(Error::internal)?.ino(),
            initial_inode
        );
        let current_data_inode = fs::metadata(&path).map_err(Error::internal)?.ino();
        assert_ne!(current_data_inode, previous_data_inode);
        previous_data_inode = current_data_inode;
    }
    assert!(
        matches!(owner.persist(OwnershipState::Intent), Err(error) if error.code() == "test_database_journal_done")
    );
    // 模拟 fork 后、exec 前仍共享同一打开文件对象的子进程。
    let inherited = owner.lock.0.try_clone().map_err(Error::internal)?;
    assert!(journal.open_try_locked(&path)?.is_none());
    drop(owner);
    assert!(path.is_file());
    assert!(lock_path.is_file());
    let mut tombstone = journal
        .open_try_locked(&path)?
        .ok_or_else(|| failure("test_lock_not_released"))?;
    assert_eq!(
        inherited.metadata().map_err(Error::internal)?.ino(),
        initial_inode
    );
    assert_eq!(
        tombstone.lock.0.metadata().map_err(Error::internal)?.ino(),
        initial_inode
    );
    drop(inherited);
    assert_eq!(tombstone.load()?.state, OwnershipState::Done);
    assert_eq!(tombstone.record().token, initial_token);
    assert!(
        matches!(tombstone.persist(OwnershipState::Owned), Err(error) if error.code() == "test_database_journal_done")
    );
    let next = target(&journal)?;
    assert_ne!(next.record().resource_id, tombstone.record().resource_id);
    assert_ne!(next.record().token, tombstone.record().token);
    assert_eq!(journal.list_paths()?.len(), 2);
    Ok(())
}

#[cfg(unix)]
#[test]
fn template_target_tokens_are_distinct_and_debug_hides_them() -> Result<()> {
    let (_directory, journal) = journal()?;
    let target = target(&journal)?;
    let template = journal.create(
        Backend::Postgres,
        "postgres:123456",
        "postgres",
        "openrust_test_owned_template",
        ResourceKind::Template,
    )?;
    assert_ne!(target.record().resource_id, template.record().resource_id);
    assert_ne!(target.record().token, template.record().token);
    assert_eq!(target.record().token.len(), 64);
    let record_debug = format!("{:?}", target.record());
    let handle_debug = format!("{target:?}");
    assert!(record_debug.contains("[REDACTED]"));
    assert!(!record_debug.contains(&target.record().token));
    assert!(!handle_debug.contains(&target.record().token));
    assert!(!format!("{template:?}").contains(&template.record().token));
    Ok(())
}

#[cfg(unix)]
#[test]
fn corrupt_and_unsupported_records_are_not_claimed_or_deleted() -> Result<()> {
    let (_directory, journal) = journal()?;
    let owner = target(&journal)?;
    let path = journal.list_paths()?.remove(0);
    let valid = serde_json::to_value(owner.record()).map_err(Error::internal)?;
    drop(owner);
    for (field, value) in [
        ("version", serde_json::json!(2)),
        ("backend", serde_json::json!("sqlite")),
        ("resource_kind", serde_json::json!("other")),
        ("state", serde_json::json!("maybe")),
        ("database_name", serde_json::json!("external_database")),
        ("token", serde_json::json!("SECRET_SHOULD_NOT_LEAK")),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        fs::write(
            &path,
            serde_json::to_vec(&invalid).map_err(Error::internal)?,
        )
        .map_err(Error::internal)?;
        let error = journal
            .open_try_locked(&path)
            .err()
            .ok_or_else(|| failure("test_invalid_record_accepted"))?;
        assert!(!format!("{error:?}").contains("SECRET_SHOULD_NOT_LEAK"));
        assert!(path.exists());
    }
    for invalid in [
        b"".as_slice(),
        b"{bad JSON SECRET_SHOULD_NOT_LEAK".as_slice(),
    ] {
        fs::write(&path, invalid).map_err(Error::internal)?;
        let error = journal
            .open_try_locked(&path)
            .err()
            .ok_or_else(|| failure("test_invalid_record_accepted"))?;
        assert_eq!(error.code(), "test_database_journal_invalid_record");
        assert!(!format!("{error:?}").contains("SECRET_SHOULD_NOT_LEAK"));
        assert!(path.exists());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn persist_does_not_repair_corruption_or_follow_a_replaced_inode() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let (directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    let path = journal.list_paths()?.remove(0);
    let original = fs::read(&path).map_err(Error::internal)?;
    fs::write(&path, b"corrupt").map_err(Error::internal)?;
    assert!(owner.load().is_err());
    assert!(owner.persist(OwnershipState::Owned).is_err());
    assert_eq!(fs::read(&path).map_err(Error::internal)?, b"corrupt");
    fs::rename(&path, directory.path().join("retired.json")).map_err(Error::internal)?;
    fs::write(&path, original).map_err(Error::internal)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(Error::internal)?;
    assert!(
        matches!(owner.load(), Err(error) if error.code() == "test_database_journal_identity_changed")
    );
    assert!(owner.persist(OwnershipState::Owned).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn unsupported_names_and_connection_like_identity_are_rejected_before_creation() -> Result<()> {
    let (_directory, journal) = journal()?;
    for name in [
        "",
        "openrust_",
        "OPENRUST_test",
        "myopenrust_test",
        "external",
        "openrust_test;drop",
        "openrust_数据库",
        "openrust_../escape",
    ] {
        assert!(
            matches!(journal.create(Backend::Mysql, "mysql:identity", "mysql", name, ResourceKind::Target), Err(error) if error.code() == "test_database_journal_identifier_invalid")
        );
    }
    assert!(
        journal
            .create(
                Backend::Mysql,
                "mysql:identity",
                "mysql",
                &format!("openrust_{}", "a".repeat(56)),
                ResourceKind::Target
            )
            .is_err()
    );
    let error = journal
        .create(
            Backend::Mysql,
            "mysql://user:SECRET_SHOULD_NOT_LEAK@host/db",
            "mysql",
            "openrust_valid",
            ResourceKind::Target,
        )
        .err()
        .ok_or_else(|| failure("test_identity_accepted_url"))?;
    assert!(!format!("{error:?}").contains("SECRET_SHOULD_NOT_LEAK"));
    assert_eq!(journal.list_paths()?, Vec::<std::path::PathBuf>::new());
    Ok(())
}

#[cfg(unix)]
#[test]
fn roots_and_records_reject_symlinks_non_regular_paths_and_non_private_modes() -> Result<()> {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (directory, journal) = journal()?;
    let root_link = directory.path().join("root-link");
    symlink(&journal.root, &root_link).map_err(Error::internal)?;
    assert!(OwnershipJournal::new(&root_link).is_err());
    assert!(OwnershipJournal::new(&directory.path().join("state/../other")).is_err());
    let owner = target(&journal)?;
    let path = journal.list_paths()?.remove(0);
    drop(owner);
    let outside = directory.path().join("outside.json");
    fs::rename(&path, &outside).map_err(Error::internal)?;
    symlink(&outside, &path).map_err(Error::internal)?;
    assert!(journal.open_try_locked(&path).is_err());
    assert!(journal.list_paths().is_err());
    fs::remove_file(&path).map_err(Error::internal)?;
    fs::create_dir(&path).map_err(Error::internal)?;
    assert!(journal.open_try_locked(&path).is_err());
    fs::remove_dir(&path).map_err(Error::internal)?;
    fs::rename(&outside, &path).map_err(Error::internal)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).map_err(Error::internal)?;
    assert!(journal.open_try_locked(&path).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).map_err(Error::internal)?;
    fs::set_permissions(&journal.root, fs::Permissions::from_mode(0o755))
        .map_err(Error::internal)?;
    assert!(OwnershipJournal::new(&journal.root).is_err());
    assert!(journal.list_paths().is_err());
    Ok(())
}

#[cfg(not(unix))]
#[test]
fn private_acl_is_explicitly_unsupported_without_a_platform_proof() -> Result<()> {
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    assert!(
        matches!(OwnershipJournal::new(&directory.path().join("state")), Err(error) if error.code() == "test_database_journal_private_acl_unsupported")
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn partial_temporary_write_failure_preserves_complete_previous_json_and_lock() -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    owner.persist(OwnershipState::Owned)?;
    let path = journal.list_paths()?.remove(0);
    let lock_path = path.with_extension("lock");
    let before = fs::read(&path).map_err(Error::internal)?;
    let lock_inode = fs::metadata(&lock_path).map_err(Error::internal)?.ino();
    let error = owner
        .persist_with_writer(OwnershipState::DropPending, |file, bytes| {
            file.write_all(&bytes[..bytes.len() / 2])?;
            Err(std::io::Error::other("controlled temporary write failure"))
        })
        .err()
        .ok_or_else(|| failure("test_write_failure_not_injected"))?;
    assert_eq!(error.code(), "test_database_journal_write_failed");
    assert_eq!(fs::read(&path).map_err(Error::internal)?, before);
    assert_eq!(owner.load()?.state, OwnershipState::Owned);
    assert_eq!(
        fs::metadata(&lock_path).map_err(Error::internal)?.ino(),
        lock_inode
    );
    assert_eq!(journal.list_paths()?, vec![path.clone()]);
    assert!(journal.open_try_locked(&path)?.is_none());
    assert_eq!(
        fs::metadata(&journal.root)
            .map_err(Error::internal)?
            .permissions()
            .mode()
            & 0o077,
        0
    );
    assert_eq!(
        fs::metadata(&path)
            .map_err(Error::internal)?
            .permissions()
            .mode()
            & 0o077,
        0
    );
    drop(owner);
    let reopened = journal
        .open_try_locked(&path)?
        .ok_or_else(|| failure("test_lock_not_released"))?;
    assert_eq!(reopened.load()?.state, OwnershipState::Owned);
    Ok(())
}

#[cfg(unix)]
#[test]
fn private_owner_validation_uses_effective_uid_not_permission_bits_alone() -> Result<()> {
    let current = rustix::process::geteuid().as_raw();
    validate_private_owner_mode(current, 0o700, current)?;
    validate_private_owner_mode(current, 0o600, current)?;
    let other = if current == u32::MAX { 0 } else { current + 1 };
    assert!(
        matches!(validate_private_owner_mode(other, 0o700, current), Err(error) if error.code() == "test_database_journal_owner_invalid")
    );
    assert!(
        matches!(validate_private_owner_mode(other, 0o600, current), Err(error) if error.code() == "test_database_journal_owner_invalid")
    );
    assert!(
        matches!(validate_private_owner_mode(current, 0o644, current), Err(error) if error.code() == "test_database_journal_permissions_invalid")
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn scanner_tolerates_only_disappearing_temporary_files() -> Result<()> {
    let (_directory, journal) = journal()?;
    let owner = target(&journal)?;
    let temporary = journal.root.join(format!(
        "{}.{}.tmp",
        owner.record().resource_id,
        "a".repeat(32)
    ));
    assert!(valid_temporary_name(&temporary));
    assert!(!validate_scanned_path(&temporary, true)?);
    assert!(validate_scanned_path(&temporary.with_extension("json"), false).is_err());
    assert!(validate_scanned_path(&temporary.with_extension("lock"), false).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn concurrent_atomic_writer_and_scanner_finish_without_transient_scan_errors() -> Result<()> {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicBool, Ordering},
    };
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    let resource = journal.list_paths()?.remove(0);
    let barrier = Arc::new(Barrier::new(2));
    let finished = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| -> Result<()> {
        let start = Arc::clone(&barrier);
        let done = Arc::clone(&finished);
        let writer = scope.spawn(move || -> Result<()> {
            start.wait();
            let result = (|| {
                for index in 0..96 {
                    owner.persist(if index % 2 == 0 {
                        OwnershipState::Owned
                    } else {
                        OwnershipState::DropPending
                    })?;
                    std::thread::yield_now();
                }
                Ok(())
            })();
            done.store(true, Ordering::Release);
            result
        });
        barrier.wait();
        let scan = (|| -> Result<()> {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
            loop {
                assert_eq!(journal.list_paths()?, vec![resource.clone()]);
                if finished.load(Ordering::Acquire) {
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    return Err(failure("test_writer_deadline"));
                }
                std::thread::yield_now();
            }
        })();
        let result = writer.join().map_err(|_| failure("test_writer_panicked"))?;
        scan?;
        result
    })
}

#[cfg(unix)]
#[test]
fn newly_created_parent_chain_is_private_and_remains_readable_from_a_fresh_journal() -> Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let directory = tempfile::tempdir().map_err(Error::internal)?;
    let root = directory.path().join("new-parent/new-child/state");
    let journal = OwnershipJournal::new(&root)?;
    let mut owner = target(&journal)?;
    owner.persist(OwnershipState::Owned)?;
    let path = journal.list_paths()?.remove(0);
    drop(owner);
    for path in [
        directory.path().join("new-parent"),
        directory.path().join("new-parent/new-child"),
        root.clone(),
    ] {
        let metadata = fs::symlink_metadata(path).map_err(Error::internal)?;
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
        assert_eq!(metadata.permissions().mode() & 0o077, 0);
    }
    let reopened = OwnershipJournal::new(&root)?;
    assert_eq!(
        reopened
            .open_try_locked(&path)?
            .ok_or_else(|| failure("test_lock_not_released"))?
            .load()?
            .state,
        OwnershipState::Owned
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn marker_confirmation_is_once_only_durable_and_idempotent_without_reviving() -> Result<()> {
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    assert!(owner.record().marker_identity.is_none());
    assert!(owner.record().last_error.is_none());
    let identity = super::super::marker::MarkerIdentity {
        backend: Backend::Mysql,
        database_name: owner.record().database_name.clone(),
        postgres_oid: None,
    };
    let mut invalid = identity.clone();
    invalid.database_name.push('2');
    assert!(owner.confirm_owned(invalid).is_err());
    owner.record_error(OwnershipState::Ambiguous, "create_unconfirmed")?;
    owner.confirm_owned(identity.clone())?;
    assert_eq!(owner.load()?.marker_identity, Some(identity.clone()));
    assert_eq!(owner.load()?.state, OwnershipState::Owned);
    owner.confirm_owned(identity.clone())?;
    owner.persist(OwnershipState::DropPending)?;
    owner.confirm_owned(identity.clone())?;
    assert_eq!(owner.load()?.state, OwnershipState::DropPending);
    owner.persist(OwnershipState::Done)?;
    owner.confirm_owned(identity.clone())?;
    assert_eq!(owner.load()?.state, OwnershipState::Done);
    assert!(owner.record_error(OwnershipState::Owned, "retry").is_err());
    let path = journal.list_paths()?.remove(0);
    drop(owner);
    let reopened = journal
        .open_try_locked(&path)?
        .ok_or_else(|| failure("test_lock_not_released"))?;
    assert_eq!(reopened.load()?.marker_identity, Some(identity));
    Ok(())
}

#[cfg(unix)]
#[test]
fn postgres_confirmation_requires_valid_oid_and_cannot_replace_it() -> Result<()> {
    let (_directory, journal) = journal()?;
    let mut owner = journal.create(
        Backend::Postgres,
        "123456",
        "postgres",
        "openrust_pg_marker",
        ResourceKind::Target,
    )?;
    let mut identity = super::super::marker::MarkerIdentity {
        backend: Backend::Postgres,
        database_name: owner.record().database_name.clone(),
        postgres_oid: None,
    };
    assert!(owner.confirm_owned(identity.clone()).is_err());
    identity.postgres_oid = Some(0);
    assert!(owner.confirm_owned(identity.clone()).is_err());
    identity.postgres_oid = Some(42);
    owner.confirm_owned(identity.clone())?;
    identity.postgres_oid = Some(43);
    assert!(
        matches!(owner.confirm_owned(identity), Err(error) if error.code() == "test_database_journal_marker_changed")
    );
    assert_eq!(
        owner
            .load()?
            .marker_identity
            .and_then(|identity| identity.postgres_oid),
        Some(42)
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn fixed_error_codes_reject_sensitive_text_and_corrupt_metadata_is_not_repaired() -> Result<()> {
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    let path = journal.list_paths()?.remove(0);
    let original = fs::read(&path).map_err(Error::internal)?;
    for code in [
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "",
        "mysql://user:SECRET@host/db",
        "Database ERR",
        "错误",
        "CODE",
    ] {
        assert!(
            matches!(owner.record_error(OwnershipState::Ambiguous, code), Err(error) if error.code() == "test_database_journal_error_code_invalid")
        );
        assert_eq!(fs::read(&path).map_err(Error::internal)?, original);
    }
    owner.record_error(OwnershipState::Ambiguous, "create_failed_123")?;
    assert_eq!(
        owner.load()?.last_error.as_deref(),
        Some("create_failed_123")
    );
    let valid = serde_json::to_value(owner.record()).map_err(Error::internal)?;
    for (field, value) in [
        ("last_error", serde_json::json!("mysql://SECRET@host/db")),
        (
            "marker_identity",
            serde_json::json!({"backend":"postgres","database_name":"openrust_test_owned","postgres_oid":42}),
        ),
        (
            "marker_identity",
            serde_json::json!({"backend":"mysql","database_name":"openrust_other","postgres_oid":null}),
        ),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        fs::write(
            &path,
            serde_json::to_vec(&invalid).map_err(Error::internal)?,
        )
        .map_err(Error::internal)?;
        let error = owner
            .load()
            .err()
            .ok_or_else(|| failure("test_invalid_record_accepted"))?;
        assert!(!format!("{error:?}").contains("SECRET"));
        assert!(owner.persist(OwnershipState::Owned).is_err());
    }
    // 语法合法但未经句柄写入的身份也必须被识别为篡改。
    let mut tampered = valid.clone();
    tampered["marker_identity"] = serde_json::json!({"backend":"mysql","database_name":"openrust_test_owned","postgres_oid":null});
    fs::write(
        &path,
        serde_json::to_vec(&tampered).map_err(Error::internal)?,
    )
    .map_err(Error::internal)?;
    assert!(
        matches!(owner.load(), Err(error) if error.code() == "test_database_journal_identity_changed")
    );
    drop(owner);
    // 重新打开同样拒绝损坏字段，而非将它当作可回收记录。
    tampered["last_error"] = serde_json::json!("SECRET invalid");
    fs::write(
        &path,
        serde_json::to_vec(&tampered).map_err(Error::internal)?,
    )
    .map_err(Error::internal)?;
    assert!(journal.open_try_locked(&path).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn older_records_default_optional_metadata_and_confirmation_rejects_unowned_stage() -> Result<()> {
    let (_directory, journal) = journal()?;
    let mut owner = target(&journal)?;
    let mut json = serde_json::to_value(owner.record()).map_err(Error::internal)?;
    let object = json
        .as_object_mut()
        .ok_or_else(|| failure("test_record_not_object"))?;
    object.remove("marker_identity");
    object.remove("last_error");
    let record: OwnershipRecord = serde_json::from_value(json).map_err(Error::internal)?;
    assert!(record.marker_identity.is_none());
    assert!(record.last_error.is_none());
    let identity = super::super::marker::MarkerIdentity {
        backend: record.backend,
        database_name: record.database_name,
        postgres_oid: None,
    };
    owner.persist(OwnershipState::DropPending)?;
    assert!(
        matches!(owner.confirm_owned(identity), Err(error) if error.code() == "test_database_journal_confirmation_invalid")
    );
    assert!(owner.load()?.marker_identity.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn management_database_names_round_trip_without_generated_name_restrictions() -> Result<()> {
    for admin in [
        "Audit-管理".to_owned(),
        "库".repeat(30),
        "Quote'`\nName".to_owned(),
    ] {
        let (_directory, journal) = journal()?;
        let owner = journal.create(
            Backend::Mysql,
            "server-123",
            &admin,
            "openrust_admin_name_test",
            ResourceKind::Target,
        )?;
        let path = journal.list_paths()?.remove(0);
        assert_eq!(owner.load()?.admin_database.as_bytes(), admin.as_bytes());
        drop(owner);
        let reopened = journal
            .open_try_locked(&path)?
            .ok_or_else(|| failure("test_lock_not_released"))?;
        assert_eq!(reopened.load()?.admin_database.as_bytes(), admin.as_bytes());
    }
    let (_directory, journal) = journal()?;
    for admin in ["", "invalid\0name"] {
        assert!(matches!(journal.create(Backend::Mysql, "server-123", admin,
            "openrust_admin_name_test", ResourceKind::Target),
            Err(error) if error.code() == "test_database_journal_identifier_invalid"));
    }
    Ok(())
}
