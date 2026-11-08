mod cache {
    use crate::{
        Result,
        cache::{Cache, CacheServiceProvider, CacheStore},
        container::{
            ClassAttribute, ClassDescriptor, ConstructorParam, ServiceKey,
            attributes::Cache as Attribute,
        },
        foundation::ProviderBuilder,
    };
    use figment::{Figment, providers::Serialized};
    use serde_json::json;
    use std::{sync::Arc, time::Duration};

    struct Stores {
        default: Arc<CacheStore>,
        named: Arc<CacheStore>,
        memo: Arc<CacheStore>,
        same_memo: Arc<CacheStore>,
        named_memo: Arc<CacheStore>,
    }

    fn store_param(
        name: &'static str,
        factory: impl Fn() -> Attribute + Send + Sync + 'static,
    ) -> ConstructorParam {
        ConstructorParam::class::<CacheStore>(name)
            .with_attribute(ClassAttribute::contextual(move || Ok(factory())))
    }

    #[tokio::test]
    async fn cache_attribute_selects_stores_and_memoizes_in_current_scope() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(CacheServiceProvider::new());
        builder.class_descriptors([ClassDescriptor::concrete::<Stores>(
            "test::CacheAttributeStores",
            vec![
                store_param("default", Attribute::default),
                store_param("named", || Attribute::new("0")),
                store_param("memo", || Attribute::default().memoized()),
                store_param("same_memo", || Attribute::new("local").memoized()),
                store_param("named_memo", || Attribute::new("0").memoized()),
            ],
            |args| {
                Ok(Arc::new(Stores {
                    default: args.service(0)?,
                    named: args.service(1)?,
                    memo: args.service(2)?,
                    same_memo: args.service(3)?,
                    named_memo: args.service(4)?,
                }))
            },
        )]);
        let runtime = builder
            .start(Figment::from(Serialized::defaults(json!({
                "cache": {"default": "local", "stores": {
                    "local": {"driver": "memory"}, "0": {"driver": "memory"}
                }}
            }))))
            .await?;
        let container = runtime.container();
        let cache = container.resolve::<Cache>().await?;
        let first = container.scope()?;
        let second = container.scope()?;
        let a = first.resolve::<Stores>().await?;
        let b = second.resolve::<Stores>().await?;
        let ttl = Duration::from_secs(30);
        cache.default_store().await?.set("key", &1, None).await?;
        cache.store("0").await?.set("key", &9, None).await?;
        assert!(Arc::ptr_eq(&a.memo, &a.same_memo));
        assert!(!Arc::ptr_eq(&a.memo, &b.memo));
        assert_eq!(a.memo.get::<i32>("key").await?, Some(1));
        assert_eq!(a.named_memo.get::<i32>("key").await?, Some(9));
        assert_eq!(a.named.get::<i32>("key").await?, Some(9));
        assert_eq!(a.memo.get::<i32>("miss").await?, None);
        a.default.set("key", &2, None).await?;
        a.default.set("miss", &7, None).await?;
        assert_eq!(a.memo.get::<i32>("key").await?, Some(1));
        assert_eq!(a.memo.get::<i32>("miss").await?, None);
        assert_eq!(b.memo.get::<i32>("key").await?, Some(2));
        assert_eq!(b.memo.get::<i32>("miss").await?, Some(7));
        a.memo.set("key", &3, None).await?;
        assert_eq!(a.same_memo.get::<i32>("key").await?, Some(3));
        a.memo.remove("key").await?;
        assert_eq!(a.memo.get::<i32>("key").await?, None);
        assert_eq!(b.memo.get::<i32>("key").await?, Some(2));

        let plain_tagged = a.default.tagged(&["article"])?;
        let memo_tagged = a.memo.tagged(&["article"])?;
        plain_tagged.set("title", &1, ttl).await?;
        assert_eq!(memo_tagged.get::<i32>("title").await?, Some(1));
        plain_tagged.set("title", &2, ttl).await?;
        assert_eq!(memo_tagged.get::<i32>("title").await?, Some(1));
        memo_tagged.set("title", &3, ttl).await?;
        assert_eq!(memo_tagged.get::<i32>("title").await?, Some(3));
        a.default.invalidate_tags(&["article"]).await?;
        assert_eq!(memo_tagged.get::<i32>("title").await?, None);
        assert_eq!(memo_tagged.get::<serde_json::Value>("swr").await?, None);
        assert_eq!(
            memo_tagged
                .get_stale_while_revalidate("swr", "title-test", ttl, ttl, || async { Ok(17) })
                .await?,
            17
        );
        assert_eq!(
            memo_tagged
                .get::<serde_json::Value>("swr")
                .await?
                .and_then(|entry| entry.get("value").and_then(serde_json::Value::as_i64)),
            Some(17)
        );
        runtime
            .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
            .await
    }

    struct AlternateStore(Arc<CacheStore>);

    #[tokio::test]
    async fn memo_attribute_uses_the_contextually_selected_manager() -> Result<()> {
        let configuration = |name: &str| {
            Figment::from(Serialized::defaults(json!({
                "cache": {"default": name, "stores": {(name): {"driver": "memory"}}}
            })))
        };
        let mut alternate = ProviderBuilder::new();
        alternate.provider(CacheServiceProvider::new());
        let alternate = alternate.start(configuration("alternate")).await?;
        let cache = alternate.container().resolve::<Cache>().await?;
        cache.default_store().await?.set("key", &17, None).await?;

        let mut host = ProviderBuilder::new();
        host.provider(CacheServiceProvider::new());
        host.class_descriptors([ClassDescriptor::concrete::<AlternateStore>(
            "test::ContextualCache",
            vec![store_param("memo", || {
                Attribute::new("alternate").memoized()
            })],
            |args| Ok(Arc::new(AlternateStore(args.service(0)?))),
        )]);
        let host = host.start(configuration("local")).await?;
        let container = host.container();
        container
            .when(ServiceKey::typed::<AlternateStore>())?
            .needs(ServiceKey::typed::<Cache>())
            .give_factory(move |_| {
                let cache = Arc::clone(&cache);
                Box::pin(async move { Ok(cache) })
            })?;
        let store = container.scope()?.resolve::<AlternateStore>().await?;
        assert_eq!(store.0.get::<i32>("key").await?, Some(17));

        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        host.shutdown(deadline).await?;
        alternate.shutdown(deadline).await
    }
}

