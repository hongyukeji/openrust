# Activitylog 操作审计扩展

## 当前范围与来源

2026-11-21 按完整固定 Spatie 源码补齐先前子集。扩展负责人直接实现并验收必要的核心接口；仅实际并行文件冲突需要协调，不再遵循早期“不得修改核心／等待转交”的分工。本轮未暂存、提交或推送。其他扩展的状态见[交付计划](../../plans/operations-ecosystem.md)，本页不表示整个生态计划完成。

主来源为 Spatie `fdcc5ee063cbf91114679336c4fbf92837196419`，完整归档位于 `/private/tmp/openrust-ecosystem-20261121/upstream/spatie-laravel-activitylog`；核对 `src/Support/*`、`src/Models/Activity.php`、`src/Models/Concerns/LogsActivity.php`、`src/Actions/*`、配置、迁移及测试。网页读取没有返回可用正文，因此依据已取得的固定 GitHub 主来源，不宣称已验证实时热门榜。旧版 `v0.x` 固定 `6d1a81924451e15f595f68c8dfb520ad6d6c44cf` 没有独立 Activitylog 包，复用其可信身份、显式策略及事务原则，不引入 SeaORM 或旧租户依赖。

## 实现

- `ActivityLogger`、`PendingActivityLog`、`activity()`、作用域状态与 CauserResolver；成功记录后重置；显式匿名、覆盖与嵌套恢复；默认操作者来自配置 guard 已认证快照，使用真实 morph 与属性。`byId(...).await` 通过认证 Provider 查询，未知 ID 不记录；`onModel`／`byModel` 捕获模型身份与属性，覆盖改变身份时不误用原模型属性。
- `LogOptions`、ChangeDetector 与原生 Eloquent 事件：dirty、允许／排除／raw／JSON 路径、描述及占位符；逐实例 `LoggingState` 与启停，Clone 复制开关；关联属性懒加载，旧外键与事务内未提交关系均可读取；自定义软删除列恢复。
- `models::Activity` 提供 JSON casts、属性读取、类型化 subject／causer；`include_soft_deleted_subjects` 只影响主体。定制 ActivityModel 及记录／清理 Action 通过原生容器命名绑定解析，普通写入执行模型原生保存／事务保存事件，不另建扩展注册内核。
- 配置提供环境启停与 buffer 开关、默认 guard／log、模型／Action 名称、软删除主体与清理周期；缓存命中不重新读取环境。迁移归包 Provider，不预装到默认应用。
- 默认迁移对齐自增 ID、数值 nullable morphs、nullable JSON 与原生 timestamps；附加可空 request_id。API 仍以字符串表示身份，UUID／字符串业务主键须定制迁移；公开毫秒时间转换为默认秒精度 SQL 时间戳。
- ActivityBuffer 共享原作用域并捕获原 Arc；通过原生 `support::defer(..., None, true)` 在 HTTP／Artisan／Worker 生命周期结束时刷新，失败生命周期也刷新。原子批次绕过逐条模型事件，失败保留待重试。模型事务提交后才入队，回滚／保存点回滚丢弃对应日志，嵌套提交按原记录顺序入队。显式 `PreparedActivity::logIn` 可随业务回滚，跨连接模型日志在业务提交后保存。
- 查询支持单组／多组、事件、精确主体与操作者、请求及数值游标；clean 遵循可选 log、正整数 days、force 和原生生产确认，默认按日历天清理。额外脱敏／16KiB／16层限制仅显式启用。

必要核心增量为 `Model::loadPathsInTransaction`、`Transaction::getConnection` 与 `Transaction::afterCommitAtLevel`，复用已有关系加载、事务连接及事务管理器；后者以当前层提交标记加最外层保存回调复现上游顺序，不改变现有 afterCommit 的嵌套优先行为。核心认证快照、defer 生命周期及其全矩阵由核心对话记录；本包测试只证明下述使用路径。

## 当前验收

隔离源码 `/private/tmp/openrust-extensions-20261121/source`，复用 target 为同批次 `/target`；Rust 1.94.0，保留 Cargo 原生增量、并发、debug 默认。未在活跃工作区运行 Cargo 构建。此前磁盘停线已经用户授权恢复；继续遵循 32GB 阈值，不再保留“等待恢复确认”的过时停点。保留源码、固定上游与日志，不删除数据库数据卷。

