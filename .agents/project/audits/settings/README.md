# 数据库设置底座 S1

日期：2026-11-21。状态：S1 实现及本阶段验收完成，核范围本地提交；后台及具体服务接入仍待开发。计划见[数据库动态设置](../../plans/database-settings.md)。

## 范围与设计对账

- 可选 `openrust-settings` 包、根应用 `settings` feature、原生迁移及 `SiteSettings` 示例。类型化设置提供默认值、填充、恢复默认、保存、刷新、锁定及解锁、密钥轮换；业务模块可声明自己的设置组。
- 复用现有数据库、加密、容器异步 scoped factory、HTTP 与 Worker 作用域。不修改同步 Config 契约，不自动覆盖邮件／短信／存储组件，不引入共享缓存、后台路由或额外权限系统。模块自有设置的声明／Provider 登记指南、实际独立组读写已覆盖。
- 主参考 [Spatie Laravel Settings](https://github.com/spatie/laravel-settings/tree/dc7c4c42bb3789c74493d756eaca2f183e9a68d5)，固定提交 `dc7c4c42bb3789c74493d756eaca2f183e9a68d5`，MIT。已读取 Settings、SettingsMapper、SettingsContainer、DatabaseSettingsRepository 及建表模板，借鉴类型化分组、仓储、加密和显式保存／刷新；没有运行 PHP 上游测试，不宣称完整移植该包。
- 旧版 `openrust-v0.x` 的 `6d1a81924` 只有运行期配置设计，没有可直接复用的对应实现。
- 保留单张 settings 表；每组保留 `__settings_version` 属性作为不透明版本锚点，使用原生事务和无值变化 UPSERT 取得写锁，比较组快照后原子保存。避免 MySQL INSERT IGNORE 共享锁升级死锁；SQLite 写锁先于读取。拒绝保留名（包括大小写变体）和属性大小写冲突，组名登记同样不区分大小写去重。
- Rust 以 trait、Serde、异步 Result 和编译型原生迁移表达 PHP 动态机制。锁定字段更新明确失败，区别于上游排除锁定字段的行为，避免后台假报成功。加密声明作用于顶层属性，凭据对象可以整体加密。
- `saveIn` 采用嵌套保存点；外层事务提交或回滚由调用者负责。保存后保留本实例原持久快照及待保存编辑，外层提交后调用 refresh；外层回滚可重试。传入事务必须属于目标数据库及表。没有提交前缓存或事件发布。

## 已有验证及适用范围

初轮使用 `58c622e69`；恢复后从 `95e904727122b0be4bb19b0f79f13ee08ce6ab60` 干净导出，叠加本阶段核定文件，在原隔离路径完成最终三库、文档及 Rust 复验。Rust 1.94.0，独占 target、关闭增量、jobs=2；31 份实现／测试／文档文件与活跃工作树逐字节一致。其他协作者未提交改动未进入本快照，不能将本结果扩展为后续时间库替换或整仓所有 WIP 的集成验收。

| 项目 | 实际结果 |
|---|---|
| SQLite、PostgreSQL 18.6、MySQL 8.4.13 | 各 9 个设置行为测试、4 个运行时测试、独立进程测试入口及 2 个编译型 doctest 通过。并发测试每轮仅允许一个写入者成功，其他写入者收到冲突。 |
| 生命周期与事务 | 实际 TCP HTTP 两应用隔离、同请求复用／后续请求新值、实际数据库 Worker 两任务作用域隔离、缺表启动、外层回滚与保存点回滚通过。 |
| 加密及兼容 | 密文存储、旧密钥读取及当前密钥重加密、错误密钥／破损密文拒绝、未知旧属性保留等通过。恢复仅覆盖密钥环和新实例重新读取，未验证生产备份导入或集群滚动发布。 |
| Rust 检查 | 全 workspace/all-targets/all-features 严格 Clippy、fmt 通过；包括最终大小写保护修正；三库行为复验也已完成。基线自带 rav1e 生命周期及 imgalifier cfg 警告仍可见，未改动这些第三方补丁。 |
| 根应用安装入口 | settings feature 二进制构建通过；独立临时应用下缺表 list、迁移及重复执行保留管理员自定义值、迁移状态、config cache 不固化数据库设置、真实 HTTP /up、正常关闭、清缓存、回滚及重迁移通过。 |
| 三语文档 | 内容检查与构建通过；现有浏览器检查 12 场景、导航及支持范围 6 场景、设置专页 12 场景通过，均覆盖三语、桌面／移动及根／子目录部署。 |

典型命令（从隔离源码执行，不能指向开发数据库）：

```sh
cargo +1.94.0 test -p openrust-settings --locked --offline
SETTINGS_TEST_DRIVER=pgsql python3 -B tests/postgres_acceptance.py -- cargo +1.94.0 test -p openrust-settings --locked --offline
SETTINGS_TEST_DRIVER=mysql python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test -p openrust-settings --locked --offline
cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings
cargo +1.94.0 fmt --all --check
python3 -B tests/settings_install.py --binary <隔离target>/debug/main
node docs/tests/check_content.mjs
node docs/tests/check_browser.mjs
node docs/tests/check_scope.mjs
node docs/tests/check_settings.mjs
```

夹具使用独立临时容器；未修改开发数据库或 Redis。未引入共享缓存，因此不将本次结果列为 Redis/database/file 缓存验收。

## 授权调整与集成边界

此前额度读取不可用曾触发停点；2026-11-21 用户明确取消额度读取及项目额度限制，现已从根 AGENTS 和当前交接移除相关执行前提。历史额度记录保留追溯，不再具有停工效力。磁盘 32 GB 停线及验收／Git 授权保留。

S2 后台表单／权限／脱敏／操作记录，以及 S3 短信／存储／邮件动态接入仍待开发。生产备份导入、集群滚动发布及真实第三方服务不计入 S1 已验范围；没有新增专有设置迁移体系，升级采用现有原生迁移并保留未知属性及管理员覆盖值。

核心侧负责 `docs/*/reference/api.md` 及最终统一 API／站点，后续负责 settings 内部 chrono→Jiff 替换与复验，本阶段不混入这些改动。

本阶段专属根目录：`/var/folders/20/lxn_zjvn3614f836z2cm3vww0000gn/T/openrust-settings-s1-20261121-3_zfbh7h`。`source/` 为本阶段隔离源码、`target/` 为独占 Cargo 产物，日志在根目录和 `source/docs-build-final.log`。依最新集成指示保留可用 target、源码与日志，待核心侧协调后再清理；共享依赖及全部数据卷保留。PostgreSQL／MySQL 临时验收容器由本批夹具回收，不操作其他对话的容器。
