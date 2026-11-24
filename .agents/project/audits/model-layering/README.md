# 模型与存储适配分层

## 范围与依据

2026-11-24 按用户确认的数据库访问、模型定义与业务服务职责，收敛三个已核对的位置。保持 Active Record 的查询、保存、关系和模型行为入口，不新增空服务层或仓储抽象。

- 固定行为参考 [Laravel Framework v13.40.0](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc)，邮箱验证参考其 [MustVerifyEmail](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/MustVerifyEmail.php)。模型能力入口保留，条件写入与精确邮箱匹配沿用项目既有行为。
- 查阅本地 `v0.x` 的 `app/models/user.rs` 与认证模块索引；本次复用当前主线的条件更新和存储适配实现，不移植旧版认证架构。
- Sanctum 安装命令产生的应用侧配置、路由和模型接入不属于本批。既有在途安装／清理工作及其他协作者改动保持独立。

## 实现边界

| 位置 | 职责与兼容性 |
| --- | --- |
| `app/models/user.rs` | 保留表字段映射、属性转换和认证能力；验证状态更新委托给框架，无直接查询构造或数据库更新。 |
| `framework/src/auth/email_verification.rs` | 使用模型连接、表及调用方列映射，保留主键、精确邮箱、原验证状态的条件更新；成功后才同步内存，不增加 Eloquent 保存事件。 |
| `framework/src/sanctum/token_model.rs` | 保留公开模型及可覆写 `find_token`／`find_token_by_digest` 入口。 |
| `framework/src/sanctum/token_store.rs` | 承接原有存储接口、Eloquent 适配、事务锁、使用时间和认证快照；调用方转到内部存储模块。 |
| `activitylog/src/models.rs` | 保留模型及 `models::ActivityModel` 公开导入路径，通过重导出提供原适配类型。 |
| `activitylog/src/model_store.rs` | 承接自定义模型注册、属性准备、普通／事务保存和记录解码；Provider 与 Repository 消费契约不变。 |

框架与扩展路径均相对 `vendor/openrust/`。字段生成器与生成区域不变，业务流程仍归既有 Actions、Manager、Server 或应用服务。

## 验收

使用 Rust 1.94.0，在项目 `target/tmp/model-layering-20261124/source` 隔离快照、独占 Cargo target 与项目内 TMPDIR 执行。数据库使用项目提供的隔离实例入口，不操作开发库。

| 验收入口 | 实际结果 |
| --- | --- |
| 根应用 `email_verification_persistence` | SQLite、MySQL 8.4.13、PostgreSQL 18（`server_version_num=180007`）分别通过；实际调用默认 User 的验证／撤销验证入口，覆盖具名连接、表前缀、幂等、精确邮箱、记录不存在和数据库失败时内存状态不变。 |
| 框架 `sanctum_authentication` | 精确运行 SQLite 与 MySQL HTTP 用例，真实 TCP／Reqwest 调用通过；MySQL 同时覆盖默认、UUID、ULID 自定义令牌模型和可覆写查找钩子。此次未新增 PostgreSQL Sanctum 验收结论。 |
| Activitylog `activitylog` | 三库各 10 项通过，覆盖普通／事务持久化、自定义模型、模型事件、回滚、缓冲及属性转换。 |
| Activitylog `provider` | SQLite 7 项通过，覆盖 HTTP Kernel、Provider 自定义模型具名绑定、清理命令，以及 Artisan 成功／非零退出／错误命令的缓冲刷新。此组固定使用 SQLite，不计作 MySQL／PostgreSQL HTTP 验收。 |
| 隔离应用正式 Artisan 脚本 | `./artisan migrate --force`、`./artisan migrate:status` 均成功，四项迁移完成且状态为 Ran。数据库文件位于本批项目内临时目录。 |
| 格式与严格 Clippy | 刷新到当前工作区的隔离快照执行 `cargo +1.94.0 fmt --all --check` 与 `cargo +1.94.0 clippy --workspace --all-targets --locked --features sanctum -- -D warnings`，通过；本批源码与最终受检快照逐文件一致。 |
| 三语文档 | 内容检查、命令入口测试、VitePress 构建通过；使用 Chrome／Playwright 在 `/` 和 `/docs/` 下验收三语桌面及移动端，12 项通过。 |

SQLite 使用锁定的 `libsqlite3-sys 0.39.1`，随包 SQLite 头文件版本为 3.54.1。测试总计 42 项驱动／入口执行通过，无忽略项；Sanctum 使用精确筛选，仅将实际执行的两个 HTTP 用例计入。

### 可复用命令

所有 Cargo 命令经 `tests/build_guard.py` 设置 Rust 1.94.0、独占 `CARGO_TARGET_DIR` 和项目内 `TMPDIR`，在隔离快照内运行：

```sh
cargo test --locked -p openrust --test email_verification_persistence
cargo test --locked -p openrust-framework --features sanctum --test sanctum_authentication bearer_and_opt_in_spa_over_http_with_sqlite -- --exact
cargo test --locked -p openrust-framework --features sanctum --test sanctum_authentication bearer_and_spa_over_http_with_mysql -- --exact
cargo test --locked -p openrust-activitylog --test activitylog --test provider
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --features sanctum -- -D warnings
```

MySQL 和 PostgreSQL 分别由 `tests/mysql_acceptance.py`、`tests/postgres_acceptance.py` 提供独占实例；`LAYERING_TEST_DRIVER` 与 `ACTIVITYLOG_TEST_DRIVER` 分别选择 `sqlite`／`mysql`／`pgsql`。MySQL Sanctum 用例显式设置 `OPENRUST_TEST_SANCTUM_MYSQL_URL` 和 `OPENRUST_REQUIRE_SANCTUM_MYSQL=1`，防止缺服务时返回而被误计通过。

另外尝试 `./artisan about`，返回既有的未实现提示；不计作通过项，也不扩展本次分层重构范围。没有更改安装生成的应用配置、路由和 Sanctum 接入，也没有推送。两组隔离数据库夹具均已回收本批容器，开发服务和数据卷保留；临时 SQLite、旧源码快照和文档产物在验收后回收，保留可复用构建缓存与验收日志。

## 后续静态复核

[其余模型与服务的复核](remaining-models-2026-11-24.md)记录剩余职责混合、可选整理与保留项；本页保留上一轮验收结论；当前落实状态见[六项实施验收](implementation-2026-11-24.md)。

[Scout 与恢复码可选整理](optional-2026-11-24.md)记录用户选定的两项后续实施与验收。

[相邻服务整理](adjacent-services-2026-11-24.md)记录 Cashier 订阅选择与 Activitylog 日志准备的职责调整及验收。

[整理后的静态复核结论](adjacent-services-2026-11-24.md#本次静态复核结论)汇总当时已完成项与 Pay 暂时保留的事务边界；该次复核没有新增行为验收结果。

[Pay 通知流程与内部存储分离](pay-notifications-2026-11-24.md)承接用户随后授权的 Pay 整理；前次暂时保留的裁决作为历史依据保留。

[Pay 整理后的静态复核](pay-notifications-2026-11-24.md#pay-整理后的静态分层复核)确认 Pay 已完成，复核范围内未发现新增待整改项；静态结论与既有行为验收分别记录。

[生成与安装产物的分层回归](generated-artifacts-2026-11-24.md)覆盖默认骨架、make:model 及 Sanctum／Passport／Fortify 安装前后、重复安装与应用定制保留。

[生成回归的语法与声明定位修复](syntax-regression-2026-11-24.md)承接字符／嵌套注释和 hidden 位置的两个测试缺口，记录 Rust 语法解析及敏感字段序列化的正反验收。
