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
    LazyLock::new(|| serde_json::from_str(include_str!("fixtures/blade_arithmetic.json")).unwrap());
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "blade-arithmetic-{}-{}",
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
        Factory::new(
            openrust::view::FileViewFinder::new([self.0.join("resources/views")]),
            self.0.join("storage/framework/views"),
        )
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
fn pinned_laravel_php_arithmetic_and_contexts_match() {
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
fn cached_numeric_nodes_preserve_special_values_and_invalidate_old_compiler_schema() {
    use openrust::view::{compilers::Compiler as _, engines::Engine};
    let root = Root::new();
    let factory = root.factory();
    let path = root.0.join("resources/views/cache.blade.html");
    fs::write(&path, "{{ 1e309 }}|{{ (-1)**0.5 }}|{{ -0.0 }}|{{ 1+2 }}").unwrap();
    assert_eq!(
        factory.render("cache", &ViewData::new()).unwrap(),
        "INF|NAN|-0|3"
    );
    let Engine::Blade(engine) = factory.getEngineResolver().resolve("blade").unwrap() else {
        panic!()
    };
    let compiler = engine.getCompiler();
    let artifact = compiler.getCompiledPath(&path).unwrap();
    let mut contents: Value = serde_json::from_slice(&fs::read(&artifact).unwrap()).unwrap();
    assert_eq!(contents["compiler_version"], 39);
    contents["compiler_version"] = json!(36);
    fs::write(&artifact, contents.to_string()).unwrap();
    assert!(compiler.isExpired(&path).unwrap());
    assert_eq!(
        factory.render("cache", &ViewData::new()).unwrap(),
        "INF|NAN|-0|3"
    );
    assert!(!compiler.isExpired(&path).unwrap());
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
    let app = openrust::foundation::Application::configure(root.0.clone())
        .withConfig(config)
        .withRouting(openrust::foundation::configuration::Routing::new(routes))
        .withExceptions(|e| {
            e.shouldRenderJsonWhen(|r, _| r.expectsJson());
        })
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    let listener = kernel.listen().await.unwrap();
    let address = listener.local_addr().unwrap();
    let kernel = Arc::new(kernel);
    let router = axum::Router::new()
        .fallback(move |request: axum::extract::Request| kernel.clone().handleTransport(request));
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
        "3"
    );
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn deprecations_continue_rendering_log_per_application_and_respect_testing() {
    let index = CORPUS["cases"]
        .as_array()
        .unwrap()
        .iter()
        .position(|v| v["template"] == "{{ 5.5%2 }}")
        .unwrap();
    let mut servers = Vec::new();
    let mut roots = Vec::new();
    for (environment, enabled, trace, channel, expected) in [
        ("production", "false", false, "single", true),
        ("testing", "false", false, "single", false),
        ("testing", "(false)", false, "single", false),
        ("testing", "empty", false, "single", false),
        ("testing", "(null)", false, "single", false),
        ("testing", "0", false, "single", false),
        ("testing", "(true)", false, "single", true),
        ("testing", "true", true, "single", true),
        ("production", "false", false, "broken", false),
        ("production", "false", false, "null", false),
    ] {
        let root = Root::new();
        fs::write(root.0.join(".env"), format!("APP_ENV={environment}\nLOG_DEPRECATIONS_CHANNEL={channel}\nLOG_DEPRECATIONS_TRACE={trace}\nLOG_DEPRECATIONS_WHILE_TESTING={}\n", enabled)).unwrap();
        let app = openrust::foundation::Application::configure(root.0.clone())
            .withConfig(config)
            .withRouting(openrust::foundation::configuration::Routing::new(routes))
            .create()
            .unwrap();
        let mut kernel = openrust::foundation::http::Kernel::new(app);
        let listener = kernel.listen().await.unwrap();
        let address = listener.local_addr().unwrap();
        let kernel = Arc::new(kernel);
        let router = axum::Router::new()
            .fallback(move |r: axum::extract::Request| kernel.clone().handleTransport(r));
        servers.push(tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        }));
        roots.push((root, address, expected, trace));
    }
    let client = reqwest::Client::new();
    // All applications remain alive; scoped configuration and named loggers must not leak.
    for (root, address, expected, trace) in &roots {
        let response = client
            .get(format!("http://{address}/case/{index}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().await.unwrap(), "1");
        let log = fs::read_to_string(root.0.join("storage/logs/http.log")).unwrap_or_default();
        assert_eq!(
            log.contains("Implicit conversion from float 5.5 to int loses precision"),
            *expected,
            "{log}"
        );
        if *expected {
            assert_eq!(log.contains("exception"), *trace, "{log}");
            if *trace {
                let record: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
                assert!(
                    record["context"]["trace"]
                        .as_str()
                        .is_some_and(|v| !v.is_empty())
                );
                assert!(
                    record["context"]["file"]
                        .as_str()
                        .unwrap()
                        .ends_with("arithmetic.rs")
                );
                assert!(record["context"]["line"].as_u64().unwrap() > 0);
            }
        }
    }
    for server in servers {
        server.abort();
        let _ = server.await;
    }
}
#[test]
fn native_nonfinite_boundaries_match_php_and_json_errors_do_not_poison_rendering() {
    let root = Root::new();
    let factory = root.factory();
    for (i, (template, expected)) in [
        ("{{ [INF][0] }}", "INF"),
        ("@include('part',['value'=>NAN])", "NAN"),
        ("<x-num :value=\"INF\" />", "INF"),
    ]
    .iter()
    .enumerate()
    {
        let name = format!("native_boundary_{i}");
        fs::write(
            root.0
                .join("resources/views")
                .join(format!("{name}.blade.html")),
            template,
        )
        .unwrap();
        assert_eq!(factory.render(&name, &ViewData::new()).unwrap(), *expected);
    }
    for (i, template) in ["@js(INF)", "@js([NAN])", "@js(['value'=>-INF])"]
        .iter()
        .enumerate()
    {
        let name = format!("json_boundary_{i}");
        fs::write(
            root.0
                .join("resources/views")
                .join(format!("{name}.blade.html")),
            template,
        )
        .unwrap();
        let error = factory.render(&name, &ViewData::new()).unwrap_err();
        assert!(
            std::error::Error::source(&error)
                .unwrap()
                .to_string()
                .contains("Inf and NaN cannot be JSON encoded")
        );
    }
    assert_eq!(
        factory
            .render("case_0", &data(&CORPUS["cases"][0]))
            .unwrap(),
        "3"
    );
}
