# Sanctum

## Token table migration {#token-table-migration}

Build the application with the `sanctum` Cargo feature to register the compiled `personal_access_tokens` migration. Use the resulting binary for migration and rollback.

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

The migration creates a polymorphic owner, a unique 64-character token hash column, nullable abilities and timestamps, and an expiration index. The feature currently registers the migration. Token authentication and `install:api` integration are tracked separately.
