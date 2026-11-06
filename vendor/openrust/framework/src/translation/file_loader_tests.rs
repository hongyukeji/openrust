//! 文件来源的离线不可变快照、命名空间和有界读取回归。
use super::*;
use serde_json::json;

// rustix 1.1.5未提供跨Apple/Linux可用的mkfifo；仅fixture借助POSIX工具创建。
#[cfg(unix)]
fn create_fifo_fixture(path: &std::path::Path) -> Result<()> {
    let status = std::process::Command::new("mkfifo")
        .args(["-m", "600"])
        .arg(path)
        .status()
        .map_err(Error::internal)?;
    if !status.success() {
        return Err(Error::internal(std::io::Error::other(format!(
            "POSIX mkfifo 测试fixture创建失败：{status}"
        ))));
    }
    Ok(())
}

#[tokio::test]
async fn global_group_and_namespace_resources_preserve_empty_and_snapshot() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    std::fs::write(
        root.path().join("en.json"),
        r#"{"hello":"hello","empty":""}"#,
    )
    .map_err(Error::internal)?;
    std::fs::create_dir(root.path().join("fr")).map_err(Error::internal)?;
    std::fs::write(
        root.path().join("fr/messages.json"),
        r#"{"hello":"bonjour"}"#,
    )
    .map_err(Error::internal)?;
    let module = tempfile::tempdir().map_err(Error::internal)?;
    std::fs::write(module.path().join("en.json"), r#"{"hello":"module"}"#)
        .map_err(Error::internal)?;
    let loader = FileTranslationLoader::new(root.path()).with_namespace("shop", module.path());
    let snapshot = loader.begin_snapshot().await?;
    assert_eq!(snapshot.resources().len(), 3);
    let en = Locale::new("en")?;
    assert_eq!(
        snapshot.load(&en, "*", None)?.get("empty"),
        Some(&json!(""))
    );
    assert_eq!(
        snapshot.load(&en, "*", Some("shop"))?.get("hello"),
        Some(&json!("module"))
    );
    assert_eq!(
        snapshot
            .load(&Locale::new("fr")?, "messages", None)?
            .get("hello"),
        Some(&json!("bonjour"))
    );
    std::fs::write(root.path().join("en.json"), r#"{"hello":"changed"}"#)
        .map_err(Error::internal)?;
    assert_eq!(
        snapshot.load(&en, "*", None)?.get("hello"),
        Some(&json!("hello"))
    );
    assert_eq!(
        loader
            .begin_snapshot()
            .await?
            .load(&en, "*", None)?
            .get("hello"),
        Some(&json!("changed"))
    );
    assert!(!loader.needs_refresh().await?);
    Ok(())
}

#[tokio::test]
async fn invalid_duplicate_oversize_and_bad_catalog_structures_fail_whole_snapshot() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    for contents in [
        r#"{"same":"a","same":"b"}"#,
        r#"{"bad":{"select":{"argument":"gender","cases":{"other":"a","other":"b"}}}}"#,
        r#"{"bad":42}"#,
        r#"{"bad":{"plural":{"argument":"n","forms":{"one":"one"}}}}"#,
    ] {
        std::fs::write(root.path().join("en.json"), contents).map_err(Error::internal)?;
        assert!(
            FileTranslationLoader::new(root.path())
                .begin_snapshot()
                .await
                .is_err()
        );
    }
    std::fs::write(root.path().join("en.json"), r#"{"key":"long value"}"#)
        .map_err(Error::internal)?;
    assert!(
        matches!(FileTranslationLoader::new(root.path()).with_limits(1, 2)?.begin_snapshot().await,
        Err(error) if error.code() == "i18n_file_source_limit")
    );
    std::fs::write(root.path().join("fr.json"), "{}").map_err(Error::internal)?;
    assert!(
        FileTranslationLoader::new(root.path())
            .with_limits(1, 1024)?
            .begin_snapshot()
            .await
            .is_err()
    );
    assert!(
        FileTranslationLoader::new(root.path())
            .with_limits(0, 100)
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn repeated_namespace_resources_fail_instead_of_silently_replacing() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    std::fs::write(root.path().join("en.json"), r#"{"key":"value"}"#).map_err(Error::internal)?;
    let loader = FileTranslationLoader::new(root.path())
        .with_namespace("shop", root.path())
        .with_namespace("shop", root.path());
    assert!(loader.begin_snapshot().await.is_err());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn fifo_is_rejected_before_blocking_and_open_rechecks_actual_handle_type() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let fifo = root.path().join("en.json");
    create_fifo_fixture(&fifo)?;
    assert!(open_regular(&fifo).is_err());
    let loader = FileTranslationLoader::new(root.path());
    let result = tokio::time::timeout(std::time::Duration::from_secs(1), loader.begin_snapshot())
        .await
        .map_err(Error::internal)?;
    assert!(result.is_err());
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn trusted_regular_file_symlinks_keep_original_locale_and_group_addresses() -> Result<()> {
    let root = tempfile::tempdir().map_err(Error::internal)?;
    let release = tempfile::tempdir().map_err(Error::internal)?;
    std::fs::write(
        release.path().join("release-v2.json"),
        r#"{"hello":"linked release"}"#,
    )
    .map_err(Error::internal)?;
    std::os::unix::fs::symlink(
        release.path().join("release-v2.json"),
        root.path().join("en.json"),
    )
    .map_err(Error::internal)?;
    std::fs::create_dir(root.path().join("fr")).map_err(Error::internal)?;
    std::os::unix::fs::symlink(
        release.path().join("release-v2.json"),
        root.path().join("fr/messages.json"),
    )
    .map_err(Error::internal)?;
    let snapshot = FileTranslationLoader::new(root.path())
        .begin_snapshot()
        .await?;
    assert_eq!(snapshot.resources().len(), 2);
    assert_eq!(
        snapshot.load(&Locale::new("en")?, "*", None)?.get("hello"),
        Some(&json!("linked release"))
    );
    assert_eq!(
        snapshot
            .load(&Locale::new("fr")?, "messages", None)?
            .get("hello"),
        Some(&json!("linked release"))
    );
    Ok(())
}
#[cfg(unix)]
#[tokio::test]
async fn trusted_links_to_fifo_broken_targets_or_cycles_fail_without_waiting_for_a_writer()
-> Result<()> {
    for kind in ["fifo", "broken", "cycle"] {
        let root = tempfile::tempdir().map_err(Error::internal)?;
        let release = tempfile::tempdir().map_err(Error::internal)?;
        let target = release.path().join("target");
        let link = root.path().join("en.json");
        match kind {
            "fifo" => create_fifo_fixture(&target)?,
            "cycle" => std::os::unix::fs::symlink(&link, &target).map_err(Error::internal)?,
            _ => {}
        }
        std::os::unix::fs::symlink(&target, &link).map_err(Error::internal)?;
        assert!(open_regular(&link).is_err());
        let loader = FileTranslationLoader::new(root.path());
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), loader.begin_snapshot())
                .await
                .map_err(Error::internal)?
                .is_err()
        );
    }
    Ok(())
}
