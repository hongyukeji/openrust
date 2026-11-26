use openrust::{
    contracts::foundation::Application as _,
    support::facades::{App, Blade, Facade, View},
    testing::TestApp,
    view::{
        ViewData, ViewValue,
        compilers::{BladeCompiler, DirectiveOutput},
    },
};
use std::{fs, sync::Arc};

async fn fixture() -> TestApp {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    app
}
#[tokio::test]
async fn compiler_binding_and_facade_share_extensions_with_real_view_rendering() {
    let app = fixture().await;
    let views = app.app().base_path().join("resources/views");
    fs::create_dir_all(&views).unwrap();
    fs::write(
        views.join("demo.blade.html"),
        "@hello('world') @allowed(true)<b>yes</b>@endallowed",
    )
    .unwrap();
    App::set_facade_application(Some(app.app()), async {
        Blade::directive("hello", |values| {
            Ok(DirectiveOutput::Text(format!(
                "<{}>",
                values[0].as_str().unwrap()
            )))
        })
        .unwrap();
        Blade::r#if("allowed", |values| Ok(values[0] == ViewValue::Bool(true))).unwrap();
        assert!(Blade::check("allowed", &[true.into()]).unwrap());
        assert_eq!(
            View::render("demo", &ViewData::new()).unwrap(),
            "&lt;world&gt; <b>yes</b>"
        );
        assert_eq!(
            Blade::render("@hello('inline')", &ViewData::new()).unwrap(),
            "&lt;inline&gt;"
        );
        assert_eq!(
            Blade::get_custom_directives()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["hello"]
        );
        assert!(Arc::ptr_eq(
            &App::make::<BladeCompiler>().await.unwrap(),
            &Blade::get_facade_root().unwrap()
        ));
        assert!(
            Blade::directive("invalid-name", |_| Ok(DirectiveOutput::Html("bad".into()))).is_err()
        );
        assert!(Blade::check("missing", &[]).is_err());
    })
    .await;
    assert!(Blade::compile_string("hello").is_err());
}
#[tokio::test]
async fn compiled_strings_reuse_native_instructions_and_capture_extension_snapshots() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let data: ViewData = [("name".into(), "&amp;<x>".into())].into();
        let double = Blade::compile_string("{{ $name }}").unwrap();
        assert_eq!(double.render(&data).unwrap(), "&amp;amp;&lt;x&gt;");
        Blade::without_double_encoding().unwrap();
        assert_eq!(
            Blade::render("{{ $name }}", &data).unwrap(),
            "&amp;&lt;x&gt;"
        );
        assert_eq!(double.render(&data).unwrap(), "&amp;amp;&lt;x&gt;");
        Blade::with_double_encoding().unwrap();
        assert_eq!(
            View::render_inline("{{ $name }}", &data).unwrap(),
            "&amp;amp;&lt;x&gt;"
        );
        struct Label(String);
        Blade::stringable(|value: &Label| Ok(value.0.clone())).unwrap();
        assert_eq!(
            Blade::apply_echo_handler(&Label("native".into())).unwrap(),
            "native"
        );
        assert!(Blade::apply_echo_handler(&17_i32).is_err());
        assert_eq!(Blade::strip_parentheses("(a + (b))"), b"a + (b)");
        assert_eq!(Blade::strip_parentheses("a"), b"a");
    })
    .await;
}
#[tokio::test]
async fn compile_uses_selected_path_and_publishes_artifacts_consumed_by_the_factory() {
    let app = fixture().await;
    let views = app.app().base_path().join("resources/views");
    fs::create_dir_all(&views).unwrap();
    let path = views.join("demo.blade.html");
    fs::write(&path, "one {{ $name }}").unwrap();
    App::set_facade_application(Some(app.app()), async {
        assert!(Blade::get_path().unwrap().is_none());
        assert!(Blade::compile(None).is_err());
        assert!(Blade::is_expired(&path).unwrap());
        Blade::set_path(&path).unwrap();
        Blade::compile(None).unwrap();
        assert_eq!(Blade::get_path().unwrap().as_deref(), Some(path.as_path()));
        assert!(!Blade::is_expired(&path).unwrap());
        assert!(Blade::get_compiled_path(&path).unwrap().is_file());
        let data: ViewData = [("name".into(), "A".into())].into();
        assert_eq!(View::render("demo", &data).unwrap(), "one A");
        fs::write(&path, "two {{ $name }}").unwrap();
        assert!(Blade::is_expired(&path).unwrap());
        Blade::compile(Some(&path)).unwrap();
        assert_eq!(View::render("demo", &data).unwrap(), "two A");
        assert!(Blade::compile(Some(&views.join("missing"))).is_err());
    })
    .await;
}
#[tokio::test]
async fn compiler_directives_do_not_leak_between_applications() {
    let first = fixture().await;
    let second = fixture().await;
    App::set_facade_application(Some(first.app()), async {
        Blade::directive("owner", |_| Ok(DirectiveOutput::Html("first".into()))).unwrap();
        App::set_facade_application(Some(second.app()), async {
            assert!(Blade::get_custom_directives().unwrap().is_empty());
            Blade::directive("owner", |_| Ok(DirectiveOutput::Html("second".into()))).unwrap();
            assert_eq!(
                View::render_inline("@owner", &ViewData::new()).unwrap(),
                "second"
            );
        })
        .await;
        assert_eq!(Blade::render("@owner", &ViewData::new()).unwrap(), "first");
    })
    .await;
}

#[test]
fn strip_parentheses_keeps_the_pinned_byte_contract_for_malformed_input() {
    assert_eq!(Blade::strip_parentheses("(abc"), b"ab");
    assert_eq!(Blade::strip_parentheses("("), b"");
    assert_eq!(Blade::strip_parentheses("(中"), vec![0xe4, 0xb8]);
}
