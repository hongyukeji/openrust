//! 示例类型表的真实 CRUD、所有者隔离与路由命名空间回归。
#![cfg(any(feature = "db-postgres", feature = "db-mysql"))]

use figment::{Figment, providers::Serialized};
use futures_util::FutureExt as _;
use openrust::auth::AuthFlows;
use openrust::database::sea_orm::ConnectionTrait;
use openrust::database::sea_orm_migration::{MigrationTrait, MigratorTrait};
use openrust::http::axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use openrust::support::{BoxFuture, CalendarContext, ServiceProvider};
use openrust::testing::{TestApp, TestDatabase};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

struct Migrator;

struct ModelDateFactoryProbe(Arc<AtomicUsize>);

impl ServiceProvider for ModelDateFactoryProbe {
    fn boot<'a>(
        &'a self,
        app: &'a mut openrust::foundation::BootContext<'_>,
    ) -> BoxFuture<'a, openrust::Result<()>> {
        Box::pin(async move {
            let calendar = app.extension_mut::<CalendarContext>()?;
            let calls = self.0.clone();
            *calendar = calendar
                .clone()
                .with_date_factory(move |operation, calendar| {
                    calls.fetch_add(1, Ordering::Relaxed);
                    operation.default_date(calendar)
                });
            Ok(())
        })
    }
}

#[openrust::database::sea_orm_migration::async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        let mut migrations = super::selected_test_auth::migrations();
        migrations.extend(super::database::migrations::Migrator::migrations());
        migrations
    }
}

fn request(
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<serde_json::Value>,
) -> TestResult<Request<Body>> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "example.test")
        .header("origin", "http://example.test");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    Ok(builder.body(Body::from(
        body.map_or_else(String::new, |value| value.to_string()),
    ))?)
}

// 检出JSON参数漏绑定、分页count丢筛选及同metadata跨账户泄漏。
async fn metadata_filter_contract(
    app: &TestApp<crate::bootstrap::Server>,
    owner_token: &str,
    other_token: &str,
    owner_id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    let created = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(other_token),
            Some(payload.clone()),
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    let created_json: serde_json::Value = serde_json::from_slice(created.body())?;
    let other_id = created_json["data"]["id"]
        .as_i64()
        .ok_or("other record id missing")?;
    assert_ne!(owner_id, other_id);
    let valid_limit = serde_json::to_string(&format!("中{}", "x".repeat(4093)))?;
    let invalid_limit = serde_json::to_string(&format!("中{}", "x".repeat(4094)))?;
    assert_eq!(valid_limit.chars().count(), 4096);
    assert_eq!(invalid_limit.chars().count(), 4097);
    for (token, condition, page, total, expected) in [
        (owner_token, r#"{"tags":["typed"]}"#, 1, 1, Some(owner_id)),
        (owner_token, "", 1, 1, Some(owner_id)),
        (owner_token, " \t ", 1, 1, Some(owner_id)),
        (owner_token, valid_limit.as_str(), 1, 0, None),
        (other_token, r#"{"tags":["typed"]}"#, 1, 1, Some(other_id)),
        (owner_token, r#"{"tags":["missing"]}"#, 1, 0, None),
        (owner_token, r#"{"tags":["typed"]}"#, 2, 1, None),
        (owner_token, r#"{"备注":"数据"}"#, 1, 1, Some(owner_id)),
        (owner_token, r#"{"tags":["' OR 1=1 --"]}"#, 1, 0, None),
    ] {
        let response = app
            .request(request(
                Method::GET,
                &metadata_uri(condition, page),
                Some(token),
                None,
            )?)
            .await?;
        response.assert_status(StatusCode::OK)?;
        let result: serde_json::Value = serde_json::from_slice(response.body())?;
        assert_eq!(result["meta"]["total"], total);
        let rows = result["data"]
            .as_array()
            .ok_or("metadata page data missing")?;
        match expected {
            Some(id) => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0]["id"], id);
            }
            None => assert_eq!(rows.as_slice(), [] as [serde_json::Value; 0]),
        }
    }
    for invalid in ["{", invalid_limit.as_str()] {
        let response = app
            .request(request(
                Method::GET,
                &metadata_uri(invalid, 1),
                Some(owner_token),
                None,
            )?)
            .await?;
        response.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
        let body: serde_json::Value = serde_json::from_slice(response.body())?;
        assert!(
            body["errors"]["metadata_contains"]
                .as_array()
                .is_some_and(|messages| !messages.is_empty())
        );
    }
    app.request(request(
        Method::DELETE,
        &format!("/api/example/records/{other_id}"),
        Some(other_token),
        None,
    )?)
    .await?
    .assert_status(StatusCode::OK)?;
    Ok(())
}

fn metadata_uri(condition: &str, page: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(condition.len() * 3);
    for byte in condition.bytes() {
        encoded.push('%');
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 15)]));
    }
    format!("/api/example/records?page={page}&per_page=1&metadata_contains={encoded}")
}

