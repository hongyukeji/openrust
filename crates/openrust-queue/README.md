# openrust-queue

Background jobs for the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M4). Planned surface: `Job` and `Handle` traits, `Queue` service with `dispatch` and `delay`, drivers `DatabaseQueue` (default, `SKIP LOCKED`), `RedisQueue` (feature `queue-redis`) and `SyncQueue`, `failed_jobs` handling, `Schedule` builder with a database lock, and the `queue work` / `queue failed` / `queue retry` / `schedule run` commands.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
