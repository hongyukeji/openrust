# openrust-cache

Cache for the [OpenRust](https://github.com/hongyukeji/openrust) framework (milestone M5). Planned surface: `Cache` service with `get` / `insert` / `remove` / `get_or_insert_with` / `increment`, with Redis as the standard application default (`cache-redis`) and explicitly selected memory (moka) or file drivers. Single-instance and clustered application deployments are both supported design targets; the default starter will provide Redis development configuration.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
