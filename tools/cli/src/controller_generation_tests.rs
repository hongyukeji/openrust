//! 模块声明预检、属性归属及普通 I/O 失败补偿的针对性回归。

use super::*;
use std::collections::BTreeMap;

type Snapshot = BTreeMap<PathBuf, Option<Vec<u8>>>;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "openrust-controller-v2-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root)?;
        let fixture = Self(root);
        fs::create_dir_all(fixture.controllers())?;
        fs::write(
            fixture.controllers().join("mod.rs"),
            "//! 控制器。\n\npub mod home;\n",
        )?;
        fs::write(fixture.controllers().join("home.rs"), "pub struct Home;\n")?;
        Ok(fixture)
    }

    fn controllers(&self) -> PathBuf {
        self.0.join("app/http/controllers")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("controller_fixture_cleanup_failed: {error}");
            assert!(std::thread::panicking(), "控制器 fixture 清理失败");
        }
    }
}

fn snapshot(root: &Path) -> anyhow::Result<Snapshot> {
    fn visit(root: &Path, directory: &Path, result: &mut Snapshot) -> anyhow::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(root)?.to_path_buf();
            if entry.file_type()?.is_dir() {
                result.insert(relative, None);
                visit(root, &path, result)?;
            } else {
                result.insert(relative, Some(fs::read(path)?));
            }
        }
        Ok(())
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result)?;
    Ok(result)
}

#[test]
fn ambiguous_same_name_declarations_fail_before_any_write() -> anyhow::Result<()> {
    for declaration in [
        "mod admin;\n",
        "pub(crate) mod admin;\n",
        "pub(super) mod admin;\n",
        "pub mod r#admin;\n",
        "pub mod admin {}\n",
        "#[cfg(any())]\npub mod admin;\n",
        "#[path = \"different.rs\"]\npub mod admin;\n",
        "pub mod admin;\npub mod admin;\n",
        "#[cfg(any())] pub mod admin; #[cfg(any())]\npub mod other;\n",
        "pub mod\nadmin;\n",
    ] {
        let fixture = Fixture::new()?;
        fs::write(fixture.controllers().join("mod.rs"), declaration)?;
        let before = snapshot(&fixture.0)?;
        assert!(
            generate("Admin/UserController", &fixture.0).is_err(),
            "{declaration}"
        );
        assert_eq!(snapshot(&fixture.0)?, before);
    }
    Ok(())
}

#[test]
fn existing_unconditional_namespace_with_comment_is_reused() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let registry = "//! 控制器。\npub mod admin; // 已有命名空间\npub mod home;\n";
    fs::write(fixture.controllers().join("mod.rs"), registry)?;
    generate("Admin/UserController", &fixture.0)?;
    assert_eq!(
        fs::read_to_string(fixture.controllers().join("mod.rs"))?,
        registry
    );
    assert!(fixture.controllers().join("admin/user.rs").is_file());
    // 没有对应文件的 leaf 声明也不能被悄悄补成另一项。
    fs::write(
        fixture.controllers().join("admin/mod.rs"),
        "pub mod missing;\npub mod user;\n",
    )?;
    let before = snapshot(&fixture.0)?;
    assert!(generate("Admin/MissingController", &fixture.0).is_err());
    assert_eq!(snapshot(&fixture.0)?, before);
    Ok(())
}

#[test]
fn insertion_keeps_cfg_path_and_documentation_on_original_items() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let registry = concat!(
        "//! 控制器。\n\n",
        "// 原注释\n#[cfg(any())]\n/// 原 zebra 说明\npub mod zebra; // 保留\n",
        "#[path = \"other.rs\"]\npub mod remote;\n",
    );
    fs::write(fixture.controllers().join("mod.rs"), registry)?;
    fs::write(
        fixture.controllers().join("other.rs"),
        "pub struct Other;\n",
    )?;
    generate("Admin/UserController", &fixture.0)?;
    let actual = fs::read_to_string(fixture.controllers().join("mod.rs"))?;
    assert_eq!(actual.replacen("pub mod admin;\n", "", 1), registry);
    assert!(actual.contains(
        "pub mod admin;\n\n// 原注释\n#[cfg(any())]\n/// 原 zebra 说明\npub mod zebra; // 保留\n"
    ));
    assert!(actual.contains("#[path = \"other.rs\"]\npub mod remote;\n"));
    Ok(())
}

