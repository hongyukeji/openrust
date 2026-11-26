//! Compiled forms of the publishable Passport migrations.

#[path = "../database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs"]
pub mod oauth_access_tokens;
#[path = "../database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs"]
pub mod oauth_auth_codes;
#[path = "../database/migrations/2016_06_01_000004_create_oauth_clients_table.rs"]
pub mod oauth_clients;
#[path = "../database/migrations/2024_06_01_000001_create_oauth_device_codes_table.rs"]
pub mod oauth_device_codes;
#[path = "../database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs"]
pub mod oauth_refresh_tokens;

pub(crate) const SOURCES: &[(&str, &[u8])] = &[
    (
        "database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs",
        include_bytes!("../database/migrations/2016_06_01_000001_create_oauth_auth_codes_table.rs"),
    ),
    (
        "database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs",
        include_bytes!(
            "../database/migrations/2016_06_01_000002_create_oauth_access_tokens_table.rs"
        ),
    ),
    (
        "database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs",
        include_bytes!(
            "../database/migrations/2016_06_01_000003_create_oauth_refresh_tokens_table.rs"
        ),
    ),
    (
        "database/migrations/2016_06_01_000004_create_oauth_clients_table.rs",
        include_bytes!("../database/migrations/2016_06_01_000004_create_oauth_clients_table.rs"),
    ),
    (
        "database/migrations/2024_06_01_000001_create_oauth_device_codes_table.rs",
        include_bytes!(
            "../database/migrations/2024_06_01_000001_create_oauth_device_codes_table.rs"
        ),
    ),
];
