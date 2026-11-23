use openrust::{
    filesystem::{FileError, FileMacro, FileMode, Filesystem},
    support::facades::{App, Facade, File},
    testing::TestApp,
};
use std::{
    fs,
    sync::{Arc, Barrier},
    thread,
};

async fn fixture() -> TestApp {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn binary_io_metadata_paths_and_hashes_use_the_public_local_service() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.bin");
        assert!(File::missing(&file).unwrap());
        assert_eq!(File::put(&file, b"abc").unwrap(), 3);
        assert_eq!(
            File::hash(&file).unwrap(),
            "900150983cd24fb0d6963f7d28e17f72"
        );
        assert_eq!(
            File::hash_with_algorithm(&file, "SHA256").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(matches!(
            File::hash_with_algorithm(&file, "invalid"),
            Err(FileError::UnknownAlgorithm(_))
        ));
        assert_eq!(File::get(&file).unwrap(), b"abc");
        assert_eq!(File::append(&file, [0, 255]).unwrap(), 2);
        assert_eq!(File::prepend(&file, b"!").unwrap(), 6);
        assert_eq!(File::shared_get(&file).unwrap(), b"!abc\0\xff");
        assert!(matches!(
            File::get_text(&file),
            Err(FileError::InvalidUtf8 { .. })
        ));
        assert_eq!(File::size(&file).unwrap(), 6);
        assert!(File::last_modified(&file).unwrap() > 0);
        assert_eq!(File::r#type(&file).unwrap(), "file");
        assert!(File::is_file(&file).unwrap());
        assert!(File::is_readable(&file).unwrap());
        assert!(File::is_writable(&file).unwrap());
        assert!(File::is_directory(dir.path()).unwrap());
        assert_eq!(File::basename(&file).unwrap(), "data.bin");
        assert_eq!(File::name(&file).unwrap(), "data");
        assert_eq!(File::extension(&file).unwrap(), "bin");
        assert_eq!(File::dirname(&file).unwrap(), dir.path());
        assert_eq!(File::name(".env").unwrap(), "");
        assert_eq!(File::extension(".env").unwrap(), "env");
        assert_eq!(File::dirname("file").unwrap(), std::path::Path::new("."));
        let copy = dir.path().join("copy");
        File::copy(&file, &copy).unwrap();
        assert!(File::has_same_hash(&file, &copy).unwrap());
        File::replace_in_file(b"abc", b"xyz", &copy).unwrap();
        assert!(!File::has_same_hash(&file, &copy).unwrap());
        assert!(!File::has_same_hash(&file, dir.path().join("missing")).unwrap());
        let moved = dir.path().join("moved");
        File::r#move(&copy, &moved).unwrap();
        assert!(!File::delete_many([dir.path().join("missing"), moved.clone()]).unwrap());
        assert!(!moved.exists());
        assert!(File::get(dir.path()).is_err());
        assert!(File::get(dir.path().join("missing")).is_err());
    })
    .await;
    assert!(File::exists(".").is_err());
}

#[test]
fn hashes_match_fixed_php_vectors_and_handle_stream_boundaries() {
    let service = Filesystem::new();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("input");
    service.put(&file, b"abc").unwrap();
    for (algorithm, expected) in [
        ("sha1", "a9993e364706816aba3e25717850c26c9cd0d89d"),
        (
            "sha384",
            "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7",
        ),
        (
            "sha512",
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f",
        ),
        ("xxh128", "06b05ab6733a618578af5f94892f3950"),
    ] {
        assert_eq!(
            service.hash_with_algorithm(&file, algorithm).unwrap(),
            expected,
            "{algorithm}"
        );
    }
    let data = vec![237; 32769];
    service.put(&file, &data).unwrap();
    let other = dir.path().join("other");
    service.put(&other, &data).unwrap();
    assert!(service.has_same_hash(&file, &other));
}

#[test]
fn locked_append_preserves_complete_records_under_concurrent_writers() {
    let dir = tempfile::tempdir().unwrap();
    let path = Arc::new(dir.path().join("records"));
    let barrier = Arc::new(Barrier::new(5));
    let mut threads = Vec::new();
    for id in 0..4 {
        let path = path.clone();
        let barrier = barrier.clone();
        threads.push(thread::spawn(move || {
            let fs = Filesystem::new();
            barrier.wait();
            for _ in 0..40 {
                fs.append_with_lock(&*path, format!("{id}:{}\n", "x".repeat(300)), true)
                    .unwrap();
            }
        }));
    }
    barrier.wait();
    for handle in threads {
        handle.join().unwrap();
    }
    let content = Filesystem::new().get_text(&*path).unwrap();
    let records: Vec<_> = content.lines().collect();
    assert_eq!(records.len(), 160);
    for record in records {
        assert_eq!(record.len(), 302);
        assert!(record[2..].bytes().all(|value| value == b'x'));
    }
}