#[cfg(feature = "db-mysql")]
mod database {
    use crate::database::sea_orm::{ConnectionTrait, DbBackend, Statement};
    use crate::{
        Error, Result,
        container::{
            ClassAttribute, ClassDescriptor, ConstructorParam,
            attributes::{DB, Database},
        },
        database::{DatabaseBackend, DatabaseServiceProvider, Db},
        foundation::ProviderBuilder,
        testing::TestDatabase,
    };
    use sea_orm_migration::{MigrationTrait, MigratorTrait};
    use serde_json::json;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    struct EmptyMigrator;

    #[sea_orm_migration::async_trait::async_trait]
    impl MigratorTrait for EmptyMigrator {
        fn migrations() -> Vec<Box<dyn MigrationTrait>> {
            Vec::new()
        }
    }

    struct Connections([Arc<Db>; 4]);
    struct Missing;

    #[tokio::test]
    async fn database_attribute_rejects_unconfigured_default_during_injection() -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(DatabaseServiceProvider);
        builder.class_descriptors([ClassDescriptor::concrete::<Missing>(
            "test::UnconfiguredDefaultDatabase",
            vec![
                ConstructorParam::class::<Db>("database")
                    .with_attribute(ClassAttribute::contextual(|| Ok(Database::default()))),
            ],
            |_| Ok(Arc::new(Missing)),
        )]);
        let runtime = builder.start(figment::Figment::new()).await?;
        let result = async {
            let container = runtime.container();
            let db = container.resolve::<Db>().await?;
            let connection = db.connection();
            let code = connection.as_ref().err().map(Error::code);
            let injection = container.resolve::<Missing>().await;
            super::verify(
                code.is_some() && injection.as_ref().err().map(Error::code) == code,
                "未配置的默认数据库应在属性注入时失败",
            )
        }
        .await;
        result.and(
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
                .await,
        )
    }

    async fn active_database(db: &Db) -> Result<String> {
        db.connection()?
            .query_one_raw(Statement::from_string(
                DbBackend::MySql,
                "SELECT DATABASE() AS active",
            ))
            .await
            .map_err(Error::internal)?
            .ok_or_else(|| Error::custom("test_database_name_missing", "当前连接缺少数据库名称"))?
            .try_get("", "active")
            .map_err(Error::internal)
    }

    fn descriptors() -> [ClassDescriptor; 2] {
        [
            ClassDescriptor::concrete::<Connections>(
                "test::Connections",
                vec![
                    ConstructorParam::class::<Db>("primary")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Database::default()))),
                    ConstructorParam::class::<Db>("reporting")
                        .with_attribute(ClassAttribute::contextual(|| Ok(DB::new("reporting")))),
                    ConstructorParam::class::<Db>("empty")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Database::new("")))),
                    ConstructorParam::class::<Db>("zero")
                        .with_attribute(ClassAttribute::contextual(|| Ok(DB::new("0")))),
                ],
                |args| {
                    Ok(Arc::new(Connections([
                        args.service(0)?,
                        args.service(1)?,
                        args.service(2)?,
                        args.service(3)?,
                    ])))
                },
            ),
            ClassDescriptor::concrete::<Missing>(
                "test::MissingDatabase",
                vec![
                    ConstructorParam::class::<Db>("database")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Database::new("absent")))),
                ],
                |_| Ok(Arc::new(Missing)),
            ),
        ]
    }

    async fn verify_connections(primary: &TestDatabase, reporting: &TestDatabase) -> Result<()> {
        let mut builder = ProviderBuilder::new();
        builder.provider(DatabaseServiceProvider);
        builder.class_descriptors(descriptors());
        let runtime = builder.start(figment::Figment::from(figment::providers::Serialized::defaults(json!({
            "database": {
                "url": primary.url(),
                "max_connections": 2,
                "min_connections": 0,
                "instances": {"reporting": {"url": reporting.url(), "max_connections": 2, "min_connections": 0}}
            }
        })))).await?;
        let result = async {
            let container = runtime.container();
            let counts = Arc::new([AtomicUsize::new(0), AtomicUsize::new(0)]);
            let seen = Arc::clone(&counts);
            container.after_resolving_attribute::<Database>(move |event, _| {
                let seen = Arc::clone(&seen);
                Box::pin(async move {
                    let attribute = event.attribute().downcast::<Database>()?;
                    assert!(matches!(attribute.connection_name(), None | Some("")));
                    assert!(event.service::<Db>()?.is_some());
                    seen[0].fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            })?;
            let seen = Arc::clone(&counts);
            container.after_resolving_attribute::<DB>(move |event, _| {
                let seen = Arc::clone(&seen);
                Box::pin(async move {
                    let attribute = event.attribute().downcast::<DB>()?;
                    assert!(matches!(
                        attribute.connection_name(),
                        Some("reporting" | "0")
                    ));
                    assert!(event.service::<Db>()?.is_some());
                    seen[1].fetch_add(1, Ordering::Relaxed);
                    Ok(())
                })
            })?;
            let values = container.resolve::<Connections>().await?;
            let primary = url::Url::parse(primary.url()).map_err(Error::internal)?;
            let reporting = url::Url::parse(reporting.url()).map_err(Error::internal)?;
            for index in [0, 2, 3] {
                super::verify(
                    active_database(&values.0[index]).await?
                        == primary.path().trim_start_matches('/'),
                    "默认连接属性没有选中主 schema",
                )?;
            }
            super::verify(
                active_database(&values.0[1]).await? == reporting.path().trim_start_matches('/'),
                "DB 具名属性没有选中 reporting schema",
            )?;
            super::verify(
                counts.each_ref().map(|count| count.load(Ordering::Relaxed)) == [2, 2],
                "DB 和 Database 应分别派发各自的后置回调",
            )?;
            let missing = container.resolve::<Missing>().await;
            super::verify(
                matches!(missing, Err(error) if error.code() == "database_instance_not_found"),
                "未知数据库名称应保留管理器错误",
            )
        }
        .await;
        result.and(
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
                .await,
        )
    }

    #[tokio::test]
    async fn database_attributes_select_real_mysql_schemas_and_keep_distinct_identity() -> Result<()>
    {
        let Ok(url) = crate::test_database_environment::database_url() else {
            eprintln!("跳过数据库属性真实 MySQL 复验，未配置测试数据库 URL");
            return Ok(());
        };
        if DatabaseBackend::from_url(&url)? != DatabaseBackend::MySql {
            return Ok(());
        }
        let primary = TestDatabase::create::<EmptyMigrator>(&url).await?;
        let reporting = match TestDatabase::create::<EmptyMigrator>(&url).await {
            Ok(reporting) => reporting,
            Err(error) => return Err::<(), _>(error).and(primary.close().await),
        };
        let result = verify_connections(&primary, &reporting).await;
        let reporting = reporting.close().await;
        let primary = primary.close().await;
        result.and(reporting).and(primary)
    }
}

#[cfg(feature = "storage-local")]
mod storage {
    use crate::{
        Result,
        container::{ClassAttribute, ClassDescriptor, ConstructorParam, attributes::Storage},
        filesystem::{FilesystemAdapter, FilesystemServiceProvider, StoragePath, UploadOptions},
        foundation::ProviderBuilder,
    };
    use serde_json::json;
    use std::{sync::Arc, time::Duration};

    struct Disks([Arc<FilesystemAdapter>; 4]);
    struct Missing;

    #[tokio::test]
    async fn storage_attribute_selects_real_local_disks_without_unknown_name_fallback() -> Result<()>
    {
        let first = tempfile::tempdir()?;
        let second = tempfile::tempdir()?;
        let mut builder = ProviderBuilder::new();
        builder.provider(FilesystemServiceProvider::new());
        builder.class_descriptors([
            ClassDescriptor::concrete::<Disks>(
                "test::Disks",
                vec![
                    ConstructorParam::class::<FilesystemAdapter>("default")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::default()))),
                    ConstructorParam::class::<FilesystemAdapter>("named")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("second")))),
                    ConstructorParam::class::<FilesystemAdapter>("zero")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("0")))),
                    ConstructorParam::class::<FilesystemAdapter>("empty")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("")))),
                ],
                |args| {
                    Ok(Arc::new(Disks([
                        args.service(0)?,
                        args.service(1)?,
                        args.service(2)?,
                        args.service(3)?,
                    ])))
                },
            ),
            ClassDescriptor::concrete::<Missing>(
                "test::MissingDisk",
                vec![
                    ConstructorParam::class::<FilesystemAdapter>("disk")
                        .with_attribute(ClassAttribute::contextual(|| Ok(Storage::new("absent")))),
                ],
                |_| Ok(Arc::new(Missing)),
            ),
        ]);
        let runtime = builder
            .start(figment::Figment::from(
                figment::providers::Serialized::defaults(json!({
                    "storage": {"default_disk": "first", "disks": {
                    "first":{"driver":"fs","root":first.path()},
                    "second":{"driver":"fs","root":second.path()}
                    }}
                })),
            ))
            .await?;
        let result = async {
            let container = runtime.container();
            let disks = container.resolve::<Disks>().await?;
            assert_eq!(
                disks.0.each_ref().map(|disk| disk.name()),
                ["first", "second", "first", "first"]
            );
            for (disk, bytes) in [
                (&disks.0[0], "default contents"),
                (&disks.0[1], "named contents"),
            ] {
                disk.put_body(
                    StoragePath::new("payload.txt")?,
                    Some("text/plain".into()),
                    axum::body::Body::from(bytes),
                    UploadOptions::default(),
                    None,
                )
                .await?;
            }
            assert_eq!(
                tokio::fs::read(first.path().join("payload.txt")).await?,
                b"default contents"
            );
            assert_eq!(
                tokio::fs::read(second.path().join("payload.txt")).await?,
                b"named contents"
            );
            let missing = container.resolve::<Missing>().await;
            assert!(matches!(missing, Err(error) if error.code() == "storage_disk_not_found"));
            Ok(())
        }
        .await;
        result.and(
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
                .await,
        )
    }
}
#[cfg(any(feature = "db-mysql", feature = "storage-local"))]
fn verify(condition: bool, detail: &'static str) -> crate::Result<()> {
    condition
        .then_some(())
        .ok_or_else(|| crate::Error::custom("test_attribute_selection", detail))
}
