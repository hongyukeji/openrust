# Sanctum-01 令牌迁移与注册

对照固定 Sanctum v4.4.0 的令牌迁移和 Provider 发布入口。OpenRust 使用 Cargo 的 `sanctum` feature，把迁移加入应用的编译注册表。迁移保留整数多态归属、令牌唯一索引、过期时间索引和可逆的 `up`／`down`。固定 Goravel 的数据库 Provider 同样将迁移命令接入应用。本批核对 `v0.x` 的 `auth/pat.rs`、`auth/database.rs` 和令牌业务测试，旧版使用单一用户外键及不同的撤销列，令牌逻辑留给下一批按 Sanctum 契约移植。 [固定迁移](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/database/migrations/2019_12_14_000001_create_personal_access_tokens_table.php) · [固定 Provider](https://github.com/laravel/sanctum/blob/a28ea5d4c2b123420562d6a3a233414f84b70774/src/SanctumServiceProvider.php) · [Goravel 数据库 Provider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/database/service_provider.go)

- SQLite 和独立 MySQL 8.4.13 的真实 Artisan 覆盖首次迁移、重复执行、按路径回滚及重建，数据库检查覆盖十个字段、四个索引、重复哈希拒绝及用户表保留。
- 后续按固定 `HasApiTokensTest` 和 `GuardTest` 推进令牌签发、能力、过期、撤销及认证。SPA 路径依赖 Session、Cookie 和 CSRF。 `install:api` 的发布和迁移交互归 Sanctum-03。
- `cargo +1.94.0 test --locked --offline --features sanctum --test migrations` 在专用 SQLite 与两个 MySQL schema 上通过四项，覆盖 MySQL `migrate:fresh`。初次直跑该专项时复用了默认构建的二进制，按 feature 重跑通过。默认工作区 618 项测试通过，8 项进程夹具忽略。Rust 1.94.0 格式与含 `sanctum` 的全目标严格 Clippy 通过。三语示例命令、隔离构建及 18 项浏览器检查通过。

## 内置迁移编号统一

内置令牌迁移重命名为 `0001_01_01_000012_create_personal_access_tokens_table.rs`，接续应用既有编号。编译注册、Artisan 路径和发布映射使用新名称。旧账本不提供迁移名兼容。

- Rust 1.94.0 在独立 MySQL 8.4.13 上执行 `cargo test --locked --offline --features sanctum --test migrations artisan_migrations_use_a_real_mysql_ledger -- --exact`。首次迁移只登记新名称，重复执行无待迁移项，按路径回滚后表和账本记录移除，重新执行恢复表与新记录。测试中的显式到期边界断言同步到现行 Sanctum 行为。
- 定向 `sanctum_authentication` 发布测试验证首次发布刷新时间、重复发布保留名称、已有内置文件时强制发布不产生副本，以及关闭日期刷新时使用新编号。

## 安装时发布迁移

固定 Laravel 应用骨架 v13.11.0 只有 users、cache 和 jobs 的默认迁移。Laravel Framework v13.40.0 的 `install:api` 安装 Sanctum，仅在应用没有令牌表迁移时调用 Provider 的发布入口。Sanctum v4.4.0 的 Provider 发布迁移和配置。OpenRust 的安装器复用同一套包资源、发布规则和 `VendorTagPublished` 事件。应用迁移发现只扫描已存在的迁移文件，移除了 Sanctum 专用文件名判断与试验用的属性解析依赖。旧版 v0.x 的 PAT 使用不同的表和归属字段，本次沿用当前 Sanctum 迁移结构。 [固定应用骨架迁移](https://github.com/laravel/laravel/tree/06d016a364a37430eec9cbc52209adce3d7667ce/database/migrations) · [固定安装器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ApiInstallCommand.php#L113-L129) · [固定发布器](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/VendorPublishCommand.php#L181-L201)

- macOS 隔离副本和一次性 MySQL 8.4.13 实测，干净骨架在 Cargo 功能关闭及手动启用时均未创建令牌表。手动 `vendor:publish` 新建迁移源文件，重建后执行并登记一次，重复发布保留原文件名。
- 实测 `install:api --without-migration-prompt` 与重复安装的 `--force`。确认提示的测试由已启用功能、尚未编入迁移的二进制触发，回车后重建并执行。发布后的迁移关闭功能后仍可回滚和重建。保留原迁移文件时，已有账本保持原编号，重复执行无待迁移项，回滚后表和记录均移除。
- Rust 1.99.1 运行启动定向测试、Sanctum 发布定向测试和实际 MySQL 迁移账本回归。根应用含 Sanctum 的严格 Clippy 和三语 VitePress 构建通过。本轮验证范围为 macOS 与隔离 MySQL，不包含 Windows 实机。
