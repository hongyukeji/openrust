use std::{fs, path::PathBuf};

use openrust::{
    contracts::http::Kernel as _,
    foundation::{Application, configuration::Routing, http::Kernel},
    http::{Body, HeaderMap, HttpError, HttpFuture, Request, Response, StatusCode},
    support::facades::{Lang, Route},
    view::{Factory, FileViewFinder, ViewData, ViewError, view},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "blade-translations-{}-{}",
            std::process::id(),
            openrust::support::Str::random(12)
        ));
        for directory in ["resources/views/components", "lang/en", "lang/zh-Hans"] {
            fs::create_dir_all(path.join(directory)).unwrap();
        }
        fs::write(
            path.join("lang/en/messages.json"),
            r#"{
            "hello":"Hello :name", "plural":"{0} none|{1} one|[2,*] :count items",
            "html":"<b>:name</b>", "fallback":"Fallback", "nested":"messages.hello"
        }"#,
        )
        .unwrap();
        fs::write(
            path.join("lang/zh-Hans/messages.json"),
            r#"{
            "hello":"你好 :name", "plural":"{0} 无|[1,*] :count 项"
        }"#,
        )
        .unwrap();
        fs::write(
            path.join("resources/views/part.blade.html"),
            "{{ __('messages.hello', ['name' => $name]) }}",
        )
        .unwrap();
        fs::write(
            path.join("resources/views/components/translated.blade.html"),
            "@props(['name']){{ __('messages.hello', ['name' => $name]) }}",
        )
        .unwrap();
        Self(path)
    }
    async fn kernel(&self) -> Kernel {
        let application = Application::configure(self.0.clone())
            .withRouting(Routing::default().web(|| Route::get("/test", render)))
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        kernel
    }
    async fn render(
        &self,
        kernel: &Kernel,
        source: &str,
        locale: &str,
        status: StatusCode,
    ) -> String {
        let path = self.0.join("resources/views/test.blade.html");
        if fs::read_to_string(&path).ok().as_deref() != Some(source) {
            fs::write(path, source).unwrap();
        }
        let incoming = axum::extract::Request::builder()
            .uri("/test")
            .header("x-locale", locale)
            .body(Body::empty())
            .unwrap();
        let (parts, body) = incoming.into_parts();
        let mut request = Request::from_parts(parts, body);
        let response = kernel.handle(&mut request).await;
        assert_eq!(response.status(), status, "{source}");
        String::from_utf8(
            axum::body::to_bytes(response.into_parts().1, 64 * 1024)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn render(request: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let locale = request
            .headers()
            .get("x-locale")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        Lang::withLocale(&locale, async {
            let rendered = Response::view(
                StatusCode::OK,
                view("test")
                    .with("name", "<Alice>")
                    .with("count", 3)
                    .with("key", "messages.hello"),
                HeaderMap::new(),
                request,
            )
            .await;
            // 模板内显式语言覆盖不应改变外层语言。
            assert_eq!(Lang::getLocale().unwrap(), locale);
            rendered
        })
        .await
        .map_err(|source| HttpError::Internal {
            source: Box::new(source),
        })?
    })
}

#[tokio::test]
async fn helpers_translate_at_render_time_including_cached_templates_and_includes() {
    let root = Root::new();
    let kernel = root.kernel().await;
    let source = "{{ __($key, ['name' => $name]) }}|{{ trans('messages.hello', ['name' => 'Bob'], 'en') }}|{{ __('messages.fallback') }}|{{ __('missing.key') }}|@include('part')|<x-translated :name=\"$name\" />";
    for _ in 0..2 {
        assert_eq!(
            root.render(&kernel, source, "zh-Hans", StatusCode::OK)
                .await,
            "你好 &lt;Alice&gt;|Hello Bob|Fallback|missing.key|你好 &lt;Alice&gt;|你好 &lt;Alice&gt;"
        );
        assert_eq!(
            root.render(&kernel, source, "en", StatusCode::OK).await,
            "Hello &lt;Alice&gt;|Hello Bob|Fallback|missing.key|Hello &lt;Alice&gt;|Hello &lt;Alice&gt;"
        );
    }
    assert_eq!(
        root.render(
            &kernel,
            "{{ __(trans('messages.nested'), ['name' => 'Bob'], null,) }}",
            "en",
            StatusCode::OK
        )
        .await,
        "Hello Bob"
    );
}

#[tokio::test]
async fn choice_and_directives_keep_replacements_and_html_escaping_semantics() {
    let root = Root::new();
    let kernel = root.kernel().await;
    for (source, expected) in [
        ("{{ trans_choice('messages.plural', $count) }}", "3 items"),
        ("{{ trans_choice('messages.plural', []) }}", "none"),
        ("{{ trans_choice('messages.plural', [1]) }}", "one"),
        ("@choice('messages.plural', 4)", "4 items"),
        (
            "{{ trans_choice('messages.plural', 5, [], 'zh-Hans') }}",
            "5 项",
        ),
        (
            "{{ __('messages.html', ['name' => $name]) }}",
            "&lt;b&gt;&lt;Alice&gt;&lt;/b&gt;",
        ),
        (
            "{!! __('messages.html', ['name' => $name]) !!}",
            "<b><Alice></b>",
        ),
        (
            "@lang('messages.html', ['name' => $name])",
            "<b><Alice></b>",
        ),
    ] {
        assert_eq!(
            root.render(&kernel, source, "en", StatusCode::OK).await,
            expected
        );
    }
}

