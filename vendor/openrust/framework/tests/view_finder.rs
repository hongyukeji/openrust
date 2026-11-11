use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "openrust-view-finder-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

use openrust::view::{FileViewFinder, ViewError, ViewFinder};

#[test]
fn finder_resolves_ordered_locations_extensions_and_namespaces() {
    let root = TempDir::new();
    let first = root.path().join("first");
    let second = root.path().join("second");
    for path in [&first, &second] {
        fs::create_dir_all(path.join("admin")).unwrap();
    }
    let fallback = second.join("admin/index.html");
    let primary = first.join("admin/index.html");
    let compiled = first.join("admin/index.blade.html");
    fs::write(&fallback, "fallback").unwrap();
    fs::write(&primary, "primary").unwrap();
    fs::write(&compiled, "compiled").unwrap();

    let mut finder = FileViewFinder::new([first.clone(), second.clone()]);
    assert_eq!(finder.find(" admin.index ").unwrap(), primary);
    finder.addExtension("blade.html").unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), compiled);
    fs::remove_file(&compiled).unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), primary);
    fs::remove_file(&primary).unwrap();
    assert_eq!(finder.find("admin.index").unwrap(), fallback);

    fs::write(&primary, "primary").unwrap();
    finder.flush();
    assert_eq!(finder.find("admin.index").unwrap(), primary);
    finder.addNamespace("package", &[&second]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), fallback);
    finder.prependNamespace("package", &[&first]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), primary);
    finder.replaceNamespace("package", &[&second]).unwrap();
    assert_eq!(finder.find("package::admin.index").unwrap(), fallback);
    assert!(matches!(
        finder.find("unknown::admin.index"),
        Err(ViewError::NotFound)
    ));
    for invalid in [
        "../admin",
        "/admin",
        "package::other::index",
        "admin\\index",
    ] {
        assert!(matches!(finder.find(invalid), Err(ViewError::InvalidName)));
    }
}

#[cfg(unix)]
#[test]
fn finder_rejects_symlinks_outside_a_configured_root() {
    let root = TempDir::new();
    let views = root.path().join("views");
    fs::create_dir(&views).unwrap();
    let outside = root.path().join("secret.html");
    fs::write(&outside, "outside").unwrap();
    std::os::unix::fs::symlink(outside, views.join("escape.html")).unwrap();
    let finder = FileViewFinder::new([views]);
    assert!(matches!(finder.find("escape"), Err(ViewError::OutsideRoot)));
}

#[test]
fn blade_compiles_escapes_and_refreshes_a_cached_template() {
    use openrust::view::{Factory, ViewData, compilers::Compiler as _};
    use serde_json::json;

    let root = TempDir::new();
    let directory = root.path().join("views");
    let cache = root.path().join("cache");
    fs::create_dir(&directory).unwrap();
    let plain = directory.join("welcome.html");
    let blade = directory.join("welcome.blade.html");
    fs::write(&plain, "plain fallback").unwrap();
    fs::write(
        &blade,
        r#"{{ $user->name }}|{!! $trusted !!}|@{{ client }}|{{-- ignored --}}@if ($none)bad@elseif ($yes)yes@else bad@endif|@foreach ($items as $item){{ $item }}@endforeach|@forelse ($missing  as  $item){{ $item }}@empty none@endforelse|@isset ($yes)ready@endisset|@empty ($missing)empty@endempty"#,
    )
    .unwrap();
    let factory = Factory::new(FileViewFinder::new([directory]), cache);
    let data = ViewData::from_iter([
        ("user".to_owned(), json!({"name": "<&'\""})),
        ("trusted".to_owned(), json!("<em>safe</em>")),
        ("yes".to_owned(), json!(true)),
        ("items".to_owned(), json!(["one", "<two>"])),
    ]);
    let engine = factory.getEngineResolver().resolve("blade").unwrap();
    let openrust::view::engines::Engine::Blade(engine) = engine else {
        panic!("blade resolves to the compiled engine");
    };
    let compiler = engine.getCompiler();
    assert!(compiler.isExpired(&blade).unwrap());
    assert_eq!(
        factory.render("welcome", &data).unwrap(),
        "&lt;&amp;&#039;&quot;|<em>safe</em>|{{ client }}|yes|one&lt;two&gt;| none|ready|empty"
    );
    assert!(!compiler.isExpired(&blade).unwrap());
    let artifact = compiler.getCompiledPath(&blade).unwrap();
    assert!(artifact.is_file());
    fs::write(&blade, "@unless ($none)updated@endunless").unwrap();
    assert!(compiler.isExpired(&blade).unwrap());
    assert_eq!(factory.render("welcome", &data).unwrap(), "updated");
    fs::write(&artifact, "incomplete data").unwrap();
    assert_eq!(factory.render("welcome", &data).unwrap(), "updated");
    fs::write(&blade, "@if ($yes)broken").unwrap();
    assert!(matches!(
        factory.render("welcome", &data),
        Err(ViewError::CompileFailed(_))
    ));
    compiler.clear().unwrap();
    assert!(!artifact.exists());
    fs::remove_file(&blade).unwrap();
    assert_eq!(factory.render("welcome", &data).unwrap(), "plain fallback");
}
