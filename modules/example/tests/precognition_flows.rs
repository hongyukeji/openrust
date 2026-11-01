//! 真实数据库中的文章创建预检；声明头与可信确认分别验收。

use super::{CaptureMailer, TestMigrator, auth_fixture_config};
use figment::{
    Figment,
    providers::{Format, Serialized, Toml},
};
use openrust::auth::AuthFlows;
use openrust::db::sea_orm::{ConnectionTrait, Statement};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use openrust::testing::{TestApp, TestDatabase, TestResponse};
use std::{sync::Arc, time::Duration};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn request(
    method: Method,
    path: &str,
    token: Option<&str>,
    body: &serde_json::Value,
    attempting: bool,
    only: Option<&str>,
) -> TestResult<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "example.test")
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if attempting {
        builder = builder.header("precognition", "true");
    }
    if let Some(only) = only {
        builder = builder.header("precognition-validate-only", only);
    }
    Ok(builder.body(Body::from(body.to_string()))?)
}

fn confirmed(response: &TestResponse, success: bool) {
    assert_eq!(
        response
            .headers()
            .get("precognition")
            .and_then(|v| v.to_str().ok()),
        Some("true")
    );
    assert!(
        response
            .headers()
            .get_all("vary")
            .iter()
            .any(|value| value.to_str().is_ok_and(|value| value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("precognition"))))
    );
    assert_eq!(
        response
            .headers()
            .get("precognition-success")
            .and_then(|v| v.to_str().ok()),
        success.then_some("true")
    );
}

async fn state(db: &openrust::Db) -> TestResult<(Vec<(i64, String, String)>, u64)> {
    let connection = db.primary_connection()?;
    let backend = db.backend()?;
    // 与既有 auth fixture 一致：模块实体使用固定物理表名，框架表才应用配置前缀。
    // 使用公开解析器校验并引用标识符，不访问模块私有 Entity。
    let names = openrust::db::DatabaseTableNames::new(
        db.table_names().prefix(),
        std::collections::BTreeMap::from([
            ("example_articles".into(), "example_articles".into()),
            (
                "example_article_events".into(),
                "example_article_events".into(),
            ),
        ]),
    )?;
    let articles = names.quote("example_articles", backend)?;
    let events = names.quote("example_article_events", backend)?;
    let mut rows = Vec::new();
    for row in connection
        .query_all_raw(Statement::from_string(
            backend,
            format!("SELECT id, title, body FROM {articles}"),
        ))
        .await?
    {
        rows.push((
            row.try_get::<i64>("", "id")?,
            row.try_get::<String>("", "title")?,
            row.try_get::<String>("", "body")?,
        ));
    }
    rows.sort();
    let count = connection
        .query_one_raw(Statement::from_string(
            backend,
            format!("SELECT COUNT(*) AS count FROM {events}"),
        ))
        .await?
        .ok_or("article event count missing")?
        .try_get::<i64>("", "count")?;
    Ok((rows, u64::try_from(count)?))
}

