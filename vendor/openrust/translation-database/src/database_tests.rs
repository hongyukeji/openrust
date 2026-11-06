//! 同一公开数据库Loader的真实后端契约；无服务配置且无SQLite时明确不适用。
use super::parse_tests::{put, resource};
use super::*;
use openrust::database::sea_orm_migration::MigrationTrait;
use openrust::database::sea_orm_migration::MigratorTrait;
use openrust::events::EventsServiceProvider;
use openrust::foundation::ProviderBuilder;
use openrust::testing::TestDatabase;
use openrust::translation::CompositeTranslationLoader;
use openrust::translation::MessageArgs;
use openrust::translation::TranslationConfig;
use openrust::translation::TranslationRefreshOutcome;
use openrust::translation::Translator;
use serde_json::json;
use std::sync::atomic::AtomicUsize;

struct Migrations;
impl MigratorTrait for Migrations {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(crate::TranslationSourceMigration)]
    }
}
async fn owned<F>(callback: F) -> Result<()>
where
    F: FnOnce(Db) -> BoxFuture<'static, Result<()>>,
{
    let database = if let Ok(url) = std::env::var("OPENRUST_TEST_DATABASE_URL") {
        TestDatabase::create::<Migrations>(&url).await?
    } else {
        #[cfg(feature = "db-sqlite")]
        {
            TestDatabase::sqlite_file::<Migrations>().await?
        }
        #[cfg(not(feature = "db-sqlite"))]
        {
            eprintln!("未执行数据库翻译服务回归：需OPENRUST_TEST_DATABASE_URL或db-sqlite");
            return Ok(());
        }
    };
    let outcome: Result<()> = async {
        let db = Db::default();
        db.initialize(database.connection()?)?;
        callback(db).await
    }
    .await;
    database.close().await?;
    outcome
}
#[tokio::test]
async fn snapshots_keep_empty_structured_messages_and_do_not_query_after_load() -> Result<()> {
    owned(|db| Box::pin(async move {
        let loader = DatabaseTranslationLoader::new(db.clone());
        let en = resource("en", None)?;
        let module = resource("en", Some("shop"))?;
        let result = loader.write(vec![put(&en, "empty", json!("")), put(&module, "greeting", json!("Hello :name")), put(&en, "count", json!({"plural":{"argument":"count","forms":{"one":"one","other":"many"}}}))]).await?;
        assert_eq!(result.revision, 1);
        assert!(result.notifications_delivered);
        let session = loader.begin_snapshot().await?;
        assert_eq!(session.resources().len(), 2);
        assert_eq!(session.load(&en.locale, "*", None)?.get("empty"), Some(&json!("")));
        assert_eq!(session.load(&en.locale, "*", Some("shop"))?.get("greeting"), Some(&json!("Hello :name")));
        db.connection()?.execute_unprepared(&format!("DROP TABLE {}", db.quoted_table_name("translation_lines")?)).await.map_err(Error::internal)?;
        assert!(session.load(&en.locale, "*", None)?.contains_key("count"));
        assert!(matches!(loader.refresh().await, Err(error) if error.code() == "i18n_database_source_unavailable"));
        Ok(())
    })).await
}

