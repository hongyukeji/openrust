struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
use openrust::config::{EnvironmentFile, EnvironmentFileError as Error};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, SystemTime},
};

fn root() -> tempfile::TempDir {
    let dir = std::env::var_os("ENVIRONMENT_FILE_TEST_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    fs::create_dir_all(&dir).unwrap();
    tempfile::tempdir_in(dir).unwrap()
}
fn fixture(source: &str) -> (tempfile::TempDir, EnvironmentFile) {
    let dir = root();
    let file = EnvironmentFile::create(dir.path().join(".env"), source).unwrap();
    (dir, file)
}
fn text(file: &EnvironmentFile) -> String {
    fs::read_to_string(file.path()).unwrap()
}

#[test]
fn insert_update_remove_and_independent_session() {
    let (_dir, file) = fixture("# before\nexport APP_NAME=Old # note\n\nUNCHANGED='keep me'\n");
    let mut edit = file.edit().unwrap();
    edit.set("APP_NAME", "我的应用")
        .unwrap()
        .set("PORT", "8080")
        .unwrap();
    assert_eq!(file.get("APP_NAME").unwrap().as_deref(), Some("Old"));
    assert_eq!(edit.get("APP_NAME").unwrap().as_deref(), Some("我的应用"));
    assert!(edit.has_changes());
    assert!(edit.save().unwrap());
    assert_eq!(
        text(&file),
        "# before\nexport APP_NAME=\"我的应用\" # note\n\nUNCHANGED='keep me'\nPORT=8080\n"
    );
    edit.remove("APP_NAME").unwrap();
    assert!(edit.save().unwrap());
    assert!(text(&file).starts_with("# before\n# note\n"));
    assert!(!file.contains("APP_NAME").unwrap());
    assert!(!file.remove("MISSING").unwrap());
    assert!(file.set("EMPTY", "").unwrap());
    assert_eq!(file.get("EMPTY").unwrap().as_deref(), Some(""));
    assert!(file.remove("EMPTY").unwrap());
    assert_eq!(file.get("EMPTY").unwrap(), None);
}
#[test]
fn literal_values_round_trip_without_rewriting_other_bytes() {
    for value in [
        "",
        "false",
        "null",
        "0",
        "中文 空格",
        "a#b",
        "a # b",
        "a'b\"c",
        r"C:\data\new",
        "$EF_MISSING",
        "${EF_FILE_HOST}",
        "a\nb",
        "a\rb",
        "a\r\nb",
        "a\tb",
        "末尾 ",
        " foo",
        "a\\\"$\\'b",
    ] {
        let (_dir, file) = fixture(
            "# header\r\nEF_FILE_HOST=example.test\r\nexport TARGET=old  # note\r\nOTHER=untouched",
        );
        file.set("TARGET", value).unwrap();
        assert_eq!(
            file.get("TARGET").unwrap().as_deref(),
            Some(value),
            "{value:?}"
        );
        let result = text(&file);
        assert!(result.starts_with("# header\r\nEF_FILE_HOST=example.test\r\nexport TARGET="));
        assert!(result.ends_with("  # note\r\nOTHER=untouched"));
        assert!(!file.set("TARGET", value).unwrap());
    }
}
#[test]
fn expressions_are_explicit_and_remaining_references_follow_the_loader() {
    let (_dir, file) =
        fixture("EF_FILE_HOST=one.test\nURL=\"https://${EF_FILE_HOST}/api\" # keep\n");
    assert_eq!(
        file.get("URL").unwrap().as_deref(),
        Some("https://one.test/api")
    );
    assert_eq!(
        file.get_raw("URL").unwrap().as_deref(),
        Some("\"https://${EF_FILE_HOST}/api\"")
    );
    file.set("EF_FILE_HOST", "two.test").unwrap();
    assert!(text(&file).contains("URL=\"https://${EF_FILE_HOST}/api\" # keep"));
    file.set("LITERAL", "${EF_FILE_HOST}").unwrap();
    assert_eq!(
        file.get("LITERAL").unwrap().as_deref(),
        Some("${EF_FILE_HOST}")
    );
    file.set_expression("EXPANDED", "${EF_FILE_HOST}").unwrap();
    assert_eq!(file.get("EXPANDED").unwrap().as_deref(), Some("two.test"));
    assert!(!file.set_expression("EXPANDED", "${EF_FILE_HOST}").unwrap());
    file.set("URL", "https://${EF_FILE_HOST}/api").unwrap();
    assert_eq!(
        file.get("URL").unwrap().as_deref(),
        Some("https://${EF_FILE_HOST}/api")
    );
}
#[test]
fn bom_multiline_crlf_and_no_final_newline_are_preserved() {
    let source = "\u{feff}# note\r\nLONG=\"first\nTARGET=inside\nlast\"\r\nTARGET='old' # outside";
    let (_dir, file) = fixture(source);
    file.set("TARGET", "new").unwrap();
    assert_eq!(text(&file), source.replace("TARGET='old'", "TARGET=new"));
    file.set("ADDED", "yes").unwrap();
    assert!(text(&file).ends_with("# outside\r\nADDED=yes\r\n"));
    file.remove("LONG").unwrap();
    assert!(text(&file).starts_with("\u{feff}# note\r\nTARGET=new"));
}
#[test]
fn same_value_does_not_change_layout_or_modification_time() {
    let (_dir, file) = fixture("NAME = 'same' # retain\r\n");
    let time = fs::metadata(file.path()).unwrap().modified().unwrap();
    assert!(!file.set("NAME", "same").unwrap());
    assert_eq!(text(&file), "NAME = 'same' # retain\r\n");
    assert_eq!(fs::metadata(file.path()).unwrap().modified().unwrap(), time);
    let mut edit = file.edit().unwrap();
    assert!(!edit.save().unwrap());
    edit.set("NAME", "new").unwrap();
    assert!(edit.save().unwrap());
    assert!(!edit.save().unwrap());
}
#[test]
fn failed_batches_duplicates_and_bad_values_are_atomic_and_redacted() {
    let (_dir, file) = fixture("NAME=old\n");
    assert!(matches!(
        file.set_many([("NAME", "new"), ("BAD KEY", "secret-value")]),
        Err(Error::InvalidKey { .. })
    ));
    assert_eq!(text(&file), "NAME=old\n");
    let mut edit = file.edit().unwrap();
    assert!(edit.set_many([("NAME", "new"), ("NAME", "again")]).is_err());
    assert!(!edit.has_changes());
    let e = file.set("NAME", "super-secret\0value").unwrap_err();
    assert!(!format!("{e:?} {e}").contains("super-secret"));
    assert!(!format!("{edit:?}").contains("NAME=old"));
    fs::write(file.path(), "NAME=one\nNAME=two\n").unwrap();
    assert!(matches!(
        file.set("NAME", "new"),
        Err(Error::DuplicateKey { .. })
    ));
    assert!(file.get("NAME").is_err());
    assert_eq!(text(&file), "NAME=one\nNAME=two\n");
}
#[test]
fn conflict_busy_and_dropped_sessions_never_replace_the_file() {
    let (_dir, file) = fixture("A=one\n");
    let mut first = file.edit().unwrap();
    let mut second = file.edit().unwrap();
    first.set("A", "two").unwrap();
    first.save().unwrap();
    second.set("A", "three").unwrap();
    assert!(matches!(second.save(), Err(Error::Conflict)));
    assert_eq!(file.get("A").unwrap().as_deref(), Some("two"));
    let mut abandoned = file.edit().unwrap();
    abandoned.set("A", "abandoned").unwrap();
    drop(abandoned);
    assert_eq!(file.get("A").unwrap().as_deref(), Some("two"));
    let guard = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(file.path().with_file_name(".env.lock"))
        .unwrap();
    guard.lock().unwrap();
    assert!(matches!(file.set("A", "blocked"), Err(Error::Busy)));
    guard.unlock().unwrap();
    drop(guard);
    assert!(!file.set("A", "two").unwrap());
    assert!(
        fs::read_dir(file.path().parent().unwrap())
            .unwrap()
            .all(|p| !p
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".env.edit-"))
    );
}
#[test]
fn missing_existing_directory_and_invalid_documents_are_not_overwritten() {
    let dir = root();
    let path = dir.path().join(".env");
    assert!(EnvironmentFile::open(&path).is_err());
    assert!(!path.exists());
    let file = EnvironmentFile::create(&path, "KEY=value\n").unwrap();
    assert!(EnvironmentFile::create(&path, "KEY=new\n").is_err());
    assert_eq!(text(&file), "KEY=value\n");
    fs::write(&path, "KEY=\"broken\n").unwrap();
    assert!(file.set("KEY", "new").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "KEY=\"broken\n");
    assert!(EnvironmentFile::open(dir.path()).is_err());
    assert!(EnvironmentFile::create(dir.path().join("bad"), "INVALID KEY=x").is_err());
    assert!(!dir.path().join("bad").exists());
}
#[test]
fn readonly_targets_and_permissions_are_respected() {
    let (_dir, file) = fixture("KEY=old\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(file.path(), fs::Permissions::from_mode(0o640)).unwrap();
        file.set("KEY", "new").unwrap();
        assert_eq!(
            fs::metadata(file.path()).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }
    let mut permissions = fs::metadata(file.path()).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(file.path(), permissions.clone()).unwrap();
    assert!(file.set("KEY", "denied").is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        permissions.set_mode(0o600);
    }
    #[cfg(windows)]
    permissions.set_readonly(false);
    fs::set_permissions(file.path(), permissions).unwrap();
    assert!(!text(&file).contains("denied"));
}
#[cfg(unix)]
#[test]
fn symlink_targets_and_lock_files_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = root();
    let target = dir.path().join("real");
    fs::write(&target, "KEY=old\n").unwrap();
    let path = dir.path().join(".env");
    symlink(&target, &path).unwrap();
    assert!(EnvironmentFile::open(&path).is_err());
    fs::remove_file(&path).unwrap();
    fs::write(&path, "KEY=old\n").unwrap();
    symlink(&target, dir.path().join(".env.lock")).unwrap();
    let file = EnvironmentFile::open(&path).unwrap();
    assert!(file.set("KEY", "new").is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "KEY=old\n");
}
#[test]
fn process_writer_child() {
    let Some(path) = std::env::var_os("ENVIRONMENT_EDITOR_CHILD") else {
        return;
    };
    let file = EnvironmentFile::open(path).unwrap();
    let prefix = std::env::var("ENVIRONMENT_EDITOR_PREFIX").unwrap();
    for n in 0..20 {
        let deadline = SystemTime::now() + Duration::from_secs(20);
        loop {
            match file.set(&format!("{prefix}_{n}"), "value") {
                Ok(_) => break,
                Err(Error::Busy) if SystemTime::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                other => panic!("{other:?}"),
            }
        }
    }
}
#[test]
fn two_real_processes_preserve_each_others_writes() {
    if std::env::var_os("ENVIRONMENT_EDITOR_CHILD").is_some() {
        return;
    }
    let (_dir, file) = fixture("INITIAL=yes\n");
    let mut children = Vec::new();
    for prefix in ["FIRST", "SECOND"] {
        children.push(ChildGuard(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "process_writer_child", "--nocapture"])
                .env("ENVIRONMENT_EDITOR_CHILD", file.path())
                .env("ENVIRONMENT_EDITOR_PREFIX", prefix)
                .spawn()
                .unwrap(),
        ));
    }
    for mut child in children {
        assert!(child.0.wait().unwrap().success());
    }
    let all = file.all().unwrap();
    assert_eq!(all.len(), 41);
    for prefix in ["FIRST", "SECOND"] {
        for n in 0..20 {
            assert_eq!(all[&format!("{prefix}_{n}")], "value");
        }
    }
}
#[test]
fn generated_character_combinations_round_trip() {
    let (_dir, file) = fixture("KEY=old\n");
    let chars = [
        'a', '中', ' ', '\'', '"', '\\', '$', '#', '\n', '\r', '\t', '=', '{', '}',
    ];
    for (i, a) in chars.iter().enumerate() {
        for b in &chars {
            let value = format!("{a}{b}{i}");
            file.set("KEY", &value).unwrap();
            assert_eq!(file.get("KEY").unwrap().as_deref(), Some(value.as_str()));
        }
    }
}

