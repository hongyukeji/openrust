//! 客户端 once 生命周期与真实 Query 读取回归代码；集中验收前不代表已执行。

use super::*;
use crate::ClientFactory;
use openrust::container::{Container, ContainerBuilder};
use serde_json::{Map, json};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;

fn container() -> Container {
    let mut bindings = ContainerBuilder::new();
    bindings.scoped::<ClientLookupCache, _>(|_| {
        Box::pin(async { Ok(Arc::new(ClientLookupCache::default())) })
    });
    bindings.build()
}
fn record() -> Result<ClientRecord> {
    ClientFactory::default().new_from_builder(Map::from_iter([
        ("id".into(), json!("019a4d4b-f21a-7191-bce9-fdc817a165aa")),
        ("revoked".into(), json!(false)),
    ]))
}

struct ScalarFactory(Arc<openrust::database::eloquent::ModelMetadata>);
impl ClientModelFactory for ScalarFactory {
    fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
        &self.0
    }
    fn is_valid_identifier(&self, value: &Value) -> bool {
        value.is_string() || value.is_i64()
    }
    fn new_from_builder(&self, attributes: Map<String, Value>) -> Result<ClientRecord> {
        Ok(Arc::new(crate::Client::from_attributes(
            Arc::clone(&self.0),
            attributes,
        )))
    }
}

#[test]
fn postgres_uuid_binding_is_a_factory_strategy_not_a_string_shape_guess() -> Result<()> {
    use openrust::database::sea_orm::DbBackend;
    let raw = "019a4d4b-f21a-7191-bce9-fdc817a165aa";
    let id = json!(raw);
    let default = ClientFactory::default();
    assert!(
        matches!(default.identifier_binding(&id, DbBackend::Postgres)?, SqlValue::Uuid(Some(value)) if value.to_string() == raw)
    );
    assert_eq!(
        default.identifier_binding(&id, DbBackend::MySql)?,
        SqlValue::from(raw)
    );
    assert_eq!(
        default.identifier_binding(&id, DbBackend::Sqlite)?,
        SqlValue::from(raw)
    );
    let custom = ScalarFactory(Arc::new(openrust::database::eloquent::ModelMetadata::new(
        "text_apps",
        "TextClient",
    )));
    assert_eq!(
        custom.identifier_binding(&id, DbBackend::Postgres)?,
        SqlValue::from(raw)
    );
    assert_eq!(
        custom.identifier_binding(&json!(9_007_199_254_740_993i64), DbBackend::Postgres)?,
        SqlValue::from(9_007_199_254_740_993i64)
    );
    Ok(())
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
struct NetworkMigrator;
#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
impl openrust::database::sea_orm_migration::MigratorTrait for NetworkMigrator {
    fn migrations() -> Vec<Box<dyn openrust::database::sea_orm_migration::MigrationTrait>> {
        Vec::new()
    }
}

#[cfg(any(feature = "db-mysql", feature = "db-postgres"))]
#[tokio::test]
async fn real_default_uuid_custom_text_and_integer_models_use_their_selected_bindings() -> Result<()>
{
    use openrust::{
        database::sea_orm::{ConnectionTrait, DbBackend},
        testing::TestDatabase,
    };
    let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") else {
        eprintln!("未执行Passport网络仓储回归：需真实MySQL或PostgreSQL测试URL");
        return Ok(());
    };
    let database = TestDatabase::create::<NetworkMigrator>(&url).await?;
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        let backend = db.backend()?;
        let uuid_type = match backend {
            DbBackend::Postgres => "UUID", DbBackend::MySql => "CHAR(36)",
            _ => return Err(Error::custom("test_backend_invalid", "本回归需MySQL或PostgreSQL")),
        };
        let default = Arc::new(ClientFactory::default());
        let mut text = openrust::database::eloquent::ModelMetadata::new("text_apps", "TextClient");
        text.incrementing = false;
        text.key_type = "string".into();
        let integer = openrust::database::eloquent::ModelMetadata::new("integer_apps", "IntegerClient");
        let cases: Vec<(Arc<dyn ClientModelFactory>, Value, &str)> = vec![
            (default, json!("019a4d4b-f21a-7191-bce9-fdc817a165aa"), uuid_type),
            (Arc::new(ScalarFactory(Arc::new(text))), json!("019a4d4b-f21a-7191-bce9-fdc817a165aa"), "VARCHAR(80)"),
            (Arc::new(ScalarFactory(Arc::new(integer))), json!(9_007_199_254_740_993i64), "BIGINT"),
        ];
        let container = container();
        let scope = container.scope()?;
        for (factory, id, column_type) in cases {
            let table = &factory.metadata().table;
            db.primary_connection()?.execute_unprepared(&format!("CREATE TABLE {} (id {column_type} PRIMARY KEY, name VARCHAR(80), secret VARCHAR(255), revoked BOOLEAN, redirect_uris TEXT, grant_types TEXT)", db.quoted_table_name(table)?)).await.map_err(Error::internal)?;
            db.table(table)?.insert(vec![
                ("id",factory.identifier_binding(&id,backend)?), ("name","selected".into()),
                ("revoked",false.into()), ("redirect_uris","[]".into()), ("grant_types","[]".into()),
            ]).await?;
            let repository = ClientRepository { db: db.clone(), factory, identity: Arc::new(RepositoryIdentity) };
            let loaded = repository.find(&crate::ClientLookupContext::from(&scope),&id).await?.ok_or_else(|| Error::custom("test_client_missing", "绑定类型未找到客户端"))?;
            assert_eq!(loaded.model_key()?,id);
            assert_eq!(loaded.name()?.as_deref(),Some("selected"));
        }
        scope.close();
        Ok(())
    }.await;
    let cleanup = database.close().await;
    outcome?;
    cleanup
}

