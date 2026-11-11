use std::path::PathBuf;

use openrust::{
    console::ConsoleError,
    database::{
        DatabaseFuture, DatabaseResult,
        migrations::{Migration, MigrationContext, MigrationEntry},
    },
    foundation::Application,
    http::{HttpError, HttpFuture, Request, Response, StatusCode},
    routing::RoutingError,
    support::{
        ProviderResult, ServiceProvider,
        facades::{Artisan, Route},
    },
    translation::__,
    view::view,
};
use serde_json::json;

pub struct ProbeProvider(pub PathBuf);

impl ServiceProvider for ProbeProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            self.mergeConfigFrom(
                app,
                json!({"enabled": true, "options": {"package": 1}}),
                "probe.shallow",
            )?;
            self.replaceConfigRecursivelyFrom(
                app,
                json!({"options": {"package": 1}, "indices": ["first", "second"]}),
                "probe.recursive",
            )?;
            // Resolve an untranslated key before boot to exercise namespace cache invalidation.
            let translator = app.make::<openrust::translation::Translator>().await?;
            let key = "probe::messages.title";
            assert_eq!(translator.string(key, &Default::default(), None)?, key);
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            self.loadRoutesFrom(app, routes)?;
            self.loadViewsFrom(app, &self.0.join("views"), "probe")
                .await?;
            self.loadTranslationsFrom(app, &self.0.join("lang"), "probe")
                .await?;
            self.loadMigrationsFrom(app, MIGRATIONS).await?;
            self.commands(app, commands);
            Ok(())
        })
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/package", || view("probe::welcome"))?;
    Route::get("/package/fallback", || view("probe::fallback"))?;
    Route::get("/package/messages", messages)?;
    Ok(())
}

fn messages(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let message = |key| {
            __(key, &Default::default()).map_err(|source| HttpError::Internal {
                source: Box::new(source),
            })
        };
        Response::json(
            StatusCode::OK,
            json!({
                "title": message("probe::messages.title")?,
                "fallback": message("probe::messages.fallback")?,
            }),
        )
    })
}

fn commands() -> Result<(), ConsoleError> {
    Artisan::command("package:probe", |command| {
        let title = __("probe::messages.title", &Default::default())
            .expect("package translation must load");
        command.comment(&title);
    })?
    .purpose("Verify the installed package")
}

struct CreateProbeTable;

impl Migration for CreateProbeTable {
    fn up<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async {
            connection
                .getSchemaBuilder()
                .create("package_probe", |table| {
                    table.id();
                })
                .await
        })
    }

    fn down<'a>(
        &'a self,
        connection: &'a mut MigrationContext,
    ) -> DatabaseFuture<'a, DatabaseResult<()>> {
        Box::pin(async { connection.getSchemaBuilder().drop("package_probe").await })
    }
}

static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "package/database/migrations/0001_create_package_probe.rs",
    &CreateProbeTable,
)];
