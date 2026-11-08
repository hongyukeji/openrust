//! 正式 Provider/HTTP 路径的原生数据库 Session；与显式 Guarded 精确截止分开计证。
#![cfg(feature = "session")]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[cfg(feature = "db-sqlite")]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use figment::{Figment, providers::Serialized};
use openrust::Error;
use openrust::Result;
use openrust::config::FrameworkConfig;
use openrust::config::HttpStatusMode;
use openrust::config::NoConfig;
use openrust::database::DatabaseServiceProvider;
use openrust::foundation::ApplicationDefinition;
use openrust::foundation::BootContext;
use openrust::foundation::RegisterContext;
use openrust::http::CookieServiceProvider;
use openrust::http::HttpServiceProvider;
use openrust::http::Inject;
use openrust::http::Json;
#[cfg(feature = "db-sqlite")]
use openrust::http::axum::http::{StatusCode, header::SET_COOKIE};
use openrust::http::axum::{
    body::Body,
    extract::ConnectInfo,
    http::{
        Request,
        header::{COOKIE, USER_AGENT},
    },
};
use openrust::routing::RouteGroup;
use openrust::session::Session;
use openrust::session::SessionServiceProvider;
use openrust::session::SessionStoreGuarantees;
use openrust::session::SessionStoreHandle;
use openrust::support::{BoxFuture, DefaultProviders, ServiceProvider};
use openrust::testing::TestApp;
#[cfg(feature = "db-sqlite")]
use openrust::testing::TestDatabase;
use openrust::testing::TestResponse;
#[cfg(feature = "db-sqlite")]
use sea_orm::{ConnectionTrait, QueryResult, Statement};
#[cfg(feature = "db-sqlite")]
use sea_orm_migration::{MigrationTrait, MigratorTrait};
use serde_json::{Value, json};

const NAME: &str = "native_database_contract";
const CUSTOM_TABLE: &str = "request_sessions";
const TIMEOUT: Duration = Duration::from_secs(10);
const RAW_ID: &str = "Ab0123456789Ab0123456789Ab0123456789Ab01";

#[cfg(feature = "db-sqlite")]
struct NativeMigrator;
#[cfg(feature = "db-sqlite")]
impl MigratorTrait for NativeMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(openrust::session::SessionMigration),
            Box::new(openrust::session::SessionMigration::for_table(CUSTOM_TABLE)),
        ]
    }
}
#[cfg(feature = "db-sqlite")]
struct GuardedMigrator;
#[cfg(feature = "db-sqlite")]
impl MigratorTrait for GuardedMigrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(openrust::session::SessionMigration),
            Box::new(openrust::session::GuardedSessionMigration),
            Box::new(openrust::session::GuardedSessionGcMigration),
        ]
    }
}

#[derive(Default)]
struct BusinessCalls(AtomicUsize);
struct Router<const AUTH: bool>;
impl<const AUTH: bool> ServiceProvider for Router<AUTH> {
    fn register<'a>(&'a self, app: &'a mut RegisterContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { app.instance(Arc::new(BusinessCalls::default())).await })
    }
    fn boot<'a>(&'a self, app: &'a mut BootContext<'_>) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            #[cfg(feature = "auth")]
            if AUTH {
                app.routes(|routes| {
                    routes.group(
                        RouteGroup::new()
                            .middleware_alias("cookies")
                            .middleware_alias("session"),
                        |routes| {
                            routes
                                .get("/native/write", write_authenticated)
                                .get("/native/read", read)
                        },
                    )
                })?;
                return Ok(());
            }
            app.routes(|routes| {
                routes.group(
                    RouteGroup::new()
                        .middleware_alias("cookies")
                        .middleware_alias("session"),
                    |routes| routes.get("/native/write", write).get("/native/read", read),
                )
            })?;
            Ok(())
        })
    }
}
async fn write(session: Session, Inject(calls): Inject<BusinessCalls>) -> Result<Json<Value>> {
    calls.0.fetch_add(1, Ordering::SeqCst);
    session.put_typed("message", "persisted")?;
    // 同样的Session属性在无AuthService绑定时不能被伪造投影为SQL user_id。
    session.put_typed("openrust.auth.user_id", 42_i64)?;
    session.save().await?;
    Ok(Json(
        json!({"id":session.id()?.as_str(), "message":session.get_json("message")?}),
    ))
}
#[cfg(feature = "auth")]
async fn write_authenticated(
    session: Session,
    calls: Inject<BusinessCalls>,
    Inject(manager): Inject<openrust::auth::AuthManager>,
) -> Result<Json<Value>> {
    let guard = manager.guard(None).await?;
    let stateful = guard.as_stateful().ok_or(Error::Unauthorized)?;
    // 单次请求认证真实检索 Provider；不轮换 Session，保留原生驱动 raw ID 契约。
    let user = stateful
        .once_using_id(&json!(42))
        .await?
        .ok_or(Error::Unauthorized)?;
    if user.auth_identifier()? != json!(42) || guard.id().await? != Some(json!(42)) {
        return Err(Error::Unauthorized);
    }
    let mut response = write(session, calls).await?;
    response.0["guard_id"] = guard.id().await?.ok_or(Error::Unauthorized)?;
    Ok(response)
}

