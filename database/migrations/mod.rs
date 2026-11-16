use openrust::database::migrations::MigrationEntry;

include!(concat!(env!("OUT_DIR"), "/migrations.rs"));