#[tokio::test]
#[allow(
    clippy::too_many_lines,
    reason = "同一真实 CRUD 流程的授权、字段类型、数据库状态与回收断言保持在一个生命周期内"
)]
async fn example_record_crud_uses_namespaced_routes_and_portable_field_types() -> TestResult {
    let Ok(admin) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("skipping example record DB flow: OPENRUST_TEST_DATABASE_URL is unset");
        return Ok(());
    };
    let database = TestDatabase::create::<Migrator>(&admin).await?;
    let key = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";
    let source = Figment::from(Serialized::defaults(openrust::config::FrameworkConfig::default()))
        .merge(Serialized::defaults(serde_json::json!({
            "app":{"env":"test","key":"0123456789abcdef0123456789abcdef"},
            "database":{"url":database.url()},
            "cache":{"default":"default","stores":{"default":{"driver":"memory"}}},
            "auth":{"jwt":{"issuer":"http://example.test","audience":"example-record-tests","active_key_id":"test","keys":{"test":key}}},
            "security":{"encryption":{"active_key_id":"test","keys":{"test":key}},"signed_links":{"issuer":"http://example.test","active_key_id":"test","keys":{"test":key}}}
        })));
    let mut builder = TestApp::<crate::bootstrap::Server>::builder(
        crate::modules::example::test_sources(&(source))?,
    );
    let date_calls = Arc::new(AtomicUsize::new(0));
    builder.configure_providers({
        let date_calls = date_calls.clone();
        move |providers| {
            providers.provider(ModelDateFactoryProbe(date_calls.clone()));
        }
    });
    builder.configure_after_register(|bindings| {
        Box::pin(async move {
            // 此验收只替换测试应用的派发器；生产模块的监听器不受影响。
            let mut providers = openrust::foundation::ProviderBuilder::new();
            providers.provider(record_save_test_events());
            let runtime = providers.start(Figment::new()).await?;
            let events = runtime
                .container()
                .resolve::<openrust::events::Events>()
                .await?;
            bindings.instance(events).await?;
            Ok(())
        })
    });
    let mut app = builder.build().await?;
    for (method, path) in [
        (Method::POST, "/api/auth/login"),
        (Method::GET, "/api/me"),
        (Method::POST, "/api/jobs/example-heartbeat"),
        (Method::GET, "/pages/about"),
    ] {
        let response = app.request(request(method, path, None, None)?).await?;
        response.assert_status(StatusCode::NOT_FOUND)?;
    }
    let anonymous = app
        .request(request(Method::GET, "/api/example/records", None, None)?)
        .await?;
    anonymous.assert_status(StatusCode::UNAUTHORIZED)?;

    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    connection
        .execute_unprepared("INSERT INTO example_record_categories (name) VALUES ('field-types')")
        .await?;
    let category_id = connection
        .query_one_raw(openrust::database::sea_orm::Statement::from_string(
            db.backend()?,
            "SELECT id FROM example_record_categories WHERE name = 'field-types'".to_owned(),
        ))
        .await?
        .ok_or("example category insert was not visible")?
        .try_get::<i64>("", "id")?;
    drop(connection);

    let auth = app.app()?.state().resolve::<AuthFlows>().await?;
    let owner_email = "example-record-owner@example.test";
    let password = "example record test password";
    let code = auth
        .register(owner_email, password, "example-record-flow")
        .await?
        .ok_or("verification code missing")?;
    assert!(
        auth.verify_email(owner_email, code.code(), "example-record-flow")
            .await?
    );
    let token = auth
        .login(owner_email, password, "example-record-flow")
        .await?;
    let token = token.access_token().to_owned();
    let payload = serde_json::json!({
        "name":"示例记录", "description":"字段类型完整演示", "priority":7, "view_count":42,
        "enabled":true, "published_at":"2026-10-29T12:00:00Z",
        "metadata":{"source":"modules/example","tags":["typed","crud"],"备注":"数据"},
        "category_id":category_id
    });
    let created = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(&token),
            Some(payload.clone()),
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    let data: serde_json::Value = serde_json::from_slice(created.body())?;
    let record = &data["data"];
    let id = record["id"].as_i64().ok_or("example record ID missing")?;
    assert_eq!(record["name"], "示例记录");
    assert_eq!(record["description"], "字段类型完整演示");
    assert_eq!(record["priority"], 7);
    assert_eq!(record["view_count"], 42);
    assert_eq!(record["enabled"], true);
    assert_eq!(record["published_at"], "2026-10-29T12:00:00Z");
    assert_eq!(record["metadata"], payload["metadata"]);
    assert_eq!(record["category_id"], category_id);

    let detail = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    detail.assert_status(StatusCode::OK)?;
    let page = app
        .request(request(
            Method::GET,
            "/api/example/records?page=1&per_page=10",
            Some(&token),
            None,
        )?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let page_json: serde_json::Value = serde_json::from_slice(page.body())?;
    assert_eq!(page_json["meta"]["total"], 1);

    let other_email = "example-record-other@example.test";
    let other_code = auth
        .register(other_email, password, "example-record-flow")
        .await?
        .ok_or("second verification code missing")?;
    assert!(
        auth.verify_email(other_email, other_code.code(), "example-record-flow")
            .await?
    );
    let other_token = auth
        .login(other_email, password, "example-record-flow")
        .await?
        .access_token()
        .to_owned();
    let foreign_page = app
        .request(request(
            Method::GET,
            "/api/example/records",
            Some(&other_token),
            None,
        )?)
        .await?;
    let foreign_json: serde_json::Value = serde_json::from_slice(foreign_page.body())?;
    assert_eq!(foreign_json["meta"]["total"], 0);
    let foreign_detail = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&other_token),
            None,
        )?)
        .await?;
    foreign_detail.assert_status(StatusCode::NOT_FOUND)?;
    assert_foreign_update_rejected(&app, &other_token, id, &payload).await?;
    assert_cancelled_save_does_not_write(&app, &db, &token, id, &payload).await?;
    assert_model_save_or_fail_rolls_back(&app, &db, id).await?;
    metadata_filter_contract(&app, &token, &other_token, id, &payload).await?;
    // 扩展断言失败时先完成原 CRUD 及测试库回收，再传播失败。
    let attribute_outcome = std::panic::AssertUnwindSafe(async {
        let calls_before = date_calls.load(Ordering::Relaxed);
        attribute_preview_contract(&app, &token, &other_token, id, &payload).await?;
        assert!(date_calls.load(Ordering::Relaxed) > calls_before);
        numeric_preview_contract(&app, &token, &payload).await?;
        assert_cached_date_save(&app, id).await?;
        assert_cached_collection_save(&app, id).await?;
        #[cfg(feature = "encryption")]
        {
            assert_encrypted_collection_save(&app, id).await?;
            assert_private_metadata(&app, &db, &token, &other_token, id).await?;
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .catch_unwind()
    .await;

    let update = serde_json::json!({
        "name":"更新记录", "description":"已更新", "priority":-3, "view_count":0,
        "enabled":false, "published_at":null, "metadata":null, "category_id":null
    });
    let updated = app
        .request(request(
            Method::PUT,
            &format!("/api/example/records/{id}"),
            Some(&token),
            Some(update),
        )?)
        .await?;
    updated.assert_status(StatusCode::OK)?;
    let updated_json: serde_json::Value = serde_json::from_slice(updated.body())?;
    assert_eq!(updated_json["data"]["priority"], -3);
    assert!(updated_json["data"]["metadata"].is_null());

    let deleted = app
        .request(request(
            Method::DELETE,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    deleted.assert_status(StatusCode::OK)?;
    let missing = app
        .request(request(
            Method::GET,
            &format!("/api/example/records/{id}"),
            Some(&token),
            None,
        )?)
        .await?;
    missing.assert_status(StatusCode::NOT_FOUND)?;

    drop(auth);
    drop(db);
    app.shutdown(tokio::time::Instant::now() + Duration::from_secs(10))
        .await?;
    database.close().await?;
    match attribute_outcome {
        Ok(result) => result,
        Err(panic_payload) => std::panic::resume_unwind(panic_payload),
    }
}

fn record_save_test_events() -> openrust::events::EventsServiceProvider {
    let mut provider = openrust::events::EventsServiceProvider::new();
    for stage in ["creating", "updating"] {
        provider.listen_named_reply(
            format!("eloquent.{stage}: modules::example::models::example_record::Model"),
            |payload, _| {
                Box::pin(async move {
                    use crate::modules::example::models::example_record::RecordForSave;
                    use openrust::support::DataValue;
                    let model = payload.downcast_ref::<RecordForSave>().ok_or_else(|| {
                        openrust::Error::custom(
                            "example_save_probe_payload",
                            "保存事件模型类型不符",
                        )
                    })?;
                    if model
                        .attributes
                        .raw_attribute("name")?
                        .as_ref()
                        .and_then(serde_json::Value::as_str)
                        == Some("监听器取消")
                    {
                        Ok(DataValue::Bool(false))
                    } else {
                        Ok(DataValue::Null)
                    }
                })
            },
        );
    }
    provider
}

async fn assert_model_save_or_fail_rolls_back(
    app: &TestApp<crate::bootstrap::Server>,
    db: &openrust::Db,
    id: i64,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::{
        database::{
            eloquent::{SaveContext, fresh, refresh, save_or_fail},
            sea_orm::EntityTrait,
        },
        events::{Events, EventsServiceProvider},
        support::CalendarContext,
    };

    let connection = db.primary_connection()?;
    let original = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("rollback fixture missing")?;
    let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
    let mut providers = openrust::foundation::ProviderBuilder::new();
    let mut provider = EventsServiceProvider::new();
    provider.listen_named_reply(
        "eloquent.saved: modules::example::models::example_record::Model",
        |payload, context| {
            Box::pin(async move {
                let model = payload
                    .downcast_ref::<example_record::RecordForSave>()
                    .ok_or_else(|| {
                        openrust::Error::custom("model_rollback_payload", "保存事件模型类型不符")
                    })?;
                let transaction = context.transaction().ok_or_else(|| {
                    openrust::Error::custom(
                        "model_rollback_transaction_missing",
                        "保存监听未收到实际事务",
                    )
                })?;
                let row = example_record::Entity::find_by_id(model.id()?)
                    .one(transaction)
                    .await
                    .map_err(openrust::Error::internal)?
                    .ok_or_else(|| {
                        openrust::Error::custom("model_rollback_row_missing", "事务内记录不存在")
                    })?;
                assert_eq!(row.name, "事务回滚");
                Err(openrust::Error::custom(
                    "model_rollback_probe",
                    "回归：保存事件失败",
                ))
            })
        },
    );
    providers.provider(provider);
    let runtime = providers.start(Figment::new()).await?;
    let events = runtime.container().resolve::<Events>().await?;
    let calendar = CalendarContext::default();
    let context = SaveContext {
        databases: &databases,
        events: Some(&events),
        calendar: &calendar,
        touch: true,
    };
    let mut model = example_record::RecordForSave::from_existing(&original)?;
    model
        .attributes
        .set_raw_attribute("name", serde_json::json!("事务回滚"))?;
    let error = save_or_fail(&mut model, &context)
        .await
        .err()
        .ok_or("save unexpectedly succeeded")?;
    assert_eq!(error.code(), "model_rollback_probe");
    assert_eq!(
        example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?,
        Some(original.clone())
    );
    assert!(model.attributes.is_dirty(&model.metadata, &["name"])?);
    let expected = Some(serde_json::Value::String(original.name));
    let persisted = fresh(&model, &context)
        .await?
        .ok_or("fresh rollback row missing")?;
    assert_eq!(persisted.id()?, id);
    assert_eq!(persisted.attributes.get_raw_original("name")?, expected);
    assert_eq!(
        model.attributes.raw_attribute("name")?,
        Some(serde_json::json!("事务回滚"))
    );
    refresh(&mut model, &context).await?;
    assert_eq!(model.attributes.raw_attribute("name")?, expected);
    assert_eq!(model.attributes.get_raw_original("name")?, expected);
    assert!(!model.attributes.is_dirty(&model.metadata, &["name"])?);
    Ok(())
}

async fn assert_cancelled_save_does_not_write(
    app: &TestApp<crate::bootstrap::Server>,
    db: &openrust::Db,
    token: &str,
    id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;

    let connection = db.primary_connection()?;
    let before = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?;
    assert!(before.is_some());
    let mut input = payload.clone();
    input["name"] = serde_json::json!("监听器取消");
    for (method, path) in [
        (Method::POST, "/api/example/records".to_owned()),
        (Method::PUT, format!("/api/example/records/{id}")),
    ] {
        let response = app
            .request(request(method, &path, Some(token), Some(input.clone()))?)
            .await?;
        response.assert_status(StatusCode::CONFLICT)?;
    }
    assert_eq!(
        example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?,
        before
    );
    let query_db = openrust::Db::default();
    query_db.initialize(connection)?;
    assert_eq!(
        query_db
            .table("example_records")?
            .where_eq("name", "监听器取消")?
            .count()
            .await?,
        0
    );
    Ok(())
}

async fn assert_foreign_update_rejected(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    let mut input = payload.clone();
    input["name"] = serde_json::json!("越权修改");
    let response = app
        .request(request(
            Method::PUT,
            &format!("/api/example/records/{id}"),
            Some(token),
            Some(input),
        )?)
        .await?;
    response.assert_status(StatusCode::NOT_FOUND)?;
    Ok(())
}

// 检出预览误写数据库、脱离owner授权或把相同JSON编码差别当作dirty。
async fn attribute_preview_contract(
    app: &TestApp<crate::bootstrap::Server>,
    owner_token: &str,
    other_token: &str,
    id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    let before = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("preview source row missing")?;
    let path = format!("/api/example/records/{id}/preview");
    preview_rejections(app, &path, owner_token, other_token, payload).await?;
    let mut unchanged = payload.clone();
    // JSON对象键顺序属于严格cast比较；使用数据库实际返回的顺序作为无变化输入。
    unchanged["metadata"] = before.metadata.clone().unwrap_or(serde_json::Value::Null);
    let clean = app
        .request(request(
            Method::POST,
            &path,
            Some(owner_token),
            Some(unchanged.clone()),
        )?)
        .await?;
    clean.assert_status(StatusCode::OK)?;
    let clean: serde_json::Value = serde_json::from_slice(clean.body())?;
    let editable = assert_preview_schema(&db, &clean["data"]).await?;
    let published = "2026-10-29T12:00:00.000000+00:00";
    assert_eq!(
        clean["data"]["original_transformed"]["published_at"],
        published
    );
    assert_eq!(
        clean["data"]["display"]["details"]["published_at"],
        published
    );
    assert_eq!(clean["data"]["display"]["details"]["is_published"], true);
    assert!(clean["data"]["proposed"].get("is_published").is_none());
    let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
    assert_model_guardable_columns(&databases).await?;
    assert_model_save_query_key(&db, id).await?;
    assert_eq!(clean["data"]["dirty"], serde_json::json!({}));
    assert_eq!(clean["data"]["changes"], serde_json::json!({}));
    assert_eq!(clean["data"]["previous"], serde_json::json!({}));
    assert_eq!(clean["data"]["has_changes"], false);
    assert_eq!(clean["data"]["persisted"], false);
    assert!(clean["data"].get("owner_record_ids").is_none());
    assert!(clean["data"].get("record_category_name").is_none());
    assert!(
        clean["data"]
            .get("owner_latest_enabled_record_id")
            .is_none()
    );
    assert_owner_record_relation(app, &path, owner_token, other_token, id, &unchanged).await?;
    let mut proposal = unchanged;
    proposal["name"] = serde_json::json!("仅预览");
    proposal["priority"] = serde_json::json!(8);
    proposal["metadata"]["tags"] = serde_json::json!(["changed"]);
    proposal["published_at"] = serde_json::Value::Null;
    proposal["category_id"] = serde_json::Value::Null;
    let changed = app
        .request(request(
            Method::POST,
            &path,
            Some(owner_token),
            Some(proposal),
        )?)
        .await?;
    changed.assert_status(StatusCode::OK)?;
    let changed: serde_json::Value = serde_json::from_slice(changed.body())?;
    let changed = &changed["data"];
    assert_eq!(changed["original"]["name"], payload["name"]);
    assert_eq!(changed["original_transformed"]["name"], payload["name"]);
    assert!(changed["original"]["metadata"].is_string());
    assert_eq!(
        changed["original_transformed"]["metadata"],
        before.metadata.clone().unwrap_or(serde_json::Value::Null)
    );
    assert!(changed["original_transformed"].get("owner_id").is_none());
    assert_eq!(changed["proposed"]["name"], "仅预览");
    assert_eq!(changed["display"]["details"]["is_published"], false);
    assert_preview_display(changed);
    assert_eq!(changed["dirty"]["priority"], 8);
    assert!(changed["dirty"].get("metadata").is_some());
    assert_eq!(changed["editable_fields"], editable);
    assert_eq!(changed["changes"], changed["dirty"]);
    assert!(before.published_at.is_some());
    assert!(before.category_id.is_some());
    for field in ["published_at", "category_id"] {
        assert_eq!(changed["dirty"].get(field), Some(&serde_json::Value::Null));
        assert_eq!(
            changed["changes"].get(field),
            Some(&serde_json::Value::Null)
        );
    }
    assert_eq!(
        changed["previous"]["published_at"],
        serde_json::to_value(before.published_at)?
    );
    assert_eq!(
        changed["previous"]["category_id"],
        serde_json::to_value(before.category_id)?
    );
    assert_eq!(changed["previous"]["priority"], payload["priority"]);
    assert_eq!(changed["persisted"], false);
    assert_eq!(changed["has_changes"], true);
    assert!(changed["original"].get("owner_id").is_none());
    assert_discarded_preview(app, &path, owner_token, payload).await?;
    let after = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("preview removed source row")?;
    assert_eq!(after, before);
    Ok(())
}

async fn assert_owner_record_relation(
    app: &TestApp<crate::bootstrap::Server>,
    path: &str,
    owner_token: &str,
    other_token: &str,
    id: i64,
    payload: &serde_json::Value,
) -> TestResult {
    let mut disabled = payload.clone();
    disabled["enabled"] = serde_json::Value::Bool(false);
    disabled["category_id"] = serde_json::Value::Null;
    let sibling = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(owner_token),
            Some(disabled),
        )?)
        .await?;
    sibling.assert_status(StatusCode::CREATED)?;
    let sibling: serde_json::Value = serde_json::from_slice(sibling.body())?;
    let sibling_id = sibling["data"]["id"]
        .as_i64()
        .ok_or("owner sibling ID missing")?;
    let other = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(other_token),
            Some(payload.clone()),
        )?)
        .await?;
    other.assert_status(StatusCode::CREATED)?;
    let other: serde_json::Value = serde_json::from_slice(other.body())?;
    let other_id = other["data"]["id"]
        .as_i64()
        .ok_or("foreign sibling ID missing")?;
    attach_preview_category(app, id, "owner").await?;
    attach_preview_category(app, other_id, "other").await?;
    let url = format!("{path}?include_owner_records=true");
    let preview = app
        .request(request(
            Method::POST,
            &url,
            Some(owner_token),
            Some(payload.clone()),
        )?)
        .await?;
    preview.assert_status(StatusCode::OK)?;
    let preview: serde_json::Value = serde_json::from_slice(preview.body())?;
    assert_eq!(
        preview["data"]["owner_record_ids"],
        serde_json::json!([id, sibling_id])
    );
    assert!(sibling_id > id);
    assert_eq!(preview["data"]["owner_latest_enabled_record_id"], id);
    assert_eq!(preview["data"]["record_category_name"], "field-types");
    assert_eq!(
        preview["data"]["record_category_links"],
        serde_json::json!([
            {"name":"field-types","label":"owner"}
        ])
    );
    assert_no_saved_category(app, sibling_id, owner_token, payload).await?;
    let denied = app
        .request(request(
            Method::POST,
            &url,
            Some(other_token),
            Some(payload.clone()),
        )?)
        .await?;
    denied.assert_status(StatusCode::NOT_FOUND)?;
    assert_owner_relation_uses_record_table(app, id, sibling_id).await?;
    for (token, id) in [(owner_token, sibling_id), (other_token, other_id)] {
        app.request(request(
            Method::DELETE,
            &format!("/api/example/records/{id}"),
            Some(token),
            None,
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
    }
    Ok(())
}

async fn assert_no_saved_category(
    app: &TestApp<crate::bootstrap::Server>,
    id: i64,
    token: &str,
    payload: &serde_json::Value,
) -> TestResult {
    assert!(!payload["category_id"].is_null());
    let response = app
        .request(request(
            Method::POST,
            &format!("/api/example/records/{id}/preview?include_owner_records=true"),
            Some(token),
            Some(payload.clone()),
        )?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let response: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(
        response["data"].get("record_category_name"),
        Some(&serde_json::Value::Null)
    );
    Ok(())
}

async fn assert_cached_date_save(app: &TestApp<crate::bootstrap::Server>, id: i64) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::{
        database::{
            eloquent::{
                AttributeMap, ModelAttribute, ModelDate, ModelDateContext, SaveContext,
                save_with_query,
            },
            sea_orm::{EntityTrait, TransactionTrait},
        },
        support::{CalendarContext, CalendarInstant, InputDate, ResolvedTimezone},
    };

    let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
    let db = databases.default_database();
    let connection = db.primary_connection()?;
    let record = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("cached date source record missing")?;
    let mut model = example_record::RecordForSave::from_existing(&record)?;
    model.metadata.date_format = Some("U.u".into());
    let calendar = CalendarContext::default();
    model.attributes = model.attributes.with_date_context(
        ModelDateContext::for_connection(calendar.clone(), &db)?
            .with_serializer(|date| date.format("Y-m-d H:i:s.u P")),
    );
    let original = record
        .published_at
        .ok_or("source publication time missing")?;
    let date = InputDate::new(
        CalendarInstant::new(original.timestamp(), 0)?,
        ResolvedTimezone::parse("UTC")?,
    );
    model.attributes.register_attribute(
        "published_at",
        ModelAttribute::new()
            .get(move |_, _| Ok(ModelDate::new(date.clone(), false).into()))
            .set(|value, _| {
                Ok(AttributeMap::from_iter([
                    ("published_at".into(), value),
                    ("view_count".into(), serde_json::json!(43).into()),
                ]))
            }),
    )?;
    let cached = model
        .attributes
        .get_attribute_value(&model.metadata, "published_at")?
        .ok_or("cached date missing")?;
    cached
        .as_date()
        .ok_or("expected date object")?
        .modify(|_| {
            Ok(InputDate::new(
                CalendarInstant::new(original.timestamp() + 3_600, 654_321)?,
                ResolvedTimezone::parse("Asia/Shanghai")?,
            ))
        })?;

    assert_eq!(
        model
            .attributes
            .project_attribute_json(&model.metadata, "published_at", cached.clone(),)?,
        serde_json::json!("2026-10-29 21:00:00.654321 +08:00"),
    );
    let context = SaveContext::new(&databases, None, &calendar);
    let cached_date = cached.as_date().ok_or("expected cached date")?.clone();
    let attempted = Arc::new(AtomicUsize::new(0));
    let query = db.table(&model.metadata.table)?.before_query({
        let attempted = attempted.clone();
        let date = cached_date.clone();
        move |_| {
            let conflict = date.modify(Ok).err().map(|error| error.code().to_owned());
            assert_eq!(conflict.as_deref(), Some("model_date_binding_pending"));
            attempted.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    });
    assert!(save_with_query(&mut model, &context, query).await?);
    assert_eq!(attempted.load(Ordering::Relaxed), 1);
    assert!(cached_date.modify(Ok)?.same_object(&cached_date));
    let changes = model.attributes.get_changes_values()?;
    assert!(changes["published_at"].as_date().is_some());
    assert_eq!(changes["view_count"].as_json()?, &serde_json::json!(43));
    let persisted = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("saved record missing")?;
    let published = persisted
        .published_at
        .ok_or("saved publication time missing")?;
    assert_eq!(published.timestamp(), original.timestamp() + 9 * 3_600);
    assert_eq!(published.timestamp_subsec_micros(), 0);
    assert_eq!(persisted.view_count, 43);

    let mut new = example_record::RecordForSave::new(record.owner_id);
    for (name, value) in [
        ("name", serde_json::json!("日期写入")),
        ("description", serde_json::json!("验证有类型的日期列")),
        ("priority", serde_json::json!(0)),
        ("view_count", serde_json::json!(0)),
        ("enabled", serde_json::json!(true)),
    ] {
        new.attributes.set_raw_attribute(name, value)?;
    }
    new.metadata.date_format = model.metadata.date_format.clone();
    new.attributes
        .set_raw_attribute_value("published_at", cached)?;
    let transaction = connection.begin().await?;
    let query = db
        .table(&new.metadata.table)?
        .using_transaction(&transaction);
    assert!(save_with_query(&mut new, &context, query).await?);
    let inserted = example_record::Entity::find_by_id(new.id()?)
        .one(&transaction)
        .await?
        .ok_or("inserted record missing")?;
    transaction.rollback().await?;
    assert_eq!(inserted.published_at, Some(published));
    Ok(())
}

async fn assert_cached_collection_save(
    app: &TestApp<crate::bootstrap::Server>,
    id: i64,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::{
        eloquent::{ArrayObject, AsArrayObject, AsCollection, SaveContext, save_with_query},
        sea_orm::{EntityTrait, TransactionTrait},
    };
    use openrust::support::Collection;
    use serde_json::json;

    let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
    let db = databases.default_database();
    let connection = db.primary_connection()?;
    let record = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("collection source record missing")?;
    let calendar = CalendarContext::default();
    let context = SaveContext::new(&databases, None, &calendar);
    let transaction = connection.begin().await?;
    let outcome: TestResult = async {
        let mut plain = example_record::RecordForSave::from_existing(&record)?;
        plain
            .attributes
            .set_attribute(&plain.metadata, "metadata", json!({"before":1}))
            .await?;
        plain
            .attributes
            .set_attribute(&plain.metadata, "metadata->nested->2", json!(true))
            .await?;
        let query = db
            .table(&plain.metadata.table)?
            .using_transaction(&transaction);
        if !save_with_query(&mut plain, &context, query).await? {
            return Err("普通 JSON 路径赋值的保存已取消".into());
        }
        let saved = example_record::Entity::find_by_id(id)
            .one(&transaction)
            .await?
            .ok_or("普通 JSON 路径赋值的记录缺失")?;
        assert_eq!(
            saved.metadata,
            Some(json!({"before":1,"nested":{"2":true}}))
        );
        for cast in [AsArrayObject::cast(), AsCollection::default().cast()] {
            let mut model = example_record::RecordForSave::from_existing(&record)?;
            model.metadata.casts.insert("metadata".into(), cast);
            model
                .attributes
                .set_attribute(&model.metadata, "metadata", json!({"name":"initial"}))
                .await?;
            let value = model
                .attributes
                .get_attribute_value(&model.metadata, "metadata")?
                .ok_or("collection attribute missing")?;
            let object = value.as_object().ok_or("collection object missing")?;
            if object.snapshot()?.as_any().is::<ArrayObject>() {
                object.modify(|value: &ArrayObject| {
                    let mut next = value.clone();
                    next.as_collection_mut()
                        .try_put("name", json!("array object"))?;
                    Ok(next)
                })?;
            } else {
                object.modify(|value: &Collection| {
                    let mut next = value.clone();
                    next.try_put("name", json!("collection"))?;
                    Ok(next)
                })?;
            }
            let expected = object
                .snapshot()?
                .json_projection()
                .ok_or("projection missing")?;
            let query = db
                .table(&model.metadata.table)?
                .using_transaction(&transaction);
            if !save_with_query(&mut model, &context, query).await? {
                return Err("collection save cancelled".into());
            }
            let saved = example_record::Entity::find_by_id(id)
                .one(&transaction)
                .await?
                .ok_or("collection row missing after save")?;
            if saved.metadata.as_ref() != Some(&expected) {
                return Err("collection writeback did not reach the database".into());
            }
        }
        Ok(())
    }
    .await;
    transaction.rollback().await?;
    outcome
}

#[cfg(feature = "encryption")]
async fn assert_private_metadata(
    app: &TestApp<crate::bootstrap::Server>,
    db: &openrust::Db,
    token: &str,
    other_token: &str,
    id: i64,
) -> TestResult {
    use crate::modules::example::models::example_record::record_query_db;
    use openrust::database::sea_orm::Value;
    use serde_json::json;

    let path = format!("/api/example/records/{id}/private-metadata");
    let input = json!({"label":"仅所有者可见","enabled":true});
    app.request(request(Method::GET, &path, None, None)?)
        .await?
        .assert_status(StatusCode::UNAUTHORIZED)?;
    app.request(request(Method::GET, &path, Some(token), None)?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    app.request(request(
        Method::PUT,
        &path,
        Some(other_token),
        Some(input.clone()),
    )?)
    .await?
    .assert_status(StatusCode::NOT_FOUND)?;
    app.request(request(
        Method::PUT,
        &path,
        Some(token),
        Some(input.clone()),
    )?)
    .await?
    .assert_status(StatusCode::NO_CONTENT)?;
    app.request(request(Method::GET, &path, Some(other_token), None)?)
        .await?
        .assert_status(StatusCode::NOT_FOUND)?;
    let response = app
        .request(request(Method::GET, &path, Some(token), None)?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    assert_eq!(
        response.headers()[http::header::CACHE_CONTROL],
        "private, no-store"
    );
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    assert_eq!(body["data"], input);

    let patch = json!({"path":"labels->2","value":"sparse"});
    app.request(request(
        Method::PATCH,
        &path,
        Some(other_token),
        Some(patch.clone()),
    )?)
    .await?
    .assert_status(StatusCode::NOT_FOUND)?;
    app.request(request(Method::PATCH, &path, Some(token), Some(patch))?)
        .await?
        .assert_status(StatusCode::NO_CONTENT)?;
    let input = json!({"label":"仅所有者可见","enabled":true,"labels":{"2":"sparse"}});
    let db = record_query_db(db)?;
    let query = db.table("example_records")?.where_eq("id", id)?;
    let raw = query
        .value::<String>("private_metadata")
        .await?
        .ok_or("缺少私有元数据密文")?;
    let encrypter = app
        .app()?
        .state()
        .resolve::<dyn openrust::encryption::ApplicationEncrypter>()
        .await?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encrypter.decrypt_string(&raw)?)?,
        input
    );
    query
        .update(vec![("private_metadata", Value::from("invalid payload"))])
        .await?;
    let response = app
        .request(request(Method::GET, &path, Some(token), None)?)
        .await?;
    response.assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    assert!(!String::from_utf8_lossy(response.body()).contains("invalid payload"));
    app.request(request(
        Method::PATCH,
        &path,
        Some(token),
        Some(json!({"path":"labels->0","value":"no overwrite"})),
    )?)
    .await?
    .assert_status(StatusCode::INTERNAL_SERVER_ERROR)?;
    assert_eq!(
        query.value::<String>("private_metadata").await?.as_deref(),
        Some("invalid payload")
    );
    Ok(())
}

#[cfg(feature = "encryption")]
async fn assert_encrypted_collection_save(
    app: &TestApp<crate::bootstrap::Server>,
    id: i64,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::{
        database::{
            eloquent::{
                ArrayObject, AsEncryptedArrayObject, AsEncryptedCollection, SaveContext,
                save_with_query,
            },
            sea_orm::{EntityTrait, TransactionTrait},
        },
        encryption::ApplicationEncrypter,
        support::Collection,
    };
    use serde_json::json;

    let state = app.app()?.state();
    let databases = state.resolve::<openrust::Databases>().await?;
    let encrypter = state.resolve::<dyn ApplicationEncrypter>().await?;
    let db = databases.default_database();
    let connection = db.primary_connection()?;
    let record = example_record::Entity::find_by_id(id)
        .one(&connection)
        .await?
        .ok_or("encrypted source record missing")?;
    let calendar = CalendarContext::default();
    let context = SaveContext::new(&databases, None, &calendar);
    let transaction = connection.begin().await?;
    let outcome: TestResult = async {
        for (cast, status, array_object) in [
            (
                AsEncryptedArrayObject::cast(encrypter.clone()),
                "array object",
                true,
            ),
            (
                AsEncryptedCollection::cast(encrypter.clone()),
                "collection",
                false,
            ),
        ] {
            let mut model = example_record::RecordForSave::from_existing(&record)?;
            model.metadata.casts.insert("description".into(), cast);
            model.attributes.set_raw_attribute(
                "description",
                json!(encrypter.encrypt_string(br#"{"status":"initial"}"#)?),
            )?;
            model.attributes.sync_original()?;
            let value = model
                .attributes
                .get_attribute_value(&model.metadata, "description")?
                .ok_or("encrypted collection missing")?;
            let object = value
                .as_object()
                .ok_or("encrypted collection object missing")?;
            if array_object {
                object.modify(|value: &ArrayObject| {
                    let mut next = value.clone();
                    next.as_collection_mut().try_put("status", json!(status))?;
                    Ok(next)
                })?;
            } else {
                object.modify(|value: &Collection| {
                    let mut next = value.clone();
                    next.try_put("status", json!(status))?;
                    Ok(next)
                })?;
            }
            model
                .attributes
                .set_attribute(&model.metadata, "description->path->applied", json!(true))
                .await?;
            let query = db
                .table(&model.metadata.table)?
                .using_transaction(&transaction);
            if !save_with_query(&mut model, &context, query).await? {
                return Err("encrypted collection save cancelled".into());
            }
            let saved = example_record::Entity::find_by_id(id)
                .one(&transaction)
                .await?
                .ok_or("encrypted collection row missing")?;
            let expected = serde_json::to_vec(&json!({"status":status,"path":{"applied":true}}))?;
            if encrypter.decrypt_string(&saved.description)? != expected {
                return Err("encrypted collection writeback did not reach the database".into());
            }
        }
        assert_native_encrypted_save(&record, &db, &transaction, &context, &encrypter).await?;
        Ok(())
    }
    .await;
    transaction.rollback().await?;
    outcome
}

#[cfg(feature = "encryption")]
async fn assert_native_encrypted_save(
    record: &crate::modules::example::models::example_record::Model,
    db: &openrust::Db,
    transaction: &openrust::database::sea_orm::DatabaseTransaction,
    context: &openrust::database::eloquent::SaveContext<'_>,
    encrypter: &std::sync::Arc<dyn openrust::encryption::ApplicationEncrypter>,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::{
        eloquent::{AttributeCast, EncryptedCast, EncryptedCastKind, save_with_query},
        sea_orm::EntityTrait,
    };
    use serde_json::json;

    let id = record.id;
    for (kind, value, expected) in [
        (EncryptedCastKind::String, json!(true), "1"),
        (
            EncryptedCastKind::Json,
            json!({"status":"native"}),
            r#"{"status":"native"}"#,
        ),
    ] {
        let mut model = example_record::RecordForSave::from_existing(record)?;
        model.metadata.casts.insert(
            "description".into(),
            AttributeCast::Encrypted(EncryptedCast::new(kind, encrypter.clone())),
        );
        model.attributes.set_raw_attribute(
            "description",
            json!(encrypter.encrypt_string(br#""previous""#)?),
        )?;
        model.attributes.sync_original()?;
        model
            .attributes
            .set_attribute(&model.metadata, "description", value)
            .await?;
        let query = db
            .table(&model.metadata.table)?
            .using_transaction(transaction);
        if !save_with_query(&mut model, context, query).await? {
            return Err("原生加密属性保存已取消".into());
        }
        let saved = example_record::Entity::find_by_id(id)
            .one(transaction)
            .await?
            .ok_or("原生加密属性的数据库行缺失")?;
        if encrypter.decrypt_string(&saved.description)? != expected.as_bytes() {
            return Err("原生加密属性的保存结果与预期明文不符".into());
        }
    }
    Ok(())
}

async fn attach_preview_category(
    app: &TestApp<crate::bootstrap::Server>,
    id: i64,
    label: &str,
) -> TestResult {
    use crate::modules::example::{
        models::example_record, services::record_preview::category_links,
    };
    use openrust::database::{eloquent::SaveContext, sea_orm::EntityTrait};
    use openrust::support::CalendarContext;

    let databases = app.app()?.state().resolve::<openrust::Databases>().await?;
    let db = databases.default_database();
    let record = example_record::Entity::find_by_id(id)
        .one(&db.primary_connection()?)
        .await?
        .ok_or("preview source record missing")?;
    let category_id = db
        .table("example_record_categories")?
        .where_eq("name", "field-types")?
        .value::<i64>("id")
        .await?
        .ok_or("preview category missing")?;
    let mut model = example_record::RecordForSave::from_existing(&record)?;
    let category = example_record::RecordCategory::new();
    let calendar = CalendarContext::default();
    let context = SaveContext::new(&databases, None, &calendar);
    assert_eq!(
        category_links(&category)
            .with_pivot_value("label", label.to_owned())?
            .attach(
                &mut model,
                &serde_json::json!(category_id),
                Vec::new(),
                &context,
                None,
            )
            .await?,
        1
    );
    assert_eq!(
        category_links(&category)
            .where_pivot_eq("label", "not-a-match")
            .detach(&mut model, None, &context, None)
            .await?,
        0
    );
    Ok(())
}

async fn assert_owner_relation_uses_record_table(
    app: &TestApp<crate::bootstrap::Server>,
    id: i64,
    sibling_id: i64,
) -> TestResult {
    use crate::modules::example::{
        models::example_record, services::record_preview::owner_relations,
    };
    use openrust::database::{DatabaseTableNames, eloquent::SaveContext, sea_orm::EntityTrait};
    use openrust::support::CalendarContext;

    let state = app.app()?.state();
    let databases = state.resolve::<openrust::Databases>().await?;
    let primary = databases.default_database().primary_connection()?;
    let record = example_record::Entity::find_by_id(id)
        .one(&primary)
        .await?
        .ok_or("relation source record missing")?;
    let db = openrust::Db::with_table_names(DatabaseTableNames::new(
        "different_",
        std::collections::BTreeMap::default(),
    )?);
    db.initialize(primary)?;
    assert_eq!(
        db.table_name("example_records")?,
        "different_example_records"
    );
    let calendar = CalendarContext::default();
    let context = SaveContext::new(&databases, None, &calendar);
    let relations = owner_relations(&record, &db, &context).await?;
    assert_eq!(relations.owner_record_ids, [id, sibling_id]);
    assert_eq!(relations.owner_latest_enabled_record_id, Some(id));
    assert_eq!(
        relations.record_category_name.as_deref(),
        Some("field-types")
    );
    Ok(())
}

fn assert_preview_display(changed: &serde_json::Value) {
    assert_eq!(changed["display"]["summary"]["name"], "仅预览");
    assert_eq!(changed["display"]["summary"]["priority"], 8);
    assert_eq!(
        changed["display"]["details"]["metadata"]["tags"],
        serde_json::json!(["changed"])
    );
    assert_eq!(changed["display"]["details"]["metadata_tag_count"], 1);
    assert!(changed["display"]["details"].get("name").is_none());
    assert!(changed["display"]["details"].get("owner_id").is_none());
}

async fn assert_model_save_query_key(db: &openrust::Db, id: i64) -> TestResult {
    use openrust::database::eloquent::ModelAttributes;
    use serde_json::{Map, json};

    let model = crate::modules::example::models::example_record::Model::attribute_metadata();
    let attributes = ModelAttributes::new(Map::from_iter([("id".into(), json!(id))]));
    attributes.set_raw_attribute("id", json!(0))?;
    let key = attributes
        .key_for_save_query(&model)?
        .as_i64()
        .ok_or("invalid example key")?;
    assert_eq!(key, id);
    let row = db
        .table(&model.table)?
        .where_eq(&model.key_name, key)?
        .first()
        .await?
        .ok_or("save predicate did not select the original record")?;
    assert_eq!(row.try_get::<i64>("", "id")?, id);
    Ok(())
}

async fn assert_model_guardable_columns(databases: &openrust::Databases) -> TestResult {
    use openrust::database::eloquent::{ModelAttributes, ModelMetadata};

    let attributes = ModelAttributes::default();
    let mut model = ModelMetadata::new("example_record_guard_probe_missing", "example::GuardProbe");
    model.guard(vec!["OWNER_ID".into(), "blocked\n".into()]);
    assert!(attributes.is_fillable(&model, databases, "missing").await?);

    model.table = "example_records".into();
    assert!(attributes.is_fillable(&model, databases, "name").await?);
    assert!(!attributes.is_fillable(&model, databases, "missing").await?);
    assert!(
        !attributes
            .is_fillable(&model, databases, "owner_id")
            .await?
    );
    assert!(attributes.is_guarded(&model, databases, "blocked").await?);
    model.connection = Some("guard-probe-unconfigured".into());
    assert!(attributes.is_fillable(&model, databases, "name").await?);

    model.class_name = "example::UncachedGuardProbe".into();
    assert_eq!(
        attributes
            .is_guarded(&model, databases, "name")
            .await
            .err()
            .map(|error| error.code().to_owned()),
        Some("database_instance_not_found".into()),
    );
    model.fillable(vec!["owner_id".into()]);
    assert!(
        attributes
            .is_fillable(&model, databases, "owner_id")
            .await?
    );
    model.fillable(Vec::new()).guard(Vec::new());
    assert!(!attributes.is_fillable(&model, databases, "a.b").await?);
    assert!(
        !attributes
            .is_fillable(&model, databases, "_private")
            .await?
    );
    Ok(())
}

async fn assert_preview_schema(
    db: &openrust::Db,
    clean: &serde_json::Value,
) -> TestResult<serde_json::Value> {
    let editable = serde_json::json!([
        "name",
        "description",
        "priority",
        "view_count",
        "enabled",
        "published_at",
        "metadata",
        "category_id"
    ]);
    assert_eq!(clean["editable_fields"], editable);
    assert_eq!(
        db.schema().get_column_listing("example_records").await?,
        [
            "id",
            "owner_id",
            "name",
            "description",
            "priority",
            "view_count",
            "enabled",
            "published_at",
            "metadata",
            "category_id",
            "private_metadata"
        ]
    );
    Ok(editable)
}

async fn assert_discarded_preview(
    app: &TestApp<crate::bootstrap::Server>,
    path: &str,
    token: &str,
    payload: &serde_json::Value,
) -> TestResult {
    let mut proposal = payload.clone();
    proposal["name"] = serde_json::json!("丢弃的名称");
    let response = app
        .request(request(
            Method::POST,
            &format!("{path}?discard=true"),
            Some(token),
            Some(proposal),
        )?)
        .await?;
    response.assert_status(StatusCode::OK)?;
    let body: serde_json::Value = serde_json::from_slice(response.body())?;
    let report = &body["data"];
    assert_eq!(report["proposed"], report["original"]);
    assert_eq!(report["proposed"]["name"], payload["name"]);
    assert_eq!(report["display"]["summary"]["name"], payload["name"]);
    for field in ["dirty", "changes", "previous"] {
        assert_eq!(report[field], serde_json::json!({}));
    }
    assert_eq!(report["has_changes"], false);
    assert_eq!(report["persisted"], false);
    Ok(())
}

// priority可成功反序列化为i32；仅依赖业务range规则拒绝，移除Validated会破坏断言。
async fn preview_rejections(
    app: &TestApp<crate::bootstrap::Server>,
    path: &str,
    owner: &str,
    other: &str,
    payload: &serde_json::Value,
) -> TestResult {
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other), StatusCode::NOT_FOUND),
    ] {
        app.request(request(Method::POST, path, token, Some(payload.clone()))?)
            .await?
            .assert_status(expected)?;
    }
    let mut invalid = payload.clone();
    invalid["priority"] = serde_json::json!(100_001);
    let rejected = app
        .request(request(Method::POST, path, Some(owner), Some(invalid))?)
        .await?;
    rejected.assert_status(StatusCode::UNPROCESSABLE_ENTITY)?;
    let error: serde_json::Value = serde_json::from_slice(rejected.body())?;
    assert!(
        error["errors"]["priority"]
            .as_array()
            .is_some_and(|messages| !messages.is_empty())
    );
    Ok(())
}

// 必须通过正常API真正落库，再用同一JWT/owner路径预览；不借内存fixture代替DB原值。
async fn numeric_preview_contract(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    payload: &serde_json::Value,
) -> TestResult {
    let before_count = owned_record_count(app, token).await?;
    let mut fixture = payload.clone();
    fixture["name"] = serde_json::json!("数值类型预览");
    fixture["metadata"] = serde_json::json!({"n":1.0});
    let created = app
        .request(request(
            Method::POST,
            "/api/example/records",
            Some(token),
            Some(fixture.clone()),
        )?)
        .await?;
    created.assert_status(StatusCode::CREATED)?;
    let created: serde_json::Value = serde_json::from_slice(created.body())?;
    let id = created["data"]["id"]
        .as_i64()
        .ok_or("numeric preview fixture ID missing")?;
    numeric_preview_cases(app, token, id, &fixture).await?;
    app.request(request(
        Method::DELETE,
        &format!("/api/example/records/{id}"),
        Some(token),
        None,
    )?)
    .await?
    .assert_status(StatusCode::OK)?;
    assert_eq!(owned_record_count(app, token).await?, before_count);
    Ok(())
}

async fn owned_record_count(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
) -> TestResult<u64> {
    let page = app
        .request(request(
            Method::GET,
            "/api/example/records?page=1&per_page=10",
            Some(token),
            None,
        )?)
        .await?;
    page.assert_status(StatusCode::OK)?;
    let page: serde_json::Value = serde_json::from_slice(page.body())?;
    page["meta"]["total"]
        .as_u64()
        .ok_or_else(|| "numeric preview row count missing".into())
}

async fn numeric_preview_cases(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    id: i64,
    fixture: &serde_json::Value,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let db = app.app()?.state().resolve::<openrust::Db>().await?;
    let connection = db.primary_connection()?;
    let record_path = format!("/api/example/records/{id}");
    for (seed, proposed, seed_float, proposed_float) in [
        (
            serde_json::json!({"n":1.0}),
            serde_json::json!({"n":1.0}),
            true,
            true,
        ),
        (
            serde_json::json!({"n":1}),
            serde_json::json!({"n":1.0}),
            false,
            true,
        ),
        (
            serde_json::json!({"n":1.0}),
            serde_json::json!({"n":1}),
            true,
            false,
        ),
    ] {
        reset_numeric_preview_seed(app, token, id, fixture, &connection).await?;
        let mut body = fixture.clone();
        body["metadata"] = seed;
        app.request(request(
            Method::PUT,
            &record_path,
            Some(token),
            Some(body.clone()),
        )?)
        .await?
        .assert_status(StatusCode::OK)?;
        let before = example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .ok_or("numeric preview DB row missing after update")?;
        let stored = before
            .metadata
            .as_ref()
            .ok_or("numeric preview DB metadata missing")?;
        assert_numeric_type(stored, seed_float, "actual database seed")?;
        body["metadata"] = proposed.clone();
        let preview = app
            .request(request(
                Method::POST,
                &format!("{record_path}/preview"),
                Some(token),
                Some(body),
            )?)
            .await?;
        preview.assert_status(StatusCode::OK)?;
        let preview: serde_json::Value = serde_json::from_slice(preview.body())?;
        assert_numeric_report(
            &preview["data"],
            stored,
            &proposed,
            seed_float,
            proposed_float,
        )?;
        let after = example_record::Entity::find_by_id(id)
            .one(&connection)
            .await?
            .ok_or("numeric preview removed DB row")?;
        assert_numeric_type(
            after
                .metadata
                .as_ref()
                .ok_or("numeric preview DB metadata removed")?,
            seed_float,
            "database after preview",
        )?;
        assert_eq!(
            serde_json::to_string(&after.metadata)?,
            serde_json::to_string(&before.metadata)?
        );
        assert_eq!(after, before);
    }
    Ok(())
}

fn assert_numeric_type(value: &serde_json::Value, floating: bool, stage: &str) -> TestResult {
    let number = value
        .get("n")
        .and_then(serde_json::Value::as_number)
        .ok_or("numeric preview n is not a number")?;
    assert_eq!(
        number.is_f64(),
        floating,
        "{stage}: database/driver JSON category differs"
    );
    assert_eq!(
        number.as_f64().map(f64::to_bits),
        Some(1.0_f64.to_bits()),
        "{stage}"
    );
    Ok(())
}

fn assert_numeric_report(
    report: &serde_json::Value,
    stored: &serde_json::Value,
    proposed: &serde_json::Value,
    stored_float: bool,
    proposed_float: bool,
) -> TestResult {
    let original_text = report["original"]["metadata"]
        .as_str()
        .ok_or("numeric preview original text missing")?;
    let proposed_text = report["proposed"]["metadata"]
        .as_str()
        .ok_or("numeric preview proposed text missing")?;
    assert_eq!(original_text, serde_json::to_string(stored)?);
    assert_eq!(proposed_text, serde_json::to_string(proposed)?);
    assert_numeric_type(
        &serde_json::from_str(original_text)?,
        stored_float,
        "report original",
    )?;
    assert_numeric_type(
        &serde_json::from_str(proposed_text)?,
        proposed_float,
        "report proposed",
    )?;
    let dirty = stored_float != proposed_float;
    assert_eq!(report["has_changes"], dirty);
    assert_eq!(report["persisted"], false);
    assert_eq!(report["changes"], report["dirty"]);
    if dirty {
        assert_eq!(
            report["dirty"].as_object().map(serde_json::Map::len),
            Some(1)
        );
        assert_eq!(report["dirty"]["metadata"].as_str(), Some(proposed_text));
        assert_eq!(report["previous"]["metadata"].as_str(), Some(original_text));
    } else {
        assert_eq!(report["dirty"], serde_json::json!({}));
        assert_eq!(report["previous"], serde_json::json!({}));
    }
    Ok(())
}

// MySQL数值等值JSON更新可保留旧类型；先真实改为不同值，再建立本轮seed。
async fn reset_numeric_preview_seed(
    app: &TestApp<crate::bootstrap::Server>,
    token: &str,
    id: i64,
    fixture: &serde_json::Value,
    connection: &openrust::database::DatabaseConnection,
) -> TestResult {
    use crate::modules::example::models::example_record;
    use openrust::database::sea_orm::EntityTrait;
    let mut body = fixture.clone();
    body["metadata"] = serde_json::json!({"n":2});
    app.request(request(
        Method::PUT,
        &format!("/api/example/records/{id}"),
        Some(token),
        Some(body),
    )?)
    .await?
    .assert_status(StatusCode::OK)?;
    let reset = example_record::Entity::find_by_id(id)
        .one(connection)
        .await?
        .ok_or("numeric preview DB row missing after reset")?;
    let number = reset
        .metadata
        .as_ref()
        .and_then(|value| value.get("n"))
        .and_then(serde_json::Value::as_number)
        .ok_or("numeric preview reset n missing")?;
    assert!(number.is_i64());
    assert_eq!(number.as_i64(), Some(2));
    Ok(())
}