#[test]
fn unsupported_or_unattached_items_are_rejected_without_guessing() {
    for registry in [
        "#[cfg(\n any()\n)]\npub mod zebra;\n",
        "/* custom block */\npub mod zebra;\n",
        "fn custom() {}\n",
        "#[cfg(any())]\n",
        "/// unattached\n",
        "#[path = r#\"other.rs\"#]\npub mod other;\n",
    ] {
        assert!(register(registry, "admin", true).is_err(), "{registry}");
    }
}

fn injected() -> anyhow::Error {
    std::io::Error::new(
        std::io::ErrorKind::WriteZero,
        "controller_injected_io_failure",
    )
    .into()
}

#[test]
fn every_prepare_and_install_failure_restores_bytes_directories_and_retry() -> anyhow::Result<()> {
    // 先从同一生产提交路径收集实际事件，不以臆测的写入次数安排故障。
    let fixture = Fixture::new()?;
    let prepared = plan("Admin/Reports/UserController", &fixture.0)?;
    let mut events = Vec::new();
    apply(&prepared, &mut |step, _| {
        events.push(step);
        Ok(())
    })?;
    let events = events
        .into_iter()
        .filter(|step| *step != Step::Cleanup)
        .collect::<Vec<_>>();
    assert!(events.contains(&Step::Backup));
    assert!(events.contains(&Step::Install));
    for (failure, _) in events.iter().enumerate() {
        let fixture = Fixture::new()?;
        let before = snapshot(&fixture.0)?;
        let prepared = plan("Admin/Reports/UserController", &fixture.0)?;
        let mut event = 0;
        let result = apply(&prepared, &mut |step, path| {
            if matches!(step, Step::Rollback | Step::Cleanup) {
                return Ok(());
            }
            let fail = event == failure;
            event += 1;
            if fail {
                if matches!(step, Step::Stage | Step::Backup) {
                    // 真正留下不完整暂存文件，后续必须清掉，原 registry 不能被截短。
                    fs::write(path, b"partial")?;
                }
                return Err(injected());
            }
            Ok(())
        });
        let Err(error) = result else {
            bail!("故障必须返回错误");
        };
        assert!(format!("{error:#}").contains("controller_injected_io_failure"));
        assert_eq!(snapshot(&fixture.0)?, before, "failure={failure}");
        generate("Admin/Reports/UserController", &fixture.0)?;
        assert!(
            fixture
                .controllers()
                .join("admin/reports/user.rs")
                .is_file()
        );
    }
    Ok(())
}

#[test]
fn failed_restore_preserves_primary_secondary_and_backup_evidence() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let registry = fixture.controllers().join("mod.rs");
    let original = fs::read(&registry)?;
    let prepared = plan("Admin/UserController", &fixture.0)?;
    let Err(error) = apply(&prepared, &mut |step, path| {
        if step == Step::Installed && path == registry {
            return Err(injected());
        }
        if step == Step::Rollback && path == registry {
            bail!("controller_injected_restore_failure");
        }
        Ok(())
    }) else {
        bail!("安装及恢复故障必须共同报告");
    };
    let diagnostic = format!("{error:#}");
    assert!(diagnostic.contains("controller_injected_io_failure"));
    assert!(diagnostic.contains("controller_injected_restore_failure"));
    assert!(diagnostic.contains("controller_cleanup_failed"));
    assert_ne!(fs::read(registry)?, original);
    assert!(
        !fixture
            .controllers()
            .join(".openrust-controller.lock")
            .exists()
    );
    // 恢复被拒时备份必须仍保留原字节，不能假报已恢复。
    assert!(snapshot(&fixture.0)?.iter().any(|(path, bytes)| {
        path.extension()
            .is_some_and(|extension| extension == "backup")
            && bytes.as_deref() == Some(original.as_slice())
    }));
    Ok(())
}

