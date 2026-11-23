# Settings 单机与集群协调验收

日期：2026-11-24。范围：在[既有 Settings S1](../settings/README.md)之上增加版本协调、缓存加速与应用刷新处理器。不安装后台产品、不分发 `.env`，不修改现有设置表、迁移台账和持久字段格式。单机与集群沿用同一 Settings 接口和配置，不新增集群模式开关。

## 调研与复用

- 延续[固定 Spatie 参考与许可](../../references/packages/settings-sources.md)，提交 `dc7c4c42bb3789c74493d756eaca2f183e9a68d5`。类型化组、仓储和已有 UUID 并发锚点继续复用；本批协调器为独立扩展，不称作上游现有接口的逐行移植。
- 查阅 [Akaunting Setting](https://github.com/akaunting/laravel-setting)的数据库／缓存分工、[Laravel v13.40.0 queue:restart](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Queue/Console/RestartCommand.php)的共享缓存信号，以及 [Apollo 通知与轮询设计](https://github.com/apolloconfig/apollo/blob/master/docs/en/design/apollo-design.md)。复用已有 Cache Repository、数据库写连接、事务 after_commit 和请求／任务作用域，不引入外部配置中心、新依赖库或第二套队列。
- [Redis Pub/Sub](https://redis.io/docs/latest/develop/pubsub/)为至多一次投递且频道不按 Redis DB 编号隔离。因此不把 Pub/Sub 当作权威记录；采用持久版本定期对账和现有缓存中的元数据提示。无 Redis、丢通知及重连均使用同一机制。该方案是有界资源的最终一致，不承诺固定时刻全节点同时生效。
- 既有 S1 已核对旧版 `openrust-v0.x`：无可直接移植的完整 Settings 底座。本批复用当前已有数据库、Cache、容器和事务代码，不另造旧版替代实现。

## 公开契约与消费者

| 契约 | 实际接线与验收 |
| --- | --- |
| `Settings<T>` | 原 get/fill/save/save_in/lock/reset 等接口保留；请求／任务仍有独立快照，后台不覆盖脏编辑；过期编辑保存返回 Conflict |
| `SettingsRepository` | 新增有默认实现的 refresh 与 versions；数据库版本批查询使用写连接，不查整组未变属性；非空自定义快照用 with_version 提供每次提交变化的版本 |
| `SettingsManager` | Provider 注册单例并作为原仓储使用；new 支持独立使用；首次实际读取启动检查；注册／boot 不访问设置表 |
| `SettingsOptions` | namespace、refresh_interval（默认5秒）、cache_ttl（默认1小时）；Provider 配置不区分单机与集群，缓存默认沿用应用 Cache store |
| 缓存快照／提示 | 每命名空间每组固定两个键：带版本的原始快照及 changed 提示；不按每次更新积累键。版本先向数据库核实，加密属性只存密文，提示只有 namespace/group/version |
| `on_change`／`forget_handler` | 按组注册幂等应用处理器；成功版本去重，失败及5秒超时保留待处理状态；首次读取仅建立基线，中间连续版本允许合并 |
| `status`／`cache_failures` | 进程内 observed/applied/pending/error 与缓存故障计数；不冒充全节点确认、持久审计或部署屏障；错误不包含凭据 |
| `poll`／`notify` | 主动核对活动组；重复、过期及异应用提示不覆盖数据；可选应用传输适配只提交元数据，数据库才决定当前版本 |
| `shutdown`／`downgrade` | 停止自有后台任务，不关闭共享连接；弱引用供回调使用，避免自持有循环；后台任务无独立 HTTP／任务门面作用域 |
| 提交与故障 | save 成功以数据库提交为准；缓存／处理器失败不伪装回滚；save_in 仅外层提交后提示，回滚不提示；提交后提示前崩溃由数据库对账恢复 |
| 实际消费者 | 默认 Provider 的两个 Application 实例、真实 HTTP 请求、数据库队列 Artisan Worker、独立 OS 进程重启、应用自有缓存投影失效均验证 |

独立应用必须使用独立表／连接及适当命名空间；namespace 只隔离缓存与通知，不隔离共享设置表的业务行。默认命名空间由环境、驱动、数据库名、表前缀及表名组成，不能区分的部署／Schema 应显式填写。缓存 I/O 超时后短暂熔断并回退数据库；显式配置不存在的 store 报错。数据库故障后新加载报错，既有作用域仍保留旧快照，恢复后自动读取最新版本。

## 实际验收

Rust 1.94.0；本机执行 Rust 测试，数据库和 Redis 使用专属 Docker 容器（tmpfs，无数据卷），不是容器内 Rust／原生 Ubuntu 或 Windows 新部署验收。MySQL 实测8.4.13，PostgreSQL18.7，Redis8。所有测试目录与隔离源码位于根 target/tmp/settings-sync/，共享数据库、Redis、缓存和数据卷未被修改。

| 数据源 | 无 Redis | Redis 8 |
| --- | --- | --- |
| SQLite | 通过 | 通过，含专属 Redis 暂停／恢复／清空 |
| MySQL 8.4 | 通过 | 通过，含专属 Redis 暂停／恢复／清空 |
| PostgreSQL 18 | 通过 | 通过，含专属 Redis 暂停／恢复／清空 |

各组合覆盖两个管理器更新、旧作用域与脏编辑保留、并发保存冲突、外层提交／回滚、失败处理器重试、重复／过期／异应用通知、数据库断连恢复、多个真实子进程更新及离线节点重启、默认 Provider、真实 HTTP、Artisan 数据库 Worker。末轮新增缓存投影回调在 MySQL＋Redis 通过；最终新增共享密文／提示字段检查在 SQLite 无 Redis及 MySQL＋Redis通过。12个并发重复提示只执行一次处理器，旧缓存快照修复后新节点复用而不完整查询属性。

无 Redis 组合故意跳过仅限专属容器的 Redis pause 用例；Redis 组合显式 include-ignored 跑通该用例。不把无 Redis 的跳过计作通过。协调用例的 sync_process_child 是子进程入口；主测试负责启动、退出检查，不能把入口空跑算成额外能力。MySQL＋Redis 末轮29项普通测试通过（含该入口）及独立进程 harness 通过；其他组合按原始日志保留，不累加重复次数当成功能数。Redis pause 会影响本批专属服务，该组合串行执行测试；Cargo 保留默认编译并发与增量。

复验命令（从仓库根运行；现有 Docker 中需有相应镜像）：

```sh
python3 -B tests/build_guard.py --workflow settings-sync -- \
  env CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 \
  python3 -B tests/settings_coordination.py --driver mysql --redis -- \
  cargo test --manifest-path target/tmp/settings-sync/source/Cargo.toml \
  -p openrust-settings --tests -- --include-ignored --test-threads=1
```

无 Redis 时去掉 --redis、--include-ignored 与 --test-threads=1；driver 可改为 sqlite 或 pgsql。日志 final-{sqlite,mysql,pgsql}-{none,redis}.log、provider-final.log、clippy-final.log、docs-build-final.log、results.json、accepted-files.json 和 container-check.json 保留在该批根目录。生产源码在六组合期间不变；末轮只补充测试／文档，没有将并行异常处理改动复制进验收快照，不宣称全仓或那些在途行为已验收。

严格 settings 包 all-targets Clippy（-D warnings）、范围 rustfmt、git diff --check 和三语 VitePress 内容检查／构建通过。VitePress 提示既有大 chunk 警告，构建退出0；未改动主题或前端构建策略。

## 边界

- 节点收敛为最终一致；不提供全局瞬时激活、持久业务变更事件、恰好一次回调或集群全部节点确认。
- 首次加载建立当前基线，进程恢复读取当前版本，不重放历史；所有写入应走仓储更新版本，保留原版本的外部 SQL 修改需显式 refresh。
- `.env`／Config、APP_KEY、监听端口、数据库连接池及短信／邮件／存储服务不会自动热改。应用可用幂等处理器完成自己的失效与重建；在途请求／任务不被强行替换。
- 本批未执行远端生产滚动部署、Windows 实机或其他 Cache 驱动专项验收，不将本机多进程／Redis Docker 验收扩大为这些结论。

## 资源收尾

收尾读数与锁／容器核对附于下方。保留源码快照、原始日志、下载依赖与数据卷；直接删除本批确认闲置的构建／文档产物，不移入废纸篓。

清理前可用 60,070,993,920 字节，清理后 66,669,920,256 字节，净增加 6,598,926,336 字节（约 6.60 GB／6.15 GiB），剩余约 66.67 GB／62.09 GiB。本批产物表观占用 6,616,596,480 字节。lsof 确认无人占用后取得本批 Cargo 锁，清理期间保留锁 inode；删除独占 target 的编译内容和隔离文档 dist。数据库验收各批次标签及 Settings 专属 Redis 标签复查均无遗留容器；删除数据卷0个。原始读数与核对详见 cleanup.json／container-check.json。未使用废纸篓，未删除共享依赖、用户数据库或其他代理产物。

## 自动检查频率调整（2026-11-24）

用户指出默认5秒偏积极并要求可关闭。本轮沿用上述已验设计，不引入新同步协议：`SettingsOptions::default()`、Provider 缺省值与发布配置统一改为60秒；此前段落的5秒是首轮验收快照，当前默认以本节和三语用法为准。设置更新不频繁时，一分钟比五秒保守；不把间隔当作任何实例规模下的容量保证，实际压力仍取决于活动进程、组、更新和连接状况。

`refresh_interval = 0`（独立构造器使用 Duration::ZERO）关闭后台检查及快照读取时自动过期，不启动零间隔循环，也不在每次读取时强制访问数据库。首次加载仍需数据库，此后保留快照；显式 refresh/poll/notify、本进程保存及事务提交后的本地失效仍可使用。关闭时不自动重试处理器，应用主动 poll/notify 驱动；缓存 enabled 为独立选项。不能把关闭刷新当成集群自动同步仍然开启。

同一进程同组的并发初次／过期读取在 I/O 锁内二次核对快照，复用已经完成的版本检查，避免大量请求排队重复查询；强制 poll、通知核实及显式 refresh 不复用这个读优化，保持主动检查语义。

本轮从 e12d076cd 隔离归档并叠加本任务文件，保留并行异常 WIP。原始记录位于 target/tmp/settings-refresh/。新增用例验证60秒默认、关闭后空闲和十次读取不增加版本查询、无缓存／有缓存两种路径下显式通知／poll／refresh／本地保存可用，以及12个并发初次读取只产生一次数据库版本检查和一次属性读取。默认 Provider 另核对发布默认60及接受0。

SQLite 无 Redis 整包30项通过、专属 Redis 故障项跳过1项；MySQL8.4＋专属 Redis8 整包31项通过，独立进程 harness 均通过（包含既有 HTTP、Artisan Worker、Redis 暂停恢复和通知去重回归，不另算功能数）。补充 Provider 发布默认值断言后单独复验；三语内容检查与 VitePress 构建通过。日志 sqlite.log、mysql-redis.log、provider.log、clippy.log、docs-final.log 保留。后续检查及资源清理读数附于下方。

末轮 Provider 专项通过；settings 包 all-targets 严格 Clippy、范围 rustfmt、三语最终构建和 git diff --check 均通过。确认本批 Cargo／文档产物无人占用后持锁直接清理，保留锁 inode、日志与源码。净释放 5,693,894,656 字节（约 5.69 GB），剩余 62,952,591,360 字节（约 62.95 GB）；本批数据库与 Redis 容器已回收，数据卷未删除，详细读数保留在 cleanup.json。

## 三语教程完整性检查（2026-11-24）

代码已分别由 e12d076cd、b6e1e0a98 本地提交。本轮只补三语使用教程与本条记录，不修改运行时实现，不夹带并行异常改动。修正仅单次开启 Cargo feature 却在下一次 Artisan 命令丢失扩展的说明；明确默认 feature 的持续启用、Windows 入口和生产迁移确认。新增完整 SiteSettings 类型、模块声明与 AppServiceProvider::boot 注册；读写示例统一 title 字段并给出预期结果，增加事务／默认恢复、完整配置键表、0关闭与显式刷新、逐节点部署步骤和故障定位表。保留作用域、加密、锁、冲突、自定义仓储和应用消费者边界。

逐项对照当前 Cargo feature、包发现、AppServiceProvider、config/mod.rs、SettingsGroup、SettingsServiceProvider、Settings<T> 与 SettingsManager 实际接口检查。教程不提前向默认应用添加业务类型或 Provider。既有运行时验收不重复执行，不将 VitePress 构建当作新增 Rust 示例的编译验收。

三语已有侧栏入口均指向 packages/settings.md；内容／相对链接／命令入口检查及隔离 VitePress 构建通过。HTTP 实际请求三语设置页均200，并核对导航链接与定义、注册、配置、事务、集群及显式刷新等内容已渲染。结果与源码指纹保留于 target/tmp/settings-tutorial/pages.json，构建日志为 build.log；没有启动持久预览服务，临时 HTTP 服务已在 finally 关闭。本轮为教程内容检查，不复跑整个站点浏览器交互或数据库验收。

确认无人占用后直接删除本轮隔离 dist，表观清理 70,176,768 字节；文件系统可用空间 63,028,355,072 → 63,098,945,536 字节，净变化 70,590,464 字节，原始读数存于 cleanup.json。没有生成新 Cargo 编译产物；源码、日志、共享 npm 依赖、数据库及数据卷保留。