#[test]
fn typed_identifiers_preserve_integer_precision_and_reject_php_unrepresentable_inputs() -> Result<()>
{
    assert_ne!(
        Identifier::from_value(&json!(1))?,
        Identifier::from_value(&json!("1"))?
    );
    assert_eq!(
        Identifier::from_value(&json!(i64::MAX))?.binding(),
        SqlValue::from(i64::MAX)
    );
    assert_eq!(
        Identifier::from_value(&json!("0001"))?.binding(),
        SqlValue::from("0001")
    );
    for invalid in [
        Value::Null,
        json!(true),
        json!(1.5),
        json!(u64::MAX),
        json!([]),
        json!({}),
    ] {
        assert!(Identifier::from_value(&invalid).is_err());
    }
    Ok(())
}

#[tokio::test]
async fn some_none_typed_keys_and_repository_identity_are_scoped() -> Result<()> {
    let container = container();
    let scope = container.scope()?;
    let cache = scope.resolve::<ClientLookupCache>().await?;
    let repository = Arc::new(RepositoryIdentity);
    let other = Arc::new(RepositoryIdentity);
    let calls = AtomicUsize::new(0);
    let loaded = record()?;
    for _ in 0..2 {
        let returned = cache
            .find(&repository, Identifier::String("1".into()), || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(Some(Arc::clone(&loaded)))
            })
            .await?
            .ok_or_else(|| Error::custom("test_client_missing", "缓存未返回模型"))?;
        assert!(Arc::ptr_eq(&returned, &loaded));
    }
    for (identity, key) in [
        (&repository, Identifier::Integer(1)),
        (&other, Identifier::String("1".into())),
    ] {
        for _ in 0..2 {
            assert!(
                cache
                    .find(identity, key.clone(), || async {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(None)
                    })
                    .await?
                    .is_none()
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let next = container.scope()?;
    let next_cache = next.resolve::<ClientLookupCache>().await?;
    let _ = next_cache
        .find(&repository, Identifier::String("1".into()), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        })
        .await?;
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    scope.close();
    assert!(scope.resolve::<ClientLookupCache>().await.is_err());
    next.close();
    Ok(())
}

#[tokio::test]
async fn errors_are_not_negative_cached_and_identity_stays_owned_until_scope_releases() -> Result<()>
{
    let cache = ClientLookupCache::default();
    let identity = Arc::new(RepositoryIdentity);
    let weak = Arc::downgrade(&identity);
    let key = Identifier::Integer(7);
    for _ in 0..2 {
        let result = cache
            .find(&identity, key.clone(), || async {
                Err(Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "test_database_failed",
                    "受控数据库失败",
                ))
            })
            .await;
        assert!(matches!(result, Err(error) if error.code() == "test_database_failed"));
    }
    assert!(
        cache
            .find(&identity, key, || async { Ok(None) })
            .await?
            .is_none()
    );
    drop(identity);
    assert!(weak.upgrade().is_some());
    drop(cache);
    assert!(weak.upgrade().is_none());
    Ok(())
}

