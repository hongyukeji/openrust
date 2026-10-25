# openrust-db

Database layer of the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M2). Planned surface: `DatabaseProvider`, `Db` pool handle with `transaction`, migration runner over `sea-orm-migration`, `Seeder` and `Factory` traits, a `dialect` module holding every PostgreSQL/MySQL-specific statement, and the `migrate` / `db:seed` / `db:reset` commands. Features: `db-postgres`, `db-mysql`.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
