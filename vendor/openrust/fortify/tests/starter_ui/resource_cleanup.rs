use super::*;
use openrust::{
    database::eloquent::Model,
    filesystem::FilesystemManager,
    queue::QueueManager,
    support::facades::{App, Config, Facade as _},
};
use openrust_fortify::starter::resource_cleanup::{CleanupKind, ResourceCleanup, StoredResource};

async fn server() -> Server {
    start_security_catalog_mail(
        "full",
        false,
        false,
        "api",
        false,
        Some(auth_mail_queue_settings()),
    )
    .await
}
async fn schema_and_disk(server: &Server) {
    use openrust::database::migrations::{MigrationEntry, MigrationRegistry, Migrator};
    const PATH: &str =
        "database/migrations/0001_01_01_000024_create_user_resource_cleanups_table.rs";
    static ENTRIES: [MigrationEntry; 1] = [MigrationEntry::new(
        PATH,
        &resource_cleanup_migration::Definition,
    )];
    let root = &server._root.0;
    std::fs::create_dir_all(root.join("database/migrations")).unwrap();
    std::fs::write(
        root.join(PATH),
        include_str!("../../stubs/starter/migrations/resource-cleanups.rs"),
    )
    .unwrap();
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app),async{
  Config::set("filesystems",json!({"default":"public","disks":{"public":{"driver":"local","root":root.join("public-photos"),"throw":true,"url":"http://localhost/storage"}}})).unwrap();
  let manager=app.make::<openrust::database::DatabaseManager>().await.unwrap();
  let migrator=Migrator::new(manager,Arc::new(MigrationRegistry(&ENTRIES)),"cleanup_migrations".into(),root.clone());
  let paths=[root.join("database/migrations")];
  assert_eq!(migrator.run(None,&paths,Default::default()).await.unwrap().len(),1);
  assert!(migrator.run(None,&paths,Default::default()).await.unwrap().is_empty());
  assert_eq!(migrator.rollback(None,&paths,Default::default()).await.unwrap().len(),1);
  assert_eq!(migrator.run(None,&paths,Default::default()).await.unwrap().len(),1);
 }).await;
}
#[tokio::test]
async fn resource_cleanup_queue_and_ledger_rollback_together_and_committed_worker_deletes_idempotently()
 {
    let server = server().await;
    schema_and_disk(&server).await;
    register_token_user(&server).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let cleanup = app.make::<ResourceCleanup>().await.unwrap();
        let queue = app.make::<QueueManager>().await.unwrap();
        let user = app::models::User::query_on(&db)
            .first()
            .await
            .unwrap()
            .unwrap();
        let fs = app
            .make::<FilesystemManager>()
            .await
            .unwrap()
            .disk(Some("public"))
            .unwrap();
        let path = format!("profile-photos/{}.png", "a".repeat(40));
        assert!(fs.put(&path, b"owned test file").await.unwrap());
        let resources = vec![StoredResource {
            disk: "public".into(),
            path: path.clone(),
        }];
        let mut tx = db.begin_transaction().await.unwrap();
        let ticket = cleanup
            .enqueue(
                &user,
                &mut tx,
                &queue,
                CleanupKind::Avatar,
                resources.clone(),
            )
            .await
            .unwrap();
        assert_eq!(
            tx.table("user_resource_cleanups").count("*").await.unwrap(),
            1
        );
        assert_eq!(tx.table("jobs").count("*").await.unwrap(), 1);
        tx.roll_back(None).await.unwrap();
        assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
        assert!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .is_none()
        );
        assert!(fs.exists(&path).await.unwrap());
        let mut tx = db.begin_transaction().await.unwrap();
        let ticket = cleanup
            .enqueue(&user, &mut tx, &queue, CleanupKind::Avatar, resources)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        assert_eq!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .unwrap()
                .status,
            "pending"
        );
        assert!(
            cleanup
                .status(&db, &ticket.id, &"z".repeat(64))
                .await
                .unwrap()
                .is_none()
        );
        let job = db.table("jobs").first().await.unwrap().unwrap();
        let Some(openrust::database::Value::Text(raw)) = job.get("payload") else {
            panic!("missing native queue payload")
        };
        assert!(!raw.contains(ticket.receipt.as_str()));
        assert!(!raw.contains(&path));
        let payload: openrust::queue::Payload = serde_json::from_str(raw).unwrap();

        assert_eq!(worker().await, 0);
        assert!(!fs.exists(&path).await.unwrap());
        assert_eq!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .unwrap()
                .status,
            "completed"
        );
        assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
        queue
            .dispatch(
                payload,
                openrust::queue::DispatchOptions {
                    connection: Some("database".into()),
                    queue: Some("user-resources".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(worker().await, 0);
        assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
        assert_eq!(db.table("users").count("*").await.unwrap(), 1);
    })
    .await;
}
#[tokio::test]
async fn resource_cleanup_rejects_nontransactional_queue_and_unowned_paths_without_persisting_jobs()
{
    let server = server().await;
    schema_and_disk(&server).await;
    register_token_user(&server).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        Config::set("queue.connections.sync", json!({"driver":"sync"})).unwrap();
        Config::set("user.cleanup.connection", json!("sync")).unwrap();
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let cleanup = app.make::<ResourceCleanup>().await.unwrap();
        let queue = app.make::<QueueManager>().await.unwrap();
        let user = app::models::User::query_on(&db)
            .first()
            .await
            .unwrap()
            .unwrap();
        for path in [
            "../business.txt".into(),
            format!("profile-photos/{}.png", "b".repeat(40)),
        ] {
            let mut tx = db.begin_transaction().await.unwrap();
            assert!(
                cleanup
                    .enqueue(
                        &user,
                        &mut tx,
                        &queue,
                        CleanupKind::Avatar,
                        vec![StoredResource {
                            disk: "public".into(),
                            path
                        }]
                    )
                    .await
                    .is_err()
            );
            tx.roll_back(None).await.unwrap();
        }
        assert_eq!(db.table("jobs").count("*").await.unwrap(), 0);
        assert_eq!(
            db.table("user_resource_cleanups").count("*").await.unwrap(),
            0
        );
        assert_eq!(db.table("users").count("*").await.unwrap(), 1);
    })
    .await;
}