#[tokio::test]
async fn article_precognition_uses_real_database_without_constructing_or_writing_the_dto()
-> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping article precognition DB flow: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<TestMigrator>(&admin).await?;
    let result = async {
        let source = Figment::from(Serialized::defaults(
            openrust::config::FrameworkConfig::default(),
        ))
        .merge(Toml::string(&auth_fixture_config(database.url())))
        .merge(("app.env", "test"));
        let captured = CaptureMailer::default();
        let mail = captured.clone();
        let mut builder = TestApp::<super::bootstrap::Server>::builder(
            crate::modules::example::test_sources(&(source))?,
        );
        builder.configure_after_register(move |bindings| {
            let mail = mail.clone();
            Box::pin(async move {
                bindings
                    .instance(Arc::new(openrust::mail::Mailer::new(mail)))
                    .await?;
                Ok(())
            })
        });
        let mut app = builder.build().await?;
        let outcome = run(&app, &captured).await;
        let shutdown = app
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await;
        outcome?;
        shutdown?;
        TestResult::Ok(())
    }
    .await;
    let cleanup = database.close().await;
    result?;
    cleanup?;
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "同一隔离数据库内连续证明预检无副作用与普通 CRUD 的真实副作用"
)]
async fn run(app: &TestApp<super::bootstrap::Server>, captured: &CaptureMailer) -> TestResult {
    let auth = app.app()?.state().resolve::<AuthFlows>().await?;
    let email = "article-precognition@example.test";
    let password = "article precognition test password";
    let code = auth
        .register(email, password, "article-precognition")
        .await?
        .ok_or("verification code missing")?;
    assert!(
        auth.verify_email(email, code.code(), "article-precognition")
            .await?
    );
    let token = auth.login(email, password, "article-precognition").await?;
    let token = token.access_token();
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let baseline = state(&db).await?;
    let mail_count = captured
        .messages
        .lock()
        .map_err(|_| "mail capture poisoned")?
        .len();
    for (payload, only, status, success) in [
        (
            serde_json::json!({"title":"有效标题"}),
            Some("title"),
            StatusCode::NO_CONTENT,
            true,
        ),
        (
            serde_json::json!({"title":"x"}),
            Some("title"),
            StatusCode::UNPROCESSABLE_ENTITY,
            false,
        ),
        (
            serde_json::json!({"title":"有效标题"}),
            Some(""),
            StatusCode::NO_CONTENT,
            true,
        ),
        (
            serde_json::json!({"title":"有效标题"}),
            None,
            StatusCode::UNPROCESSABLE_ENTITY,
            false,
        ),
    ] {
        let response = app
            .request(request(
                Method::POST,
                "/api/example/articles",
                Some(token),
                &payload,
                true,
                only,
            )?)
            .await?;
        response.assert_status(status)?;
        confirmed(&response, success);
        if success {
            assert_eq!(response.body(), &[] as &[u8]);
        } else {
            let error: serde_json::Value = response.json()?;
            let field = if only.is_some() { "title" } else { "body" };
            assert!(error["errors"][field].is_array());
        }
        assert_eq!(state(&db).await?, baseline);
        assert_eq!(
            captured
                .messages
                .lock()
                .map_err(|_| "mail capture poisoned")?
                .len(),
            mail_count
        );
    }
    let refused = app
        .request(request(
            Method::POST,
            "/api/example/articles",
            None,
            &serde_json::json!({"title":"有效标题"}),
            true,
            Some("title"),
        )?)
        .await?;
    refused.assert_status(StatusCode::UNAUTHORIZED)?;
    confirmed(&refused, false);
    assert_eq!(state(&db).await?, baseline);

    let created = app
        .request(request(
            Method::POST,
            "/api/example/articles",
            Some(token),
            &serde_json::json!({"title":"普通创建", "body":"确实持久化"}),
            false,
            None,
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    assert!(!created.headers().contains_key("precognition"));
    assert!(!created.headers().contains_key("precognition-success"));
    let after = state(&db).await?;
    assert_eq!(after.0.len(), baseline.0.len() + 1);
    assert_eq!(after.1, baseline.1 + 1);
    let id = after
        .0
        .iter()
        .find(|row| row.1 == "普通创建")
        .ok_or("created article missing")?
        .0;
    let path = format!("/api/example/articles/{id}");
    // 更新未启用 middleware，只有声明头仍执行完整 DTO 提取与业务更新。
    let invalid = app
        .request(request(
            Method::PUT,
            &path,
            Some(token),
            &serde_json::json!({"title":"有效标题"}),
            true,
            Some("title"),
        )?)
        .await?;
    invalid.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
    let error: serde_json::Value = invalid.json()?;
    assert!(error["errors"]["body"].is_array());
    assert!(!invalid.headers().contains_key("precognition"));
    assert!(!invalid.headers().contains_key("precognition-success"));
    assert_eq!(state(&db).await?, after);
    let updated = app
        .request(request(
            Method::PUT,
            &path,
            Some(token),
            &serde_json::json!({"title":"声明头普通更新", "body":"仍执行业务"}),
            true,
            Some("title"),
        )?)
        .await?;
    updated.assert_status(StatusCode::OK)?;
    assert!(!updated.headers().contains_key("precognition"));
    assert!(!updated.headers().contains_key("precognition-success"));
    let final_state = state(&db).await?;
    assert_eq!(final_state.0.len(), after.0.len());
    assert_eq!(final_state.1, after.1 + 1);
    assert!(
        final_state
            .0
            .iter()
            .any(|row| row.0 == id && row.1 == "声明头普通更新" && row.2 == "仍执行业务")
    );
    Ok(())
}
