use openrust::database::migrations::MigrationEntry;

#[path = "0001_01_01_000000_create_users_table.rs"]
mod create_users_table;

#[path = "0001_01_01_000001_create_cache_table.rs"]
mod create_cache_table;

pub(crate) const ENTRIES: &[MigrationEntry] = &[
    MigrationEntry::new(
        "database/migrations/0001_01_01_000000_create_users_table.rs",
        &create_users_table::Definition,
    ),
    MigrationEntry::new(
        "database/migrations/0001_01_01_000001_create_cache_table.rs",
        &create_cache_table::Definition,
    ),
];