#[test]
fn bom_only_and_concatenated_quotes_follow_native_loader() {
    let (_dir, file) = fixture("\u{feff}");
    file.set("KEY", "value").unwrap();
    assert_eq!(text(&file), "\u{feff}KEY=value\n");
    for source in [
        "KEY=\"first\"tail\nOTHER=yes\n",
        "KEY='single'\"double\"\nOTHER=yes\n",
        "KEY=\"a\\'b\"\nOTHER=yes\n",
    ] {
        let (_dir, file) = fixture(source);
        file.set("OTHER", "new").unwrap();
        assert_eq!(text(&file), source.replace("OTHER=yes", "OTHER=new"));
        file.set("KEY", "replaced").unwrap();
        assert_eq!(file.get("KEY").unwrap().as_deref(), Some("replaced"));
    }
}

#[test]
fn interrupted_writer_releases_lock_without_partial_data() {
    if std::env::var_os("ENVIRONMENT_EDITOR_CHILD").is_some() {
        return;
    }
    let (dir, file) = fixture("KEY=original\n");
    let ready = dir.path().join("ready");
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lock_holder_child"])
            .env(
                "ENVIRONMENT_EDITOR_LOCK",
                file.path().with_file_name(".env.lock"),
            )
            .env("ENVIRONMENT_EDITOR_READY", &ready)
            .spawn()
            .unwrap(),
    );
    let deadline = SystemTime::now() + Duration::from_secs(10);
    while !ready.exists() {
        assert!(SystemTime::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(file.set("KEY", "blocked"), Err(Error::Busy)));
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    assert_eq!(file.get("KEY").unwrap().as_deref(), Some("original"));
    file.set("KEY", "recovered").unwrap();
    assert_eq!(file.get("KEY").unwrap().as_deref(), Some("recovered"));
}
#[test]
fn lock_holder_child() {
    let Some(path) = std::env::var_os("ENVIRONMENT_EDITOR_LOCK") else {
        return;
    };
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    file.lock().unwrap();
    fs::write(
        std::env::var_os("ENVIRONMENT_EDITOR_READY").unwrap(),
        "ready",
    )
    .unwrap();
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[cfg(windows)]
#[test]
fn windows_replacement_preserves_protected_acl() {
    let (_dir, file) = fixture("KEY=old\n");
    let powershell = |script| {
        let output = Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", script])
            .env("ENVIRONMENT_EDITOR_ACL", file.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    powershell(
        "$ErrorActionPreference='Stop';$p=$env:ENVIRONMENT_EDITOR_ACL;$acl=Get-Acl -LiteralPath $p;$acl.SetAccessRuleProtection($true,$true);Set-Acl -LiteralPath $p -AclObject $acl",
    );
    let original = powershell("(Get-Acl -LiteralPath $env:ENVIRONMENT_EDITOR_ACL).Sddl");
    file.set("KEY", "new").unwrap();
    assert_eq!(
        powershell("(Get-Acl -LiteralPath $env:ENVIRONMENT_EDITOR_ACL).Sddl"),
        original
    );
}

#[test]
fn oversized_and_non_utf8_files_are_rejected_without_replacement() {
    let dir = root();
    let path = dir.path().join(".env");
    let large = "x".repeat(8 * 1024 * 1024 + 1);
    assert!(matches!(
        EnvironmentFile::create(&path, &large),
        Err(Error::TooLarge)
    ));
    assert!(!path.exists());
    fs::write(&path, large.as_bytes()).unwrap();
    assert!(matches!(EnvironmentFile::open(&path), Err(Error::TooLarge)));
    fs::write(&path, [0xff, 0xfe]).unwrap();
    assert!(EnvironmentFile::open(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), [0xff, 0xfe]);
}
