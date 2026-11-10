/// Runs and rolls back registered migrations against a migration repository.
pub struct Migrator {
    _private: (),
}

/// Options for running pending migrations.
pub struct RunOptions {
    pub pretend: bool,
    pub step: bool,
}

/// Options for selecting and previewing migrations to roll back.
pub struct RollbackOptions {
    pub pretend: bool,
    pub step: Option<u64>,
    pub batch: Option<u64>,
}
