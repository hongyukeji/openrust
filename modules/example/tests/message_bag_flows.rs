//! 通过唯一业务模块的生产路由验证公开错误包接口，不依赖数据库或 Session。

use super::config_sources::enabled_example_sources;
use std::time::Duration;

use figment::{Figment, providers::Serialized};
use openrust::http::axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use openrust::testing::TestApp;
use serde_json::{Value, json};

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn sources() -> Figment {
    Figment::from(Serialized::defaults(
        openrust::config::FrameworkConfig::default(),
    ))
    .merge(Serialized::defaults(json!({
        "app": {"env": "test"},
        "cache": {"default": "default", "stores": {"default": {"driver": "memory"}}},
        "security": {"encryption": {
            "active_key_id": "test",
            "keys": {"test": "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}
        }}
    })))
}

async fn post(app: &TestApp<crate::bootstrap::Server>, input: Value) -> TestResult<Value> {
    let response = app
        .request(
            Request::post("/api/example/message-bag")
                .header("host", "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&input)?))?,
        )
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: Value = response.json()?;
    assert_eq!(body["success"], true);
    assert_eq!(body["code"], "ok");
    Ok(body["data"].clone())
}

#[tokio::test]
async fn module_message_bag_aggregates_create_and_update_errors() -> TestResult {
    let mut app =
        TestApp::<crate::bootstrap::Server>::builder(enabled_example_sources(&(sources()))?)
            .build()
            .await?;
    let result = async {
        let errors = post(&app, json!({"title": "", "email": "bad", "record_id": 0})).await?;
        assert_eq!(errors["default"]["keys"], json!(["title", "email"]));
        assert_eq!(
            errors["default"]["messages"],
            json!({
                "title": ["标题不能为空", "标题不能为空"],
                "email": ["邮箱格式无效"]
            })
        );
        assert_eq!(errors["default"]["first_title"], "标题不能为空");
        assert_eq!(errors["default"]["first_missing"], "");
        assert_eq!(
            errors["default"]["all"],
            json!(["标题不能为空", "标题不能为空", "邮箱格式无效"])
        );
        assert_eq!(
            errors["default"]["unique"],
            json!({
                "0": "标题不能为空", "2": "邮箱格式无效"
            })
        );
        assert_eq!(
            errors["default"]["formatted"],
            json!([
                "title: 标题不能为空",
                "title: 标题不能为空",
                "email: 邮箱格式无效"
            ])
        );
        assert_eq!(errors["default"]["format_zero"], errors["default"]["all"]);
        assert_eq!(errors["default"]["has_title"], true);
        assert_eq!(errors["default"]["has_empty_selection"], true);
        let encoded: Value = serde_json::from_str(
            errors["default"]["json"]
                .as_str()
                .ok_or("message JSON missing")?,
        )?;
        assert_eq!(encoded, errors["default"]["messages"]);

        assert_eq!(errors["update"]["keys"], json!(["record_id"]));
        assert_eq!(errors["update"]["first_record_id"], "记录编号必须为正数");
        assert_eq!(errors["update"]["all"], json!(["记录编号必须为正数"]));
        assert_eq!(
            errors["missing_bag"],
            json!({"stored": false, "first_title": ""})
        );

        let update_only = post(
            &app,
            json!({
                "title": "文章", "email": "writer@example.test", "record_id": 0
            }),
        )
        .await?;
        assert_eq!(update_only["default"]["keys"], json!([]));
        assert_eq!(update_only["default"]["first_title"], "");
        assert_eq!(update_only["default"]["has_title"], false);
        assert_eq!(update_only["default"]["has_empty_selection"], false);
        assert_eq!(update_only["update"]["all"], json!(["记录编号必须为正数"]));

        let create_only = post(
            &app,
            json!({
                "title": "", "email": "writer@example.test", "record_id": 7
            }),
        )
        .await?;
        assert_eq!(create_only["default"]["keys"], json!(["title"]));
        assert_eq!(create_only["update"]["keys"], json!([]));
        assert_eq!(create_only["update"]["first_record_id"], "");
        Ok(())
    }
    .await;
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await?;
    result
}
