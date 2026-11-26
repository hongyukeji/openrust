use openrust::{
    filesystem::{FileMacro, Filesystem},
    foundation::vite::{Vite as ViteService, ViteMacro},
    support::facades::{App, Facade, File, Vite},
    testing::TestApp,
};
use std::{cell::Cell, rc::Rc};
#[tokio::test]
async fn synchronous_file_and_vite_macros_preserve_non_send_argument_and_result_identity() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/macros/synchronous-values.json")).unwrap();
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        let file = App::make::<Filesystem>().await.unwrap();
        let vite = App::make::<ViteService>().await.unwrap();
        File::r#macro("identity", |_, value: Rc<Cell<u32>>| {
            value.set(7);
            Ok(value)
        })
        .unwrap();
        Vite::r#macro("identity", |_, value: Rc<Cell<u32>>| {
            value.set(7);
            Ok(value)
        })
        .unwrap();
        let a = Rc::new(Cell::new(1));
        let actual = File::macro_call::<Rc<Cell<u32>>, _>("identity", a.clone()).unwrap();
        assert_eq!(Rc::ptr_eq(&a, &actual), expected[0]["same"]);
        assert_eq!(a.get(), expected[0]["number"]);
        let a = Rc::new(Cell::new(1));
        let actual = Vite::macro_call::<Rc<Cell<u32>>, _>("identity", a.clone()).unwrap();
        assert_eq!(Rc::ptr_eq(&a, &actual), expected[1]["same"]);
        assert_eq!(a.get(), expected[1]["number"]);
        let a = Rc::new(Cell::new(1));
        assert!(Rc::ptr_eq(
            &a,
            &file
                .macro_call::<Rc<Cell<u32>>, _>("identity", a.clone())
                .unwrap()
        ));
        let a = Rc::new(Cell::new(1));
        assert!(Rc::ptr_eq(
            &a,
            &vite
                .macro_call::<Rc<Cell<u32>>, _>("identity", a.clone())
                .unwrap()
        ));
        File::mixin(
            [("mixin", FileMacro::new(|_, value: Rc<Cell<u32>>| Ok(value)))],
            true,
        )
        .unwrap();
        Vite::mixin(
            [("mixin", ViteMacro::new(|_, value: Rc<Cell<u32>>| Ok(value)))],
            true,
        )
        .unwrap();
        let a = Rc::new(Cell::new(1));
        assert!(Rc::ptr_eq(
            &a,
            &File::macro_call::<Rc<Cell<u32>>, _>("mixin", a.clone()).unwrap()
        ));
        assert!(Rc::ptr_eq(
            &a,
            &Vite::macro_call::<Rc<Cell<u32>>, _>("mixin", a.clone()).unwrap()
        ));
        assert!(File::macro_call::<String, _>("identity", a.clone()).is_err());
        assert!(Vite::macro_call::<String, _>("identity", a).is_err());
        let openrust::filesystem::FileError::MacroType {
            name,
            expected,
            actual,
        } = File::macro_call::<u32, _>("identity", 1_u32).unwrap_err()
        else {
            panic!("需要具类型宏错误")
        };
        assert_eq!(name, "identity");
        assert_eq!(expected, std::any::type_name::<Rc<Cell<u32>>>());
        assert_eq!(actual, std::any::type_name::<u32>());
        assert!(Vite::macro_call::<u32, _>("identity", 1_u32).is_err());
    })
    .await;
}
fn endpoint(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let value = Rc::new(Cell::new(1));
        let file =
            File::macro_call::<Rc<Cell<u32>>, _>("identity", value.clone()).map_err(|source| {
                openrust::http::HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
        let vite =
            Vite::macro_call::<Rc<Cell<u32>>, _>("identity", value.clone()).map_err(|source| {
                openrust::http::HttpError::Internal {
                    source: Box::new(source),
                }
            })?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({"file":Rc::ptr_eq(&value,&file),"vite":Rc::ptr_eq(&value,&vite),"number":value.get()}),
        )
    })
}
#[tokio::test]
async fn real_http_can_use_owned_non_send_values_during_synchronous_macro_calls() {
    let mut app = TestApp::create(|b| {
        b.with_routing(
            openrust::foundation::configuration::Routing::default()
                .web(|| openrust::support::facades::Route::get("/sync-macros", endpoint)),
        )
    })
    .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(app.app()), async {
        File::r#macro("identity", |_, value: Rc<Cell<u32>>| {
            value.set(7);
            Ok(value)
        })
        .unwrap();
        Vite::r#macro("identity", |_, value: Rc<Cell<u32>>| {
            value.set(7);
            Ok(value)
        })
        .unwrap();
    })
    .await;
    app.get("/sync-macros")
        .await
        .unwrap()
        .assert_ok()
        .assert_exact_json(&serde_json::json!({"file":true,"vite":true,"number":7}));
}
#[tokio::test]
async fn compiled_module_callbacks_keep_non_send_values_local_and_only_cache_inclusion_paths() {
    use openrust::filesystem::RequiredFile;
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/macros/synchronous-values.json")).unwrap();
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    let first = app.root().join("compiled.rs");
    let once = app.root().join("once.rs");
    std::fs::write(&first, "编译模块标记").unwrap();
    std::fs::write(&once, "编译模块标记").unwrap();
    App::set_facade_application(Some(app.app()), async {
        for path in [&first, &once] {
            File::register_module(path, |_, value: Rc<Cell<u32>>| {
                value.set(9);
                Ok(value)
            })
            .unwrap();
        }
        let value = Rc::new(Cell::new(1));
        let result = File::get_require::<Rc<Cell<u32>>, _>(&first, value.clone()).unwrap();
        assert_eq!(Rc::ptr_eq(&value, &result), expected[2]["module_same"]);
        assert_eq!(value.get(), expected[2]["number"]);
        let value = Rc::new(Cell::new(1));
        let RequiredFile::Value(result) =
            File::require_once::<Rc<Cell<u32>>, _>(&once, value.clone()).unwrap()
        else {
            panic!("首次加载必须执行模块")
        };
        assert_eq!(Rc::ptr_eq(&value, &result), expected[3]["once_same"]);
        assert_eq!(value.get(), expected[3]["number"]);
        let unused = Rc::new(Cell::new(1));
        assert_eq!(
            matches!(
                File::require_once::<Rc<Cell<u32>>, _>(&once, unused.clone()).unwrap(),
                RequiredFile::AlreadyIncluded
            ),
            expected[3]["duplicate"]
        );
        assert_eq!(expected[3]["duplicate_type"], "bool");
        assert_eq!(unused.get(), 1);
        let filesystem = Filesystem::new();
        filesystem
            .register_module(&first, |_, value: Rc<Cell<u32>>| Ok(value))
            .unwrap();
        let value = Rc::new(Cell::new(1));
        assert!(Rc::ptr_eq(
            &value,
            &filesystem
                .get_require::<Rc<Cell<u32>>, _>(&first, value.clone())
                .unwrap()
        ));
    })
    .await;
}