#[test]
fn cleanup_error_after_commit_does_not_claim_generation_was_rolled_back() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let prepared = plan("Admin/UserController", &fixture.0)?;
    let Err(error) = apply(&prepared, &mut |step, _| {
        if step == Step::Cleanup {
            bail!("controller_injected_cleanup_failure");
        }
        Ok(())
    }) else {
        bail!("提交后的清理错误应报告已生成");
    };
    assert!(
        error
            .to_string()
            .contains("controller_generated_cleanup_failed")
    );
    assert!(fixture.controllers().join("admin/user.rs").is_file());
    assert!(fs::read_to_string(fixture.controllers().join("mod.rs"))?.contains("pub mod admin;"));
    Ok(())
}

#[test]
fn stale_plan_and_active_generator_do_not_overwrite_other_work() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let prepared = plan("Admin/UserController", &fixture.0)?;
    let registry = fixture.controllers().join("mod.rs");
    fs::write(&registry, "pub mod other;\n")?;
    let before = snapshot(&fixture.0)?;
    assert!(apply(&prepared, &mut |_, _| Ok(())).is_err());
    assert_eq!(snapshot(&fixture.0)?, before);
    let lock = fixture.controllers().join(".openrust-controller.lock");
    fs::write(&lock, "another invocation")?;
    assert!(generate("Admin/UserController", &fixture.0).is_err());
    assert_eq!(fs::read_to_string(lock)?, "another invocation");
    Ok(())
}

#[test]
fn actual_exclusive_install_error_preserves_a_racing_leaf() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let prepared = plan("AuditController", &fixture.0)?;
    let leaf = fixture.controllers().join("audit.rs");
    let mut expected = snapshot(&fixture.0)?;
    expected.insert(
        leaf.strip_prefix(&fixture.0)?.to_path_buf(),
        Some(b"external work".to_vec()),
    );
    let Err(error) = apply(&prepared, &mut |step, path| {
        if step == Step::Install && path == leaf {
            fs::write(path, b"external work")?;
        }
        Ok(())
    }) else {
        bail!("真实 hard_link 必须拒绝安装到已存在的文件");
    };
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| error.kind() == std::io::ErrorKind::AlreadyExists)
    }));
    assert_eq!(snapshot(&fixture.0)?, expected);
    Ok(())
}

#[test]
fn reused_parent_registry_is_checked_even_when_not_written() -> anyhow::Result<()> {
    let fixture = Fixture::new()?;
    let registry = fixture.controllers().join("mod.rs");
    fs::write(&registry, "pub mod admin;\npub mod home;\n")?;
    fs::create_dir(fixture.controllers().join("admin"))?;
    fs::write(fixture.controllers().join("admin/mod.rs"), "//! 子模块。\n")?;
    let prepared = plan("Admin/UserController", &fixture.0)?;
    assert!(!prepared.files.iter().any(|change| change.path == registry));
    assert!(prepared.inputs.iter().any(|input| input.path == registry));

    // 计划与 apply 之间的确定性改变；之后工作区保持静止，不依赖线程调度。
    fs::write(&registry, "pub mod home;\n")?;
    let changed = snapshot(&fixture.0)?;
    let mut stages = 0;
    let Err(error) = apply(&prepared, &mut |_, _| {
        stages += 1;
        Ok(())
    }) else {
        bail!("丢失父模块声明必须使旧计划失败");
    };
    assert!(format!("{error:#}").contains("controller input changed before commit"));
    assert_eq!(stages, 0);
    assert_eq!(snapshot(&fixture.0)?, changed);
    assert!(!fixture.controllers().join("admin/user.rs").exists());
    Ok(())
}
