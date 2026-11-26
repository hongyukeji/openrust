use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::Config,
    view::{Factory, FileViewFinder, ViewData, view},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("blade-request-config-{:032x}", fastrand::u128(..)));
        fs::create_dir_all(path.join("views/components")).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, source: &str) {
        fs::write(
            self.0.join("views").join(format!("{name}.blade.html")),
            source,
        )
        .unwrap();
    }
    async fn kernel(&self, name: &'static str) -> Kernel {
        fs::write(self.0.join("app-name"), name).unwrap();
        let app = Application::configure(self.0.clone())
            .with_config(|base, _| {
                let name = fs::read_to_string(base.join("app-name")).unwrap();
                Ok(BTreeMap::from([
                ("app".into(), serde_json::json!({"name": name,"debug":true,"env":"testing", "locale":"en", "supported_locales":[], "version":"application-version", "nullable":null, "zero":0,"disabled":false})),
                ("view".into(),serde_json::json!({"paths":[base.join("views")],"compiled":base.join("cache")})),
            ]))
            })
            .with_routing(Routing::new(|_| {
                let mut router = openrust::routing::Router::new();
                router.any("/view", render)?;
                Ok(router)
            }))
            .create().unwrap();
        let mut kernel = Kernel::new(app);
        kernel.bootstrap().await.unwrap();
        kernel
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn render(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        if let Some(name) = request.header("x-config-locale")?.map(str::to_owned) {
            Config::set("app.locale", name.into())?;
        }
        Response::view(StatusCode::OK, view("main"), HeaderMap::new(), request).await
    })
}
async fn send(
    kernel: &Kernel,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> (StatusCode, String) {
    let mut builder = axum::extract::Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "example.test");
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let (parts, body) = builder
        .body(Body::from(body.to_owned()))
        .unwrap()
        .into_parts();
    let mut request = Request::from_parts(parts, body);
    let response = kernel.handle(&mut request).await;
    let (parts, body) = response.into_parts();
    (
        parts.status,
        String::from_utf8(
            axum::body::to_bytes(body, 1024 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap(),
    )
}

#[tokio::test]
async fn config_helpers_read_at_render_time_preserve_null_defaults_and_html_escaping() {
    let root = Root::new();
    root.write("main", "{{ config('app.name') }}|{{ Config::get('app.missing', 'fallback') }}|{{ config('app.nullable', 'wrong') === null }}|{{ Config::get('app.zero', 8) }}|{{ config('app.disabled', true) === false }}|{{ Config::has('app.nullable') }}|{{ Config::has('app.missing') === false }}|{{ config('app.locale') }}");
    let kernel = root.kernel("<Alpha>").await;
    assert_eq!(
        send(&kernel, "GET", "/view", &[], "").await,
        (StatusCode::OK, "&lt;Alpha&gt;|fallback|1|0|1|1|1|en".into())
    );
    assert_eq!(
        send(&kernel, "GET", "/view", &[("x-config-locale", "fr")], "").await,
        (StatusCode::OK, "&lt;Alpha&gt;|fallback|1|0|1|1|1|fr".into())
    );
    assert_eq!(
        send(&kernel, "GET", "/view", &[], "").await.1,
        "&lt;Alpha&gt;|fallback|1|0|1|1|1|en"
    );
}

#[tokio::test]
async fn request_helpers_delegate_query_input_headers_and_urls_to_the_live_request() {
    let root = Root::new();
    root.write("main", "{{ request()->query('name') }}|{{ Request::input('name') }}|{{ request()->input('nested.key') }}|{{ request()->query('missing', 'fallback') }}|{{ request()->header('X-DEMO') }}|{{ Request::path() }}|{{ request()->getPathInfo() }}|{{ request()->getBaseUrl() }}|{{ request()->url() }}|{{ request()->fullUrl() }}|{{ request()->method() }}|{{ request()->secure() === false }}|{{ request()->query()['name'] }}");
    let kernel = root.kernel("App").await;
    let (status, body) = send(
        &kernel,
        "POST",
        "/view?name=query",
        &[("content-type", "application/json"), ("x-demo", "<header>")],
        r#"{"name":"<body>","nested":{"key":"value"}}"#,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        "query|&lt;body&gt;|value|fallback|&lt;header&gt;|view|/view||http://example.test/view|http://example.test/view?name=query|POST|1|query"
    );
    root.write("main","{{ Request::input('null', 'wrong') === null }}|{{ Request::header('missing', 'absent') }}|{{ request()->input()['null'] === null }}");
    assert_eq!(
        send(
            &kernel,
            "POST",
            "/view",
            &[("content-type", "application/json")],
            r#"{"null":null}"#
        )
        .await,
        (StatusCode::OK, "1|absent|1".into())
    );
}

#[tokio::test]
async fn cached_templates_children_components_and_concurrent_apps_keep_their_context() {
    let first = Root::new();
    let second = Root::new();
    for root in [&first, &second] {
        root.write("main","@extends('layout')@section('content')@include('part')<x-read :value=\"config('app.name')\" />@endsection");
        root.write("layout", "{{ config('app.name') }}:@yield('content')");
        root.write(
            "part",
            "{{ Request::query('id') }}:{{ app()->currentLocale() }}:",
        );
        root.write(
            "components/read",
            "@props(['value']){{ $value }}:{{ request()->input('id') }}",
        );
    }
    let a = Arc::new(first.kernel("Alpha").await);
    let b = Arc::new(second.kernel("Bravo").await);
    let mut jobs = Vec::new();
    for n in 0..12 {
        let (kernel, name) = if n % 2 == 0 {
            (a.clone(), "Alpha")
        } else {
            (b.clone(), "Bravo")
        };
        jobs.push(tokio::spawn(async move {
            let query = format!("/view?id={n}");
            assert_eq!(
                send(
                    &kernel,
                    "GET",
                    &query,
                    &[("x-config-locale", if n % 2 == 0 { "fr" } else { "zh-Hant" })],
                    ""
                )
                .await,
                (
                    StatusCode::OK,
                    format!(
                        "{name}:{n}:{}:{name}:{n}",
                        if n % 2 == 0 { "fr" } else { "zh-Hant" }
                    )
                )
            );
        }));
    }
    for job in jobs {
        job.await.unwrap();
    }
    // 前面各次使用相同模板文件，后续请求仍在渲染时读取新值。
    assert_eq!(
        send(&a, "GET", "/view?id=again", &[], "").await.1,
        "Alpha:again:en:Alpha:again"
    );
}

#[tokio::test]
async fn invalid_calls_fail_without_mutating_configuration_and_next_render_recovers() {
    let root = Root::new();
    let kernel = root.kernel("Safe").await;
    for source in [
        "{{ config() }}",
        "{{ config(null) }}",
        "{{ config(['app.name'=>'bad']) }}",
        "{{ Config::set('app.name', 'bad') }}",
        "{{ config('app.name', 1, 2) }}",
        "{{ request() }}",
        "{{ request()->query(3) }}",
        "{{ request()->path('bad') }}",
        "{{ request()->header(null) }}",
        "{{ request()->header('bad name') }}",
        "{{ request()->input('x', 1, 2) }}",
        "{{ request()->merge(['x'=>1]) }}",
    ] {
        root.write("main", source);
        assert_eq!(
            send(&kernel, "GET", "/view", &[], "").await.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{source}"
        );
    }
    root.write(
        "main",
        "{{ config('app.name') }}|{{ request()->query('x', 'safe') }}",
    );
    assert_eq!(
        send(&kernel, "GET", "/view", &[], "").await,
        (StatusCode::OK, "Safe|safe".into())
    );
}

#[test]
fn request_helpers_outside_http_render_fail_instead_of_reusing_an_old_request() {
    let root = Root::new();
    root.write("main", "{{ request()->query('secret', 'wrong') }}");
    let factory = Factory::new(
        FileViewFinder::new([root.0.join("views")]),
        root.0.join("cache"),
    );
    let error = factory.render("main", &ViewData::new()).unwrap_err();
    match error {
        openrust::view::ViewError::RenderFailed(source) => {
            assert!(source.to_string().contains("HTTP"), "{source}")
        }
        other => panic!("错误种类不符：{other:?}"),
    }
}

#[tokio::test]
async fn application_locale_and_metadata_are_read_in_the_render_scope() {
    let root = Root::new();
    root.write("main", "{{ App::currentLocale() }}|{{ App::getLocale() }}|{{ Application::REPOSITORY }}|{{ Application::VERSION }}|{{ request()->localeSelection() === null }}|@include('part')|<x-current />");
    root.write("part", "{{ App::currentLocale() }}");
    root.write("components/current", "{{ App::getLocale() }}");
    let kernel = root.kernel("App").await;
    for locale in ["fr", "zh-Hant", "en", "fr"] {
        let (status, body) = send(
            &kernel,
            "GET",
            "/view",
            &[("x-config-locale", locale), ("accept-language", "de")],
            "",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            format!(
                "{locale}|{locale}|{}|{}|1|{locale}|{locale}",
                Application::REPOSITORY,
                Application::VERSION
            )
        );
    }
    assert!(
        send(&kernel, "GET", "/view", &[], "")
            .await
            .1
            .starts_with("en|en|")
    );
    for invalid in [
        "{{ App::currentLocale('fr') }}",
        "{{ App::getLocale('en') }}",
        "{{ request()->localeSelection('locale') }}",
    ] {
        root.write("main", invalid);
        assert_eq!(
            send(&kernel, "GET", "/view", &[], "").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

#[tokio::test]
async fn string_config_helper_preserves_strings_and_errors_for_missing_or_other_types() {
    let root = Root::new();
    root.write("main", "{{ Config::string('app.name') }}");
    let kernel = root.kernel("<App>").await;
    assert_eq!(
        send(&kernel, "GET", "/view", &[], "").await,
        (StatusCode::OK, "&lt;App&gt;".into())
    );
    for key in ["app.nullable", "app.zero", "app.disabled", "app.missing"] {
        root.write("main", &format!("{{{{ Config::string('{key}') }}}}"));
        assert_eq!(
            send(&kernel, "GET", "/view", &[], "").await.0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}

#[tokio::test]
async fn app_helper_reads_the_current_application_and_rejects_bad_arguments() {
    let root = Root::new();
    root.write("main", "{{ app()->currentLocale() }}|{{ app()->getLocale() }}|{{ app()->isLocale(app()->currentLocale()) }}|{{ app()->isLocale('de') }}|{{ app()->version() }}|{{ config('app.version') }}|@include('application-part')|<x-application />");
    root.write("application-part", "{{ app()->getLocale() }}");
    root.write("components/application", "{{ app()->currentLocale() }}");
    let kernel = root.kernel("App").await;
    for locale in ["en", "zh-Hant", "fr", "en"] {
        let (status, body) = send(
            &kernel,
            "GET",
            "/view",
            &[("x-config-locale", locale), ("accept-language", "de")],
            "",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body,
            format!(
                "{locale}|{locale}|1||{}|application-version|{locale}|{locale}",
                Application::VERSION
            )
        );
    }
    for source in [
        "{{ app() }}",
        "{{ app('config') }}",
        "{{ app()->currentLocale('fr') }}",
        "{{ app()->getLocale(null) }}",
        "{{ app()->version('bad') }}",
        "{{ app()->isLocale() }}",
        "{{ app()->isLocale(false) }}",
        "{{ app()->isLocale('en', 'fr') }}",
    ] {
        root.write("main", source);
        assert_eq!(
            send(&kernel, "GET", "/view", &[], "").await.0,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{source}"
        );
    }
    root.write("main", "{{ app()->currentLocale() }}");
    assert_eq!(
        send(&kernel, "GET", "/view", &[], "").await,
        (StatusCode::OK, "en".into())
    );
}

#[tokio::test]
async fn app_helper_resolves_configuration_and_application_aliases_as_real_objects() {
    let root = Root::new();
    root.write("main", "@include('app-object', ['application' => app(), 'settings' => app('config')])|{{ app()->make('config')->has('app.nullable') }}|{{ app('app')->getLocale() }}|{{ app()->environment('testing') }}|{{ app()->bound('config') }}|{{ app()->make('config')->get('app.version') }}");
    root.write(
        "app-object",
        "{{ $application->version() }}|{{ $settings->get('app.name') }}",
    );
    let kernel = root.kernel("<object>").await;
    let (status, body) = send(&kernel, "GET", "/view", &[], "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        format!(
            "{}|&lt;object&gt;|1|en|1|1|application-version",
            Application::VERSION
        )
    );
}
