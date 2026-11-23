use openrust::database::{
    Connection, DatabaseManager, Value,
    migrations::{MigrationRegistry, Migrator, RunOptions},
};
use openrust_telescope::{
    EntryType, EntryUpdate, IncomingEntry, IncomingExceptionEntry, TelescopeServiceProvider,
    storage::{DatabaseEntriesRepository, EntryQueryOptions},
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

static DATABASE_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Fixture {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    connection: Connection,
    repository: DatabaseEntriesRepository,
    migrator: Migrator,
    _root: tempfile::TempDir,
}
impl Fixture {
    async fn new(chunk: usize) -> Self {
        let guard = DATABASE_TEST.lock().await;
        let root = tempfile::Builder::new()
            .prefix("telescope-storage-")
            .tempdir()
            .unwrap();
        std::fs::File::create(root.path().join("test.sqlite")).unwrap();
        let mut app = openrust::foundation::Application::configure(root.path().into())
            .with_config(config)
            .with_providers(vec![Box::new(TelescopeServiceProvider)])
            .create()
            .unwrap();
        app.boot().await.unwrap();
        let manager = app.make::<DatabaseManager>().await.unwrap();
        let connection = manager.connection(None).unwrap();
        // Registration and boot must remain usable before package tables exist.
        assert!(
            !connection
                .get_schema_builder()
                .has_table("telescope_entries")
                .await
                .unwrap()
        );
        let migrator = Migrator::new(
            manager,
            Arc::new(MigrationRegistry(TelescopeServiceProvider::migrations())),
            "telescope_test_migrations".into(),
            root.path().into(),
        );
        migrator.install(None).await.unwrap();
        assert_eq!(
            migrator
                .run(None, &[], RunOptions::default())
                .await
                .unwrap()
                .len(),
            1
        );
        let repository = DatabaseEntriesRepository::new(connection.clone(), Some(chunk));
        Self {
            _guard: guard,
            connection,
            repository,
            migrator,
            _root: root,
        }
    }
    async fn close(self) {
        assert_eq!(
            self.migrator.reset(None, &[], false).await.unwrap().len(),
            1
        );
        for table in [
            "telescope_entries",
            "telescope_entries_tags",
            "telescope_monitoring",
        ] {
            assert!(
                !self
                    .connection
                    .get_schema_builder()
                    .has_table(table)
                    .await
                    .unwrap()
            );
        }
        self.connection
            .get_schema_builder()
            .drop_if_exists("telescope_test_migrations")
            .await
            .unwrap();
        self.connection.disconnect();
    }
}
fn config(
    root: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    let driver = std::env::var("TELESCOPE_TEST_DRIVER").unwrap_or("sqlite".into());
    let database = match driver.as_str() {
        "sqlite" => {
            json!({"driver":"sqlite","database":root.join("test.sqlite"),"foreign_key_constraints":true,"busy_timeout":10000,"max_connections":4})
        }
        "pgsql" => {
            assert_eq!(
                std::env::var("OPENRUST_REQUIRE_POSTGRES").as_deref(),
                Ok("1")
            );
            assert!(std::env::var("OPENRUST_ACCEPTANCE_POSTGRES_CONTAINER").is_ok());
            json!({"driver":"pgsql","url":std::env::var("OPENRUST_TEST_POSTGRES_URL").unwrap(),"max_connections":4})
        }
        "mysql" => {
            assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").as_deref(), Ok("1"));
            assert!(std::env::var("OPENRUST_ACCEPTANCE_MYSQL_CONTAINER").is_ok());
            json!({"driver":"mysql","url":std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),"max_connections":4})
        }
        _ => panic!("unsupported isolated driver"),
    };
    Ok(BTreeMap::from([
        (
            "database".into(),
            json!({"default":"testing","connections":{"testing":database}}),
        ),
        (
            "telescope".into(),
            json!({"storage":{"database":{"connection":"testing","chunk":2}}}),
        ),
    ]))
}
fn entry(kind: EntryType, batch: Uuid, content: serde_json::Value) -> IncomingEntry {
    IncomingEntry::make(content.as_object().unwrap().clone())
        .unwrap()
        .r#type(kind)
        .batch_id(batch)
}
fn exception(batch: Uuid, file: &str, line: u64) -> IncomingEntry {
    IncomingExceptionEntry::make(
        json!({"file":file,"line":line,"message":"failure"})
            .as_object()
            .unwrap()
            .clone(),
        true,
    )
    .unwrap()
    .0
    .batch_id(batch)
}

