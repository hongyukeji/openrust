# Scout

Scout connects Eloquent models to search engines, model lifecycle synchronization and queue workers. The bundled `collection` engine searches model data in memory; `null` disables search. Applications can register additional engines.

## Installation {#installation}

Enable `"scout"` in the application's `Cargo.toml` `[features].default`, keeping existing features when using `./artisan`. Package discovery loads `ScoutServiceProvider`. The package owns its configuration and creates no search tables.

```sh
./artisan vendor:publish --tag=scout-config
```

Published Rust configuration must be connected to the compiled application loader and rebuilt. Publishing preserves existing files; cached configuration bypasses package environment reads. No additional application Provider or configuration is installed in the default skeleton.

## Configuration {#configuration}

`SCOUT_DRIVER` defaults to `collection`; `SCOUT_PREFIX` defaults to an empty string. `SCOUT_QUEUE`, `SCOUT_AFTER_COMMIT`, `SCOUT_SOFT_DELETE` and `SCOUT_IDENTIFY` default to `false`. Import chunks default to 500. Configure a queue connection and name with `scout.queue = {"connection":"database","queue":"search"}` in compiled configuration, or set it to `true` to use the native default queue.

Collection searches use the model's PostgreSQL, MySQL or SQLite connection. They fetch all matching database candidates and filter `to_searchable_array` using case-insensitive substring matching. Empty queries retain eligible models, whitespace is literal, and `"0"` is a search term. This engine is intended for modest datasets. Database full-text search is available through the separate [native search interfaces](../digging-deeper/search.md).

The collection engine maintains no separate index: update, delete, flush and index creation operations have no index state to change. Flushing never deletes model rows. Remote engines, the Scout database engine, semantic search, hybrid search and embedding providers are not bundled. Selecting an unknown driver or using semantic/hybrid search on collection returns an error.

## Searchable Models {#searchable-models}

Implement `openrust_scout::Searchable` on an existing `Model + Clone`. Its default `to_searchable_array` uses `to_array`; override it to choose fields. `should_be_searchable` controls eligibility; `search_index_should_be_updated` controls automatic updates.

```rust
use openrust_scout::{EngineManager, Searchable};

impl Searchable for Article {}

// Inside the application's Provider boot method:
app.make::<EngineManager>().await?
    .register::<Article>(app, "Article").await?;
```

Explicit registration replaces dynamic PHP trait boot and supplies the model alias used by commands. Keep business models in the application. `boot_searchable(app)` can register a model under its full Rust type name. `searchable_as` applies the prefix to the table name; `indexable_as`, `get_scout_key` and `get_scout_key_name` can be overridden. Custom string keys should also return `ModelKeyType::String` from `get_scout_key_type`.

The native observer handles saves, deletes, force deletes and restores. With `after_commit=true`, synchronization waits for the outer transaction; rolled-back callbacks are discarded. With it disabled, an external engine may observe changes before rollback. `soft_delete=true` retains soft-deleted models for indexing and excludes them from ordinary searches.

## Searching {#searching}

Call the model API inside a bootstrapped HTTP, Artisan or Worker lifecycle.

```rust
let page = Article::search("rust")?
    .where_in("category", vec![1_i64.into(), 2_i64.into()])
    .order_by("id", "asc")
    .paginate(15, 1).await?;
```

Use `where(field, operator, value)`, `where_in`, `where_not_in`, `order_by`, `order_by_desc`, `latest`, `oldest` and `take`. Rust's explicit operator argument replaces PHP's overloaded two-argument `where`. Without explicit ordering, collection results use descending Scout key order. `with_trashed` and `only_trashed` control soft-deleted results. Pagination requires positive page sizes and uses native paginator types; collection pagination counts all matching models even when `take` is present.

`raw` returns ordered IDs, total and provider-specific JSON. `keys`, `first`, `get`, `paginate` and `simple_paginate` return IDs, models or pages. `cursor` is a stream: collection matching first determines IDs, then native SQL streams the mapped models in result order. `query(callback)` modifies the model-loading query; `callback` replaces collection's initial database constraints; `with_raw_results` modifies results before model mapping. Callbacks return `Result` and preserve errors. `within` and `options` are available to custom engines; collection does not maintain named indexes.

## Synchronization and Queues {#synchronization}

`searchable`, `unsearchable`, `searchable_sync` and `unsearchable_sync` operate on a model. Import `SearchableCollection` to use the same methods on native Eloquent collections. Sync variants bypass the queue. Queue updates reload current model data; queued deletions carry keys so deletion still works after the row disappears.

Run the normal [queue worker](../digging-deeper/queues.md) for the configured connection and queue. Register searchable models during each application boot so fresh Worker processes know their job handlers. Queue failures use native retry and failure handling. The collection engine needs no worker for search itself.

Use `disable_search_syncing` and `enable_search_syncing` for application-level control of a model type. `without_syncing_to_search(future)` temporarily suppresses automatic synchronization in the current async call chain and restores it after nested calls or cancellation. Manual `searchable` calls remain explicit requests.

## Commands {#commands}

```sh
./artisan scout:import Article
./artisan scout:import Article --fresh --chunk=500
./artisan scout:flush Article
```

`scout:import` respects model eligibility and the configured queue; `--fresh` flushes the engine before importing, and `-c` is the short form of `--chunk`. `scout:flush` clears only engine state. Unknown model aliases and zero chunk sizes fail explicitly. Use `./main artisan` after deployment.

## Custom Engines {#custom-engines}

Implement `engines::Engine<Article>` and register an instance with `manager.extend::<Article>("custom", engine)`. Select it through `scout.driver` or the model's `scout_driver`. Engines receive typed models and `Builder` settings, return `SearchResults`, and preserve errors through `ScoutFuture`. Search result IDs map back to native models in engine order. Each application owns its engine registrations; custom engines provide their own network clients, credentials, index settings and consistency guarantees.
