use openrust::{
    console::{AsOutput, ConsoleError, Output, view::TaskResult},
    foundation::{Application, configuration::Routing},
    http::{HttpError, Request, Response},
    support::{
        BackedEnum,
        facades::{Artisan, Route},
    },
};
use serde_json::{Value, json};
use std::sync::Arc;

#[test]
fn fixed_source_cases_numeric_json_and_invalid_backings() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../.agents/project/audits/artisan-tutorial/task-result-oracle.json"
    ))
    .unwrap();
    let native = TaskResult::cases();
    assert_eq!(native.len(), 3);
    for (i, case) in cases.iter().take(3).enumerate() {
        let value = case["value"].as_i64().unwrap();
        assert_eq!(native[i].value(), value);
        assert_eq!(format!("{:?}", native[i]), case["name"].as_str().unwrap());
        assert_eq!(TaskResult::try_from(value).unwrap(), native[i]);
        assert_eq!(TaskResult::try_from_backing(value), Some(native[i]));
        assert_eq!(i64::from(native[i]), value);
        assert_eq!(serde_json::to_string(&native[i]).unwrap(), case["json"]);
        assert_eq!(
            serde_json::from_value::<TaskResult>(json!(value)).unwrap(),
            native[i]
        );
    }
    for case in cases.iter().skip(3) {
        let value = case["invalid"].as_i64().unwrap();
        assert!(TaskResult::try_from(value).is_err());
        assert_eq!(TaskResult::try_from_backing(value), None);
        assert!(serde_json::from_value::<TaskResult>(json!(value)).is_err());
        assert_eq!(case["try_from"], Value::Null);
        assert_eq!(case["from_error"], true);
    }
    // JSON是整数背值；请求输入的弱类型转换另由既有BackedEnum消费者处理。
    for value in [
        json!("1"),
        json!(2.0),
        json!(true),
        Value::Null,
        json!({"value":1}),
        json!([]),
    ] {
        assert!(
            serde_json::from_value::<TaskResult>(value.clone()).is_err(),
            "{value}"
        );
    }
}
fn commands() -> Result<(), ConsoleError> {
    Artisan::command_with(
        "tasks:states",
        openrust::console::AsyncCommandCallback::new(|_, _, output| {
            Box::pin(async move {
                for value in TaskResult::cases() {
                    output.as_output().writeln(
                        serde_json::to_string(&value).unwrap().as_str(),
                        openrust::console::OutputOptions::default(),
                    )?;
                }
                Ok(0)
            })
        }),
    )?;
    Ok(())
}
fn state_response(
    request: &mut Request,
) -> openrust::http::HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async move {
        let state = request.r#enum::<TaskResult>("state", None).await?;
        Response::json(http::StatusCode::OK, json!({"state":state}))
    })
}
fn routes() -> Result<(), openrust::routing::RoutingError> {
    Route::get("/task-result", state_response)?;
    Route::get("/task-result/artisan", || async {
        let mut output = Output::default();
        Artisan::call("tasks:states", &[], &mut output)
            .await
            .map_err(|e| HttpError::Internal {
                source: Box::new(e),
            })?;
        Response::json(http::StatusCode::OK, json!({"output":output.stdout}))
    })?;
    Ok(())
}
#[test]
#[ignore = "由父消费者执行真实Artisan子进程"]
fn task_result_cli_driver() {
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().commands(commands))
        .create()
        .unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    let code = rt
        .block_on(app.handle_command(&["tasks:states".into(), "--no-ansi".into()]))
        .unwrap();
    drop(rt);
    drop(root);
    std::process::exit(code);
}
#[test]
fn real_artisan_uses_formal_task_result_type() {
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "task_result_cli_driver",
            "--ignored",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1\n2\n3\n"));
    assert!(result.stderr.is_empty());
}
#[tokio::test]
async fn real_http_reads_request_enum_and_calls_artisan() {
    use axum::handler::Handler as _;
    use openrust::contracts::http::Kernel as _;
    async fn transport(
        axum::extract::State(kernel): axum::extract::State<Arc<openrust::foundation::http::Kernel>>,
        request: axum::extract::Request,
    ) -> axum::response::Response {
        kernel.handle_transport(request).await
    }
    let root = tempfile::tempdir().unwrap();
    let app = Application::configure(root.path().into())
        .with_routing(Routing::default().web(routes).commands(commands))
        .create()
        .unwrap();
    let mut kernel = openrust::foundation::http::Kernel::new(app);
    kernel.bootstrap().await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            transport.with_state(Arc::new(kernel)).into_make_service(),
        )
        .await
        .unwrap()
    });
    for (query, value) in [
        ("1", json!(1)),
        ("2", json!(2)),
        ("3", json!(3)),
        ("4", Value::Null),
        ("", Value::Null),
        ("2.0", json!(2)),
    ] {
        let response = reqwest::get(format!("http://{address}/task-result?state={query}"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"state":value})
        );
    }
    let response = reqwest::get(format!("http://{address}/task-result/artisan"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.json::<Value>().await.unwrap(),
        json!({"output":"1\n2\n3\n"})
    );
    // 请求错误后仍可正常运行已装配命令。
    let response = reqwest::get(format!("http://{address}/task-result?state=invalid"))
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let response = reqwest::get(format!("http://{address}/task-result?state=3"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(response.json::<Value>().await.unwrap(), json!({"state":3}));
    server.abort();
    let _ = server.await;
}