async fn read(session: Session) -> Result<Json<Value>> {
    Ok(Json(
        json!({"id":session.id()?.as_str(), "message":session.get_json("message")?}),
    ))
}

struct Harness<const AUTO: bool, const AUTH: bool>;
impl<const AUTO: bool, const AUTH: bool> ApplicationDefinition for Harness<AUTO, AUTH> {
    type Config = NoConfig;
    fn default_providers() -> DefaultProviders {
        if AUTO {
            DefaultProviders::auto().replace(
                openrust::foundation::ProviderId::of::<CookieServiceProvider>(),
                Box::new(CookieServiceProvider::plain()),
            )
        } else {
            DefaultProviders::empty()
        }
    }
    fn providers(_: &NoConfig) -> Vec<Box<dyn ServiceProvider>> {
        let mut providers: Vec<Box<dyn ServiceProvider>> = if AUTO {
            Vec::new()
        } else {
            vec![
                Box::new(HttpServiceProvider),
                Box::new(DatabaseServiceProvider),
                Box::new(CookieServiceProvider::plain()),
                Box::new(SessionServiceProvider::new()),
            ]
        };
        #[cfg(feature = "auth")]
        if AUTH {
            providers.push(Box::new(
                openrust::auth::AuthServiceProvider::with_authenticator(RejectLegacyAuthenticator),
            ));
        }
        providers.push(Box::new(Router::<AUTH>));
        providers
    }
}

#[cfg(feature = "auth")]
struct RejectLegacyAuthenticator;
#[cfg(feature = "auth")]
impl openrust::auth::Authenticator for RejectLegacyAuthenticator {
    fn authenticate<'a>(
        &'a self,
        _: openrust::auth::AuthCredential<'a>,
    ) -> BoxFuture<'a, Result<openrust::auth::AuthPrincipal>> {
        // 原生数据库 user_id 必须来自具名 Guard，不能借用旧 Authenticator 的常量身份。
        Box::pin(async { Err(Error::Unauthorized) })
    }
}

fn sources(default: Option<&str>, selected: Option<&str>, named: bool, driver: &str) -> Figment {
    let mut defaults = FrameworkConfig::default();
    defaults.api.http_status = HttpStatusMode::Semantic;
    let mut input = json!({
        "app":{"env":"test"},
        "database":{"url":default},
        "cache":{"default":"contract-memory","stores":{"contract-memory":{"driver":"memory"}}},
        "session":{"driver":driver,"connection":null,"table":"sessions","cookie":NAME,"secure":false,"lifetime":7,"lottery":[0,100]},
        "security":{"encryption":{"active_key_id":"test","keys":{"test":"KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="}}}
    });
    if named {
        input["database"]["instances"] = json!({"selected":{"url":selected}});
        input["session"]["connection"] = json!("selected");
        input["session"]["table"] = json!(CUSTOM_TABLE);
    }
    Figment::from(Serialized::defaults(defaults)).merge(Serialized::defaults(input))
}

async fn request<const AUTO: bool, const AUTH: bool>(
    app: &TestApp<Harness<AUTO, AUTH>>,
    path: &str,
    agent: &[u8],
) -> Result<TestResponse> {
    let mut request = Request::get(path)
        .header("host", "localhost")
        .header(COOKIE, format!("{NAME}={RAW_ID}"))
        .header(
            USER_AGENT,
            openrust::http::axum::http::HeaderValue::from_bytes(agent).map_err(Error::internal)?,
        )
        .body(Body::empty())
        .map_err(Error::internal)?;
    request.extensions_mut().insert(ConnectInfo(
        "203.0.113.9:54321"
            .parse::<SocketAddr>()
            .map_err(Error::internal)?,
    ));
    app.request_with_limits(request, 16 * 1024, TIMEOUT).await
}

