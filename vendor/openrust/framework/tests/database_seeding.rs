use std::{collections::BTreeMap, sync::Arc, time::Duration};

use openrust::{
    config::Repository,
    database::{DatabaseManager, Value},
};
use serde_json::json;

#[tokio::test]
async fn scoped_defaults_are_concurrent_and_restored_after_cancellation() {
    let settings = Repository::new(BTreeMap::from([(
        "database".into(),
        json!({"default":"first","connections":{
            "first":{"driver":"sqlite","database":":memory:"},
            "second":{"driver":"sqlite","database":":memory:"},
            "third":{"driver":"sqlite","database":":memory:"}
        }}),
    )]));
    let manager = DatabaseManager::new(Arc::new(settings), ".");
    let barrier = tokio::sync::Barrier::new(2);

    let scoped = |name: &'static str, marker: i64| {
        let manager = &manager;
        let barrier = &barrier;
        async move {
            manager
                .usingConnection(name, async {
                    barrier.wait().await;
                    let connection = manager.connection(None).unwrap();
                    assert_eq!(connection.getName(), name);
                    connection
                        .statement("create table marker (value integer)", &[])
                        .await
                        .unwrap();
                    connection
                        .insert("insert into marker values (?)", &[marker.into()])
                        .await
                        .unwrap();
                    barrier.wait().await;
                    assert_eq!(
                        manager
                            .connection(None)
                            .unwrap()
                            .table("marker")
                            .value("value")
                            .await
                            .unwrap(),
                        Value::from(marker)
                    );
                    assert!(
                        tokio::time::timeout(
                            Duration::from_millis(5),
                            manager.usingConnection("first", async {
                                assert_eq!(manager.getDefaultConnection().unwrap(), "first");
                                std::future::pending::<()>().await;
                            })
                        )
                        .await
                        .is_err()
                    );
                    assert_eq!(manager.getDefaultConnection().unwrap(), name);
                })
                .await;
        }
    };
    tokio::join!(scoped("second", 2), scoped("third", 3));
    assert_eq!(manager.getDefaultConnection().unwrap(), "first");
    assert!(
        !manager
            .connection(None)
            .unwrap()
            .getSchemaBuilder()
            .hasTable("marker")
            .await
            .unwrap()
    );
}
