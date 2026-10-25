# openrust-db

Database layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M2). Planned surface: `DatabasePlugin`, `Db` pool handle with `transaction`, migration runner over `sea-orm-migration`, `src/seed.rs` async seed entry with `fake::Dummy` data, a `dialect` module holding every PostgreSQL/MySQL-specific statement, and the `migrate` / `db seed` / `db reset` / `db status` commands. Features: `db-postgres`, `db-mysql`.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