#[test]
fn locked_put_waits_before_truncating_and_shared_reads_observe_the_completed_write() {
    use std::sync::mpsc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("locked");
    fs::write(&path, b"original").unwrap();
    let held = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    held.lock().unwrap();
    let (sender, receiver) = mpsc::channel();
    let writer = path.clone();
    let handle = thread::spawn(move || {
        sender.send(()).unwrap();
        Filesystem::new()
            .put_with_lock(writer, b"new", true)
            .unwrap();
    });
    receiver.recv().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"original");
    held.unlock().unwrap();
    handle.join().unwrap();
    assert_eq!(Filesystem::new().shared_get(&path).unwrap(), b"new");
}

#[test]
fn atomic_replace_and_sequential_binary_replacements_preserve_failure_boundaries() {
    let service = Filesystem::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data");
    service.put(&path, b"ab\0ab").unwrap();
    service
        .replace_many_in_file(&[(b"ab", b"xy"), (b"xy", b"z"), (b"", b"ignored")], &path)
        .unwrap();
    assert_eq!(service.get(&path).unwrap(), b"z\0z");
    service.replace(&path, b"replacement").unwrap();
    assert_eq!(service.get(&path).unwrap(), b"replacement");
    assert!(
        service
            .replace(dir.path().join("missing/child"), b"fail")
            .is_err()
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    #[cfg(unix)]
    {
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        service
            .replace_with_mode(&link, b"linked", Some(0o640))
            .unwrap();
        assert!(link.is_symlink());
        assert_eq!(service.get(&path).unwrap(), b"linked");
        assert_eq!(service.mode(&path).unwrap(), 0o640);
        assert_eq!(
            service.chmod(&path, None).unwrap(),
            FileMode::Current("0640".into())
        );
        assert_eq!(
            service.chmod(&path, Some(0)).unwrap(),
            FileMode::Current("0640".into())
        );
        assert_eq!(
            service.chmod(&path, Some(0o600)).unwrap(),
            FileMode::Changed
        );
        assert_eq!(service.r#type(&link).unwrap(), "link");
        use std::os::unix::ffi::OsStringExt;
        let name = std::ffi::OsString::from_vec(vec![255, b'.', b'x']);
        assert_eq!(service.name(&name), std::ffi::OsString::from_vec(vec![255]));
        assert_eq!(service.extension(name), "x");
    }
}

#[tokio::test]
async fn macros_share_the_provider_singleton_and_remain_application_scoped() {
    let first = fixture().await;
    let second = fixture().await;
    App::set_facade_application(Some(first.app()), async {
        File::r#macro("bytes", |fs, path: std::path::PathBuf| fs.size(path)).unwrap();
        let service = App::make::<Filesystem>().await.unwrap();
        assert!(service.has_macro("bytes"));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data");
        File::put(&path, b"abc").unwrap();
        assert_eq!(
            File::macro_call::<u64, _>("bytes", path.clone()).unwrap(),
            3
        );
        assert!(File::macro_call::<String, _>("bytes", path).is_err());
        assert!(File::macro_call::<u64, _>("bytes", ()).is_err());
        File::mixin([("bytes", FileMacro::new(|_, (): ()| Ok(42_u64)))], false).unwrap();
        File::r#macro("nested", |service, (): ()| {
            service.r#macro("inner", |_, (): ()| Ok(9_u64));
            service.macro_call::<u64, _>("inner", ())
        })
        .unwrap();
        assert_eq!(File::macro_call::<u64, _>("nested", ()).unwrap(), 9);
        App::set_facade_application(Some(second.app()), async {
            assert!(!File::has_macro("bytes").unwrap());
        })
        .await;
        File::mixin([("bytes", FileMacro::new(|_, (): ()| Ok(42_u64)))], true).unwrap();
        assert_eq!(File::macro_call::<u64, _>("bytes", ()).unwrap(), 42);
        File::flush_macros().unwrap();
        assert!(!service.has_macro("bytes"));
        assert!(File::macro_call::<u64, _>("bytes", ()).is_err());
    })
    .await;
}

