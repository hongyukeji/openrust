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
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("compiler_version");
    fs::write(&artifact, serde_json::to_vec(&legacy).unwrap()).unwrap();
    assert!(compiler.isExpired(&blade).unwrap());
    compiler.compile(&blade).unwrap();
    assert!(!compiler.isExpired(&blade).unwrap());
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

#[test]
fn view_contracts_resolve_files_namespaces_and_engines() {
    use openrust::{
        contracts::view::{Engine as _, Factory as ViewFactory},
        view::{Factory, ViewData},
    };
    use std::sync::{Arc, Mutex};

    let root = TempDir::new();
    let app = root.path().join("app");
    let old = root.path().join("old");
    let replacement = root.path().join("replacement");
    for folder in [&app, &old, &replacement] {
        fs::create_dir(folder).unwrap();
    }
    let explicit = app.join("literal.blade.html");
    fs::write(&explicit, "{{ $title }}|{{ $base }}").unwrap();
    fs::write(old.join("index.html"), "old").unwrap();
    fs::write(replacement.join("index.html"), "new").unwrap();
    let factory = Factory::new(
        FileViewFinder::new([app.clone()]),
        root.path().join("compiled"),
    );
    let contract: &dyn ViewFactory = &factory;
    let trace = Arc::new(Mutex::new(Vec::new()));
    let record = trace.clone();
    contract.creator(
        &["literal", "pkg::index"],
        Arc::new(move |view| {
            record.lock().unwrap().push("created");
            view.with("base", "creator".into());
            Ok(())
        }),
    );
    let record = trace.clone();
    contract.composer(
        &["literal"],
        Arc::new(move |view| {
            record.lock().unwrap().push("composed");
            assert_eq!(view.getData()["base"], "creator");
            Ok(())
        }),
    );
    assert!(contract.exists("literal"));
    assert!(!contract.exists("missing"));
    let mut view = contract
        .make(
            "literal",
            ViewData::from_iter([("title".into(), "<data>".into())]),
            ViewData::from_iter([
                ("title".into(), "merged".into()),
                ("base".into(), "merged".into()),
            ]),
        )
        .unwrap();
    assert_eq!(view.name(), "literal");
    assert_eq!(view.render().unwrap(), "&lt;data&gt;|creator");
    view.with("title", "again".into());
    assert_eq!(view.render().unwrap(), "again|creator");
    assert_eq!(*trace.lock().unwrap(), ["created", "composed", "composed"]);

    assert_eq!(contract.share("base", "shared".into()), "shared");
    let mut file = contract
        .file(
            Path::new("literal.blade.html"),
            ViewData::new(),
            ViewData::new(),
        )
        .unwrap();
    file.with("title", "file".into());
    assert_eq!(file.render().unwrap(), "file|shared");
    let outside = root.path().join("outside.html");
    fs::write(&outside, "private").unwrap();
    assert!(matches!(
        contract.file(&outside, ViewData::new(), ViewData::new()),
        Err(ViewError::OutsideRoot)
    ));
    assert!(matches!(
        contract.file(
            Path::new("../outside.html"),
            ViewData::new(),
            ViewData::new()
        ),
        Err(ViewError::OutsideRoot)
    ));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, app.join("escape.html")).unwrap();
        assert!(matches!(
            contract.file(Path::new("escape.html"), ViewData::new(), ViewData::new()),
            Err(ViewError::OutsideRoot)
        ));
    }
    assert!(matches!(
        contract.file(Path::new("missing.html"), ViewData::new(), ViewData::new()),
        Err(ViewError::NotFound)
    ));

    contract.addNamespace("pkg", &[&old]).unwrap();
    let mut namespaced = contract
        .make("pkg::index", ViewData::new(), ViewData::new())
        .unwrap();
    assert_eq!(namespaced.render().unwrap(), "old");
    contract.replaceNamespace("pkg", &[&replacement]).unwrap();
    assert_eq!(namespaced.render().unwrap(), "new");
    assert!(matches!(
        contract.addNamespace("../invalid", &[&old]),
        Err(ViewError::InvalidName)
    ));

    assert!(matches!(
        contract.file(
            Path::new("unsupported.txt"),
            ViewData::new(),
            ViewData::new()
        ),
        Err(ViewError::UnknownEngine)
    ));
    let engine = factory.getEngineResolver().resolve("blade").unwrap();
    let data = ViewData::from_iter([
        ("title".into(), "engine".into()),
        ("base".into(), "safe".into()),
    ]);
    assert_eq!(engine.get(&app, &explicit, &data).unwrap(), "engine|safe");
    assert!(matches!(
        engine.get(&app, &outside, &data),
        Err(ViewError::OutsideRoot)
    ));
}