#[tokio::test]
async fn concurrent_lookup_is_single_flight_and_initialization_cancellation_can_retry() -> Result<()>
{
    let cache = Arc::new(ClientLookupCache::default());
    let identity = Arc::new(RepositoryIdentity);
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let first = {
        let (cache, identity, entered, release, calls) = (
            Arc::clone(&cache),
            Arc::clone(&identity),
            Arc::clone(&entered),
            Arc::clone(&release),
            Arc::clone(&calls),
        );
        tokio::spawn(async move {
            cache
                .find(&identity, Identifier::Integer(1), || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    entered.notify_one();
                    release.notified().await;
                    Ok(None)
                })
                .await
        })
    };
    entered.notified().await;
    // 取消已进入初始化且尚未取得结果的真实 future；不以 sleep 猜执行阶段。
    first.abort();
    assert!(first.await.is_err_and(|error| error.is_cancelled()));
    let (second, third) = tokio::join!(
        cache.find(&identity, Identifier::Integer(1), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(None)
        }),
        cache.find(&identity, Identifier::Integer(1), || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }),
    );
    assert!(second?.is_none());
    assert!(third?.is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[cfg(feature = "db-sqlite")]
mod sqlite {
    use super::*;
    use openrust::database::sea_orm::{ConnectionTrait, Database, DatabaseConnection};

    const ID: &str = "019a4d4b-f21a-7191-bce9-fdc817a165aa";
    async fn connection(name: &str) -> Result<DatabaseConnection> {
        let connection = Database::connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?;
        connection.execute_unprepared("CREATE TABLE oauth_clients (id TEXT PRIMARY KEY, name TEXT, secret TEXT, revoked INTEGER, redirect_uris TEXT, grant_types TEXT, provider TEXT)").await.map_err(Error::internal)?;
        connection
            .execute_raw(openrust::database::sea_orm::Statement::from_sql_and_values(
                openrust::database::sea_orm::DbBackend::Sqlite,
                "INSERT INTO oauth_clients VALUES (?, ?, NULL, 0, '[]', '[]', NULL)",
                [ID.into(), name.into()],
            ))
            .await
            .map_err(Error::internal)?;
        Ok(connection)
    }
    fn repository(db: Db) -> ClientRepository {
        ClientRepository {
            db,
            factory: Arc::new(ClientFactory::default()),
            identity: Arc::new(RepositoryIdentity),
        }
    }

    struct IntegerFactory {
        metadata: Arc<openrust::database::eloquent::ModelMetadata>,
        calls: AtomicUsize,
    }
    impl ClientModelFactory for IntegerFactory {
        fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
            &self.metadata
        }
        fn is_valid_identifier(&self, value: &Value) -> bool {
            value.is_i64()
        }
        fn new_from_builder(&self, attributes: Map<String, Value>) -> Result<ClientRecord> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err(Error::custom_with_status(
                    openrust::http::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    "test_factory_failed",
                    "受控模型水合失败",
                ));
            }
            Ok(Arc::new(crate::Client::from_attributes(
                Arc::clone(&self.metadata),
                attributes,
            )))
        }
    }

    #[tokio::test]
    async fn selected_integer_factory_key_table_and_hydration_errors_are_preserved() -> Result<()> {
        let connection = Database::connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?;
        connection.execute_unprepared("CREATE TABLE tenant_apps (app_key INTEGER PRIMARY KEY, name TEXT, secret TEXT, revoked INTEGER); INSERT INTO tenant_apps VALUES (9007199254740993, 'custom', 'existing-hash', 0)").await.map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection)?;
        let mut metadata =
            openrust::database::eloquent::ModelMetadata::new("tenant_apps", "test::TenantClient");
        metadata.key_name = "app_key".into();
        let factory = Arc::new(IntegerFactory {
            metadata: Arc::new(metadata),
            calls: AtomicUsize::new(0),
        });
        let repository = ClientRepository {
            db,
            factory: factory.clone(),
            identity: Arc::new(RepositoryIdentity),
        };
        let container = container();
        let scope = container.scope()?;
        let id = json!(9_007_199_254_740_993i64);
        assert!(
            matches!(repository.find(&crate::ClientLookupContext::from(&scope), &id).await, Err(error) if error.code() == "test_factory_failed")
        );
        let loaded = repository
            .find(&crate::ClientLookupContext::from(&scope), &id)
            .await?
            .ok_or_else(|| Error::custom("test_client_missing", "模型水合错误后未重试"))?;
        assert_eq!(loaded.model_key()?, id);
        assert_eq!(loaded.secret()?.as_deref(), Some("existing-hash"));
        assert!(loaded.plain_secret()?.is_none());
        let again = repository
            .find(&crate::ClientLookupContext::from(&scope), &id)
            .await?
            .ok_or_else(|| Error::custom("test_client_missing", "未命中模型缓存"))?;
        assert!(Arc::ptr_eq(&loaded, &again));
        assert_eq!(factory.calls.load(Ordering::SeqCst), 2);
        assert!(
            repository
                .find(
                    &crate::ClientLookupContext::from(&scope),
                    &json!("9007199254740993")
                )
                .await?
                .is_none()
        );
        scope.close();
        Ok(())
    }

    struct ProtocolFactory {
        metadata: Arc<openrust::database::eloquent::ModelMetadata>,
        calls: AtomicUsize,
    }
    impl ClientModelFactory for ProtocolFactory {
        fn metadata(&self) -> &openrust::database::eloquent::ModelMetadata {
            &self.metadata
        }
        fn is_valid_identifier(&self, value: &Value) -> bool {
            value.is_string() || value.is_i64()
        }
        fn new_from_builder(&self, raw: Map<String, Value>) -> Result<ClientRecord> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(crate::Client::from_attributes(
                self.metadata.clone(),
                raw,
            )))
        }
    }

    #[tokio::test]
    async fn ordinary_raw_query_and_integer_protocol_query_never_poison_each_other() -> Result<()> {
        let connection = Database::connect("sqlite::memory:")
            .await
            .map_err(Error::internal)?;
        connection.execute_unprepared("CREATE TABLE apps (id TEXT PRIMARY KEY, name TEXT, revoked INTEGER); INSERT INTO apps VALUES ('001','ordinary',0),('1','protocol',0)").await.map_err(Error::internal)?;
        let db = Db::default();
        db.initialize(connection)?;
        let mut metadata =
            openrust::database::eloquent::ModelMetadata::new("apps", "fixture::ProtocolClient");
        metadata.key_type = "integer".to_owned();
        let factory = Arc::new(ProtocolFactory {
            metadata: Arc::new(metadata),
            calls: AtomicUsize::new(0),
        });
        let repository = ClientRepository::from_db(db, factory.clone())?;
        let container = container();
        for protocol_first in [false, true] {
            let scope = container.scope()?;
            let context = crate::ClientLookupContext::from(&scope);
            let raw = json!("001");
            let before = factory.calls.load(Ordering::SeqCst);
            let (ordinary, protocol) = if protocol_first {
                let p = repository
                    .find_active_by_protocol(&context, "001")
                    .await?
                    .ok_or_else(invalid_identifier)?;
                let o = repository
                    .find(&context, &raw)
                    .await?
                    .ok_or_else(invalid_identifier)?;
                (o, p)
            } else {
                let o = repository
                    .find(&context, &raw)
                    .await?
                    .ok_or_else(invalid_identifier)?;
                let p = repository
                    .find_active_by_protocol(&context, "001")
                    .await?
                    .ok_or_else(invalid_identifier)?;
                (o, p)
            };
            assert_eq!(ordinary.name()?.as_deref(), Some("ordinary"));
            assert_eq!(protocol.name()?.as_deref(), Some("protocol"));
            assert!(!Arc::ptr_eq(&ordinary, &protocol));
            let again = repository
                .find_active_by_protocol(&context, "001")
                .await?
                .ok_or_else(invalid_identifier)?;
            assert!(Arc::ptr_eq(&again, &protocol));
            assert_eq!(factory.calls.load(Ordering::SeqCst), before + 2);
            let numeric = repository
                .find(&context, &json!(1))
                .await?
                .ok_or_else(invalid_identifier)?;
            assert!(!Arc::ptr_eq(&numeric, &protocol));
            assert_eq!(factory.calls.load(Ordering::SeqCst), before + 3);
            assert!(
                repository
                    .find_active_by_protocol(&context, "bad-integer")
                    .await?
                    .is_none()
            );
            assert!(
                repository
                    .find_active_by_protocol(&context, "9223372036854775808")
                    .await?
                    .is_none()
            );
            assert!(
                repository
                    .find_active_by_protocol(&context, "2")
                    .await?
                    .is_none()
            );
            assert_eq!(factory.calls.load(Ordering::SeqCst), before + 3);
            scope.close();
        }
        Ok(())
    }

    #[tokio::test]
    async fn malformed_uuid_never_queries_and_valid_database_errors_propagate() -> Result<()> {
        let container = container();
        let scope = container.scope()?;
        let repository = repository(Db::default());
        assert!(
            repository
                .find(
                    &crate::ClientLookupContext::from(&scope),
                    &json!("not-a-uuid")
                )
                .await?
                .is_none()
        );
        assert!(
            repository
                .find(&crate::ClientLookupContext::from(&scope), &json!(ID))
                .await
                .is_err()
        );
        assert!(
            repository
                .find(&crate::ClientLookupContext::from(&scope), &json!(ID))
                .await
                .is_err()
        );
        scope.close();
        Ok(())
    }

    #[tokio::test]
    async fn default_read_replica_and_same_model_revocation_are_observable() -> Result<()> {
        let db = Db::default();
        db.initialize_with_replicas(
            connection("primary").await?,
            vec![connection("replica").await?],
        )?;
        let repository = repository(db);
        let container = container();
        let scope = container.scope()?;
        let id = json!(ID);
        let loaded = repository
            .find_active(&crate::ClientLookupContext::from(&scope), &id)
            .await?
            .ok_or_else(|| Error::custom("test_client_missing", "未读取客户端"))?;
        assert_eq!(loaded.name()?, Some("replica".into()));
        assert!(loaded.plain_secret()?.is_none());
        let again = repository
            .clone()
            .find(&crate::ClientLookupContext::from(&scope), &id)
            .await?
            .ok_or_else(|| Error::custom("test_client_missing", "未命中缓存"))?;
        assert!(Arc::ptr_eq(&loaded, &again));
        let protocol = repository
            .find_active_by_protocol(&crate::ClientLookupContext::from(&scope), ID)
            .await?
            .ok_or_else(invalid_identifier)?;
        assert!(Arc::ptr_eq(&loaded, &protocol));
        let mut attributes = loaded.attributes()?;
        attributes.insert("revoked".into(), json!(true));
        loaded.set_raw_attributes(attributes)?;
        assert!(
            repository
                .find_active(&crate::ClientLookupContext::from(&scope), &id)
                .await?
                .is_none()
        );
        assert!(
            repository
                .find_active_by_protocol(&crate::ClientLookupContext::from(&scope), ID)
                .await?
                .is_none()
        );
        scope.close();
        Ok(())
    }

    #[tokio::test]
    async fn cached_missing_is_stable_but_next_scope_observes_the_database() -> Result<()> {
        let connection = connection("initial").await?;
        let db = Db::default();
        db.initialize(connection)?;
        let repository = repository(db.clone());
        let container = container();
        let scope = container.scope()?;
        let missing = json!("019a4d4b-f21a-7191-bce9-fdc817a165ab");
        assert!(
            repository
                .find(&crate::ClientLookupContext::from(&scope), &missing)
                .await?
                .is_none()
        );
        db.primary_connection()?.execute_unprepared("INSERT INTO oauth_clients VALUES ('019a4d4b-f21a-7191-bce9-fdc817a165ab', 'created', NULL, 0, '[]', '[]', NULL)").await.map_err(Error::internal)?;
        assert!(
            repository
                .find(&crate::ClientLookupContext::from(&scope), &missing)
                .await?
                .is_none()
        );
        let next = container.scope()?;
        assert_eq!(
            repository
                .find(&crate::ClientLookupContext::from(&next), &missing)
                .await?
                .ok_or_else(|| Error::custom("test_client_missing", "新入口未读取新增客户端"))?
                .name()?,
            Some("created".into())
        );
        scope.close();
        next.close();
        Ok(())
    }
}