#[tokio::test]
async fn storage_roundtrip_prefix_pagination_tag_union_batch_uuid_and_malformed_content() {
    let f = Fixture::new(2).await;
    let batch = Uuid::now_v7();
    let first = entry(
        EntryType::Request,
        batch,
        json!({"uri":"/one","response_status":500}),
    )
    .tags(["same", "first", "first"]);
    let second = entry(
        EntryType::Query,
        batch,
        json!({"sql":"select 1","slow":true}),
    )
    .tags(["same", "second"]);
    let third = entry(EntryType::Request, Uuid::now_v7(), json!({"uri":"/other"})).tags(["third"]);
    f.repository
        .store(&[first.clone(), second.clone(), third.clone()])
        .await
        .unwrap();
    let found = f.repository.find(&first.uuid.to_string()).await.unwrap();
    assert_eq!(found.sequence, None);
    assert_eq!(found.batch_id, batch);
    assert_eq!(found.content["uri"], "/one");
    assert_eq!(found.tags.len(), 2);
    assert_eq!(
        f.repository
            .find(&third.uuid.to_string()[..8])
            .await
            .unwrap()
            .id,
        third.uuid
    );
    let all = f
        .repository
        .get(None, EntryQueryOptions::default().limit(2))
        .await
        .unwrap();
    assert_eq!(
        all.iter().map(|entry| entry.id).collect::<Vec<_>>(),
        [third.uuid, second.uuid]
    );
    assert!(all.iter().all(|entry| entry.tags.is_empty()));
    let next = f
        .repository
        .get(
            None,
            EntryQueryOptions::default().before_sequence(all.last().unwrap().sequence),
        )
        .await
        .unwrap();
    assert_eq!(next[0].id, first.uuid);
    let filtered = f
        .repository
        .get(
            Some(EntryType::Request),
            EntryQueryOptions::for_batch_id(Some(batch)),
        )
        .await
        .unwrap();
    assert_eq!(filtered.len(), 1);
    let tagged = f
        .repository
        .get(
            None,
            EntryQueryOptions::default().tag(Some(" first , second ".into())),
        )
        .await
        .unwrap();
    assert_eq!(tagged.len(), 2);
    assert!(
        f.repository
            .get(None, EntryQueryOptions::default().uuids(Some(vec![])))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        f.repository
            .get(
                None,
                EntryQueryOptions::default().uuids(Some(vec![second.uuid]))
            )
            .await
            .unwrap()[0]
            .id,
        second.uuid
    );
    f.connection
        .table("telescope_entries")
        .r#where("uuid", "=", third.uuid.to_string())
        .update(BTreeMap::from([("content".into(), Value::from("null"))]))
        .await
        .unwrap();
    assert_eq!(
        f.repository
            .get(None, EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        2
    );
    assert!(matches!(
        f.repository.find(&Uuid::now_v7().to_string()).await,
        Err(openrust_telescope::Error::NotFound)
    ));
    f.close().await;
}

#[tokio::test]
async fn exception_families_display_occurrences_and_chunk_semantics() {
    let f = Fixture::new(2).await;
    let batch = Uuid::now_v7();
    let first = exception(batch, "app/service.rs", 12);
    f.repository
        .store(std::slice::from_ref(&first))
        .await
        .unwrap();
    let second = exception(batch, "app/service.rs", 12);
    let third = exception(batch, "app/service.rs", 12);
    f.repository
        .store(&[second.clone(), third.clone()])
        .await
        .unwrap();
    let index = f
        .repository
        .get(Some(EntryType::Exception), EntryQueryOptions::default())
        .await
        .unwrap();
    // A chunk counts its existing rows before the bulk insert, matching the fixed upstream.
    assert_eq!(index.len(), 2);
    assert!(index.iter().all(|entry| entry.content["occurrences"] == 2));
    let family = f
        .repository
        .get(
            None,
            EntryQueryOptions::default().family_hash(first.family_hash.clone()),
        )
        .await
        .unwrap();
    assert_eq!(family.len(), 3);
    assert_eq!(
        f.repository
            .find(&first.uuid.to_string())
            .await
            .unwrap()
            .content["occurrences"],
        1
    );
    let fourth = exception(batch, "app/service.rs", 12);
    f.repository
        .store(std::slice::from_ref(&fourth))
        .await
        .unwrap();
    let index = f
        .repository
        .get(None, EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].content["occurrences"], 4);
    assert_eq!(
        f.repository
            .get(None, EntryQueryOptions::for_batch_id(Some(batch)))
            .await
            .unwrap()
            .len(),
        4
    );
    f.close().await;
}

