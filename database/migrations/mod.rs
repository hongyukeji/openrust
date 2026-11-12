use openrust::database::migrations::MigrationEntry;

#[path = "0001_01_01_000000_create_users_table.rs"]
mod create_users_table;

#[path = "0001_01_01_000001_create_cache_table.rs"]
mod create_cache_table;

#[path = "0001_01_01_000002_create_jobs_table.rs"]
mod create_jobs_table;

#[path = "0001_01_01_000003_create_failed_jobs_table.rs"]
mod create_failed_jobs_table;

#[path = "0001_01_01_000004_create_job_batches_table.rs"]
mod create_job_batches_table;

#[path = "0001_01_01_000005_create_job_batches_jobs_table.rs"]
mod create_job_batches_jobs_table;

#[path = "0001_01_01_000006_create_job_batches_outbox_table.rs"]
mod create_job_batches_outbox_table;

#[path = "0001_01_01_000007_create_job_batches_additions_table.rs"]
mod create_job_batches_additions_table;

#[path = "0001_01_01_000008_create_job_batches_continuations_table.rs"]
mod create_job_batches_continuations_table;

#[path = "0001_01_01_000009_add_chain_successor_to_job_batches_jobs.rs"]
mod add_chain_successor_to_job_batches_jobs;

pub(crate) const ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new(
        "database/migrations/0001_01_01_000000_create_users_table.rs",
        &create_users_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000001_create_cache_table.rs",
        &create_cache_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000002_create_jobs_table.rs",
        &create_jobs_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000003_create_failed_jobs_table.rs",
        &create_failed_jobs_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000004_create_job_batches_table.rs",
        &create_job_batches_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000005_create_job_batches_jobs_table.rs",
        &create_job_batches_jobs_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000006_create_job_batches_outbox_table.rs",
        &create_job_batches_outbox_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000007_create_job_batches_additions_table.rs",
        &create_job_batches_additions_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000008_create_job_batches_continuations_table.rs",
        &create_job_batches_continuations_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000009_add_chain_successor_to_job_batches_jobs.rs",
        &add_chain_successor_to_job_batches_jobs::Definition,
    ),
];