- `activitylog-final-tests.log`：23 项通过，10 数据、7 Provider、6 support；覆盖三库持久化行为、模型关系与实例开关，HTTP 成功／失败／响应前取消，Artisan 成功／非零／错误，配置模型及 Action 的命名解析、自定义模型 Creating 事件、认证 provider 原始 ID／属性及请求隔离。
- `activitylog-final-postgres.log`：PostgreSQL 18.6，10 项通过；`activitylog-final-mysql.log`：MySQL 8.4.13，10 项通过。均通过 REQUIRE 与归属守卫实际连接专属容器，结束后移除本批容器；未访问开发库或删除卷。字符串身份数据用例显式定制 morph 列，不能冒充默认数值迁移验证。
- support 固定 fixture：8 组 LogOptions、4 组 status 与 900 组 ChangeDetector PHP 结果；此前 PHP 8.5.12 固定镜像执行未修改的固定上游类生成，本轮回归读取相同 fixture，未声称本轮再次在线执行 PHP。
- 三语正文与完整 VitePress 构建通过，专项 `check_activitylog.mjs` 12 组、通用 `check_browser.mjs` 12 组通过，覆盖三语、桌面／手机、`/`／`/docs/`。日志 `activitylog-final-docs.log`、`activitylog-final-browser.log`、`activitylog-final-browser-generic.log`。正文仅保留开发者用法及实际边界。

- `activitylog-final-native.log`：根原生 Activitylog 配置及默认骨架两项通过；`activitylog-final-native-postgres.log`、`activitylog-final-native-mysql.log` 各一项通过。真实应用子进程验证包发现、发布／保留定制／force、迁移／重复数据保留、缓存时故意传入非法 ACTIVITYLOG 布尔环境值仍命中缓存、清理及回滚；直接查询数据库目录确认默认数值 nullable morph、nullable JSON、原生自增及实际日志持久化。
- `activitylog-final-clippy.log`：`cargo +1.94.0 clippy --locked --offline -p openrust-activitylog --all-targets -- -D warnings` 通过；覆盖所属包及其必要框架依赖。全 workspace 最终组合检查仍由核心对话跟踪，不能从本项扩大推断。
- `activitylog-final-rustdoc.log`：`RUSTDOCFLAGS='-D warnings' cargo +1.94.0 doc --locked --offline --no-deps -p openrust-activitylog` 通过；第三方 vendor 已有 cfg／弃用警告未修改。
- `activitylog-final-fmt.log`：隔离源码 `cargo +1.94.0 fmt --all --check` 通过；最终 `git diff --check` 和三语内容检查通过。所属包、根测试、三语正文／专项浏览器脚本及三个必要核心文件共31文件与已验收隔离源码逐字节一致。

最后核对 Available=39,948,564KiB，高于32GB停线。复用 target 保留，未反复冷清理；未暂存／提交／推送、未修改开发数据库或删除任何数据库卷。

## 保留的运行边界

- Rust 不能动态发现 PHP 方法；关联名和别名须按原生命名关系显式登记。模型 trait 显式提供 `LoggingState`；泛型模型接口及异步 `byId` 替代 PHP 动态重载。
- 同步模型回调只读取已认证 guard；需要 I/O 的认证应先由中间件／guard 完成，或显式 await `byId`。独立调用者提供解析器。
- standalone 没有安全的异步进程退出钩子，须显式 flush。响应前取消、SIGKILL 不保证刷新；硬超时子进程的本地刷新仍受原有截止时间约束。未传事务句柄的显式 Logger 调用不自动加入业务事务；事务内模型事件已按提交／回滚处理。不提供外部副作用恰好一次保证。
- 本包不自动提供管理 HTTP 接口、租户授权或防篡改存储；应用负责读取授权及数据选择。

本页取代先前错误的 UUID／毫秒 SQL／默认拒绝／禁止核心改动／自动缓冲不可用等描述，不再保留逐次中间快照副本；历史差异从 Git 及批次日志追溯。
