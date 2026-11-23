# 数据库设置底座 S1

日期：2026-11-21。状态：S1 实现及本阶段验收完成，核范围本地提交；框架扩展交付闭合；后台产品及具体服务接入属于应用层按需工作。设计边界及验收见本文。

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

2026-11-21 用户明确框架边界：S2 后台表单／权限策略／脱敏展示／操作记录，以及 S3 短信／存储／邮件具体适配属于应用层接入参考，不是本扩展的未完成项，也不自动开启这些工作。生产备份导入、集群滚动发布及真实第三方服务不计入 S1 已验范围；没有新增专有设置迁移体系，升级采用现有原生迁移并保留未知属性及管理员覆盖值。

核心侧负责 `docs/*/reference/api.md` 及最终统一 API／站点，后续负责 settings 内部 chrono→Jiff 替换与复验，本阶段不混入这些改动。

本阶段专属根目录：`/var/folders/20/lxn_zjvn3614f836z2cm3vww0000gn/T/openrust-settings-s1-20261121-3_zfbh7h`。源码与验收日志保留；核心侧已改用时间库整合 target，明确通知不再复用本批旧基线 target。已通过 lsof 确认无打开句柄后直接删除本批 `target/` 和 `source/docs/.vitepress/dist/`；没有自有 npm 安装目录，`source/docs/node_modules` 是指向共享依赖的符号链接，未删除共享依赖。未修改 root/settings 的在途时间库接线，也未操作其他对话容器或数据卷。

清理逻辑占用 6,594,520 KiB（6.753 GB）；项目 df Available 从 37,293,296 增至 43,627,204 KiB，实测增加 6.486 GB。清理后可用 44.674 GB（41.606 GiB），高于32 GB停线。详细读数另存专属根目录 `cleanup.json`；源码、日志、共享缓存及全部数据卷保留。


## 默认应用 Provider 边界修正

2026-11-21 用户指出默认 `app/providers/app_service_provider.rs` 提前接入扩展。定位到 `c44864afe` 把 Settings Provider、SiteSettings 演示业务组和包迁移写入根应用，后续支付 WIP 沿用了根 boot 登记包迁移的做法。`#[cfg(feature)]` 只隔离编译，没有改变职责归属。此前规范要求区分应用／框架／扩展，但缺少默认骨架的明确禁则；功能验收验证可运行，未验证示例是否污染默认应用，核心整合审查漏掉了该问题。

先复核既有 `openrust-package-discovery` 的 metadata／`openrust_providers()` 机制、固定 Laravel ServiceProvider::loadMigrationsFrom 和官方应用 AppServiceProvider，再采取最小修正，不新建安装管理器或平行注册机制。固定源码：[Laravel ServiceProvider](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/ServiceProvider.php)；[官方应用骨架](https://github.com/laravel/laravel/blob/13.x/app/Providers/AppServiceProvider.php)于本日直接HTTPS读取，后者为分支参考，不冒充固定提交。联网检索未返回正文，官方原文保留在本批专属目录。旧版 `v0.x:app/providers/app.rs` 已复核；当前包发现和迁移登记可直接复用，未移植旧版业务接线。

- 根 AppServiceProvider 只保留现有 User 模型登记，boot为空；bootstrap清单只列应用 Provider。移除根 app/settings 的演示类型与模块声明；网站／模块类型仍在包测试和使用文档中展示，用户应用按实际业务显式注册。
- Settings通过Cargo metadata与`openrust_providers()`使用现有包发现；自身Provider在boot登记默认迁移，不查询数据库。自定义表或连接时跳过默认迁移，由消费应用登记对应迁移，避免误建默认表。
- 支付WIP交原Plan对话按同一边界修正：包迁移归PayServiceProvider，文档不再要求编辑根AppServiceProvider；不代为提交其在途实现。其他Socialite／文档改动完整保留。
- 项目规范新增[默认应用与扩展装配](../../conventions.md#默认应用与扩展装配)，并从项目指引直达。默认应用、包发现、显式安装生成和业务示例分别规定职责。`tests/application_skeleton.rs`随根Cargo测试运行，检查默认Provider目录和bootstrap清单的扩展引用／feature分支；手动重新注入Settings与Pay引用均被拒绝，不增加解析依赖。该约束只用于框架仓库默认骨架，不禁止用户应用实际定制。

验收源码来自 `5de45c53b` Git归档，仅叠加本批修正，未复制并行支付／Socialite工作区改动；目录 `/private/tmp/openrust-provider-boundary-20261121/`。Settings三库原有13项行为／HTTP／Worker及独立进程回归通过；新增包Provider专项以SQLite验证真实包清单发现、无数据库启动、默认迁移幂等、自定义表及连接跳过。三语内容检查／站点构建及根／子目录桌面／移动12组浏览器通过。后续构建、严格检查与资源收尾结果附于下方；前轮b168的API产物仍只对应其固定源码，不能当作本次修改后的新API导出。

实际验收：Rust1.94、jobs2、incremental0／debug0，Settings包 `--tests` 在SQLite／真实PG18／MySQL8.4各退出0（其中新增Provider专项固定SQLite，验证包发现、无数据库文件启动、默认迁移一次性及自定义表／连接跳过；不把该专项重复运行计作三驱动迁移测试）。既有三库读写／HTTP／Worker作用域与独立进程用例通过。根 `cargo test --test application_skeleton` 在默认关闭和 `--features settings` 各通过；`tests/settings_install.py` 实际运行两种二进制，关闭时包发现／迁移均无Settings，启用时自动发现包、迁移／重跑／保留值／回滚、配置缓存与真实HTTP通过。

`cargo +1.94.0 clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`、`fmt --all --check`、三语内容与构建、通用12组合和Settings12组合浏览器通过；`git diff --check`通过。证据包括 `results.json`、`provider-final.log`、`settings-{sqlite,postgres,mysql}.log`、`skeleton-feature-{off,on}.log`、`skeleton-reject-{settings,pay}.log`、`install-{off,on}.log`、`clippy.log`、`fmt.log`及`docs-*.log`。首轮新增测试的PathBuf与mut声明编译错误已修正，初轮日志不计成功结果。该验收为固定归档叠加本批修改，不将并行新支付／微信整理称作同一快照整合；支付自身迁移装配已由Plan在 `7edd0fbca` 独立修正并通过三库原生Artisan复验，见[支付验收](../pay/README.md#最终装配复验)。

收尾确认专属target无人占用后，直接移除target／测试二进制及临时文档dist／cache；源码、日志、共享依赖、IDE target与旧正式API交付保留。`cleanup.json`记录表观5,492,260KiB，可用空间43,396,636→48,643,568KiB，实际净增5,246,932KiB（约5.373GB），剩余约49.811GB／46.390GiB；Docker卷前后均[]、删除0。没有修改开发数据库、推送或部署。

## 已完成计划归档（2026-11-23）

框架范围为类型化设置底座；后台产品、权限和具体服务适配由应用接入。保留单表版本锚点、事务冲突检测、显式异步加载及请求／任务作用域，不把数据库保存等同组件自动生效。

原计划 `database-settings.md` 已删除；历史设计、调研及执行前快照可从提交 `f80096ea6` 的 `.agents/project/plans/database-settings.md` 查阅。本文保留实际验收与未验边界，不因归档补计测试。

## 单机与集群协调扩展（2026-11-24）

后续完成的版本协调、可选缓存加速、应用刷新处理器、六种数据源／Redis组合及资源清理见[Settings 协调验收](../settings-coordination/README.md)。此扩展不回写 S1 历史验收结论；仍不自动实现应用后台、业务权限或具体服务热替换。
