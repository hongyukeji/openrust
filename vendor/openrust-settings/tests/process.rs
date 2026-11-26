mod support;

use openrust::{config::Repository, contracts::config::Repository as _, database::DatabaseManager};
use openrust_settings::{DatabaseSettingsRepository, Settings};
use serde_json::json;
use std::{collections::BTreeMap, sync::Arc};
use support::{Fixture, Site};

#[tokio::main]
async fn main() {
    if let Ok(path) = std::env::var("SETTINGS_PROCESS_FILE") {
        let input: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let config = Repository::new(BTreeMap::from([(
            "database".into(),
            input["database"].clone(),
        )]));
        let manager = DatabaseManager::new(Arc::new(Repository::new(config.all())), ".");
        let connection = manager.connection(None).unwrap();
        let repository = Arc::new(
            DatabaseSettingsRepository::new(connection.clone())
                .with_table(input["table"].as_str().unwrap())
                .unwrap(),
        );
        let settings = Settings::<Site>::load(repository, &config, None)
            .await
            .unwrap();
        assert_eq!(
            settings.get().await.name,
            std::env::var("SETTINGS_EXPECTED_NAME").unwrap()
        );
        connection.disconnect();
        return;
    }
    let f = Fixture::new().await;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    use std::io::Write as _;
    file.write_all(
        serde_json::to_string(&json!({"database":f.config.get("database"),"table":f.table}))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    for name in ["Before restart", "After change"] {
        let settings = f.site().await;
        settings.fill(json!({"name":name})).await.unwrap();
        settings.save().await.unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .env("SETTINGS_PROCESS_FILE", file.path())
            .env("SETTINGS_EXPECTED_NAME", name)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "separate settings reader failed: {}",
            String::from_utf8_lossy(&child.stderr)
        );
    }
    f.close().await;
    println!("independent-process settings reads: passed");
}