#[cfg(feature = "db-sqlite")]
async fn stored(fixture: &TestDatabase, table: &str) -> Result<Option<QueryResult>> {
    let connection = fixture.connection()?;
    // table仅为本测试固定字面值；ID必须真实绑定，不能对raw Cookie ID做摘要替代。
    connection.query_one_raw(Statement::from_sql_and_values(connection.get_database_backend(),
        format!("SELECT id,user_id,ip_address,user_agent,payload,last_activity FROM {table} WHERE id=?"), [RAW_ID.into()],
    )).await.map_err(Error::internal)
}
#[cfg(feature = "db-sqlite")]
fn required(row: Option<QueryResult>) -> Result<QueryResult> {
    row.ok_or_else(|| {
        Error::custom(
            "native_http_row_missing",
            "正式Session HTTP未持久化原始Cookie ID",
        )
    })
}
#[cfg(feature = "db-sqlite")]
fn data(response: &TestResponse) -> Result<Value> {
    Ok(response.json::<Value>()?["data"].clone())
}

#[cfg(feature = "db-sqlite")]
async fn native_sources<const AUTH: bool>(
    default: &TestDatabase,
    selected: &TestDatabase,
    named: bool,
) -> Result<Figment> {
    let sources = sources(Some(default.url()), Some(selected.url()), named, "database");
    #[cfg(feature = "auth")]
    let sources = if AUTH {
        let connection = default.connection()?;
        connection
            .execute_unprepared(
                "CREATE TABLE users (id INTEGER PRIMARY KEY, password TEXT, remember_token TEXT)",
            )
            .await
            .map_err(Error::internal)?;
        connection
            .execute_unprepared("INSERT INTO users (id) VALUES (42)")
            .await
            .map_err(Error::internal)?;
        sources.merge(Serialized::defaults(
            json!({"auth":{"providers":{"users":{"driver":"database","table":"users"}}}}),
        ))
    } else {
        sources
    };
    Ok(sources)
}