#[tokio::test]
async fn update_disable_delete_publish_new_versions_without_mutating_old_snapshot() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db);
            let en = resource("en", None)?;
            loader.write(vec![put(&en, "key", json!("old"))]).await?;
            let old = loader.begin_snapshot().await?;
            loader.write(vec![put(&en, "key", json!("new"))]).await?;
            assert!(loader.needs_refresh().await?);
            let updated = loader.begin_snapshot().await?;
            assert_eq!(
                updated.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("new"))
            );
            assert_eq!(
                old.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            let disabled = TranslationMutation::Put {
                resource: en.clone(),
                key: "key".into(),
                value: json!("hidden"),
                enabled: false,
            };
            assert_eq!(loader.write(vec![disabled]).await?.revision, 3);
            assert_eq!(
                loader.refresh().await?.load(&en.locale, "*", None)?,
                BTreeMap::new()
            );
            assert_eq!(
                loader
                    .write(vec![TranslationMutation::Delete {
                        resource: en.clone(),
                        key: "key".into()
                    }])
                    .await?
                    .revision,
                4
            );
            assert_eq!(loader.refresh().await?.resources(), &[]);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn bounds_and_invalid_content_reject_without_changing_revision() -> Result<()> {
    owned(|db| Box::pin(async move {
        let loader = DatabaseTranslationLoader::new(db.clone());
        let en = resource("en", None)?;
        loader.write(vec![put(&en, "one", json!("1")), put(&en, "two", json!("2")), put(&en, "three", json!("3"))]).await?;
        let bounded = DatabaseTranslationLoader::new(db.clone()).with_limits(2, Duration::from_secs(30))?;
        assert!(matches!(bounded.begin_snapshot().await, Err(error) if error.code() == "i18n_database_row_limit"));
        assert!(loader.write(vec![put(&en, "bad", json!({"invalid":true}))]).await.is_err());
        assert_eq!(loader.current_revision().await?, 1);
        assert!(DatabaseTranslationLoader::new(db).with_limits(0, Duration::from_secs(1)).is_err());
        Ok(())
    })).await
}

#[tokio::test]
async fn notices_are_hints_and_lost_notice_converges_by_bounded_revision_check() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let first = DatabaseTranslationLoader::new(db.clone())
                .with_limits(100, Duration::from_nanos(1))?;
            let peer = DatabaseTranslationLoader::new(db);
            let en = resource("en", None)?;
            let published = first.begin_snapshot().await?;
            peer.write(vec![put(&en, "key", json!("peer"))]).await?;
            // 不发通知，显式配置的核对间隔到期后仍发现修订。
            assert!(first.needs_refresh_since(published.as_ref()).await?);
            assert!(first.needs_refresh().await?);
            first.refresh().await?;
            first.notify_revision(1);
            first.notify_revision(0);
            first.notify_revision(1);
            assert!(!first.needs_refresh().await?);
            first.notify_revision(u64::MAX);
            assert!(!first.needs_refresh().await?);
            assert!(!first.needs_refresh().await?);
            Ok(())
        })
    })
    .await
}