#[tokio::test]
async fn invalid_arguments_and_missing_translator_fail_instead_of_leaking_keys_or_source() {
    let root = Root::new();
    let kernel = root.kernel().await;
    for source in [
        "{{ __('key', [], 'en', 1) }}",
        "{{ trans_choice('key') }}",
        "{{ __(123) }}",
        "{{ __('key', 'invalid') }}",
        "{{ __('key', ['x' => []]) }}",
        "{{ __('key', [], 12) }}",
        "{{ trans_choice('messages.plural', 'invalid') }}",
        "{{ system('id') }}",
        "@lang",
        "@choice('key')",
    ] {
        root.render(&kernel, source, "en", StatusCode::INTERNAL_SERVER_ERROR)
            .await;
    }
    fs::write(
        root.0.join("resources/views/no-context.blade.html"),
        "{{ __('messages.hello') }}",
    )
    .unwrap();
    let factory = Factory::new(
        FileViewFinder::new([root.0.join("resources/views")]),
        root.0.join("cache"),
    );
    assert!(matches!(
        factory.render("no-context", &ViewData::new()),
        Err(ViewError::RenderFailed(_))
    ));
}

#[tokio::test]
async fn translation_blocks_trim_rendered_keys_and_share_upstream_replacements() {
    let root = Root::new();
    let kernel = root.kernel().await;
    for (source, expected) in [
        ("@lang messages.hello @endlang", "Hello :name"),
        (
            "@lang(['name' => $name]) \n messages.hello \n @endlang",
            "Hello <Alice>",
        ),
        ("@lang(['name' => 'Ada']) {{$key}} @endlang", "Hello Ada"),
        (
            "@lang(['name' => $name]) messages.html @endlang",
            "<b><Alice></b>",
        ),
        (
            "@lang(['name' => 'outer']) :name @lang(['name' => 'inner']) messages.hello @endlang @endlang",
            "inner Hello inner",
        ),
    ] {
        assert_eq!(
            root.render(&kernel, source, "en", StatusCode::OK).await,
            expected
        );
    }
    assert_eq!(
        root.render(
            &kernel,
            "@lang(['name' => 'Ada']) messages.hello @endlang",
            "zh-Hans",
            StatusCode::OK
        )
        .await,
        "你好 Ada"
    );
    for source in [
        "@lang(['name' => []]) messages.hello @endlang",
        "@lang messages.hello",
        "@endlang",
    ] {
        root.render(&kernel, source, "en", StatusCode::INTERNAL_SERVER_ERROR)
            .await;
    }
}

#[tokio::test]
async fn helper_nulls_group_values_and_directive_fallback_follow_their_own_contracts() {
    let root = Root::new();
    fs::write(
        root.0.join("lang/en/arrays.json"),
        r#"{"items":["Hi :name","Bye :name"],"labels":{"title":"Title :name"}}"#,
    )
    .unwrap();
    let kernel = root.kernel().await;
    for (source, expected) in [
        ("A{{ __() }}B{{ __(null) }}C", "ABC"),
        ("{{ __(null, 'ignored', 42) }}ok", "ok"),
        (
            "@foreach(__('arrays.items', ['name' => 'Ada']) as $item){{ $item }};@endforeach",
            "Hi Ada;Bye Ada;",
        ),
        (
            "{{ trans('arrays.labels', ['name' => 'Ada'])['title'] }}",
            "Title Ada",
        ),
        (
            "@lang('messages.fallback', [], null, false)",
            "messages.fallback",
        ),
        ("@lang('messages.fallback', [], null, true)", "Fallback"),
        ("@lang('messages.fallback', [], 'en', false)", "Fallback"),
        (
            "{{ __('messages.hello', ['name' => 'Ada'], '') }}",
            "你好 Ada",
        ),
        (
            "{{ trans_choice('messages.plural', 3, ['count' => null]) }}",
            "3 项",
        ),
        (
            "{{ trans_choice('messages.plural', 3, ['count' => 0]) }}",
            "0 项",
        ),
    ] {
        assert_eq!(
            root.render(&kernel, source, "zh-Hans", StatusCode::OK)
                .await,
            expected,
            "{source}"
        );
    }
    for source in [
        "{{ trans() }}",
        "{{ trans(null) }}",
        "@lang('key', [], null, [])",
        "{{ __('arrays.items') }}",
    ] {
        root.render(&kernel, source, "en", StatusCode::INTERNAL_SERVER_ERROR)
            .await;
    }
}

#[tokio::test]
async fn template_helpers_preserve_missing_key_hook_context_and_replacements() {
    use openrust::{contracts::container::Container as _, contracts::foundation::Application as _};
    let root = Root::new();
    let kernel = root.kernel().await;
    kernel
        .get_application()
        .container()
        .get::<openrust::translation::Translator>("translator")
        .unwrap()
        .handleMissingKeysUsing(|missing| {
            assert_eq!(missing.key, "hook.missing");
            assert_eq!(missing.replacements["name"], "ada");
            Some(format!("{}:{}::Name", missing.locale, missing.fallback))
        });
    let source = "{{ __('hook.missing', ['name' => 'ada']) }}|@lang('hook.missing', ['name' => 'ada'], null, false)";
    for (locale, expected) in [
        ("zh-Hans", "zh-Hans:true:Ada|zh-Hans:false:Ada"),
        ("en", "en:true:Ada|en:false:Ada"),
    ] {
        assert_eq!(
            root.render(&kernel, source, locale, StatusCode::OK).await,
            expected
        );
    }
}
