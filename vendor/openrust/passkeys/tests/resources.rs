use openrust::{config::Environment, testing::TestApp};
use openrust_passkeys::{PasskeysResourcesServiceProvider, PasskeysServiceProvider};
use serde_json::json;

#[path = "../config/passkeys.rs"]
mod configuration;

#[tokio::test]
async fn standalone_publishing_uses_native_tags_without_booting_authentication() {
    let mut app = TestApp::create(|builder| {
        builder.with_providers(vec![Box::new(PasskeysResourcesServiceProvider)])
    })
    .unwrap();
    for tag in ["passkeys-config", "passkeys-migrations"] {
        let result = app
            .artisan([
                "vendor:publish",
                &format!(
                    "--provider={}",
                    std::any::type_name::<PasskeysServiceProvider>()
                ),
                &format!("--tag={tag}"),
                "--no-interaction",
            ])
            .await;
        result.assert_successful();
    }
    let path = app.root().join("config/passkeys.rs");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        include_str!("../config/passkeys.rs")
    );
    std::fs::write(&path, "// application customization\n").unwrap();
    app.artisan([
        "vendor:publish",
        "--tag=passkeys-config",
        "--no-interaction",
    ])
    .await
    .assert_successful();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "// application customization\n"
    );
    let migrations = app.root().join("database/migrations");
    let count = || {
        std::fs::read_dir(&migrations)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|file| {
                file.file_name()
                    .to_string_lossy()
                    .contains("create_passkeys_table")
            })
            .count()
    };
    assert_eq!(count(), 1);
    app.artisan([
        "vendor:publish",
        "--tag=passkeys-migrations",
        "--no-interaction",
    ])
    .await
    .assert_successful();
    assert_eq!(count(), 1);
}

#[tokio::test]
async fn published_configuration_compiles_with_the_native_loader_contract() {
    let mut app = TestApp::create(|builder| builder).unwrap();
    app.boot().await.unwrap();
    let environment = app.app().make::<Environment>().await.unwrap();
    let value = configuration::load(
        &environment,
        &json!({"url":"https://example.test","key":"application-key"}),
    )
    .unwrap();
    assert_eq!(value["relying_party_id"], "example.test");
    assert_eq!(value["allowed_origins"], json!(["https://example.test"]));
    assert_eq!(value["timeout"], 60000);
    assert_eq!(value["management_middleware"], json!(["password.confirm"]));
}