async fn observed_loader<F>(db: Db, callback: F) -> Result<()>
where
    F: FnOnce(Arc<DatabaseTranslationLoader>, Arc<AtomicUsize>) -> BoxFuture<'static, Result<()>>,
{
    let count = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&count);
    let mut events = EventsServiceProvider::new();
    events.listen::<TranslationSourceChanged, _>(move |_, _| {
        let observed = Arc::clone(&observed);
        Box::pin(async move {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    });
    let mut providers = ProviderBuilder::new();
    providers.provider(events);
    let runtime = providers.start(figment::Figment::new()).await?;
    let outcome: Result<()> = async {
        let loader = Arc::new(
            DatabaseTranslationLoader::new(db)
                .with_events((*runtime.container().resolve::<Events>().await?).clone()),
        );
        callback(loader, count).await
    }
    .await;
    let shutdown = runtime
        .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
        .await;
    outcome?;
    shutdown
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
async fn mysql_noncanonical_persisted_locale_and_case_insensitive_hash_hits_fail_closed()
-> Result<()> {
    owned(|db| Box::pin(async move {
        if db.backend()? != DbBackend::MySql {
            eprintln!("未执行MySQL非规范身份回归：此用例需要原URL fixture指向真实MySQL");
            return Ok(());
        }
        observed_loader(db.clone(), move |loader, count| Box::pin(async move {
            let english = resource("EN", None)?;
            assert_eq!(english.locale.as_str(), "en");
            loader.write(vec![put(&english, "key", json!("old"))]).await?;
            let service = Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            let retained = loader.begin_snapshot().await?;
            let canonical = identity_hash(&english, "key")?;
            for (field, corrupted, restored) in [
                ("locale", "EN".to_owned(), "en".to_owned()),
                ("identity_hash", canonical.to_ascii_uppercase(), canonical.clone()),
            ] {
                let generation = service.refresh_status()?.generation;
                assert_eq!(db.table("translation_lines")?.where_eq("identity_hash", canonical.clone())?
                    .update(vec![(field, corrupted.clone().into())]).await?, 1);
                let hit = db.table("translation_lines")?.where_eq("identity_hash", canonical.clone())?
                    .first().await?.ok_or_else(|| Error::custom("test_case_insensitive_hash_required",
                        "本回归需实际MySQL大小写不敏感主键命中，不能以无命中代替碰撞校验"))?;
                assert_eq!(text(&hit, field)?, corrupted);
                assert!(matches!(service.refresh().await, Err(error)
                    if error.code() == "i18n_source_refresh_failed"));
                assert_eq!(service.refresh_status()?.generation, generation);
                assert_eq!(service.translate(&english.locale, "key", &MessageArgs::new()), "old");
                for mutation in [put(&english, "key", json!("must-not-write")),
                    TranslationMutation::Delete { resource: english.clone(), key: "key".into() }] {
                    assert!(matches!(loader.write(vec![mutation]).await, Err(error)
                        if error.code() == "i18n_database_source_unavailable" && error.status().is_server_error()));
                    assert_eq!(loader.current_revision().await?, 1);
                    assert_eq!(db.table("translation_lines")?.count().await?, 1);
                    assert_eq!(db.table("translation_lines")?.where_eq("identity_hash", canonical.clone())?
                        .value::<String>("value_json").await?.as_deref(), Some("\"old\""));
                    assert_eq!(count.load(Ordering::SeqCst), 1);
                    assert_eq!(loader.hint.load(Ordering::SeqCst), 1);
                }
                assert_eq!(db.table("translation_lines")?.where_eq("identity_hash", canonical.clone())?
                    .update(vec![(field, restored.into())]).await?, 1);
                assert_eq!(service.refresh().await?, TranslationRefreshOutcome::Updated { generation: generation + 1 });
                assert_eq!(retained.load(&english.locale, "*", None)?.get("key"), Some(&json!("old")));
            }
            Ok(())
        })).await
    })).await
}

#[tokio::test]
async fn invalid_revision_boundaries_rollback_messages_and_do_not_publish_notifications()
-> Result<()> {
    owned(|db| Box::pin(async move {
        observed_loader(db.clone(), move |loader, count| Box::pin(async move {
            let english = resource("en", None)?;
            loader.write(vec![put(&english, "key", json!("old"))]).await?;
            let service = Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            let hash = identity_hash(&english, "key")?;
            for revision in [-1_i64, i64::MAX] {
                assert_eq!(db.table("translation_revisions")?.where_eq("id", 1_i32)?
                    .update(vec![("revision", revision.into())]).await?, 1);
                assert!(matches!(loader.write(vec![put(&english, "key", json!("must-not-change")),
                    put(&english, "new", json!("must-not-insert"))]).await, Err(error)
                    if error.code() == "i18n_database_source_unavailable" && error.status().is_server_error()));
                assert_eq!(db.table("translation_revisions")?.where_eq("id", 1_i32)?
                    .value::<i64>("revision").await?, Some(revision));
                assert_eq!(db.table("translation_lines")?.count().await?, 1);
                assert_eq!(db.table("translation_lines")?.where_eq("identity_hash", hash.clone())?
                    .value::<String>("value_json").await?.as_deref(), Some("\"old\""));
                assert_eq!(count.load(Ordering::SeqCst), 1);
                assert_eq!(loader.hint.load(Ordering::SeqCst), 1);
                assert_eq!(service.refresh_status()?.generation, 1);
                assert_eq!(service.translate(&english.locale, "key", &MessageArgs::new()), "old");
            }
            Ok(())
        })).await
    })).await
}

#[tokio::test]
async fn public_write_reports_database_failure_as_5xx_and_keeps_user_validation_4xx() -> Result<()>
{
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db.clone());
            let english = resource("en", None)?;
            db.primary_connection()?
                .execute_unprepared(&format!(
                    "DROP TABLE {}",
                    db.quoted_table_name("translation_revisions")?
                ))
                .await
                .map_err(Error::internal)?;
            assert!(
                matches!(loader.write(vec![put(&english, "key", json!(42))]).await, Err(error)
            if error.code() == "i18n_translation_resource_invalid"
            && error.status() == openrust::http::axum::http::StatusCode::BAD_REQUEST)
            );
            let write_result = loader
                .write(vec![put(
                    &english,
                    "key",
                    json!("private-message-sentinel"),
                )])
                .await;
            let Err(error) = write_result else {
                return Err(Error::custom(
                    "test_database_failure_required",
                    "缺表写入不能成功",
                ));
            };
            assert_eq!(error.code(), "i18n_database_source_unavailable");
            assert_eq!(
                error.status(),
                openrust::http::axum::http::StatusCode::SERVICE_UNAVAILABLE
            );
            let public = format!("{error:?} {error}");
            for forbidden in [
                "translation_revisions",
                "UPDATE ",
                "mysql://",
                "postgres://",
                "private-message-sentinel",
            ] {
                assert!(!public.contains(forbidden));
            }
            assert_eq!(db.table("translation_lines")?.count().await?, 0);
            Ok(())
        })
    })
    .await
}

