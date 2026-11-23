use openrust::{
    contracts::{container::Container as _, foundation::Application as _, http::Kernel as _},
    database::{
        DatabaseManager,
        migrations::{Migrator, RunOptions},
    },
    foundation::{Application, DiscoveredPackage, PackageManifest, http::Kernel},
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use serde_json::json;

struct Host {
    database: std::path::PathBuf,
    table: &'static str,
    connection: Option<&'static str>,
}
impl ServiceProvider for Host {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            let config = app.config()?;
            config.set("database", json!({"default":"sqlite","connections":{"sqlite":{"driver":"sqlite","database":self.database}},"migrations":{"table":"migrations"}}))?;
            config.set("settings.repositories.database.table", json!(self.table))?;
            config.set(
                "settings.repositories.database.connection",
                json!(self.connection),
            )?;
            Ok(())
        })
    }
}

static PACKAGES: &[DiscoveredPackage] = &[DiscoveredPackage::new(
    "openrust-settings",
    "0.0.0",
    "openrust_settings",
    &[],
    openrust_settings::openrust_providers,
)];

#[tokio::test]
async fn discovered_provider_boots_without_database_and_owns_default_migration() {
    for (table, connection, expected) in [
        ("settings", None, 1),
        ("custom_settings", None, 0),
        ("settings", Some("other"), 0),
    ] {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("uncreated.sqlite");
        let application = Application::configure(root.path().to_path_buf())
            .with_packages(PackageManifest::new(PACKAGES))
            .with_providers(vec![Box::new(Host {
                database: database.clone(),
                table,
                connection,
            })])
            .create()
            .unwrap();
        let mut kernel = Kernel::new(application);
        kernel.bootstrap().await.unwrap();
        assert!(
            !database.exists(),
            "registration and boot must not query or create the database"
        );
        std::fs::File::create(&database).unwrap();
        let app = kernel.get_application();
        let migrations = app.make::<Migrator>().await.unwrap();
        let results = migrations
            .run(None, &[], RunOptions::default())
            .await
            .unwrap();
        assert_eq!(results.len(), expected);
        let database = app.container().make::<DatabaseManager>().unwrap();
        let mut schema = database.connection(None).unwrap().get_schema_builder();
        assert_eq!(schema.has_table("settings").await.unwrap(), expected == 1);
        assert!(
            migrations
                .run(None, &[], RunOptions::default())
                .await
                .unwrap()
                .is_empty()
        );
    }
}
