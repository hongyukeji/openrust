use openrust::{
    filesystem::{ConditionValue, ConditionalResult, FileError, Filesystem},
    support::facades::{App, Facade, File},
    testing::TestApp,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
async fn fixture() -> TestApp {
    let mut app = TestApp::create(|b| b).unwrap();
    app.boot().await.unwrap();
    app
}

#[tokio::test]
async fn conditions_preserve_input_truthiness_null_fallback_and_scalar_results() {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let original = File::get_facade_root().unwrap();
        let skipped = File::when("0", |_, _| -> Result<Option<u32>, FileError> {
            panic!("字符串0不能执行回调")
        })
        .unwrap()
        .into_target()
        .unwrap();
        assert!(Arc::ptr_eq(&skipped, &original));
        assert_eq!(
            File::unless(false, |files, value| {
                assert!(!value);
                assert!(std::ptr::eq(files, original.as_ref()));
                Ok(Some(0))
            })
            .unwrap()
            .into_value(),
            Some(0)
        );
        assert!(Arc::ptr_eq(
            &File::when(true, |_, _| Ok::<Option<u8>, FileError>(None))
                .unwrap()
                .into_target()
                .unwrap(),
            &original
        ));
        assert_eq!(
            File::when_else(
                false,
                |_, _| panic!("不能执行真分支"),
                |_, value| {
                    assert!(!value);
                    Ok(Some(false))
                }
            )
            .unwrap()
            .into_value(),
            Some(false)
        );
        assert_eq!(
            File::unless_else(
                true,
                |_, _| panic!("不能执行假分支"),
                |_, value| {
                    assert!(value);
                    Ok(Some("fallback"))
                }
            )
            .unwrap()
            .into_value(),
            Some("fallback")
        );
        assert_eq!(
            File::when("00", |_, value| Ok(Some(value)))
                .unwrap()
                .into_value(),
            Some("00")
        );
        assert!(
            File::when(Vec::<u8>::new(), |_, _| Ok(Some(true)))
                .unwrap()
                .into_target()
                .is_some()
        );
        assert!(
            File::when(serde_json::Value::Null, |_, _| Ok(Some(true)))
                .unwrap()
                .into_target()
                .is_some()
        );
        assert_eq!(
            File::when(serde_json::json!({}), |_, value| Ok(Some(value)))
                .unwrap()
                .into_value(),
            Some(serde_json::json!({}))
        );
        assert!(
            File::when(Some(0_i64), |_, _| Ok(Some(true)))
                .unwrap()
                .into_target()
                .is_some()
        );
        assert_eq!(
            File::when(-2_i64, |_, value| Ok(Some(value)))
                .unwrap()
                .into_value(),
            Some(-2)
        );
    })
    .await;
}

struct Input {
    enabled: bool,
    owned: Box<str>,
}
impl ConditionValue for Input {
    fn is_truthy(&self) -> bool {
        self.enabled
    }
}
#[tokio::test]
async fn condition_resolvers_evaluate_once_and_move_original_non_clone_value_to_selected_callback()
{
    let app = fixture().await;
    let tests = AtomicUsize::new(0);
    App::set_facade_application(Some(app.app()), async {
        let result = File::when_test(
            |files| {
                tests.fetch_add(1, Ordering::SeqCst);
                assert!(!files.has_macro("missing"));
                Input {
                    enabled: true,
                    owned: "original".into(),
                }
            },
            |_, input| Ok(Some(input.owned)),
        )
        .unwrap();
        assert_eq!(result.into_value().unwrap().as_ref(), "original");
        assert_eq!(tests.load(Ordering::SeqCst), 1);
        assert_eq!(
            File::unless_test(|_| "0", |_, value| Ok(Some(value)))
                .unwrap()
                .into_value(),
            Some("0")
        );
        assert!(matches!(
            File::when(true, |_, _| Err::<Option<()>, _>(
                FileError::UnknownAlgorithm("selected-error".into())
            )),
            Err(FileError::UnknownAlgorithm(_))
        ));
    })
    .await;
}

#[tokio::test]
async fn proxies_capture_conditions_skip_side_effects_and_preserve_real_operation_results_and_errors()
 {
    let app = fixture().await;
    App::set_facade_application(Some(app.app()), async {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("condition.txt");
        let skipped = File::when_capture()
            .unwrap()
            .capture(|files| files.exists(&path))
            .call(|files| files.put(&path, "wrong"))
            .unwrap();
        assert!(skipped.into_target().is_some());
        assert!(!path.exists());
        let written = File::unless_capture()
            .unwrap()
            .capture(|files| files.exists(&path))
            .call(|files| files.put(&path, "right"))
            .unwrap();
        assert_eq!(written.into_value(), Some(5));
        assert_eq!(File::get_text(&path).unwrap(), "right");
        assert_eq!(
            File::when_condition("yes")
                .unwrap()
                .call(|_| Ok::<Option<usize>, FileError>(None))
                .unwrap()
                .into_value(),
            Some(None)
        );
        assert!(
            File::unless_condition("0")
                .unwrap()
                .call(|files| files.get(dir.path().join("absent")))
                .is_err()
        );
        assert!(
            File::when_condition(false)
                .unwrap()
                .call(|files| files.get(dir.path().join("absent")))
                .unwrap()
                .into_target()
                .is_some()
        );
        assert!(matches!(
            File::when_capture().unwrap().call(|_| Ok(1u32)),
            Err(FileError::MissingCondition)
        ));
        assert!(
            File::when_capture()
                .unwrap()
                .try_capture(|files| files.is_empty_directory(dir.path().join("missing")))
                .is_err()
        );
        let proxy = File::when_capture().unwrap().condition("0");
        assert!(
            proxy
                .call(|_| -> Result<(), FileError> { panic!("显式假条件不能执行") })
                .unwrap()
                .into_target()
                .is_some()
        );
    })
    .await;
}

