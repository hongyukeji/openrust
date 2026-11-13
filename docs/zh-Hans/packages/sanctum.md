# Sanctum

## 令牌表迁移 {#token-table-migration}

使用 Cargo 的 `sanctum` 功能编译应用，注册 `personal_access_tokens` 迁移。迁移和回滚都使用带有该功能的二进制。

```sh
cargo build --features sanctum
./target/debug/main artisan migrate
./target/debug/main artisan migrate:status
```

迁移创建多态归属字段、唯一的 64 字符令牌哈希列、可空的能力和时间字段，以及过期时间索引。当前功能负责注册迁移。令牌认证和 `install:api` 集成另有交付批次。