async fn insert_collision(db: &Db, resource: &TranslationResource) -> Result<()> {
    db.table("translation_lines")?
        .insert(vec![
            (
                "identity_hash",
                identity_hash(resource, "collision")?.into(),
            ),
            ("locale", resource.locale.as_str().into()),
            ("group_name", resource.group.clone().into()),
            ("namespace", "".into()),
            ("line_key", "different-identity".into()),
            ("value_json", "\"stored\"".into()),
            ("enabled", true.into()),
        ])
        .await?;
    Ok(())
}
#[tokio::test]
async fn failed_transaction_rolls_back_lines_revision_and_never_notifies() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let count = Arc::new(AtomicUsize::new(0));
            let observed = count.clone();
            let mut provider = EventsServiceProvider::new();
            provider.listen::<TranslationSourceChanged, _>(move |_, _| {
                let observed = observed.clone();
                Box::pin(async move {
                    observed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
            });
            let mut builder = ProviderBuilder::new();
            builder.provider(provider);
            let runtime = builder.start(figment::Figment::new()).await?;
            let outcome: Result<()> = async {
                let loader = DatabaseTranslationLoader::new(db.clone())
                    .with_events((*runtime.container().resolve::<Events>().await?).clone());
                let en = resource("en", None)?;
                insert_collision(&db, &en).await?;
                assert!(
                    loader
                        .write(vec![
                            put(&en, "new", json!("must rollback")),
                            put(&en, "collision", json!("must not overwrite"))
                        ])
                        .await
                        .is_err()
                );
                assert_eq!(count.load(Ordering::SeqCst), 0);
                assert_eq!(loader.current_revision().await?, 0);
                assert_eq!(db.table("translation_lines")?.count().await?, 1);
                assert!(
                    db.table("translation_lines")?
                        .where_eq("line_key", "new")?
                        .first()
                        .await?
                        .is_none()
                );
                assert!(loader.begin_snapshot().await.is_err());
                Ok(())
            }
            .await;
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
                .await?;
            outcome
        })
    })
    .await
}

#[tokio::test]
async fn notification_observes_commit_and_failure_does_not_claim_rollback() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let count = Arc::new(AtomicUsize::new(0));
            let observed = count.clone();
            let reader = db.clone();
            let mut provider = EventsServiceProvider::new();
            provider.listen::<TranslationSourceChanged, _>(move |event, context| {
                let reader = reader.clone();
                let observed = observed.clone();
                let expected = event.revision;
                Box::pin(async move {
                    assert!(context.transaction().is_none());
                    let value = reader
                        .table("translation_revisions")?
                        .where_eq("id", 1_i32)?
                        .value::<i64>("revision")
                        .await?;
                    assert_eq!(
                        value,
                        Some(i64::try_from(expected).map_err(Error::internal)?)
                    );
                    observed.fetch_add(1, Ordering::SeqCst);
                    Err(Error::custom("notification_fixture_failed", "通知测试失败"))
                })
            });
            let mut builder = ProviderBuilder::new();
            builder.provider(provider);
            let runtime = builder.start(figment::Figment::new()).await?;
            let outcome: Result<()> = async {
                let loader = DatabaseTranslationLoader::new(db.clone())
                    .with_events((*runtime.container().resolve::<Events>().await?).clone());
                let en = resource("en", None)?;
                let result = loader
                    .write(vec![put(&en, "key", json!("committed"))])
                    .await?;
                assert_eq!(result.revision, 1);
                assert!(!result.notifications_delivered);
                assert_eq!(count.load(Ordering::SeqCst), 1);
                assert!(!loader.retry_notifications(1).await?.notifications_delivered);
                assert_eq!(count.load(Ordering::SeqCst), 2);
                assert_eq!(loader.current_revision().await?, 1);
                assert!(loader.retry_notifications(2).await.is_err());
                assert_eq!(count.load(Ordering::SeqCst), 2);
                assert_eq!(
                    loader
                        .refresh()
                        .await?
                        .load(&en.locale, "*", None)?
                        .get("key"),
                    Some(&json!("committed"))
                );
                Ok(())
            }
            .await;
            runtime
                .shutdown(tokio::time::Instant::now() + Duration::from_secs(5))
                .await?;
            outcome
        })
    })
    .await
}

