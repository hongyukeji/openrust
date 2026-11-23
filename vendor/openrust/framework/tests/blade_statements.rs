use openrust::view::{Factory, ViewData};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicUsize, Ordering},
    },
};
static CORPUS: LazyLock<Value> =
    LazyLock::new(|| serde_json::from_str(include_str!("fixtures/blade_statements.json")).unwrap());
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "blade-statements-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("resources/views/components")).unwrap();
        fs::write(root.join(".env"), "APP_ENV=production\n").unwrap();
        for (name, source) in CORPUS["views"].as_object().unwrap() {
            fs::write(
                root.join("resources/views")
                    .join(format!("{}.blade.html", name.replace('.', "/"))),
                source.as_str().unwrap(),
            )
            .unwrap();
        }
        for (i, case) in CORPUS["cases"].as_array().unwrap().iter().enumerate() {
            fs::write(
                root.join("resources/views")
                    .join(format!("case_{i}.blade.html")),
                case["template"].as_str().unwrap(),
            )
            .unwrap();
        }
        Self(root.canonicalize().unwrap())
    }
    fn factory(&self) -> Factory {
        let factory = Factory::new(
            openrust::view::FileViewFinder::new([self.0.join("resources/views")]),
            self.0.join("storage/framework/views"),
        );
        configure(&factory);
        factory
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn data(case: &Value) -> ViewData {
    case["data"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone().into()))
        .collect()
}
#[test]
fn pinned_reference_statement_boundaries_escaping_and_whitespace_match() {
    assert_eq!(CORPUS["php"], "8.4.14");
    assert!(
        CORPUS["illuminate"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == "v13.40.0")
    );
    let root = Root::new();
    let factory = root.factory();
    let mut failures = Vec::new();
    for (i, case) in CORPUS["cases"].as_array().unwrap().iter().enumerate() {
        let result = factory.render(&format!("case_{i}"), &data(case));
        let matched = match &result {
            Ok(output) => {
                case.get("error").is_none() && Some(output.as_str()) == case["output"].as_str()
            }
            Err(_) => case.get("error").is_some(),
        };
        if !matched {
            failures.push(format!(
                "{i} {} data={} expected={} actual={result:?}",
                case["template"],
                case["data"],
                case.get("output").unwrap_or(&case["error"])
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.into_iter().take(24).collect::<Vec<_>>().join("\n")
    );
}
#[test]
fn cached_statement_schema_invalidates_previous_compiler_artifacts() {
    use openrust::view::{compilers::Compiler as _, engines::Engine};
    let root = Root::new();
    let factory = root.factory();
    let path = root.0.join("resources/views/cache.blade.html");
    fs::write(&path, "@i{{-- cache --}}f(true)\n<b>cache</b>@ENDIF\n").unwrap();
    assert_eq!(
        factory.render("cache", &ViewData::new()).unwrap(),
        "<b>cache</b>"
    );
    let Engine::Blade(engine) = factory.getEngineResolver().resolve("blade").unwrap() else {
        panic!()
    };
    let compiler = engine.getCompiler();
    let artifact = compiler.getCompiledPath(&path).unwrap();
    let mut contents: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(contents["compiler_version"], 45);
    contents["compiler_version"] = json!(44);
    fs::write(&artifact, contents.to_string()).unwrap();
    assert!(compiler.isExpired(&path).unwrap());
    assert_eq!(
        factory.render("cache", &ViewData::new()).unwrap(),
        "<b>cache</b>"
    );
    assert!(!compiler.isExpired(&path).unwrap());
    let rebuilt: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(rebuilt["compiler_version"], 45);
}
fn routes(
    _: &openrust::foundation::Application,
) -> Result<openrust::routing::Router, openrust::routing::RoutingError> {
    let mut router = openrust::routing::Router::new();
    router.get("/case/{id}", http_case)?;
    Ok(router)
}
fn http_case(
    request: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    let index: usize = request
        .uri()
        .path()
        .rsplit('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let case = &CORPUS["cases"][index];
    let mut view = openrust::view::view(format!("case_{index}"));
    for (k, v) in data(case) {
        view = view.with(k, v);
    }
    openrust::http::Response::view(
        openrust::http::StatusCode::OK,
        view,
        Default::default(),
        request,
    )
}
fn config(
    root: &std::path::Path,
    env: &openrust::config::Environment,
) -> Result<std::collections::BTreeMap<String, Value>, openrust::config::ConfigError> {
    Ok(std::collections::BTreeMap::from([
        (
            "app".into(),
            json!({"env":env.get("APP_ENV").unwrap_or("production"),"debug":false,"host":"127.0.0.1","port":0}),
        ),
        (
            "logging".into(),
            json!({"default":"single","deprecations":{"channel":env.get("LOG_DEPRECATIONS_CHANNEL").unwrap_or("null"),"trace":env.get("LOG_DEPRECATIONS_TRACE")==Some("true")},"channels":{"single":{"driver":"single","formatter":"json","path":root.join("storage/logs/http.log")},"broken":{"driver":"invalid"}}}),
        ),
    ]))
}
#[tokio::test]
async fn actual_http_preserves_outputs_errors_json_html_and_recovery() {
    let root = Root::new();
    let mut app = openrust::foundation::Application::configure(root.0.clone())
        .with_config(config)
        .with_routing(openrust::foundation::configuration::Routing::new(routes))
        .with_exceptions(|e| {
            e.shouldRenderJsonWhen(|r, _| r.expects_json());
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    use openrust::contracts::{container::Container as _, foundation::Application as _};
    configure(&app.container().make::<Factory>().unwrap());
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handle_transport(request));
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let client = reqwest::Client::new();
    for (i, case) in CORPUS["cases"].as_array().unwrap().iter().enumerate() {
        let response = client
            .get(format!("http://{address}/case/{i}"))
            .header("Accept", "application/json")
            .send()
            .await
            .unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        if case.get("error").is_some() {
            assert_eq!(status.as_u16(), 500, "{i}: {body}");
            assert!(serde_json::from_str::<Value>(&body).is_ok());
        } else {
            assert_eq!(status.as_u16(), 200, "{i}: {body}");
            assert_eq!(Some(body.as_str()), case["output"].as_str(), "{i}");
        }
    }
    let bad = CORPUS["cases"]
        .as_array()
        .unwrap()
        .iter()
        .position(|v| v["template"] == "{{ 1/0 }}")
        .unwrap();
    let response = client
        .get(format!("http://{address}/case/{bad}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 500);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .contains("text/html")
    );
    assert_eq!(
        client
            .get(format!("http://{address}/case/0"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        CORPUS["cases"][0]["output"].as_str().unwrap()
    );
    server.abort();
    let _ = server.await;
}

fn configure(factory: &Factory) {
    use openrust::view::{compilers::DirectiveOutput, engines::Engine};
    let Engine::Blade(engine) = factory.getEngineResolver().resolve("blade").unwrap() else {
        panic!()
    };
    let compiler = engine.getCompiler();
    compiler
        .directive("tag", |_| Ok(DirectiveOutput::Html("<tag>".into())))
        .unwrap();
    compiler
        .directive("emit", |args| {
            Ok(DirectiveOutput::Text(
                args.first()
                    .and_then(openrust::view::ViewValue::as_str)
                    .unwrap_or_default()
                    .into(),
            ))
        })
        .unwrap();
    compiler
        .r#if("active", |args| {
            Ok(args
                .first()
                .and_then(openrust::view::ViewValue::as_bool)
                .unwrap_or(true))
        })
        .unwrap();
}

#[test]
fn blade_trims_php_leading_bytes_but_file_engine_keeps_original_content() {
    let root = Root::new();
    let source = " \t\r\n\u{b}\0<p>kept</p>";
    fs::write(root.0.join("resources/views/plain.html"), source).unwrap();
    fs::write(root.0.join("resources/views/compiled.blade.html"), source).unwrap();
    let factory = root.factory();
    assert_eq!(factory.render("plain", &ViewData::new()).unwrap(), source);
    assert_eq!(
        factory.render("compiled", &ViewData::new()).unwrap(),
        "<p>kept</p>"
    );
}

#[test]
fn custom_compiler_precedence_and_conditions_remain_isolated() {
    use openrust::view::{compilers::DirectiveOutput, engines::Engine};
    let root = Root::new();
    let first = root.factory();
    let second = root.factory();
    let baseline = root.factory();
    let Engine::Blade(engine) = first.getEngineResolver().resolve("blade").unwrap() else {
        panic!()
    };
    engine
        .getCompiler()
        .directive("if", |_| Ok(DirectiveOutput::Html("<override>".into())))
        .unwrap();
    let case = &CORPUS["overrides"]["directive"];
    assert_eq!(
        first
            .renderInline(case["template"].as_str().unwrap(), &ViewData::new())
            .unwrap(),
        case["output"].as_str().unwrap()
    );
    let Engine::Blade(engine) = second.getEngineResolver().resolve("blade").unwrap() else {
        panic!()
    };
    engine.getCompiler().r#if("if", |_| Ok(false)).unwrap();
    let case = &CORPUS["overrides"]["condition"];
    assert_eq!(
        second
            .renderInline(case["template"].as_str().unwrap(), &ViewData::new())
            .unwrap(),
        case["output"].as_str().unwrap()
    );
    assert_eq!(
        baseline
            .renderInline(case["template"].as_str().unwrap(), &ViewData::new())
            .unwrap(),
        "<b>A</b>"
    );
}

#[test]
fn fragment_buffers_and_full_view_fallback_match_pinned_php() {
    let root = Root::new();
    fs::write(
        root.0.join("resources/views/fragment_composed.blade.html"),
        CORPUS["fragments"]["source"].as_str().unwrap(),
    )
    .unwrap();
    for (mode, expected) in CORPUS["fragments"]["cases"].as_object().unwrap() {
        let factory = Arc::new(root.factory());
        factory.share("sharedFragmentPass", 0);
        factory.creator("fragment_composed", |view| {
            view.data_mut().insert("pass".into(), 0.into());
            Ok(())
        });
        let weak = Arc::downgrade(&factory);
        let last_pass = Arc::new(AtomicUsize::new(0));
        let counter = last_pass.clone();
        factory.composer("fragment_composed", move |view| {
            let pass = view.data()["pass"].as_u64().unwrap() + 1;
            view.data_mut().insert("pass".into(), pass.into());
            counter.store(usize::try_from(pass).unwrap(), Ordering::Relaxed);
            weak.upgrade().unwrap().share("sharedFragmentPass", pass);
            Ok(())
        });
        let view = factory.make("fragment_composed", ViewData::new()).unwrap();
        let view = match mode.as_str() {
            "named" => view.fragments(["right", "missing", "left", "left"]),
            "all" => view.allFragments(),
            "empty" => view.fragments(Vec::<String>::new()),
            "missing" => view.fragment("missing"),
            "left" => view.fragment("left"),
            _ => panic!("unknown oracle mode"),
        };
        assert_eq!(
            factory.renderView(view).unwrap(),
            expected["output"].as_str().unwrap(),
            "{mode}"
        );
        assert_eq!(
            last_pass.load(Ordering::Relaxed) as u64,
            expected["pass"].as_u64().unwrap(),
            "{mode}"
        );
    }
}
