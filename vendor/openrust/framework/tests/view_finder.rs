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

#[tokio::test]
async fn layouts_and_composers_isolate_concurrent_http_requests() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        container::ServiceKey,
        contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application, configuration::Routing, http::Kernel},
        http::Request,
        support::facades::{Route, View as ViewFacade},
        view::{Component, ComponentOutput, Factory, ViewResult, view},
    };
    use std::{path::PathBuf, sync::Arc};
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    struct Badge;
    impl Component for Badge {
        fn render(&self) -> ViewResult<ComponentOutput> {
            Ok(ComponentOutput::InlineTemplate(
                "<mark>{{ $text }}</mark>".to_owned(),
            ))
        }
    }

    async fn transport(
        State(kernel): State<Arc<Kernel>>,
        incoming: axum::extract::Request,
    ) -> axum::response::Response {
        let (parts, body) = incoming.into_parts();
        let (parts, body) = kernel
            .handle(&mut Request::from_parts(parts, body))
            .await
            .into_parts();
        axum::response::Response::from_parts(parts, body)
    }

    async fn start(
        path: PathBuf,
        app: &'static str,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let application = Application::configure(path)
            .withRouting(Routing::default().web(|| {
                Route::get("/page", || view("page").with("title", "route"))?;
                Route::get("/plain", || view("layout"))?;
                Route::get("/broken", || view("broken"))
            }))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        let factory = kernel
            .get_application()
            .container()
            .get::<Factory>(ServiceKey::typed::<Factory>())
            .unwrap();
        factory.component("badge", |_| Ok(Badge));
        factory.share("app", app);
        factory.share("title", "shared");
        factory.creator("page", |view| {
            view.data_mut().insert("title".into(), "creator".into());
            Ok(())
        });
        factory.composer("pa*", |view| {
            assert_eq!(view.data()["title"], "route");
            assert!(ViewFacade::exists("layout")?);
            view.data_mut().insert("label".into(), "<composer>".into());
            Ok(())
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let service = transport.with_state(Arc::new(kernel)).into_make_service();
        let task = tokio::spawn(async move { axum::serve(listener, service).await.unwrap() });
        (address, task)
    }

    async fn send(address: std::net::SocketAddr, path: &str, status: u16) -> String {
        let mut socket = tokio::net::TcpStream::connect(address).await.unwrap();
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        socket.write_all(request.as_bytes()).await.unwrap();
        let mut response = String::new();
        socket.read_to_string(&mut response).await.unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(
            headers.starts_with(&format!("HTTP/1.1 {status} ")),
            "{response}"
        );
        body.to_owned()
    }

    let root = TempDir::new();
    let sources = root.path().join("resources/views");
    fs::create_dir_all(sources.join("components")).unwrap();
    for (name, source) in [
        (
            "layout",
            "<main>{{ $app }}|@yield('content', '<fallback>')</main>",
        ),
        ("middle", "@extends('layout')@section('content', 'parent')"),
        (
            "page",
            r#"@extends('middle')@section('content')@parent|<x-card :title="$label"><x-slot:header><b>{{ $title }}</b></x-slot:header>@include('partial')<x-badge text="&"/></x-card>@endsection"#,
        ),
        (
            "components/card",
            "<article>{{ $header }}<h2>{{ $title }}</h2>{{ $slot }}</article>",
        ),
        ("partial", "<i>{{ $label }}</i>"),
        ("broken", "@extends('missing')"),
    ] {
        fs::write(sources.join(format!("{name}.blade.html")), source).unwrap();
    }
    let ((first, first_task), (second, second_task)) = tokio::join!(
        start(root.path().to_owned(), "A"),
        start(root.path().to_owned(), "B"),
    );
    let (a, b, unrelated) = tokio::join!(
        send(first, "/page", 200),
        send(second, "/page", 200),
        send(first, "/plain", 200),
    );
    assert_eq!(
        a,
        "<main>A|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(
        b,
        "<main>B|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(unrelated, "<main>A|&lt;fallback&gt;</main>");
    assert!(send(first, "/broken", 500).await.contains("500"));
    assert_eq!(send(first, "/plain", 200).await, unrelated);
    first_task.abort();
    second_task.abort();
}
