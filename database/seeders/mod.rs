use openrust::database::seeder::SeederEntry;

mod database_seeder;

pub(crate) const ENTRIES: &[SeederEntry] = &[SeederEntry::new(
    "DatabaseSeeder",
    &database_seeder::DatabaseSeeder,
)];
