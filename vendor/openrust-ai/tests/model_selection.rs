use std::{error::Error, fs};

use openrust::{config::EnvironmentFile, testing::TestApp};

#[tokio::test]
async fn selection_only_writes_when_requested_and_preserves_other_entries()
-> Result<(), Box<dyn Error + Send + Sync>> {
    let mut app =
        TestApp::create(|builder| builder.with_providers(openrust_ai::openrust_providers()))?;
    let path = app.root().join(".env.testing");
    let original = "# existing comments\nDEEPSEEK_MODEL=previous\n";
    fs::write(&path, original)?;
    app.artisan([
        "ai:select-model",
        "deepseek",
        "--model=MixedCase/Model",
        "--no-interaction",
    ])
    .await
    .assert_successful()
    .expects_output("MixedCase/Model");
    assert_eq!(fs::read_to_string(&path)?, original);
    app.artisan(["ai:select-model", "deepseek", "--no-interaction"])
        .await
        .assert_exit_code(1);
    assert_eq!(fs::read_to_string(&path)?, original);
    app.artisan([
        "ai:select-model",
        "deepseek",
        "--model=MixedCase/Model",
        "--save",
        "--default",
        "--no-interaction",
    ])
    .await
    .assert_successful();
    let file = EnvironmentFile::open(&path)?;
    assert_eq!(
        file.get("DEEPSEEK_MODEL")?.as_deref(),
        Some("MixedCase/Model")
    );
    assert_eq!(file.get("AI_PROVIDER")?.as_deref(), Some("deepseek"));
    assert!(fs::read_to_string(&path)?.starts_with("# existing comments\n"));
    Ok(())
}