#[tokio::test]
async fn directory_queries_filter_depth_hidden_and_vcs_without_following_links() {
    use openrust::filesystem::DirectoryDepth;
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let root = tempfile::tempdir().unwrap();
        let root = root.path();
        for directory in ["dir", "dir/deep", "dir-a", ".secret", ".git"] {
            File::ensure_directory_exists(root.join(directory)).unwrap();
        }
        for file in [
            "z",
            "a",
            ".hidden",
            "dir/nested",
            "dir/deep/end",
            "dir-a/item",
            ".secret/item",
            ".git/config",
        ] {
            File::put(root.join(file), file).unwrap();
        }
        let relative = |files: Vec<openrust::filesystem::FileInfo>| {
            files
                .into_iter()
                .map(|file| file.relative_pathname().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(relative(File::files(root).unwrap()), ["a", "z"]);
        assert_eq!(
            relative(File::all_files(root).unwrap()),
            ["a", "dir-a/item", "dir/deep/end", "dir/nested", "z"]
        );
        assert_eq!(
            relative(File::all_files_with_hidden(root, true).unwrap()),
            [
                ".hidden",
                ".secret/item",
                "a",
                "dir-a/item",
                "dir/deep/end",
                "dir/nested",
                "z"
            ]
        );
        assert_eq!(
            relative(
                File::files_with_options(
                    root,
                    false,
                    DirectoryDepth::parse([">= 1", "<2"]).unwrap()
                )
                .unwrap()
            ),
            ["dir-a/item", "dir/nested"]
        );
        assert!(DirectoryDepth::parse(["nope"]).is_err());
        assert_eq!(File::all_directories(root).unwrap().len(), 3);
        assert!(File::files(root.join("missing")).is_err());
        let empty = root.join("empty");
        File::make_directory(&empty).unwrap();
        assert!(File::is_empty_directory(&empty).unwrap());
        File::put(empty.join(".x"), b"x").unwrap();
        assert!(!File::is_empty_directory(&empty).unwrap());
        assert!(File::is_empty_directory_ignoring_dot_files(&empty, true).unwrap());
        assert!(!File::make_directory_with_options(&empty, 0o755, false, true).unwrap());
        assert!(File::make_directory(&empty).is_err());
        #[cfg(unix)]
        {
            let external = tempfile::tempdir().unwrap();
            fs::write(external.path().join("kept"), b"keep").unwrap();
            std::os::unix::fs::symlink(external.path(), root.join("external")).unwrap();
            assert!(
                !relative(File::all_files(root).unwrap())
                    .iter()
                    .any(|name| name.contains("kept"))
            );
            assert!(File::clean_directory(root).unwrap());
            assert!(external.path().join("kept").exists());
        }
    })
    .await;
}

#[tokio::test]
async fn directory_copy_move_and_clean_preserve_expected_root_and_failure_results() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let root = tempfile::tempdir().unwrap();
        let from = root.path().join("from");
        let target = root.path().join("target");
        File::ensure_directory_exists(from.join("nested")).unwrap();
        File::put(from.join("nested/file"), b"content").unwrap();
        File::put(from.join(".hidden"), b"hidden").unwrap();
        assert!(File::copy_directory(&from, &target).unwrap());
        assert_eq!(File::get(target.join("nested/file")).unwrap(), b"content");
        assert!(target.join(".hidden").exists());
        assert!(!File::copy_directory(root.path().join("absent"), &target).unwrap());
        assert!(File::delete_directories(&target).unwrap());
        assert!(!target.join("nested").exists());
        assert!(target.join(".hidden").exists());
        assert!(!File::delete_directories(&target).unwrap());
        let moved = root.path().join("moved");
        File::ensure_directory_exists(&moved).unwrap();
        File::put(moved.join("occupied"), b"x").unwrap();
        assert!(!File::move_directory(&from, &moved).unwrap());
        assert!(File::move_directory_with_overwrite(&from, &moved, true).unwrap());
        assert!(!moved.join("occupied").exists());
        assert!(moved.join("nested/file").exists());
        assert!(File::clean_directory(&moved).unwrap());
        assert!(moved.is_dir());
        assert!(File::is_empty_directory(&moved).unwrap());
        assert!(File::delete_directory(&moved).unwrap());
        assert!(!File::delete_directory(&moved).unwrap());
    })
    .await;
}