#[cfg(feature = "db-sqlite")]
async fn native_case<const AUTH: bool>(
    default: &TestDatabase,
    selected: &TestDatabase,
    named: bool,
) -> Result<()> {
    let sources = native_sources::<AUTH>(default, selected, named).await?;
    let mut app = TestApp::<Harness<false, AUTH>>::builder(sources)
        .build()
        .await?;
    let outcome: Result<()> = async {
        let store = app.app()?.state().resolve::<SessionStoreHandle>().await?;
        assert_eq!(store.guarantees(), SessionStoreGuarantees::NativeDatabase);
        assert_eq!(store.store().activity_lifetime_seconds(), Some(7 * 60));
        let table = if named { CUSTOM_TABLE } else { "sessions" };
        let target = if named { selected } else { default };
        let other = if named { default } else { selected };
        let before = time::OffsetDateTime::now_utc().unix_timestamp();
        let agent = "汉🙂".repeat(300);
        let response = request(&app, "/native/write", agent.as_bytes()).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(data(&response)?["id"], RAW_ID);
        if AUTH {
            assert_eq!(
                data(&response)?["guard_id"],
                42,
                "必须由请求默认 Guard 投影真实 Provider 账户"
            );
        }
        let cookie = response
            .headers()
            .get_all(SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with(&format!("{NAME}=")))
            .ok_or_else(|| {
                Error::custom("native_http_cookie_missing", "正式Session没有出站Cookie")
            })?;
        assert!(cookie.starts_with(&format!("{NAME}={RAW_ID};")));
        assert!(cookie.contains("Max-Age=420"));
        let row = required(stored(target, table).await?)?;
        assert_eq!(
            row.try_get::<String>("", "id").map_err(Error::internal)?,
            RAW_ID
        );
        assert_eq!(
            row.try_get::<Option<String>>("", "ip_address")
                .map_err(Error::internal)?,
            Some("203.0.113.9".into())
        );
        assert_eq!(
            row.try_get::<String>("", "user_agent")
                .map_err(Error::internal)?,
            "汉🙂".repeat(250)
        );
        let expected_user = if AUTH { Some(42_i64) } else { None };
        assert_eq!(
            row.try_get::<Option<i64>>("", "user_id")
                .map_err(Error::internal)?,
            expected_user
        );
        let activity: i64 = row.try_get("", "last_activity").map_err(Error::internal)?;
        assert!(activity >= before && activity <= time::OffsetDateTime::now_utc().unix_timestamp());
        let payload: String = row.try_get("", "payload").map_err(Error::internal)?;
        let decoded = STANDARD.decode(payload).map_err(Error::internal)?;
        let attributes: Value = serde_json::from_slice(&decoded).map_err(Error::internal)?;
        assert_eq!(attributes["message"], "persisted");
        assert!(
            attributes.get("attributes").is_none()
                && attributes.get("id_hash").is_none()
                && attributes.get("version").is_none(),
            "原生payload必须是属性，不是旧增强envelope"
        );
        assert!(stored(other, table).await?.is_none());
        // 合法U+FFFD必须保留；非法FF则按固定PHP默认mbstring转换为问号。
        let response = request(&app, "/native/read", "valid-�".as_bytes()).await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(data(&response)?["message"], "persisted");
        assert_eq!(
            required(stored(target, table).await?)?
                .try_get::<String>("", "user_agent")
                .map_err(Error::internal)?,
            "valid-�"
        );
        let response = request(&app, "/native/read", b"bad-\xff").await?;
        response.assert_status(StatusCode::OK)?;
        assert_eq!(data(&response)?["id"], RAW_ID);
        assert_eq!(
            required(stored(target, table).await?)?
                .try_get::<String>("", "user_agent")
                .map_err(Error::internal)?,
            "bad-?"
        );
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn native_provider_http_selects_named_and_default_connection_with_request_metadata()
-> Result<()> {
    for named in [false, true] {
        let default = TestDatabase::sqlite_file::<NativeMigrator>().await?;
        let selected = match TestDatabase::sqlite_file::<NativeMigrator>().await {
            Ok(database) => database,
            Err(error) => {
                let _ = default.close().await;
                return Err(error);
            }
        };
        let outcome = native_case::<false>(&default, &selected, named).await;
        let selected_cleanup = selected.close().await;
        let default_cleanup = default.close().await;
        outcome?;
        selected_cleanup?;
        default_cleanup?;
    }
    Ok(())
}

#[cfg(feature = "db-sqlite")]
#[cfg(feature = "auth")]
#[tokio::test]
async fn native_user_id_projection_requires_real_auth_service_binding() -> Result<()> {
    let default = TestDatabase::sqlite_file::<NativeMigrator>().await?;
    let selected = match TestDatabase::sqlite_file::<NativeMigrator>().await {
        Ok(database) => database,
        Err(error) => {
            let _ = default.close().await;
            return Err(error);
        }
    };
    let outcome = native_case::<true>(&default, &selected, true).await;
    let selected_cleanup = selected.close().await;
    let default_cleanup = default.close().await;
    outcome?;
    selected_cleanup?;
    default_cleanup
}

#[tokio::test]
async fn auto_native_database_without_url_is_not_silently_omitted_or_changed_to_array() -> Result<()>
{
    let mut app = TestApp::<Harness<true, false>>::builder(sources(None, None, false, "database"))
        .build()
        .await?;
    let outcome: Result<()> = async {
        let store = app.app()?.state().resolve::<SessionStoreHandle>().await?;
        assert_eq!(store.guarantees(), SessionStoreGuarantees::NativeDatabase);
        let response = request(&app, "/native/write", b"no-database").await?;
        assert!(
            response.status().is_server_error(),
            "缺默认DB时Session I/O必须明确失败，不能省略或回退array：status={:?}",
            response.status()
        );
        assert_eq!(
            app.app()?
                .state()
                .resolve::<BusinessCalls>()
                .await?
                .0
                .load(Ordering::SeqCst),
            0
        );
        Ok(())
    }
    .await;
    let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
    outcome?;
    shutdown
}

#[cfg(feature = "db-sqlite")]
#[tokio::test]
async fn explicit_guarded_provider_finalizes_separately_from_native_guarantees() -> Result<()> {
    let database = TestDatabase::sqlite_file::<GuardedMigrator>().await?;
    let outcome: Result<()> = async {
        let mut app = TestApp::<Harness<false, false>>::builder(sources(
            Some(database.url()),
            None,
            false,
            "database-guarded",
        ))
        .build()
        .await?;
        let checks: Result<()> = async {
            let store = app.app()?.state().resolve::<SessionStoreHandle>().await?;
            assert_eq!(store.guarantees(), SessionStoreGuarantees::ServerSide);
            let response = request(&app, "/native/write", b"guarded").await?;
            response.assert_status(StatusCode::OK)?;
            assert_eq!(data(&response)?["id"], RAW_ID);
            let connection = database.connection()?;
            let row = connection
                .query_one_raw(Statement::from_string(
                    connection.get_database_backend(),
                    "SELECT id,payload,expires_at FROM sessions",
                ))
                .await
                .map_err(Error::internal)?
                .ok_or_else(|| {
                    Error::custom("guarded_http_row_missing", "显式增强Session没有持久化")
                })?;
            assert_ne!(
                row.try_get::<String>("", "id").map_err(Error::internal)?,
                RAW_ID
            );
            let payload: String = row.try_get("", "payload").map_err(Error::internal)?;
            assert!(
                serde_json::from_str::<Value>(&payload)
                    .map_err(Error::internal)?
                    .get("id_hash")
                    .is_some()
            );
            Ok(())
        }
        .await;
        let shutdown = app.shutdown(tokio::time::Instant::now() + TIMEOUT).await;
        checks?;
        shutdown
    }
    .await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}
