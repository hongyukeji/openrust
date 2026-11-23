use openrust::view::{
    Component, ComponentOutput, Factory, FileViewFinder, ViewData, ViewResult, ViewValue,
    compilers::DirectiveOutput,
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "view-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("views/components")).unwrap();
        Self(root)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(
            self.0.join("views").join(format!("{name}.blade.html")),
            source,
        )
        .unwrap();
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
fn native_values_keep_nonfinite_floats_negative_zero_and_order_until_json_encoding() {
    let values = ViewValue::Array(vec![
        f64::INFINITY.into(),
        f64::NEG_INFINITY.into(),
        f64::NAN.into(),
        (-0.0_f64).into(),
    ]);
    assert!(values[0].as_f64().unwrap().is_infinite());
    assert!(values[1].as_f64().unwrap().is_sign_negative());
    assert!(values[2].as_f64().unwrap().is_nan());
    assert_eq!(values[3].as_f64().unwrap().to_bits(), (-0.0_f64).to_bits());
    assert!(values.to_json().is_err());
    assert!(serde_json::to_string(&values).is_err());
    let mut map = ViewData::new();
    map.insert("second".into(), 2.into());
    map.insert("first".into(), values);
    assert_eq!(
        map.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["second", "first"]
    );
    assert!(serde_json::to_value(&map).is_err());
    let finite: ViewValue = serde_json::json!({"text":"NAN","zero":-0.0}).into();
    assert_eq!(finite["text"].as_str(), Some("NAN"));
    assert_eq!(
        finite["zero"].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        finite.to_json().unwrap(),
        serde_json::json!({"text":"NAN","zero":-0.0})
    );
}
struct NativeComponent(ViewValue);
impl Component for NativeComponent {
    fn data(&self) -> ViewData {
        ViewData::from_iter([("value".into(), self.0.clone())])
    }
    fn render(&self) -> ViewResult<ComponentOutput> {
        Ok(ComponentOutput::InlineTemplate("{{ $value }}".into()))
    }
}
#[test]
fn nonfinite_numbers_cross_arrays_includes_components_and_native_extension_parameters() {
    let root = Root::new();
    root.write("part", "{{ $value }}");
    root.write("components/num", "@props(['value']){{ $value }}");
    root.write("native","{{ [INF][0] }}|@include('part',['value'=>NAN])|<x-num :value=\"-INF\" />|<x-native :value=\"NAN\" />|@nativeNumber([INF,NAN,-0.0])");
    let factory = root.factory();
    factory.component("native", |data| Ok(NativeComponent(data["value"].clone())));
    let openrust::view::engines::Engine::Blade(engine) =
        factory.getEngineResolver().resolve("blade").unwrap()
    else {
        panic!("blade");
    };
    engine
        .getCompiler()
        .directive("nativeNumber", |args| {
            assert!(args[0][0].as_f64().unwrap().is_infinite());
            assert!(args[0][1].as_f64().unwrap().is_nan());
            assert_eq!(args[0][2].as_f64().unwrap().to_bits(), (-0.0_f64).to_bits());
            Ok(DirectiveOutput::Text("native".into()))
        })
        .unwrap();
    assert_eq!(
        factory.render("native", &ViewData::new()).unwrap(),
        "INF|NAN|-INF|NAN|native"
    );
}
#[test]
fn cached_templates_receive_fresh_native_data_and_json_failure_does_not_poison_other_renders() {
    let one = Root::new();
    let two = Root::new();
    for root in [&one, &two] {
        root.write(
            "value",
            "{{ $number }}|@foreach($items as $value){{ $value }}:{{ $loop->index }};@endforeach",
        );
        root.write("json", "@js($items)");
    }
    let a = one.factory();
    let b = two.factory();
    let data = |number: f64| {
        ViewData::from_iter([
            ("number".into(), number.into()),
            (
                "items".into(),
                ViewValue::Array(vec![number.into(), (-0.0_f64).into()]),
            ),
        ])
    };
    let first = a.render("value", &data(f64::INFINITY)).unwrap();
    assert_eq!(first, "INF|INF:0;-0:1;");
    assert!(a.render("json", &data(f64::INFINITY)).is_err());
    assert_eq!(a.render("value", &data(2.0)).unwrap(), "2|2:0;-0:1;");
    assert_eq!(
        b.render("value", &data(f64::NAN)).unwrap(),
        "NAN|NAN:0;-0:1;"
    );
    assert_eq!(a.render("value", &data(f64::INFINITY)).unwrap(), first);
    assert!(a.render("json", &data(2.0)).is_ok());
}

#[test]
fn pinned_php_native_array_include_component_and_json_boundaries_match() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/blade_native_values.json")).unwrap();
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
    let openrust::view::engines::Engine::Blade(engine) =
        factory.getEngineResolver().resolve("blade").unwrap()
    else {
        panic!("blade");
    };
    engine
        .getCompiler()
        .directive("nativeKind", |args| {
            Ok(DirectiveOutput::Text(
                if args[0].is_f64() {
                    "double"
                } else if args[0].is_array() || args[0].is_object() {
                    "array"
                } else {
                    panic!("unexpected native argument")
                }
                .into(),
            ))
        })
        .unwrap();
    let mut failures = Vec::new();
    for (i, case) in corpus["cases"].as_array().unwrap().iter().enumerate() {
        let number: f64 = match case["kind"].as_str().unwrap() {
            "INF" => f64::INFINITY,
            "-INF" => f64::NEG_INFINITY,
            "NAN" => f64::NAN,
            "-0" => -0.0,
            "finite" => 2.0,
            _ => panic!("unrecognized kind"),
        };
        let data = ViewData::from_iter([
            ("number".into(), number.into()),
            (
                "items".into(),
                ViewValue::Array(vec![number.into(), (-0.0_f64).into()]),
            ),
            (
                "nested".into(),
                ViewValue::Object(ViewData::from_iter([("value".into(), number.into())])),
            ),
        ]);
        let name = format!("case_{i}");
        root.write(&name, case["template"].as_str().unwrap());
        let actual = factory.render(&name, &data);
        let matches = match &actual {
            Ok(output) => {
                case.get("error").is_none() && Some(output.as_str()) == case["output"].as_str()
            }
            Err(openrust::view::ViewError::RenderFailed(source)) => {
                case.get("error").is_some()
                    && source
                        .to_string()
                        .contains("Inf and NaN cannot be JSON encoded")
            }
            Err(_) => false,
        };
        if !matches {
            failures.push(format!(
                "{i} {} {} expected={} actual={actual:?}",
                case["kind"],
                case["template"],
                case.get("output").unwrap_or(&case["error"])
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
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
    router.get("/native", || {
        openrust::view::view("response").with("number", f64::INFINITY)
    })?;
    router.get("/nan", || {
        openrust::view::view("response").with("number", f64::NAN)
    })?;
    router.get("/json-fail", || {
        openrust::view::view("json").with("items", vec![f64::NAN])
    })?;
    router.get("/finite", || {
        openrust::view::view("response").with("number", 2.0_f64)
    })?;
    Ok(router)
}
#[tokio::test]
async fn actual_http_preserves_native_numbers_json_errors_recovery_and_application_isolation() {
    let mut servers = Vec::new();
    let mut endpoints = Vec::new();
    let mut roots = Vec::new();
    for label in ["alpha", "bravo"] {
        let root = Root::new();
        root.write(
            "response",
            "{{ $label }}|{{ $number }}|@include('part',['value'=>$number])",
        );
        root.write("part", "{{ $value }}");
        root.write("json", "@js($items)");
        let mut app = openrust::foundation::Application::configure(root.0.clone())
            .with_config(http_config)
            .with_routing(openrust::foundation::configuration::Routing::new(
                http_routes,
            ))
            .create()
            .unwrap();
        app.boot().await.unwrap();
        app.make::<Factory>().await.unwrap().share("label", label);
        let mut kernel = openrust::foundation::http::Kernel::new(app);
        let listener = kernel.listen().await.unwrap();
        let address = listener.local_addr().unwrap();
        let kernel = std::sync::Arc::new(kernel);
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
            ("native", 200, format!("{label}|INF|INF")),
            ("nan", 200, format!("{label}|NAN|NAN")),
            ("json-fail", 500, String::new()),
            ("finite", 200, format!("{label}|2|2")),
            ("native", 200, format!("{label}|INF|INF")),
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
        let listener = tokio::net::TcpListener::bind(address).await.unwrap();
        drop(listener);
    }
    drop(roots);
}

#[test]
fn native_view_values_documentation_example() {
    let mut data = ViewData::new();
    data.insert("number".into(), f64::INFINITY.into());
    data.insert("items".into(), vec![f64::NAN, -0.0].into());
    data.insert(
        "profile".into(),
        serde_json::json!({"name": "James"}).into(),
    );
    let value = ViewValue::Object(data);
    assert!(value.to_json().is_err());
    assert_eq!(value["profile"]["name"], "James");
}
