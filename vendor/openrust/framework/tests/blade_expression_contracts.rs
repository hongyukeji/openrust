use openrust::view::{
    Component, ComponentOutput, Factory, FileViewFinder, ViewData, ViewError, ViewResult,
    ViewValue,
    compilers::{BladeCompiler, Compiler, DirectiveOutput},
    engines::Engine,
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("blade-expression-{:032x}", fastrand::u128(..)));
        fs::create_dir_all(root.join("views/components")).unwrap();
        Self(root.canonicalize().unwrap())
    }
    fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.0.join("views").join(format!("{name}.blade.html"));
        fs::write(&path, source).unwrap();
        path
    }
    fn factory(&self) -> Factory {
        Factory::new(
            FileViewFinder::new([self.0.join("views")]),
            self.0.join("cache"),
        )
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn pinned_php_array_unpacking_preserves_keys_order_types_and_failure_boundaries() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/blade_unpacking.json")).unwrap();
    assert_eq!(corpus["php"], "8.4.14");
    assert!(
        corpus["illuminate"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "v13.40.0")
    );
    let root = Root::new();
    for (name, source) in corpus["views"].as_object().unwrap() {
        root.write(&name.replace('.', "/"), source.as_str().unwrap());
    }
    let factory = root.factory();
    root.write(
        "spread_doc",
        "{{ [...['color' => 'blue'], ...['color' => 'red']]['color'] }}",
    );
    assert_eq!(
        factory.render("spread_doc", &ViewData::new()).unwrap(),
        "red"
    );
    root.write("empty_part", "ok");
    root.write("empty_include", "@include('empty_part', [...[]])");
    assert_eq!(
        factory.render("empty_include", &ViewData::new()).unwrap(),
        "ok"
    );
    let mut failures = Vec::new();
    for (index, case) in corpus["cases"].as_array().unwrap().iter().enumerate() {
        let data = case["data"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(key, value)| (key.clone(), value.clone().into()))
            .collect();
        let name = format!("case_{index}");
        root.write(&name, case["template"].as_str().unwrap());
        let actual = factory.render(&name, &data);
        let matches = match &actual {
            Ok(output) => {
                case.get("error").is_none() && Some(output.as_str()) == case["output"].as_str()
            }
            Err(ViewError::RenderFailed(source)) => {
                case.get("error").is_some()
                    && (source.to_string().contains("unpacked")
                        || source.to_string().contains("index overflow"))
            }
            Err(_) => false,
        };
        if !matches {
            failures.push(format!("{index}: {case} => {actual:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

struct Caption(String);
impl Component for Caption {
    fn data(&self) -> ViewData {
        ViewData::from_iter([("caption".into(), self.0.clone().into())])
    }
    fn render(&self) -> ViewResult<ComponentOutput> {
        Ok(ComponentOutput::InlineTemplate(
            "<b>{{ $caption }}</b>{{ $slot }}".into(),
        ))
    }
}

#[test]
fn rust_projection_callbacks_and_compilation_have_explicit_lifecycles() {
    let root = Root::new();
    let path = root.write("lifecycle", "{{ $prepared }}|@formatName([...$items])|@enabled <x-caption>{{ $component->caption }}</x-caption>@endenabled");
    let factory = root.factory();
    let Engine::Blade(engine) = factory.getEngineResolver().resolve("blade").unwrap() else {
        panic!("blade");
    };
    let compiler = engine.getCompiler();
    let calls = Arc::new(AtomicUsize::new(0));
    let directives = calls.clone();
    compiler
        .directive("formatName", move |arguments| {
            directives.fetch_add(1, Ordering::SeqCst);
            Ok(DirectiveOutput::Text(
                arguments[0][0].as_str().unwrap().to_uppercase(),
            ))
        })
        .unwrap();
    compiler.r#if("enabled", |_| Ok(true)).unwrap();
    let creations = Arc::new(AtomicUsize::new(0));
    let creators = creations.clone();
    factory.creator("lifecycle", move |_| {
        creators.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let components = creations.clone();
    factory.component("caption", move |_| {
        components.fetch_add(1, Ordering::SeqCst);
        Ok(Caption("<caption>".into()))
    });
    let composers = calls.clone();
    factory.composer("lifecycle", move |view| {
        composers.fetch_add(1, Ordering::SeqCst);
        view.data_mut()
            .insert("prepared".into(), "<prepared>".into());
        Ok(())
    });
    compiler.compile(&path).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(creations.load(Ordering::SeqCst), 0);
    let data = ViewData::from_iter([("items".into(), vec!["<name>"].into())]);
    assert_eq!(
        factory.render("lifecycle", &data).unwrap(),
        "&lt;prepared&gt;|&lt;NAME&gt;| <b>&lt;caption&gt;</b>&lt;caption&gt;"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(creations.load(Ordering::SeqCst), 2);
    compiler
        .directive("formatName", |_| {
            Ok(DirectiveOutput::Text("replacement".into()))
        })
        .unwrap();
    assert!(
        factory
            .render("lifecycle", &data)
            .unwrap()
            .contains("replacement")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(creations.load(Ordering::SeqCst), 4);
}

#[test]
fn unsupported_php_runtime_syntax_fails_compilation_but_literals_and_unknown_directives_survive() {
    let root = Root::new();
    let compiler = BladeCompiler::new(root.0.join("cache"));
    for (i, source) in [
        "{{ strtoupper($name) }}",
        "{{ $callback($name) }}",
        "{{ $model->method() }}",
        "{{ $$name }}",
        "{{ [&$name] }}",
        "{{ [...&$items] }}",
        "@foreach($items as &$item){{ $item }}@endforeach",
        "@php $value = 1; @endphp",
        "@PHP($value = 1)",
        "@use('App\\Model')",
        "@if(false){{ arbitrary_function() }}@endif",
        "{{ [1 => ...$items] }}",
    ]
    .iter()
    .enumerate()
    {
        let path = root.write(&format!("invalid_{i}"), source);
        assert!(
            matches!(compiler.compile(&path), Err(ViewError::CompileFailed(_))),
            "{source}"
        );
        assert!(
            !compiler.getCompiledPath(&path).unwrap().exists(),
            "{source}"
        );
    }
    assert_eq!(
        compiler
            .render(
                &root.0.join("literal"),
                "@verbatim@php x @endphp@endverbatim|@@use('x')|@unknown(x)|{{ 'strtoupper(x)' }}",
                &ViewData::new()
            )
            .unwrap(),
        "@php x @endphp|@use('x')|@unknown(x)|strtoupper(x)"
    );
}

#[test]
fn unpacking_reads_fresh_values_and_does_not_mutate_sources_or_poison_cached_templates() {
    let root = Root::new();
    root.write("value", "{{ [...$items][0] ?? 'empty' }}");
    let factory = root.factory();
    for (number, expected) in [
        (f64::INFINITY, "INF"),
        (f64::NAN, "NAN"),
        (-0.0, "-0"),
        (2.0, "2"),
    ] {
        let source = ViewValue::from(vec![number]);
        let data = ViewData::from_iter([("items".into(), source.clone())]);
        let output = factory.render("value", &data).unwrap();
        assert_eq!(output, expected);
        assert_eq!(
            source[0].as_f64().unwrap().to_bits(),
            data["items"][0].as_f64().unwrap().to_bits()
        );
    }
    assert!(
        factory
            .render(
                "value",
                &ViewData::from_iter([("items".into(), false.into())])
            )
            .is_err()
    );
    assert_eq!(
        factory
            .render(
                "value",
                &ViewData::from_iter([("items".into(), vec!["after"].into())])
            )
            .unwrap(),
        "after"
    );
}

fn http_config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            serde_json::json!({"env":"production","host":"127.0.0.1","port":0,"debug":false}),
        ),
        (
            "view".into(),
            serde_json::json!({"paths":[root.join("views")],"compiled":root.join("cache")}),
        ),
    ]))
}
fn http_routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.get("/spread", || {
        openrust::view::view("spread").with("items", vec![f64::INFINITY, f64::NAN, -0.0])
    })?;
    router.get("/invalid", || {
        openrust::view::view("spread").with("items", false)
    })?;
    router.get("/finite", || {
        openrust::view::view("spread").with("items", vec![2])
    })?;
    router.get("/php", || openrust::view::view("php"))?;
    Ok(router)
}

#[tokio::test]
async fn actual_http_unpacks_native_data_recovers_from_failures_and_isolates_applications() {
    let mut servers = Vec::new();
    let mut endpoints = Vec::new();
    let mut roots = Vec::new();
    for label in ["alpha", "bravo"] {
        let root = Root::new();
        root.write(
            "spread",
            "{{ $label }}|@include('part',[...['values'=>[...$items]]])",
        );
        root.write(
            "part",
            "@foreach($values as $key=>$value){{ $key }}:{{ $value }};@endforeach",
        );
        root.write("php", "@php $value = 1; @endphp");
        let mut app = openrust::foundation::Application::configure(root.0.clone())
            .withConfig(http_config)
            .withRouting(openrust::foundation::configuration::Routing::new(
                http_routes,
            ))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        app.make::<Factory>().await.unwrap().share("label", label);
        let mut kernel = openrust::foundation::http::Kernel::new(app);
        let listener = kernel.listen().await.unwrap();
        let address = listener.local_addr().unwrap();
        let kernel = Arc::new(kernel);
        let router = axum::Router::new().fallback(move |request: axum::extract::Request| {
            kernel.clone().handleTransport(request)
        });
        servers.push(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }));
        endpoints.push((address, label));
        roots.push(root);
    }
    let client = reqwest::Client::new();
    for (address, label) in &endpoints {
        for (path, status, expected) in [
            ("spread", 200, format!("{label}|0:INF;1:NAN;2:-0;")),
            ("invalid", 500, String::new()),
            ("php", 500, String::new()),
            ("finite", 200, format!("{label}|0:2;")),
            ("spread", 200, format!("{label}|0:INF;1:NAN;2:-0;")),
        ] {
            let response = client
                .get(format!("http://{address}/{path}"))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
            let body = response.text().await.unwrap();
            if status == 200 {
                assert_eq!(body, expected);
            }
        }
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
    for (address, _) in endpoints {
        drop(tokio::net::TcpListener::bind(address).await.unwrap());
    }
    drop(roots);
}