#[tokio::test]
async fn layouts_and_composers_isolate_concurrent_http_requests() {
    use axum::{extract::State, handler::Handler as _};
    use openrust::{
        container::ServiceKey,
        contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
        foundation::{Application, configuration::Routing, http::Kernel},
        http::{Body, HttpError, HttpFuture, Request, Response, StatusCode},
        support::facades::{Route, View as ViewFacade},
        view::{Component, ComponentOutput, Factory, ViewData, ViewResult, view},
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

    fn render_card(factory: &dyn openrust::contracts::view::Factory) -> ViewResult<String> {
        let mut view = factory.make("contract_card", ViewData::new(), ViewData::new())?;
        view.with("title", "<card>".into());
        view.render()
    }

    fn contract_route(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
        Box::pin(async {
            let factory = ViewFacade::contract().map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })?;
            let html = tokio::task::spawn_blocking(move || render_card(factory.as_ref()))
                .await
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?
                .map_err(|source| HttpError::Internal {
                    source: Box::new(source),
                })?;
            Ok(Response::new(StatusCode::OK, Body::from(html)))
        })
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
                Route::get("/contract", contract_route)?;
                Route::get("/missing-include", || view("missing_include"))?;
                Route::get("/loops", || {
                    view("loops").with(
                        "groups",
                        serde_json::json!({"<first>": ["<&", "two"], "second": []}),
                    )
                })?;
                Route::get("/broken-loop", || view("broken_loop"))?;
                Route::get("/dynamic", || {
                    view("dynamic")
                        .with("componentName", "accordion")
                        .with("label", "<title>")
                })?;
                Route::get("/dynamic-invalid", || {
                    view("dynamic").with("componentName", "../accordion")
                })?;
                Route::get("/dynamic-missing", || {
                    view("dynamic").with("componentName", "missing")
                })?;
                Route::get("/props", || view("props").with("userValue", "\"<&"))?;
                Route::get("/invalid-props", || view("invalid_props"))?;
                Route::get("/invalid-attributes", || {
                    view("invalid_attributes").with("array", serde_json::json!([1, 2]))
                })?;
                Route::get("/stacks", || view("stacks"))?;
                Route::get("/empty-stack", || view("stack_layout"))?;
                Route::get("/broken-stack", || view("broken_stack"))?;
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
        let contract = kernel
            .get_application()
            .container()
            .get::<dyn openrust::contracts::view::Factory>(ServiceKey::typed::<
                dyn openrust::contracts::view::Factory,
            >())
            .unwrap();
        contract.share("contract_app", app.into());
        contract.composer(
            &["contract_card"],
            Arc::new(|view| {
                view.with("suffix", "!".into());
                Ok(())
            }),
        );
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
    fs::create_dir_all(sources.join("components/accordion")).unwrap();
    fs::create_dir_all(sources.join("components/ranked")).unwrap();
    for (name, source) in [
        (
            "layout",
            "<main>{{ $app }}|@yield('content', '<fallback>')</main>",
        ),
        ("middle", "@extends('layout')@section('content', 'parent')"),
        (
            "page",
            r#"@extends('middle')@section('content')@parent|<x-card :title="$label"><x-slot:header><b>{{ $title }}</b></x-slot:header>@include('partial')@includeIf('missing')@includeIf('partial')@includeWhen($label, 'partial')@includeWhen(false, 'missing')@includeUnless(true, 'missing')@includeUnless(false, 'partial')<x-badge text="&"/></x-card>@endsection"#,
        ),
        (
            "components/card",
            "<article>{{ $header }}<h2>{{ $title }}</h2>{{ $slot }}</article>",
        ),
        ("partial", "<i>{{ $label }}</i>"),
        (
            "contract_card",
            "{{ $contract_app }}:{{ $title }}{{ $suffix }}",
        ),
        ("broken", "@extends('missing')"),
        ("missing_include", "@includeWhen(true, 'missing')"),
        (
            "loops",
            concat!(
                "@foreach ($groups as $key => $items)",
                "{{ $key }}={{ $loop->index }}/{{ $loop->iteration }}/",
                "{{ $loop->remaining }}/{{ $loop->count }}",
                "@if ($loop->first)F@endif@if ($loop->last)L@endif",
                "@if ($loop->odd)O@endif@if ($loop->even)E@endif:",
                "@include('loop_items')({{ $loop->depth }});",
                "@endforeach@isset($loop)leaked@endisset",
            ),
        ),
        (
            "loop_items",
            concat!(
                "@forelse ($items as $index=>$item)",
                "{{ $index }}={{ $item }}[{{ $loop->parent->iteration }}:",
                "{{ $loop->depth }}]",
                "@empty empty[{{ $loop->iteration }}]@endforelse",
            ),
        ),
        (
            "broken_loop",
            "@foreach ($items as key => $item)@endforeach",
        ),
        (
            "dynamic",
            concat!(
                r#"<x-dynamic-component :component="$componentName" :title="$label">"#,
                "<x-slot:header>{{ $app }}</x-slot:header>",
                r#"<x-dynamic-component component="badge" text="<&"/>"#,
                "</x-dynamic-component><x-ranked/>",
            ),
        ),
        (
            "components/accordion/accordion",
            "<details>{{ $header }}|{{ $title }}|{{ $slot }}@isset($component)leaked@endisset</details>",
        ),
        ("components/ranked", "root"),
        ("components/ranked/index", "index"),
        ("components/ranked/ranked", "leaf"),
        (
            "props",
            concat!(
                r#"<x-alert display-name="Override" class="wide" :data-owner="$userValue" "#,
                r#"id="outer" role="log" style="color: red" disabled :hidden="false" :title="null">"#,
                r#"<x-plain :data-child="$app"/>"#,
                "</x-alert><x-alert/>",
            ),
        ),
        (
            "components/alert",
            concat!(
                "@props(['displayName' => 'Notice', 'variant',])",
                r#"<aside {{ $attributes->merge(['class' => 'panel', 'role' => 'status', 'style' => 'padding: 0;']) }}>"#,
                "{{ $displayName }}|@isset($id)leaked@endisset{{ $slot }}</aside>",
            ),
        ),
        ("components/plain", "<span {{ $attributes }}></span>"),
        ("invalid_props", "@props(['not an identifier' => 'bad'])"),
        ("invalid_attributes", r#"<x-plain :data-invalid="$array"/>"#),
        (
            "stacks",
            concat!(
                "@extends('stack_layout')@push('scripts', 'A')@prepend('scripts', 'a')",
                "@include('stack_partial')@include('stack_partial')",
                "@push('scripts', 'C')@once('shared')duplicate@endonce",
            ),
        ),
        (
            "stack_partial",
            concat!(
                "@push('scripts')B@endpush",
                "@pushOnce('scripts')D@endPushOnce",
                "@prependOnce('scripts', 'shared')b@endPrependOnce",
                "@once[once]@endonce",
            ),
        ),
        (
            "stack_layout",
            "<main>@hasStack('scripts'){{ $app }}:@endif@stack('scripts', '<empty>')</main>",
        ),
        ("broken_stack", "@push('scripts')oops@endprepend"),
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
        "<main>A|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(
        b,
        "<main>B|parent|<article><b>route</b><h2>&lt;composer&gt;</h2><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><i>&lt;composer&gt;</i><mark>&amp;</mark></article></main>"
    );
    assert_eq!(unrelated, "<main>A|&lt;fallback&gt;</main>");
    assert!(send(first, "/broken", 500).await.contains("500"));
    assert!(send(first, "/missing-include", 500).await.contains("500"));
    assert!(send(first, "/broken-loop", 500).await.contains("500"));
    let (a, b) = tokio::join!(send(first, "/loops", 200), send(second, "/loops", 200));
    assert_eq!(
        a,
        "&lt;first&gt;=0/1/1/2FO:0=&lt;&amp;[1:2]1=two[1:2](1);second=1/2/0/2LE: empty[2](1);"
    );
    assert_eq!(a, b);
    let (a, b) = tokio::join!(send(first, "/dynamic", 200), send(second, "/dynamic", 200));
    assert_eq!(
        a,
        "<details>A|&lt;title&gt;|<mark>&lt;&amp;</mark></details>root"
    );
    assert_eq!(
        b,
        "<details>B|&lt;title&gt;|<mark>&lt;&amp;</mark></details>root"
    );
    assert!(send(first, "/dynamic-invalid", 500).await.contains("500"));
    assert!(send(first, "/dynamic-missing", 500).await.contains("500"));
    fs::remove_file(sources.join("components/ranked.blade.html")).unwrap();
    assert!(
        send(first, "/dynamic", 200)
            .await
            .ends_with("</details>index")
    );
    let (a, b) = tokio::join!(send(first, "/props", 200), send(second, "/props", 200));
    for (html, app) in [(a, "A"), (b, "B")] {
        assert_eq!(
            html,
            format!(
                concat!(
                    r#"<aside class="panel wide" role="log" style="padding: 0; color: red;" data-owner="&quot;&lt;&amp;" id="outer" disabled="disabled">"#,
                    r#"Override|<span data-child="{}"></span></aside>"#,
                    r#"<aside class="panel" role="status" style="padding: 0;">Notice|</aside>"#,
                ),
                app
            )
        );
    }
    assert!(send(first, "/invalid-props", 500).await.contains("500"));
    assert!(
        send(first, "/invalid-attributes", 500)
            .await
            .contains("500")
    );
    assert!(send(first, "/broken-stack", 500).await.contains("500"));
    let (a, b) = tokio::join!(send(first, "/stacks", 200), send(second, "/stacks", 200));
    assert_eq!(a, "[once]<main>A:baACBDB</main>");
    assert_eq!(b, "[once]<main>B:baACBDB</main>");
    assert_eq!(
        send(first, "/empty-stack", 200).await,
        "<main><empty></main>"
    );
    assert_eq!(send(first, "/plain", 200).await, unrelated);
    let (a, b) = tokio::join!(
        send(first, "/contract", 200),
        send(second, "/contract", 200)
    );
    assert_eq!(a, "A:&lt;card&gt;!");
    assert_eq!(b, "B:&lt;card&gt;!");
    first_task.abort();
    second_task.abort();
}