#[tokio::test]
async fn captured_service_remains_original_across_application_scopes_and_direct_service_borrows() {
    let first = fixture().await;
    let second = fixture().await;
    let proxy = App::set_facade_application(Some(first.app()), async {
        File::r#macro("name", |_, (): ()| Ok("first")).unwrap();
        File::when_condition(true).unwrap()
    })
    .await;
    App::set_facade_application(Some(second.app()), async {
        File::r#macro("name", |_, (): ()| Ok("second")).unwrap();
        assert_eq!(
            proxy
                .call(|files| files.macro_call::<&'static str, _>("name", ()))
                .unwrap()
                .into_value(),
            Some("first")
        );
        assert_eq!(
            File::when(true, |files, _| Ok(Some(
                files.macro_call::<&'static str, _>("name", ())?
            )))
            .unwrap()
            .into_value(),
            Some("second")
        );
    })
    .await;
    let filesystem = Filesystem::new();
    match filesystem
        .unless(0, |_, _| Ok::<Option<()>, FileError>(None))
        .unwrap()
    {
        ConditionalResult::Target(target) => assert!(std::ptr::eq(target, &filesystem)),
        _ => panic!("空结果必须返回原服务"),
    }
    assert_eq!(
        filesystem
            .when_condition(1)
            .call(|_| Ok(9))
            .unwrap()
            .into_value(),
        Some(9)
    );
}

fn conditional_http(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        let path = App::base_path("conditional.txt")?;
        let result = File::unless_capture()?
            .capture(|files| files.exists(&path))
            .call(|files| files.put(&path, "created"))?;
        openrust::http::Response::json(
            openrust::http::StatusCode::OK,
            serde_json::json!({"written":result.into_value()}),
        )
    })
}
fn conditional_failure(
    _: &mut openrust::http::Request,
) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async {
        File::when_condition(true)?.call(|files| files.get(App::base_path("absent.txt")?))?;
        unreachable!()
    })
}
struct ConditionCommand(openrust::console::CommandDefinition);
impl openrust::console::Command for ConditionCommand {
    fn definition(&self) -> &openrust::console::CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        _: &'a openrust::foundation::Application,
        _: &'a openrust::console::Input,
        output: &'a mut openrust::console::Output,
    ) -> openrust::console::ConsoleFuture<'a, Result<i32, openrust::console::ConsoleError>> {
        Box::pin(async move {
            let value = (|| {
                let path = App::base_path("conditional.txt")?;
                File::when_test(
                    |files| files.exists(&path),
                    |files, _| files.get_text(&path).map(Some),
                )
            })()
            .map_err(|e: FileError| openrust::console::ConsoleError::new(e.to_string()))?;
            output
                .stdout
                .push_str(&value.into_value().expect("HTTP生成的文件必须可读"));
            Ok(0)
        })
    }
}
#[tokio::test]
async fn actual_http_and_artisan_execute_or_skip_file_operations_and_report_selected_failures() {
    use openrust::{foundation::configuration::Routing, support::facades::Route};
    let mut app = TestApp::create(|b| {
        b.with_commands(|artisan| {
            artisan.add_command(Box::new(ConditionCommand(
                openrust::console::CommandDefinition::new(clap::Command::new("file:conditional")),
            )))
        })
        .with_routing(Routing::default().web(|| {
            Route::get("/conditional", conditional_http)?;
            Route::get("/conditional-error", conditional_failure)
        }))
    })
    .unwrap();
    app.get("/conditional")
        .await
        .unwrap()
        .assert_ok()
        .assert_json_path("written", &serde_json::json!(7));
    app.get("/conditional")
        .await
        .unwrap()
        .assert_ok()
        .assert_json_path("written", &serde_json::Value::Null);
    app.artisan(["file:conditional"])
        .await
        .assert_successful()
        .expects_output("created");
    app.call(
        http::Request::get("/conditional-error")
            .header("Accept", "application/json")
            .body(openrust::http::Body::empty())
            .unwrap(),
    )
    .await
    .unwrap()
    .assert_status(openrust::http::StatusCode::INTERNAL_SERVER_ERROR);
}
