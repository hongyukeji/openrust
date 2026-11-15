# Sanctum-01 令牌迁移与注册

对照固定 Sanctum v4.4.0 的令牌迁移和 Provider 发布入口。OpenRust 使用 Cargo 的 `sanctum` feature，把迁移加入应用的编译注册表。迁移保留整数多态归属、令牌唯一索引、过期时间索引和可逆的 `up`／`down`。固定 Goravel 的数据库 Provider 同样将迁移命令接入应用。本批核对 `v0.x` 的 `auth/pat.rs`、`auth/database.rs` 和令牌业务测试，旧版使用单一用户外键及不同的撤销列，令牌逻辑留给下一批按 Sanctum 契约移植。 [固定迁移](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/database/migrations/2019_12_14_000001_create_personal_access_tokens_table.php) · [固定 Provider](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/SanctumServiceProvider.php) · [Goravel 数据库 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)

- SQLite 和独立 MySQL 8.4.13 的真实 Artisan 覆盖首次迁移、重复执行、按路径回滚及重建，数据库检查覆盖十个字段、四个索引、重复哈希拒绝及用户表保留。
- 后续按固定 `HasApiTokensTest` 和 `GuardTest` 推进令牌签发、能力、过期、撤销及认证。SPA 路径依赖 Session、Cookie 和 CSRF。 `install:api` 的发布和迁移交互归 Sanctum-03。
- `cargo +1.94.0 test --locked --offline --features sanctum --test migrations` 在专用 SQLite 与两个 MySQL schema 上通过四项，覆盖 MySQL `migrate:fresh`。初次直跑该专项时复用了默认构建的二进制，按 feature 重跑通过。默认工作区 618 项测试通过，8 项进程夹具忽略。Rust 1.94.0 格式与含 `sanctum` 的全目标严格 Clippy 通过。三语示例命令、隔离构建及 18 项浏览器检查通过。

## 内置迁移编号统一

内置令牌迁移重命名为 `0001_01_01_000012_create_personal_access_tokens_table.rs`，接续应用既有编号。编译注册、Artisan 路径和发布映射使用新名称。旧账本不提供迁移名兼容。

- Rust 1.94.0 在独立 MySQL 8.4.13 上执行 `cargo test --locked --offline --features sanctum --test migrations artisan_migrations_use_a_real_mysql_ledger -- --exact`。首次迁移只登记新名称，重复执行无待迁移项，按路径回滚后表和账本记录移除，重新执行恢复表与新记录。测试中的显式到期边界断言同步到现行 Sanctum 行为。
- 定向 `sanctum_authentication` 发布测试验证首次发布刷新时间、重复发布保留名称、已有内置文件时强制发布不产生副本，以及关闭日期刷新时使用新编号。