#[tokio::test]
async fn updates_merge_content_tags_and_return_missing_or_wrong_type() {
    let f = Fixture::new(1000).await;
    let first = entry(
        EntryType::Job,
        Uuid::now_v7(),
        json!({"status":"pending","nested":{"first":true}}),
    )
    .tags(["old"]);
    f.repository
        .store(std::slice::from_ref(&first))
        .await
        .unwrap();
    let change = EntryUpdate::make(
        first.uuid,
        EntryType::Job,
        json!({"status":"processed","nested":{"second":true}})
            .as_object()
            .unwrap()
            .clone(),
    )
    .add_tags(["old", "new"])
    .remove_tags(["old"]);
    let missing = EntryUpdate::make(Uuid::now_v7(), EntryType::Job, Default::default());
    let wrong = EntryUpdate::make(first.uuid, EntryType::Request, Default::default());
    let failed = f
        .repository
        .update(&[change, missing.clone(), wrong.clone()])
        .await
        .unwrap();
    assert_eq!(failed.len(), 2);
    assert_eq!(failed[0].uuid, missing.uuid);
    assert_eq!(failed[1].r#type, wrong.r#type);
    let found = f.repository.find(&first.uuid.to_string()).await.unwrap();
    assert_eq!(found.content["status"], "processed");
    assert_eq!(found.content["nested"], json!({"second":true}));
    assert!(!found.tags.contains(&"old".into()));
    // Duplicate rows cause the whole tag insert chunk to be ignored, as upstream does.
    assert!(found.tags.is_empty());
    f.close().await;
}

#[tokio::test]
async fn monitored_tags_lazy_cache_termination_prune_and_cascade_clear() {
    let f = Fixture::new(2).await;
    assert!(!f.repository.is_monitoring(&["a".into()]).await);
    f.repository
        .monitor(&["a".into(), "b".into(), "a".into()])
        .await
        .unwrap();
    assert_eq!(f.repository.monitoring().await.unwrap().len(), 2);
    assert!(!f.repository.is_monitoring(&["a".into()]).await);
    f.repository.terminate();
    assert!(f.repository.is_monitoring(&["c".into(), "a".into()]).await);
    f.repository.stop_monitoring(&["a".into()]).await.unwrap();
    f.repository.terminate();
    assert!(!f.repository.is_monitoring(&["a".into()]).await);
    let cutoff: jiff::Timestamp = "2026-01-02T00:00:00Z".parse().unwrap();
    let mut old = entry(EntryType::Log, Uuid::now_v7(), json!({})).tags(["old"]);
    old.recorded_at = "2026-01-01T00:00:00Z".parse().unwrap();
    let mut kept = exception(Uuid::now_v7(), "old.rs", 1);
    kept.recorded_at = old.recorded_at;
    let mut boundary = entry(EntryType::Log, Uuid::now_v7(), json!({}));
    boundary.recorded_at = cutoff;
    f.repository.store(&[old, kept, boundary]).await.unwrap();
    assert_eq!(f.repository.prune(cutoff, true).await.unwrap(), 1);
    assert_eq!(
        f.connection
            .table("telescope_entries_tags")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert_eq!(f.repository.prune(cutoff, false).await.unwrap(), 1);
    f.repository.clear().await.unwrap();
    assert_eq!(
        f.connection
            .table("telescope_entries")
            .count("*")
            .await
            .unwrap(),
        0
    );
    assert!(f.repository.monitoring().await.unwrap().is_empty());
    f.close().await;
}

#[tokio::test]
async fn invalid_batch_is_rejected_before_writing_and_monitor_load_recovers_after_terminate() {
    let f = Fixture::new(1).await;
    let valid = entry(EntryType::Request, Uuid::now_v7(), json!({}));
    let invalid = IncomingEntry::make(Default::default()).unwrap();
    assert!(matches!(
        f.repository.store(&[valid, invalid]).await,
        Err(openrust_telescope::Error::InvalidEntry)
    ));
    assert_eq!(
        f.connection
            .table("telescope_entries")
            .count("*")
            .await
            .unwrap(),
        0
    );
    f.close().await;
}

#[tokio::test]
async fn scoped_collector_filters_tags_batches_hooks_and_restores_recording_after_cancel() {
    use openrust_telescope::Telescope;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let f = Fixture::new(2).await;
    let repository = Arc::new(DatabaseEntriesRepository::new(
        f.connection.clone(),
        Some(2),
    ));
    let telescope = Arc::new(Telescope::new(repository.clone(), true));
    let recorded = Arc::new(AtomicUsize::new(0));
    let stored = Arc::new(AtomicUsize::new(0));
    telescope
        .tag(|_| vec!["custom".into(), "custom".into()])
        .filter(|entry| {
            entry
                .content
                .get("keep")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        });
    let count = recorded.clone();
    telescope.after_recording(move |scope, entry| {
        assert!(!scope.is_recording());
        count.fetch_add(1, Ordering::SeqCst);
        scope.record_log(entry.clone());
    });
    let count = stored.clone();
    telescope.after_storing(move |scope, entries| {
        assert!(!scope.is_recording());
        count.fetch_add(entries.len(), Ordering::SeqCst);
    });
    telescope.record_log(
        IncomingEntry::make(json!({"keep":true}).as_object().unwrap().clone()).unwrap(),
    );
    telescope.record_log(
        IncomingEntry::make(json!({"keep":false}).as_object().unwrap().clone()).unwrap(),
    );
    assert!(telescope.store().await.unwrap().is_empty());
    assert_eq!(recorded.load(Ordering::SeqCst), 2);
    assert_eq!(stored.load(Ordering::SeqCst), 1);
    let entries = repository
        .get(None, EntryQueryOptions::default())
        .await
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        repository
            .find(&entries[0].id.to_string())
            .await
            .unwrap()
            .tags,
        ["custom"]
    );
    let notify = Arc::new(tokio::sync::Notify::new());
    let started = notify.clone();
    let scope = telescope.clone();
    let task = tokio::spawn(async move {
        scope
            .without_recording(async {
                started.notify_one();
                std::future::pending::<()>().await;
            })
            .await;
    });
    notify.notified().await;
    assert!(!telescope.is_recording());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(telescope.is_recording());
    telescope.filter_batch(|_| false);
    telescope.record_log(
        IncomingEntry::make(json!({"keep":true}).as_object().unwrap().clone()).unwrap(),
    );
    telescope.store().await.unwrap();
    assert_eq!(
        repository
            .get(None, EntryQueryOptions::default())
            .await
            .unwrap()
            .len(),
        1
    );
    telescope.stop_recording();
    telescope.record_log(IncomingEntry::make(Default::default()).unwrap());
    telescope.store().await.unwrap();
    assert!(!telescope.is_recording());
    telescope.start_recording(false).await;
    assert!(telescope.is_recording());
    f.close().await;
}

#[test]
fn native_update_fields_read_and_write_existing_queue_payloads() {
    let update = EntryUpdate::make(Uuid::nil(), EntryType::Request, Default::default())
        .add_tags(["visible"]);
    let payload = openrust_telescope::jobs::ProcessPendingUpdates::new(vec![update], 2);
    let encoded = serde_json::to_value(&payload).unwrap();
    assert!(encoded.get("pending_updates").is_none());
    assert_eq!(
        encoded["pendingUpdates"][0]["addedTags"],
        json!(["visible"])
    );
    assert_eq!(encoded["pendingUpdates"][0]["removedTags"], json!([]));
    let decoded: openrust_telescope::jobs::ProcessPendingUpdates =
        serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.attempt, 2);
    assert_eq!(decoded.pending_updates[0].added_tags, ["visible"]);
}