#[tokio::test]
async fn query_dimension_separates_protocol_conversion_in_both_orders_and_none_cache() -> Result<()>
{
    for protocol_first in [false, true] {
        let cache = ClientLookupCache::default();
        let identity = Arc::new(RepositoryIdentity);
        let argument = Identifier::String("001".to_owned());
        let ordinary = record()?;
        let protocol = record()?;
        let load_ordinary = || async { Ok(Some(ordinary.clone())) };
        let load_protocol = || async { Ok(Some(protocol.clone())) };
        let (a, b) = if protocol_first {
            let b = cache
                .find_query(
                    &identity,
                    argument.clone(),
                    Identifier::Integer(1),
                    load_protocol,
                )
                .await?;
            let a = cache
                .find(&identity, argument.clone(), load_ordinary)
                .await?;
            (a, b)
        } else {
            let a = cache
                .find(&identity, argument.clone(), load_ordinary)
                .await?;
            let b = cache
                .find_query(
                    &identity,
                    argument.clone(),
                    Identifier::Integer(1),
                    load_protocol,
                )
                .await?;
            (a, b)
        };
        assert!(Arc::ptr_eq(&a.ok_or_else(invalid_identifier)?, &ordinary));
        assert!(Arc::ptr_eq(&b.ok_or_else(invalid_identifier)?, &protocol));
        assert!(!Arc::ptr_eq(&ordinary, &protocol));
        let again = cache
            .find_query(
                &identity,
                argument.clone(),
                Identifier::Integer(1),
                || async {
                    Err(Error::custom(
                        "must_not_load_again",
                        "重复协议查询应命中同一once",
                    ))
                },
            )
            .await?
            .ok_or_else(invalid_identifier)?;
        assert!(Arc::ptr_eq(&again, &protocol));
        let miss = Identifier::String("003".to_owned());
        assert!(
            cache
                .find(&identity, miss.clone(), || async { Ok(None) })
                .await?
                .is_none()
        );
        assert!(
            cache
                .find_query(&identity, miss, Identifier::Integer(3), || async {
                    record().map(Some)
                })
                .await?
                .is_some()
        );
        let miss = Identifier::String("004".to_owned());
        assert!(
            cache
                .find_query(&identity, miss.clone(), Identifier::Integer(4), || async {
                    Ok(None)
                })
                .await?
                .is_none()
        );
        assert!(
            cache
                .find(&identity, miss, || async { record().map(Some) })
                .await?
                .is_some()
        );
    }
    Ok(())
}