#[tokio::test]
async fn database_failure_uses_bounded_retry_and_explicit_refresh_can_recover() -> Result<()> {
    owned(|db| Box::pin(async move {
        let loader = DatabaseTranslationLoader::new(db.clone());
        let en = resource("en", None)?;
        loader.write(vec![put(&en, "key", json!("verified"))]).await?;
        let verified = loader.begin_snapshot().await?;
        let table = db.quoted_table_name("translation_revisions")?;
        db.connection()?.execute_unprepared(&format!("DROP TABLE {table}")).await.map_err(Error::internal)?;
        loader.notify_revision(2);
        assert!(matches!(loader.needs_refresh().await, Err(error) if error.code() == "i18n_database_source_unavailable"));
        db.connection()?.execute_unprepared(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, revision BIGINT NOT NULL)")).await.map_err(Error::internal)?;
        db.table("translation_revisions")?.insert(vec![("id", 1_i32.into()), ("revision", 1_i64.into())]).await?;
        // 来源恢复后，间隔内仍返回上次固定故障，不在每个异步入口重试SQL。
        assert!(matches!(loader.needs_refresh_since(verified.as_ref()).await, Err(error) if error.code() == "i18n_database_source_unavailable"));
        assert!(matches!(loader.needs_refresh().await, Err(error) if error.code() == "i18n_database_source_unavailable"));
        assert_eq!(verified.load(&en.locale, "*", None)?.get("key"), Some(&json!("verified")));
        assert_eq!(loader.refresh().await?.load(&en.locale, "*", None)?.get("key"), Some(&json!("verified")));
        assert!(!loader.needs_refresh().await?);
        Ok(())
    })).await
}

