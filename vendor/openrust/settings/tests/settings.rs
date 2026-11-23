mod support;
use openrust::{
    database::{Value, migrations::RunOptions},
    encryption::{Cipher, Encrypter},
};
use openrust_settings::{
    DatabaseSettingsRepository, Settings, SettingsError as Error, SettingsRepository,
    SettingsUpdate,
};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use support::*;

#[tokio::test]
async fn defaults_false_zero_null_empty_and_reset_are_distinct() {
    let f = Fixture::new().await;
    let s = f.site().await;
    assert_eq!(s.get().await.name, "Default");
    assert_eq!(f.connection.table(&f.table).count("*").await.unwrap(), 0);
    s.fill(json!({"name":"","enabled":false,"count":0,"description":null}))
        .await
        .unwrap();
    s.save().await.unwrap();
    let r = f.site().await.get().await;
    assert_eq!(r.name, "");
    assert!(!r.enabled);
    assert_eq!(r.count, 0);
    assert!(r.description.is_none());
    s.reset("name").await.unwrap();
    s.save().await.unwrap();
    assert_eq!(f.site().await.get().await.name, "Default");
    assert!(
        !f.repository
            .load("site")
            .await
            .unwrap()
            .properties
            .contains_key("name")
    );
    assert!(
        f.migrator
            .run(None, &[], RunOptions::default())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(!f.site().await.get().await.enabled);
    f.close().await;
}

#[tokio::test]
async fn invalid_edits_are_atomic_and_do_not_leak_values() {
    let f = Fixture::new().await;
    let s = f.site().await;
    for edit in [
        json!({"name":"uncommitted","count":"secret-invalid-type"}),
        json!({"name":"uncommitted","count":101}),
        json!({"unknown":"secret"}),
    ] {
        let error = s.fill(edit).await.unwrap_err();
        assert!(!format!("{error:?} {error}").contains("secret"));
        assert_eq!(s.get().await.name, "Default");
    }
    s.save().await.unwrap();
    assert_eq!(f.connection.table(&f.table).count("*").await.unwrap(), 0);
    assert!(!format!("{s:?}").contains("Default"));
    f.close().await;
}

#[tokio::test]
async fn stale_edits_conflict_on_initial_create_existing_and_reset() {
    let f = Fixture::new().await;
    let a = f.site().await;
    let b = f.site().await;
    a.fill(json!({"name":"A"})).await.unwrap();
    a.save().await.unwrap();
    b.fill(json!({"enabled":false})).await.unwrap();
    assert_eq!(b.save().await, Err(Error::Conflict));
    assert_eq!(f.site().await.get().await.name, "A");
    b.refresh().await.unwrap();
    a.reset("name").await.unwrap();
    a.save().await.unwrap();
    b.fill(json!({"name":"stale"})).await.unwrap();
    assert_eq!(b.save().await, Err(Error::Conflict));
    b.refresh().await.unwrap();
    assert_eq!(b.get().await.name, "Default");
    f.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_writers_have_exactly_one_winner() {
    let f = Fixture::new().await;
    for _ in 0..4 {
        let a = f.site().await;
        let b = f.site().await;
        a.fill(json!({"name":"A"})).await.unwrap();
        b.fill(json!({"name":"B"})).await.unwrap();
        let (a, b) = tokio::join!(a.save(), b.save());
        assert!(
            matches!(
                (a, b),
                (Ok(()), Err(Error::Conflict)) | (Err(Error::Conflict), Ok(()))
            ),
            "exactly one stale editor must be rejected: {a:?}, {b:?}"
        );
        assert!(matches!(
            f.site().await.get().await.name.as_str(),
            "A" | "B"
        ));
    }
    f.close().await;
}

#[tokio::test]
async fn locked_values_reject_changes_and_unlock_is_explicit() {
    let f = Fixture::new().await;
    let s = f.site().await;
    s.fill(json!({"name":"Protected"})).await.unwrap();
    s.save().await.unwrap();
    let stale = f.site().await;
    s.lock("name").await.unwrap();
    assert!(s.is_locked("name").await.unwrap());
    assert_eq!(
        s.fill(json!({"name":"replacement"})).await,
        Err(Error::Locked)
    );
    assert_eq!(s.reset("name").await, Err(Error::Locked));
    stale.fill(json!({"name":"stale"})).await.unwrap();
    assert_eq!(stale.save().await, Err(Error::Conflict));
    s.unlock("name").await.unwrap();
    s.reset("name").await.unwrap();
    s.save().await.unwrap();
    assert_eq!(f.site().await.get().await.name, "Default");
    f.close().await;
}

#[tokio::test]
async fn nested_transaction_rollback_and_failed_savepoints_publish_nothing() {
    let f = Fixture::new().await;
    let s = f.site().await;
    s.fill(json!({"name":"Outer edit"})).await.unwrap();
    let mut tx = f.connection.begin_transaction().await.unwrap();
    s.saveIn(&mut tx).await.unwrap();
    assert_eq!(tx.transaction_level(), 1);
    tx.roll_back(None).await.unwrap();
    assert_eq!(f.site().await.get().await.name, "Default");
    s.save().await.unwrap();
    assert_eq!(f.site().await.get().await.name, "Outer edit");
    s.fill(json!({"enabled":false})).await.unwrap();
    let mut tx = f.connection.begin_transaction().await.unwrap();
    s.saveIn(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    s.refresh().await.unwrap();
    assert!(!s.get().await.enabled);
    let mut tx = f.connection.begin_transaction().await.unwrap();
    let raw = f.repository.load("absent_group").await.unwrap();
    let bad = BTreeMap::from([
        ("a".into(), SettingsUpdate::Set(json!(1))),
        ("z".into(), SettingsUpdate::Lock),
    ]);
    assert_eq!(
        f.repository
            .saveIn(&mut tx, "absent_group", &raw, &bad)
            .await
            .unwrap_err(),
        Error::UnknownProperty
    );
    tx.commit().await.unwrap();
    assert!(
        f.repository
            .load("absent_group")
            .await
            .unwrap()
            .version()
            .is_none()
    );
    f.close().await;
}

#[tokio::test]
async fn encrypted_objects_rotate_restore_and_fail_without_keys() {
    let f = Fixture::new().await;
    let old = Arc::new(Encrypter::new([1; 32], Cipher::Aes256Cbc).unwrap());
    let s = Settings::<Secrets>::load(f.repository.clone(), &f.config, Some(old.clone()))
        .await
        .unwrap();
    s.fill(json!({"credentials":{"key":"private-token","nested":{"password":"secret"}}}))
        .await
        .unwrap();
    s.save().await.unwrap();
    let raw = f.repository.load("module_delivery").await.unwrap();
    assert!(
        !serde_json::to_string(&raw.properties["credentials"].payload)
            .unwrap()
            .contains("private-token")
    );
    assert!(!format!("{raw:?}").contains("private-token"));
    let next = Arc::new(
        Encrypter::new([2; 32], Cipher::Aes256Cbc)
            .unwrap()
            .previousKeys(vec![vec![1; 32]])
            .unwrap(),
    );
    let rotated = Settings::<Secrets>::load(f.repository.clone(), &f.config, Some(next))
        .await
        .unwrap();
    assert_eq!(rotated.get().await.credentials["key"], "private-token");
    rotated.reEncrypt().await.unwrap();
    let current = Arc::new(Encrypter::new([2; 32], Cipher::Aes256Cbc).unwrap());
    let restored = Settings::<Secrets>::load(f.repository.clone(), &f.config, Some(current))
        .await
        .unwrap();
    assert_eq!(
        restored.get().await.credentials["nested"]["password"],
        "secret"
    );
    assert_eq!(
        Settings::<Secrets>::load(f.repository.clone(), &f.config, Some(old))
            .await
            .unwrap_err(),
        Error::Decryption
    );
    // A malformed encrypted override must fail instead of reverting to defaults.
    f.connection
        .table(&f.table)
        .r#where("group", "=", "module_delivery")
        .r#where("name", "=", "credentials")
        .update(BTreeMap::from([(
            "payload".into(),
            Value::Json(json!("broken")),
        )]))
        .await
        .unwrap();
    let key = Arc::new(Encrypter::new([2; 32], Cipher::Aes256Cbc).unwrap());
    assert_eq!(
        Settings::<Secrets>::load(f.repository.clone(), &f.config, Some(key))
            .await
            .unwrap_err(),
        Error::Decryption
    );
    assert_eq!(restored.refresh().await, Err(Error::Decryption));
    assert_eq!(restored.get().await.credentials["key"], "private-token");
    f.close().await;
}

#[tokio::test]
async fn unknown_future_properties_are_preserved_and_groups_are_isolated() {
    let f = Fixture::new().await;
    let empty = f.repository.load("site").await.unwrap();
    let changes = BTreeMap::from([(
        "future_field".into(),
        SettingsUpdate::Set(json!({"option":true})),
    )]);
    f.repository.save("site", &empty, &changes).await.unwrap();
    let site = f.site().await;
    site.fill(json!({"name":"Existing application"}))
        .await
        .unwrap();
    site.save().await.unwrap();
    assert_eq!(
        f.repository.load("site").await.unwrap().properties["future_field"].payload,
        json!({"option":true})
    );
    assert!(
        f.repository
            .load("module_delivery")
            .await
            .unwrap()
            .properties
            .is_empty()
    );
    let wrong = DatabaseSettingsRepository::new(f.connection.clone())
        .withTable("missing_settings")
        .unwrap();
    assert_eq!(wrong.load("site").await.unwrap_err(), Error::Database);
    assert!(
        DatabaseSettingsRepository::new(f.connection.clone())
            .withTable("settings;drop table users")
            .is_err()
    );
    assert_eq!(
        f.repository.load("../group").await.unwrap_err(),
        Error::InvalidDefinition
    );
    f.close().await;
}

#[tokio::test]
async fn oversized_or_reserved_properties_do_not_create_partial_groups() {
    let f = Fixture::new().await;
    let snapshot = f.repository.load("bounded").await.unwrap();
    let changes = BTreeMap::from([
        ("a".into(), SettingsUpdate::Set(json!("a".repeat(600_000)))),
        ("b".into(), SettingsUpdate::Set(json!("b".repeat(600_000)))),
    ]);
    assert_eq!(
        f.repository
            .save("bounded", &snapshot, &changes)
            .await
            .unwrap_err(),
        Error::LimitExceeded
    );
    assert_eq!(f.repository.load("bounded").await.unwrap(), snapshot);
    for name in [
        "__settings_version",
        "__SETTINGS_VERSION",
        "__Settings_private",
    ] {
        let changes = BTreeMap::from([(name.into(), SettingsUpdate::Reset)]);
        assert_eq!(
            f.repository
                .save("bounded", &snapshot, &changes)
                .await
                .unwrap_err(),
            Error::InvalidDefinition
        );
    }
    let changes = BTreeMap::from([
        ("key".into(), SettingsUpdate::Set(json!(1))),
        ("Key".into(), SettingsUpdate::Set(json!(2))),
    ]);
    assert_eq!(
        f.repository
            .save("bounded", &snapshot, &changes)
            .await
            .unwrap_err(),
        Error::InvalidDefinition
    );
    assert_eq!(f.repository.load("bounded").await.unwrap(), snapshot);
    f.close().await;
}
