//! 经真实示例路由和数据库家族撤销的SSE集成fixture，不创建替代应用或控制器。

use http_body_util::BodyExt as _;
use openrust::Db;
use openrust::Error;
use openrust::Result;
use openrust::foundation::ApplicationDefinition;
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use openrust::testing::TestApp;
use serde_json::json;
use std::time::Duration;

pub(crate) async fn exercise<A: ApplicationDefinition>(
    fixture: &TestApp<A>,
    user_id: i64,
    token: &str,
    refresh: &str,
) -> Result<()> {
    let db = fixture.app()?.state().resolve::<Db>().await?;
    db.table("example_articles")?
        .insert(vec![
            ("id", 7001_i64.into()),
            ("owner_id", user_id.into()),
            ("title", "managed realtime".into()),
            ("body", "owned fixture".into()),
        ])
        .await?;
    let response = fixture
        .raw_response(
            Request::builder()
                .uri("/api/example/articles/7001/events")
                .header(header::HOST, "localhost")
                .header(header::ORIGIN, "http://localhost")
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .map_err(Error::internal)?,
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .map_err(Error::internal)?
            .transpose()
            .map_err(Error::internal)?
            .is_some()
    );
    let logout = fixture
        .raw_response(
            Request::builder()
                .method("POST")
                .uri("/api/example/auth/logout")
                .extension(ConnectInfo(std::net::SocketAddr::from((
                    [127, 0, 0, 1],
                    42001,
                ))))
                .header(header::HOST, "localhost")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"refresh_token":refresh}).to_string()))
                .map_err(Error::internal)?,
        )
        .await?;
    assert_eq!(logout.status(), StatusCode::OK);
    let realtime = fixture
        .app()?
        .state()
        .resolve::<openrust::realtime::Realtime>()
        .await?;
    realtime
        .send_to_user(
            user_id,
            "example.article.7001",
            &json!({"event":"after-revocation"}),
            Duration::from_secs(30),
        )
        .await?;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), body.frame())
            .await
            .map_err(Error::internal)?
            .transpose()
            .map_err(Error::internal)?
            .is_none()
    );
    Ok(())
}
