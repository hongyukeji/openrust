#![cfg(unix)]

use std::{
    fs,
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    os::unix::fs::PermissionsExt as _,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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

struct ServeProcess(Child);

impl Drop for ServeProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_homepage(process: &mut ServeProcess, port: u16, name: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        assert!(process.0.try_wait().unwrap().is_none());
        if let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) {
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let mut response = String::new();
            if write!(
                stream,
                "GET /welcome HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
            )
            .is_ok()
                && stream.read_to_string(&mut response).is_ok()
                && response.starts_with("HTTP/1.1 200")
                && response.contains(name)
            {
                return;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("development server did not serve {name:?} on {port}");
}

#[test]
fn artisan_serve_binds_only_http_retries_ports_reloads_and_drains() {
    let root = Fixture(std::env::temp_dir().join(format!(
        "openrust-serve-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    let views = root.0.join("resources/views");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("welcome.blade.html"),
        "<title>{{ config('app.name') }}</title>",
    )
    .unwrap();
    let environment = root.0.join(".env.serve-test");
    fs::write(&environment, "APP_NAME=FirstServe\n").unwrap();

    let command = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_main"));
        command
            .env_clear()
            .env("APP_BASE_PATH", &root.0)
            .env("APP_DISABLED_RUNNERS", "http")
            .env("QUEUE_CONNECTION", "not-configured")
            .env("CACHE_STORE", "array")
            .env("SESSION_DRIVER", "array")
            .env(
                "APP_KEY",
                "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .args(["artisan", "serve", "--env=serve-test"]);
        command
    };

    let (occupied, next, port) = loop {
        let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        if let Some(next_port) = occupied.local_addr().unwrap().port().checked_add(1)
            && let Ok(next) = TcpListener::bind(("127.0.0.1", next_port))
        {
            break (occupied, next, next_port);
        }
    };
    let first_port = occupied.local_addr().unwrap().port();
    let failed = command()
        .args(["--no-reload", "--port", &first_port.to_string()])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("cannot bind HTTP listener"));
    drop(next);

    let mut process = ServeProcess(
        command()
            .args(["--tries=1", "--host", &format!("127.0.0.1:{first_port}")])
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for_homepage(&mut process, port, "FirstServe");
    drop(occupied);
    fs::write(&environment, "APP_NAME=SecondServe\n").unwrap();
    wait_for_homepage(&mut process, port, "SecondServe");

    let pid = process.0.id().to_string();
    assert!(
        Command::new("sh")
            .args(["-c", "kill -TERM \"$1\"", "sh", &pid])
            .status()
            .unwrap()
            .success()
    );
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert_eq!(status.code(), Some(0));
            break;
        }
        assert!(Instant::now() < deadline, "development server did not stop");
        thread::sleep(Duration::from_millis(25));
    }
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok());
}
