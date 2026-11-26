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
    localization::__,
    view::{Component, ComponentOutput, ViewResult, view},
};
use serde_json::json;

pub struct ProbeProvider(pub PathBuf);

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    let resources = std::env::var_os("PROBE_RESOURCES").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources"),
        PathBuf::from,
    );
    vec![Box::new(ProbeProvider(resources))]
}

struct Mark;

impl Component for Mark {
    fn render(&self) -> ViewResult<ComponentOutput> {
        Ok(ComponentOutput::InlineTemplate(
            "Typed package component".into(),
        ))
    }
}

impl ServiceProvider for ProbeProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            self.merge_config_from(
                app,
                json!({"enabled": true, "options": {"package": 1}}),
                "probe.shallow",
            )?;
            self.replace_config_recursively_from(
                app,
                json!({"options": {"package": 1}, "indices": ["first", "second"]}),
                "probe.recursive",
            )?;
            // Resolve an untranslated key before boot to exercise namespace cache invalidation.
            let localizer = app.make::<openrust::localization::Localizer>().await?;
            let key = "probe::messages.title";
            assert_eq!(localizer.string(key, &Default::default(), None)?, key);
            let literal = "JSON package only";
            let _ = localizer.string(literal, &Default::default(), None)?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async {
            self.load_routes_from(app, routes)?;
            self.load_views_from(app, &self.0.join("views"), "probe")
                .await?;
            self.load_translations_from(app, &self.0.join("lang"), "probe")
                .await?;
            self.load_json_translations_from(app, &self.0.join("lang"))
                .await?;
            self.load_view_components_as(app, "probe", "mark", |_| Ok(Mark))
                .await?;
            self.load_migrations_from(app, MIGRATIONS).await?;
            self.commands(app, commands);
            Ok(())
        })
    }
}

fn routes() -> Result<(), RoutingError> {
    Route::get("/package", || view("probe::welcome"))?;
    Route::get("/package/fallback", || view("probe::fallback"))?;
    Route::get("/package/messages", messages)?;
    Route::get("/package/components", || view("probe::components-demo"))?;
    Ok(())
}

fn messages(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async {
        let message = |key| __(key, &Default::default()).map_err(HttpError::from);
        Response::json(
            StatusCode::OK,
            json!({
                "title": message("probe::messages.title")?,
                "fallback": message("probe::messages.fallback")?,
                "json_title": message("JSON common")?,
                "json_fallback": message("JSON package only")?,
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
                .get_schema_builder()
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
        Box::pin(async { connection.get_schema_builder().drop("package_probe").await })
    }
}

static MIGRATIONS: &[MigrationEntry] = &[MigrationEntry::new(
    "package/database/migrations/0001_create_package_probe.rs",
    &CreateProbeTable,
)];
