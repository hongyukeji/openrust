use openrust::foundation::Application;
use openrust_pennant::{
    FeatureDefinition, FeatureFuture, FeatureManager, PennantServiceProvider, Scope,
};
use serde_json::{Value, json};
use std::sync::Arc;
struct NewApi;
impl FeatureDefinition for NewApi {
    fn name() -> &'static str {
        "new-api"
    }
    fn resolve<'a>(&'a self, _: &'a Scope) -> FeatureFuture<'a, Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let directory = tempfile::tempdir()?;
    let mut app = Application::configure(directory.path().to_owned())
        .withConfig(|_, _| Ok([("pennant".into(), json!({"default":"array"}))].into()))
        .withProviders(vec![Box::new(PennantServiceProvider)])
        .create()?;
    app.boot().await?;
    let store = app.make::<FeatureManager>().await?.store(None)?;
    store.defineValue("new-api", json!(true))?;
    store.define(
        "theme",
        Arc::new(|scope| {
            Box::pin(async move {
                Ok(json!(if scope == Scope::Null {
                    "default"
                } else {
                    "blue"
                }))
            })
        }),
    )?;
    let user = store.r#for("user-42");
    assert!(user.active("new-api").await?);
    assert_eq!(user.value("theme").await?, json!("blue"));
    user.loadMissing(&["new-api", "theme"]).await?;
    assert!(user.allAreActive(&["new-api", "theme"]).await?);
    store.defineClass(|| Box::pin(async { Ok(Arc::new(NewApi)) }))?;
    user.activateWithValue(&["theme"], json!("green")).await?;
    user.deactivate("new-api").await?;
    user.forget(&["theme"]).await?;
    store.activateForEveryone("new-api", json!(true)).await?;
    Ok(())
}
