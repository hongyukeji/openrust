/// The result reported for an individual migration.
#[repr(u8)]
pub enum MigrationResult {
    Success = 1,
    Failure = 2,
    Skipped = 3,
}