async fn worker() -> i32 {
    let mut output = openrust::console::Output::default();
    openrust::support::facades::Artisan::call(
        "queue:work",
        &[
            "database".into(),
            "--queue=user-resources".into(),
            "--once".into(),
            "--sleep=0".into(),
            "--tries=1".into(),
            "--memory=0".into(),
        ],
        &mut output,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn resource_cleanup_storage_failure_remains_trackable_and_redelivery_recovers() {
    let server = server().await;
    schema_and_disk(&server).await;
    register_token_user(&server).await;
    let app = server._kernel.get_application();
    App::set_facade_application(Some(app), async {
        let db = app
            .make::<openrust::database::DatabaseManager>()
            .await
            .unwrap()
            .connection(None)
            .unwrap();
        let cleanup = app.make::<ResourceCleanup>().await.unwrap();
        let queue = app.make::<QueueManager>().await.unwrap();
        let mut user = app::models::User::query_on(&db)
            .first()
            .await
            .unwrap()
            .unwrap();
        let disk = app
            .make::<FilesystemManager>()
            .await
            .unwrap()
            .disk(Some("public"))
            .unwrap();
        let path = format!("profile-photos/{}.webp", "c".repeat(40));
        let local = disk.path(&path).unwrap();
        std::fs::create_dir_all(&local).unwrap();
        let mut tx = db.begin_transaction().await.unwrap();
        let ticket = cleanup
            .enqueue(
                &user,
                &mut tx,
                &queue,
                CleanupKind::AccountDeletion,
                vec![StoredResource {
                    disk: "public".into(),
                    path: path.clone(),
                }],
            )
            .await
            .unwrap();
        assert!(user.delete_in_transaction(&mut tx).await.unwrap());
        tx.commit().await.unwrap();
        let row = db.table("jobs").first().await.unwrap().unwrap();
        let Some(openrust::database::Value::Text(raw)) = row.get("payload") else {
            panic!("missing native job")
        };
        let payload: openrust::queue::Payload = serde_json::from_str(raw).unwrap();
        assert!([0, 1].contains(&worker().await));
        assert_eq!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
        assert!(local.is_dir());
        assert_eq!(cleanup.prune_completed(None).await.unwrap(), 0);
        std::fs::remove_dir(&local).unwrap();
        assert!(disk.put(&path, b"retry fixture").await.unwrap());
        queue
            .dispatch(
                payload,
                openrust::queue::DispatchOptions {
                    connection: Some("database".into()),
                    queue: Some("user-resources".into()),
                    ..Default::default()
                },
                None,
            )
            .await
            .unwrap();
        assert_eq!(worker().await, 0);
        assert!(!disk.exists(&path).await.unwrap());
        assert_eq!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .unwrap()
                .status,
            "completed"
        );
        db.table("user_resource_cleanups")
            .r#where("id", "=", ticket.id.clone())
            .update([("expires_at".into(), 0.into())].into())
            .await
            .unwrap();
        assert!(
            cleanup
                .status(&db, &ticket.id, &ticket.receipt)
                .await
                .unwrap()
                .is_none()
        );
        let mut output = openrust::console::Output::default();
        assert_eq!(
            openrust::support::facades::Artisan::call("auth:prune-user-cleanups", &[], &mut output)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            db.table("user_resource_cleanups").count("*").await.unwrap(),
            0
        );
        assert_eq!(
            cleanup.prune_completed(Some("acceptance")).await.unwrap(),
            0
        );
    })
    .await;
}
