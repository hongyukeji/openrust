#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn artisan_script_preserves_arguments_working_directory_and_exit_status() {
    let root = Fixture(std::env::temp_dir().join(format!(
        "openrust artisan entry {} {}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    let project = root.0.join("应用 with spaces");
    let tools = root.0.join("tools");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir(&tools).unwrap();
    let artisan = project.join("artisan");
    fs::write(&artisan, include_bytes!("../artisan")).unwrap();
    let cargo = tools.join("cargo");
    fs::write(
        &cargo,
        b"#!/bin/sh\nprintf '%s\\n' \"$$\" \"$PWD\" \"$@\"\nexit 37\n",
    )
    .unwrap();
    for path in [&artisan, &cargo] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let arguments = ["command:name", "two words", "", "中文", "a;b", "$(literal)"];
    let child = Command::new(&artisan)
        .args(arguments)
        .current_dir(&root.0)
        .env(
            "PATH",
            std::env::join_paths([
                tools.as_path(),
                std::path::Path::new("/usr/bin"),
                std::path::Path::new("/bin"),
            ])
            .unwrap(),
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let pid = child.id();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(37));
    assert!(output.stderr.is_empty());
    let mut expected = format!("{pid}\n{}\nrun\n--quiet\n--\nartisan\n", project.display());
    for argument in arguments {
        expected.push_str(argument);
        expected.push('\n');
    }
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    assert_ne!(
        fs::metadata(concat!(env!("CARGO_MANIFEST_DIR"), "/artisan"))
            .unwrap()
            .permissions()
            .mode()
            & 0o111,
        0
    );
}