#[tokio::test]
async fn migration_seed_and_identity_hash_are_unique_and_failed_duplicate_writes_preserve_data()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db.clone());
            let en = resource("en", None)?;
            loader
                .write(vec![put(&en, "retained", json!("value"))])
                .await?;
            // TestDatabase的迁移已初始化唯一修订行；通过公开查询核对schema的可观察约束。
            assert_eq!(db.table("translation_revisions")?.count().await?, 1);
            assert_eq!(loader.current_revision().await?, 1);
            assert!(
                db.table("translation_revisions")?
                    .insert(vec![("id", 1_i32.into()), ("revision", 0_i64.into())])
                    .await
                    .is_err()
            );
            let row = db
                .table("translation_lines")?
                .first()
                .await?
                .ok_or_else(database_error)?;
            let hash: String = row.try_get("", "identity_hash").map_err(Error::internal)?;
            assert_eq!(hash.len(), 64);
            assert_eq!(hash, identity_hash(&en, "retained")?);
            assert!(
                db.table("translation_lines")?
                    .insert(vec![
                        ("identity_hash", hash.into()),
                        ("locale", "en".into()),
                        ("group_name", "*".into()),
                        ("namespace", "".into()),
                        ("line_key", "other".into()),
                        ("value_json", "\"other\"".into()),
                        ("enabled", true.into())
                    ])
                    .await
                    .is_err()
            );
            assert_eq!(
                loader
                    .refresh()
                    .await?
                    .load(&en.locale, "*", None)?
                    .get("retained"),
                Some(&json!("value"))
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn raw_text_wrapper_injection_fails_refresh_without_replacing_the_published_catalog()
-> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = Arc::new(DatabaseTranslationLoader::new(db.clone()));
            let english = resource("en", None)?;
            assert_eq!(
                loader
                    .write(vec![put(&english, "key", json!("old"))])
                    .await?
                    .revision,
                1
            );
            let service =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            let retained = loader.begin_snapshot().await?;
            let before = service.refresh_status()?;
            assert_eq!(before.generation, 1);
            let invalid = "\"injected\",\"extra\":0";
            let hash = identity_hash(&english, "key")?;
            assert_eq!(
                db.table("translation_lines")?
                    .where_eq("identity_hash", hash.clone())?
                    .update(vec![("value_json", invalid.into())])
                    .await?,
                1
            );
            assert_eq!(
                db.table("translation_lines")?
                    .where_eq("identity_hash", hash.clone())?
                    .value::<String>("value_json")
                    .await?
                    .as_deref(),
                Some(invalid)
            );
            // 外部 TEXT 损坏没有推进修订或发送通知；显式刷新必须真实验证当前列值。
            assert_eq!(loader.current_revision().await?, 1);
            assert!(matches!(service.refresh().await,Err(error)
            if error.code() == "i18n_source_refresh_failed"));
            let failed = service.refresh_status()?;
            assert_eq!(failed.generation, before.generation);
            assert_eq!(failed.last_error_code, Some("i18n_source_refresh_failed"));
            assert_eq!(
                service.translate(&english.locale, "key", &MessageArgs::new()),
                "old"
            );
            assert_eq!(
                retained.load(&english.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            let state = loader.state.lock().await;
            assert_eq!(state.revision, Some(1));
            assert_eq!(state.observed_revision, Some(1));
            assert!(state.failed);
            drop(state);
            assert!(service.refresh_if_due().await.is_err());
            assert_eq!(service.refresh_status()?.generation, before.generation);
            // 同修订修复后只有成功刷新才发布下一代，失败没有消耗父发布游标。
            assert_eq!(
                db.table("translation_lines")?
                    .where_eq("identity_hash", hash)?
                    .update(vec![("value_json", "\"repaired\"".into())])
                    .await?,
                1
            );
            assert_eq!(loader.current_revision().await?, 1);
            assert_eq!(
                service.refresh().await?,
                TranslationRefreshOutcome::Updated { generation: 2 }
            );
            assert_eq!(
                service.translate(&english.locale, "key", &MessageArgs::new()),
                "repaired"
            );
            assert_eq!(
                retained.load(&english.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn migration_and_loader_share_prefix_mapping_and_repeated_up_retains_revision() -> Result<()>
{
    owned(|db| {
        Box::pin(async move {
            use openrust::database::{DatabaseTableNames, sea_orm_migration::SchemaManager};
            let names = DatabaseTableNames::new(
                "translated_",
                BTreeMap::from([(
                    "translation_lines".into(),
                    "custom_translation_lines".into(),
                )]),
            )?;
            let mapped = Db::with_table_names(names.clone());
            mapped.initialize(db.connection()?)?;
            let connection = mapped.connection()?;
            let manager = SchemaManager::new(&connection);
            names
                .scope(crate::TranslationSourceMigration.up(&manager))
                .await
                .map_err(Error::internal)?;
            let loader = DatabaseTranslationLoader::new(mapped.clone());
            let en = resource("en", None)?;
            loader
                .write(vec![put(&en, "mapped", json!("retained"))])
                .await?;
            names
                .scope(crate::TranslationSourceMigration.up(&manager))
                .await
                .map_err(Error::internal)?;
            assert_eq!(loader.current_revision().await?, 1);
            assert_eq!(mapped.table("translation_revisions")?.count().await?, 1);
            assert_eq!(
                loader
                    .refresh()
                    .await?
                    .load(&en.locale, "*", None)?
                    .get("mapped"),
                Some(&json!("retained"))
            );
            assert_eq!(db.table("translation_lines")?.count().await?, 0);
            names
                .scope(crate::TranslationSourceMigration.down(&manager))
                .await
                .map_err(Error::internal)?;
            assert_eq!(db.table("translation_revisions")?.count().await?, 1);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn two_i18n_parents_sharing_one_loader_each_publish_the_new_revision() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = Arc::new(DatabaseTranslationLoader::new(db));
            let en = resource("en", None)?;
            loader.write(vec![put(&en, "key", json!("old"))]).await?;
            let first =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            let second =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            loader.write(vec![put(&en, "key", json!("new"))]).await?;
            assert_eq!(
                first.refresh_if_due().await?,
                TranslationRefreshOutcome::Updated { generation: 2 }
            );
            assert_eq!(
                second.translate(&en.locale, "key", &MessageArgs::new()),
                "old"
            );
            // A 已推进共享来源观测值，B 仍必须相对自己的已发布 session 判断。
            assert_eq!(
                second.refresh_if_due().await?,
                TranslationRefreshOutcome::Updated { generation: 2 }
            );
            for service in [&first, &second] {
                assert_eq!(
                    service.translate(&en.locale, "key", &MessageArgs::new()),
                    "new"
                );
                assert_eq!(
                    service.refresh_if_due().await?,
                    TranslationRefreshOutcome::Unchanged { generation: 2 }
                );
            }
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn detached_refresh_and_capture_do_not_consume_an_i18n_parent_cursor() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = Arc::new(DatabaseTranslationLoader::new(db));
            let en = resource("en", None)?;
            loader.write(vec![put(&en, "key", json!("old"))]).await?;
            let service =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            loader.write(vec![put(&en, "key", json!("new"))]).await?;
            let independent = loader.as_ref().clone();
            let refreshed = independent.refresh().await?;
            let captured = independent.begin_snapshot().await?;
            assert_eq!(
                refreshed.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("new"))
            );
            assert_eq!(
                captured.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("new"))
            );
            assert_eq!(
                service.translate(&en.locale, "key", &MessageArgs::new()),
                "old"
            );
            assert_eq!(
                service.refresh_if_due().await?,
                TranslationRefreshOutcome::Updated { generation: 2 }
            );
            assert_eq!(
                service.translate(&en.locale, "key", &MessageArgs::new()),
                "new"
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn revision_probe_does_not_consume_another_published_session_progress() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db);
            let en = resource("en", None)?;
            loader.write(vec![put(&en, "key", json!("old"))]).await?;
            let first = loader.begin_snapshot().await?;
            let second = loader.begin_snapshot().await?;
            loader.write(vec![put(&en, "key", json!("new"))]).await?;
            assert!(loader.needs_refresh_since(first.as_ref()).await?);
            // 仅探测也会更新 checked/checked_hint，不能让 B 在30秒内被误判无更新。
            assert!(loader.needs_refresh_since(second.as_ref()).await?);
            assert!(loader.needs_refresh_since(first.as_ref()).await?);
            assert_eq!(
                first.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            assert_eq!(
                second.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn composite_parents_preserve_each_database_child_published_cursor() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let database = Arc::new(DatabaseTranslationLoader::new(db));
            let en = resource("en", None)?;
            database.write(vec![put(&en, "key", json!("old"))]).await?;
            let first_source = Arc::new(CompositeTranslationLoader::new(vec![database.clone()]));
            let second_source = Arc::new(CompositeTranslationLoader::new(vec![database.clone()]));
            let first =
                Translator::from_loader(TranslationConfig::default(), first_source.clone()).await?;
            let second =
                Translator::from_loader(TranslationConfig::default(), second_source).await?;
            database.write(vec![put(&en, "key", json!("new"))]).await?;
            let detached = first_source.begin_snapshot().await?;
            assert_eq!(
                detached.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("new"))
            );
            for service in [&first, &second] {
                assert_eq!(
                    service.refresh_if_due().await?,
                    TranslationRefreshOutcome::Updated { generation: 2 }
                );
                assert_eq!(
                    service.translate(&en.locale, "key", &MessageArgs::new()),
                    "new"
                );
            }
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn false_high_hints_never_publish_a_nonexistent_database_revision() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = Arc::new(DatabaseTranslationLoader::new(db));
            let en = resource("en", None)?;
            loader
                .write(vec![put(&en, "key", json!("verified"))])
                .await?;
            let first =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            let second =
                Translator::from_loader(TranslationConfig::default(), loader.clone()).await?;
            loader.notify_revision(u64::MAX);
            for service in [&first, &second] {
                assert_eq!(
                    service.refresh_if_due().await?,
                    TranslationRefreshOutcome::Unchanged { generation: 1 }
                );
                assert_eq!(
                    service.translate(&en.locale, "key", &MessageArgs::new()),
                    "verified"
                );
            }
            let state = loader.state.lock().await;
            assert_eq!(state.revision, Some(1));
            assert_eq!(state.observed_revision, Some(1));
            drop(state);
            assert_eq!(loader.current_revision().await?, 1);
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn sessions_from_other_loader_owners_or_other_types_fail_closed() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db.clone());
            let en = resource("en", None)?;
            loader
                .write(vec![put(&en, "key", json!("verified"))])
                .await?;
            let published = loader.begin_snapshot().await?;
            assert!(
                !loader
                    .clone()
                    .needs_refresh_since(published.as_ref())
                    .await?
            );
            let other = DatabaseTranslationLoader::new(db);
            assert!(
                matches!(other.needs_refresh_since(published.as_ref()).await,
            Err(error) if error.code() == "i18n_database_source_unavailable")
            );
            assert!(other.state.lock().await.checked.is_none());
            let foreign = translation_session(BTreeMap::new())?;
            assert!(matches!(loader.needs_refresh_since(foreign.as_ref()).await,
            Err(error) if error.code() == "i18n_database_source_unavailable"));
            assert_eq!(
                published.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("verified"))
            );
            Ok(())
        })
    })
    .await
}

#[tokio::test]
async fn database_revision_cannot_roll_back_below_a_successfully_observed_revision() -> Result<()> {
    owned(|db| {
        Box::pin(async move {
            let loader = DatabaseTranslationLoader::new(db.clone());
            let en = resource("en", None)?;
            loader.write(vec![put(&en, "key", json!("old"))]).await?;
            let published = loader.begin_snapshot().await?;
            loader.write(vec![put(&en, "key", json!("new"))]).await?;
            assert!(loader.needs_refresh_since(published.as_ref()).await?);
            db.table("translation_revisions")?
                .where_eq("id", 1_i32)?
                .update(vec![("revision", 1_i64.into())])
                .await?;
            assert!(matches!(loader.begin_snapshot().await,
            Err(error) if error.code() == "i18n_database_source_unavailable"));
            assert_eq!(
                published.load(&en.locale, "*", None)?.get("key"),
                Some(&json!("old"))
            );
            assert!(loader.state.lock().await.failed);
            Ok(())
        })
    })
    .await
}

#[cfg(feature = "db-mysql")]
#[tokio::test]
async fn replica_revision_lag_never_looks_like_primary_revision_rollback() -> Result<()> {
    owned(|primary| {
        Box::pin(async move {
            owned(move |stale| {
                Box::pin(async move {
                    // 两个自有临时库提供确定的旧副本路由，不冒称真实复制拓扑验收。
                    let routed = Db::default();
                    routed.initialize_with_replicas(
                        primary.connection()?,
                        vec![stale.connection()?],
                    )?;
                    let loader = DatabaseTranslationLoader::new(routed);
                    let english = resource("en", None)?;
                    loader
                        .write(vec![put(&english, "revision", json!("primary"))])
                        .await?;
                    let published = loader.begin_snapshot().await?;
                    loader.state.lock().await.checked = None;
                    assert!(!loader.needs_refresh_since(published.as_ref()).await?);
                    assert!(!loader.state.lock().await.failed);
                    assert_eq!(
                        stale
                            .table("translation_revisions")?
                            .where_eq("id", 1_i32)?
                            .value::<i64>("revision")
                            .await?,
                        Some(0)
                    );
                    Ok(())
                })
            })
            .await
        })
    })
    .await
}
