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
        .with_config(|_, _| Ok([("pennant".into(), json!({"default":"array"}))].into()))
        .with_providers(vec![Box::new(PennantServiceProvider)])
        .create()?;
    app.boot().await?;
    let store = app.make::<FeatureManager>().await?.store(None)?;
    store.define_value("new-api", json!(true))?;
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
    user.load_missing(&["new-api", "theme"]).await?;
    assert!(user.all_are_active(&["new-api", "theme"]).await?);
    store.define_class(|| Box::pin(async { Ok(Arc::new(NewApi)) }))?;
    user.activate_with_value(&["theme"], json!("green")).await?;
    user.deactivate("new-api").await?;
    user.forget(&["theme"]).await?;
    store.activate_for_everyone("new-api", json!(true)).await?;
    Ok(())
}
