# openrust-cache

Cache for the [OpenRust](https://github.com/hongyukeji/openrust) framework. This crate is currently a scaffold; the M5 contract is recorded in [ADR-014](../../.agents/project/decisions/adr-014-cache-driver-and-store-contract.md) and is not implemented yet. Planned v1 drivers are process-local memory (default, no external service), file, and optional Redis (`cache-redis`). `Cache` will support a configured default store and explicitly selected named stores through the same API. Memory and file are node-local; cross-node shared cache use requires an explicitly configured shared driver. Driver failures remain errors, and atomic operations are exposed only where the driver provides the corresponding primitive.

**Status:** scaffold only. Scope and contracts are defined in the framework design; implementation lands in its milestone.