#[tokio::test]
async fn lazy_lines_reopen_the_file_and_default_json_differs_from_strict_decoding() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("input");
        File::put(&path, b"a\r\nb\n").unwrap();
        let lines = File::lines(&path).unwrap();
        assert_eq!(
            lines
                .all()
                .unwrap()
                .into_iter()
                .map(|(_, line)| line)
                .collect::<Vec<_>>(),
            [b"a".to_vec(), b"b".to_vec(), vec![]]
        );
        File::put(&path, b"changed").unwrap();
        assert_eq!(lines.all().unwrap()[0].1, b"changed");
        File::put(&path, b"").unwrap();
        assert_eq!(lines.all().unwrap().len(), 1);
        File::delete(&path).unwrap();
        assert!(lines.all().is_err());
        assert!(File::lines(&path).is_err());
        assert!(File::json(&path).is_err());
        File::put(&path, b"{").unwrap();
        assert_eq!(File::json(&path).unwrap(), serde_json::Value::Null);
        assert!(matches!(
            File::json_strict(&path),
            Err(FileError::Json { .. })
        ));
        File::put(&path, b"{\"enabled\":true}").unwrap();
        assert_eq!(
            File::json_with_options(&path, true, true).unwrap()["enabled"],
            true
        );
        assert_eq!(File::mime_type(&path).unwrap(), "application/json");
        assert_eq!(
            File::guess_extension(&path).unwrap().as_deref(),
            Some("json")
        );
        File::put(&path, b"plain text").unwrap();
        assert_eq!(File::mime_type(&path).unwrap(), "text/plain");
        assert!(File::copy_directory(dir.path(), dir.path().join("nested")).is_err());
    })
    .await;
}

#[tokio::test]
async fn glob_excludes_dot_files_and_links_preserve_local_path_semantics() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["b.txt", "a.txt", ".hidden.txt", "x.log"] {
            File::put(root.join(name), name).unwrap();
        }
        let pattern = format!("{}/*.txt", root.display());
        let files = File::glob(&pattern).unwrap();
        assert_eq!(
            files
                .iter()
                .map(|path| path.file_name().unwrap().to_str().unwrap())
                .collect::<Vec<_>>(),
            ["a.txt", "b.txt"]
        );
        assert!(
            File::glob(&format!("{}/missing*", root.display()))
                .unwrap()
                .is_empty()
        );
        assert!(File::glob("[").is_err());
        #[cfg(unix)]
        {
            let target = root.join("a.txt");
            let link = root.join("absolute");
            File::link(&target, &link).unwrap();
            assert_eq!(File::get(&link).unwrap(), b"a.txt");
            assert_eq!(File::r#type(&link).unwrap(), "link");
            File::ensure_directory_exists(root.join("nested")).unwrap();
            let link = root.join("nested/relative");
            File::relative_link(&target, &link).unwrap();
            assert_eq!(
                fs::read_link(&link).unwrap(),
                std::path::Path::new("../a.txt")
            );
            assert_eq!(File::get(&link).unwrap(), b"a.txt");
            assert!(File::link(target, &link).is_err());
        }
    })
    .await;
}

fn file_http(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("http");
        File::put(&path, b"http payload")?;
        let hash = File::hash(&path)?;
        File::r#macro("consumer", |_, (): ()| Ok("http-and-artisan".to_owned()))?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({"hash":hash,"bytes":File::size(&path)?}),
        )
    })
}
fn file_http_failure(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let dir = tempfile::tempdir()?;
        File::get(dir.path().join("missing"))?;
        unreachable!()
    })
}
struct FileCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for FileCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        _: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let value = File::macro_call::<String, _>("consumer", ())
                .map_err(|error| openrust::console::ConsoleError::new(error.to_string()))?;
            let dir = tempfile::tempdir()
                .map_err(|error| openrust::console::ConsoleError::new(error.to_string()))?;
            File::put(dir.path().join("cli"), &value)
                .map_err(|error| openrust::console::ConsoleError::new(error.to_string()))?;
            output.stdout.push_str(&value);
            Ok(0)
        })
    }
}
#[tokio::test]
async fn public_file_calls_share_one_service_across_http_and_artisan_and_render_failures() {
    use openrust::{foundation::configuration::Routing, support::facades::Route};
    let mut app = TestApp::create(|builder| {
        builder
            .with_commands(|artisan| {
                artisan.add_command(Box::new(FileCommand(
                    openrust::console::CommandDefinition::new(clap::Command::new("file:probe")),
                )))
            })
            .with_routing(Routing::default().web(|| {
                Route::get("/file", file_http)?;
                Route::get("/file-error", file_http_failure)
            }))
    })
    .unwrap();
    app.get("/file")
        .await
        .unwrap()
        .assert_ok()
        .assert_json_path("bytes", &serde_json::json!(12));
    app.artisan(["file:probe"])
        .await
        .assert_successful()
        .expects_output("http-and-artisan");
    app.call(
        http::Request::get("/file-error")
            .header("Accept", "application/json")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
}
