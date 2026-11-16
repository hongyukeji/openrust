use crate::support::publishing::{PublishSource, PublishableResource};

const PROVIDER: &str = "OpenRust\\Sanctum\\SanctumServiceProvider";

pub(crate) fn publishable_resources() -> [PublishableResource; 2] {
    [
        PublishableResource {
            provider: PROVIDER,
            tag: "sanctum-config".into(),
            path: "config/sanctum.rs".into(),
            source: PublishSource::Embedded(include_bytes!("stubs/sanctum.stub")),
            migration: false,
        },
        PublishableResource {
            provider: PROVIDER,
            tag: "sanctum-migrations".into(),
            path: "database/migrations/0001_01_01_000012_create_personal_access_tokens_table.rs"
                .into(),
            source: PublishSource::Embedded(include_bytes!("stubs/personal_access_tokens.stub")),
            migration: true,
        },
    ]
}
