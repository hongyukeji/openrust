use openrust::{
    container::ServiceKey,
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    queue::QueueManager,
    support::{ProviderResult, ServiceProvider},
};
use std::sync::Arc;

use crate::{
    ModerationCredentialProvider, ModerationManager, configuration, credentials::FileCredentials,
    drivers,
};

#[path = "../database/migrations/2026_11_26_000012_create_moderation_account_rate_buckets_table.rs"]
mod account_rates_migration;
#[path = "../database/migrations/2026_11_26_000006_add_moderation_callback_reference.rs"]
mod callback_reference_migration;
#[path = "../database/migrations/2026_11_26_000009_add_moderation_cancellation.rs"]
mod cancellation_migration;
#[path = "../database/migrations/2026_11_26_000002_create_moderation_checks_table.rs"]
mod checks_migration;
#[path = "../database/migrations/2026_11_26_000010_create_moderation_review_decisions_table.rs"]
mod decisions_migration;
#[path = "../database/migrations/2026_11_26_000008_create_moderation_review_groups_table.rs"]
mod groups_migration;
#[path = "../database/migrations/2026_11_26_000015_create_moderation_late_notices_table.rs"]
mod late_notices_migration;
#[path = "../database/migrations/2026_11_26_000007_add_moderation_lookup_hint.rs"]
mod lookup_hint_migration;
#[path = "../database/migrations/2026_11_26_000004_add_moderation_query_budget.rs"]
mod query_budget_migration;
#[path = "../database/migrations/2026_11_26_000011_add_moderation_query_retry_at.rs"]
mod query_retry_migration;
#[path = "../database/migrations/2026_11_26_000005_add_moderation_recovery_dispatch.rs"]
mod recovery_dispatch_migration;
#[path = "../database/migrations/2026_11_26_000013_create_moderation_remote_leases_table.rs"]
mod remote_leases_migration;
#[path = "../database/migrations/2026_11_26_000001_create_moderation_reviews_table.rs"]
mod reviews_migration;
#[path = "../database/migrations/2026_11_26_000017_create_moderation_revision_controls_table.rs"]
mod revision_controls_migration;
#[path = "../database/migrations/2026_11_26_000016_add_moderation_video_segment_plan.rs"]
mod segment_plan_migration;
#[path = "../database/migrations/2026_11_26_000003_add_moderation_submission_lease.rs"]
mod submission_lease_migration;
#[path = "../database/migrations/2026_11_26_000019_add_moderation_video_audio_track_count.rs"]
mod video_audio_track_count_migration;
#[path = "../database/migrations/2026_11_26_000014_add_moderation_video_duration.rs"]
mod video_duration_migration;
#[path = "../database/migrations/2026_11_26_000018_create_moderation_video_tombstones_table.rs"]
mod video_tombstones_migration;

pub struct ModerationServiceProvider;

impl ServiceProvider for ModerationServiceProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            if !app.configuration_is_cached() {
                let env = app.make::<openrust::config::Environment>().await?;
                self.merge_config_from(app, configuration::load(&env)?, "moderation")?;
            }
            let container = app.container();
            container.singleton_if::<dyn ModerationCredentialProvider, _>(|container| {
                Ok(Arc::new(FileCredentials(container.get("config")?)))
            })?;
            #[cfg(feature = "webhooks")]
            crate::provider_webhooks::register(container)?;
            container.singleton_named("moderation", |container| {
                let manager = ModerationManager::new(container.get("config")?);
                drivers::register(
                    &manager,
                    container.make()?,
                    container.make()?,
                    container.make()?,
                    container.make()?,
                );
                Ok(Arc::new(manager))
            })?;
            container.alias("moderation", ServiceKey::typed::<ModerationManager>())?;
            Ok(())
        })
    }

    fn boot<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            self.load_migrations_with_application_overrides(app, Self::migrations())
                .await?;
            self.publishes(app, MIGRATION_SOURCES, "moderation-migrations")?;
            app.make::<QueueManager>().await?.register(
                crate::recovery::JOB_NAME,
                Arc::new(crate::recovery::RecoverVideoReview),
            )?;
            self.commands(app, crate::recovery::register_command);
            self.publishes(
                app,
                &[(
                    "config/moderation.rs",
                    include_bytes!("../config/moderation.rs"),
                )],
                "moderation-config",
            )
        })
    }
}

openrust::compiled_migration_assets!("content-moderation";
    "2026_11_26_000001_create_moderation_reviews_table.rs" => reviews_migration,
    "2026_11_26_000002_create_moderation_checks_table.rs" => checks_migration,
    "2026_11_26_000003_add_moderation_submission_lease.rs" => submission_lease_migration,
    "2026_11_26_000004_add_moderation_query_budget.rs" => query_budget_migration,
    "2026_11_26_000005_add_moderation_recovery_dispatch.rs" => recovery_dispatch_migration,
    "2026_11_26_000006_add_moderation_callback_reference.rs" => callback_reference_migration,
    "2026_11_26_000007_add_moderation_lookup_hint.rs" => lookup_hint_migration,
    "2026_11_26_000008_create_moderation_review_groups_table.rs" => groups_migration,
    "2026_11_26_000009_add_moderation_cancellation.rs" => cancellation_migration,
    "2026_11_26_000010_create_moderation_review_decisions_table.rs" => decisions_migration,
    "2026_11_26_000011_add_moderation_query_retry_at.rs" => query_retry_migration,
    "2026_11_26_000012_create_moderation_account_rate_buckets_table.rs" => account_rates_migration,
    "2026_11_26_000013_create_moderation_remote_leases_table.rs" => remote_leases_migration,
    "2026_11_26_000014_add_moderation_video_duration.rs" => video_duration_migration,
    "2026_11_26_000015_create_moderation_late_notices_table.rs" => late_notices_migration,
    "2026_11_26_000016_add_moderation_video_segment_plan.rs" => segment_plan_migration,
    "2026_11_26_000017_create_moderation_revision_controls_table.rs" => revision_controls_migration,
    "2026_11_26_000018_create_moderation_video_tombstones_table.rs" => video_tombstones_migration,
    "2026_11_26_000019_add_moderation_video_audio_track_count.rs" => video_audio_track_count_migration,
);

impl ModerationServiceProvider {
    pub fn migrations() -> &'static [openrust::database::migrations::MigrationEntry] {
        MIGRATION_ENTRIES
    }
}

pub fn openrust_providers() -> Vec<Box<dyn ServiceProvider>> {
    vec![Box::new(ModerationServiceProvider)]
}
