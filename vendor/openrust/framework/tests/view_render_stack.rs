use openrust::{
    support::facades::{App, Blade, Facade as _},
    testing::TestApp,
    view::ViewData,
};

#[tokio::test]
async fn nested_blade_blocks_render_within_the_parser_depth_limit_on_default_threads() {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        for depth in [1, 16, 40, 64] {
            let source = format!(
                "{}content\n{}",
                "@if(true)\n".repeat(depth),
                "@endif\n".repeat(depth)
            );
            let compiled = Blade::compile_string(&source).unwrap();
            assert_eq!(compiled.render(&ViewData::new()).unwrap().trim(), "content");
        }
        let excessive = format!(
            "{}content\n{}",
            "@if(true)\n".repeat(65),
            "@endif\n".repeat(65)
        );
        assert!(Blade::compile_string(&excessive).is_err());
    })
    .await;
}
