# 测试基线

> 受众：AI Agent
> 更新：2026-10-28
> 摘要：当前目录迁移与原专项测试入口，分开记录真实服务和未验收边界。

## R2-A3 缓存发布与锁外唤醒

安全自定义Wake在Notify同步回调中重新解析已发布singleton保存的Resolver，旧代码确定性返回service_cycle（红测）；修复后缓存工厂先结束active frame并移除已结束出边，再发布结果与通知，完成slot不再创建等待边。取消也经相同顺序清理。scope/root reset锁内仅提交关闭状态和缓存结果，所有通知与用户值释放在锁外执行，避免Waker同步重入State/outcome死锁。

19项容器定向回归通过，新增4项覆盖成功通知立即解析、取消通知观察、已结束父构造的后台等待边回收，以及手动scope/root reset两条status/attempt通知的同步重入。首次Clippy发现测试显式panic及随后single_match_else，已直接修复测试写法，无生产lint豁免；重跑默认/full Clippy通过。默认workspace290 passed/22 ignored，全特性513/32；双warnings-denied docs及deny通过。MSRV1.97全target/all-features与no-default也通过；独立公开consumer locked check/test1项通过，根裸cargo run的health/HTML/API/example均200、SIGINT0。日志前缀/tmp/openrust-r2-publication-。

不同模型gpt-6-sol最终静态复审无blocker/should-fix，审查未替代实际运行。

本簇无DB/auth/queue/配置/生成模板或依赖改动，不将A2真实双库/生成证明转记新容器生产集成。生产仍是旧Plugin唯一路径；Provider切换、异步资源清理、自动HTTP/job scope及模块/CRUD都未完成。

确认无独占target构建/server进程后cargo clean先dry-run再实际清理/tmp/openrust-r2-publication-target，回收47221files/15.3GiB，磁盘余118GiB；未知root/consumer共享缓存未动，/tmp红绿/门禁日志与公开consumer源码保留用于诊断。当前8个变更Markdown的40本地链接/10锚点均可解析，git diff --check与fmt通过。

## R2-A2 配置目录与来源失败语义

ConfigLoader::figment现在返回Result，全部第一方loader调用同步传播；显式new/APP_CONFIG_DIR/自定义namespace目录为空、缺失、非目录、读目录/条目失败即报错，未显式默认config缺失仍可使用内置默认。坏symlink不当作默认目录缺失，已发现领域TOML使用file_exact精确读取并在figment阶段验证来源，不向父目录搜索或静默跳文件。ConfigLoader Debug与bootstrap日志只打印安全来源key。

红测实际复现旧代码对显式缺目录静默退默认；修复后25项config定向测试通过，包含显式/默认/空目录、坏文件、TOML目录、坏symlink与路径哨兵。权限拒绝断言仅在OS实际强制目录权限时执行，root豁免分支不算权限拒绝证明。真实server config show在有效秘密前缀目录+debug日志下不输出路径哨兵，坏目录子进程exit1、含来源key且无原值/无panic。

实际通过fmt、默认及all-features workspace all-target Clippy -D warnings、默认286 passed/22 ignored与full509/32、warnings-denied双doc、deny、MSRV1.97 all-target/all-features与no-default。未把ignored或无URL早退当真实服务通过；日志前缀/tmp/openrust-r2-config-。

复用既有PG18/MySQL8.4，各12 targets/48 passed/0 ignored及testing database1项；新生成普通/PG auth/MySQL auth三工程check/test，双库各1项真实TCP/隔离SQL/fake mail流程通过；普通生成应用裸run/health200/SIGINT0。八个独立consumer locked test全部通过，cache/communication/SQLite/queue/gRPC实际run，gRPC15项/四类RPC、realtime真实Redis跨进程1项通过；query授权consumer仍仅编译，无SQLite完整支持或供应商实发声明。根裸cargo run再次验证health/HTML/API/example与SIGINT0。

不同模型gpt-6-sol最终只读复审无blocker/should-fix，未替代实际运行。Cargo/锁图/配置值/Redis默认/env优先级/模板行为未改；Provider/schema/核心与模块合并操作仍未实现。

构建达到聚合21.06GiB软限后停止扩展，确认无构建进程并仅对独占/tmp/openrust-r2-config-target dry-run/clean，报告移除57698files/18.8GiB；保留状态续跑未验收阶段。最终root启动后再次dry-run/clean，移除40556files/17.4GiB，磁盘余119GiB。未知root/consumer缓存未触碰；/tmp生成源码与日志保留作诊断证据。

独立于A2，随后确认的容器缓存发布竞态归R2-A3；该修复与新增回归见本文件A3节，不以A2配置门禁覆盖该行为。

## R2-A 类型化容器基元

新增 src/core/container.rs，15项回归实际通过：typed dyn接口、覆盖与bind生命周期、singleton并发单次构造/应用隔离、root scoped/reset与子scope、同链/跨根等待环、取消恢复、嵌套缺绑定码/链、source保留与秘密Debug/HTTP脱敏、在途reset、scope Drop缓存释放、保存Resolver延迟解析、关闭singleton owner/waiter、业务析构重入、弱句柄引用环释放。scope/工厂全用显式句柄，不创建全局容器。

冻结源码后实际执行 cargo fmt --all --check；默认及all-features workspace all-target Clippy -D warnings；cargo test --workspace（279 passed/22 ignored）与all-features（502 passed/32 ignored）；warnings-denied默认/full workspace doc；cargo deny check；cargo +1.97.0 check --workspace --all-targets --all-features；root no-default check，全部通过。忽略和无服务skip不记真实外部服务通过。本簇未改DB/queue/auth/配置加载或CLI模板行为，不将R1双库/生成验证转记新Provider通过。

/tmp/openrust-r2-container-consumer-mqyjcnb4 是仅依赖公开openrust的独立下游，check/test通过，1项真实公开dyn绑定/root reset/scope断言；首轮fixture错误使用Tokio重导出属性宏且未声明直接Tokio包，已改为公开Runtime显式block_on后重跑通过，未因此改变框架API。根argv恰为cargo run，/health、HTML、/api、/api/example各200，SIGINT0。启动仍是旧生产Plugin路径，不把它当新Provider已接入。

不同模型gpt-6-sol逐轮静态审查发现秘密Debug、错误码/链和Resolver强引用环，均修复并补测试；最终冻结弱句柄版本无blocker/should-fix，审查没有替代运行。Error保留内部source，Debug仅code/status，HTTP仍按5xx隐藏详情。无新依赖，Cargo.toml/Cargo.lock未变。

资源：独占/tmp/openrust-r2-target在软限阶段确认无构建进程后dry-run/clean回收63458files/19.3GiB；最终再次dry-run/clean回收45539files/14.1GiB，磁盘余119GiB。未知归属root/consumer缓存未触碰，日志和临时consumer源码保留用于诊断。日志前缀/tmp/openrust-r2-。

本次仅容器基元；Provider/Registry切换、异步shutdown/managed任务与HTTP/job自动scope、配置合并/模块overlay、TestApp和modules迁移均未完成。AppBuilder仍使用唯一旧生产引擎，无新旧运行开关；后继按R2合同和todo继续。

## R1 当前工程边界与验收入口

按 [ADR-073](../decisions/adr-073-root-application-and-example-module.md)，根 openrust package 同时提供框架 src/lib.rs 与默认 server 薄 src/main.rs。default-members=["."]、default-run="server"；Laravel 应用职责直接位于根，唯一 app/example 通过 app/mod.rs 注册薄业务 Plugin。以下是结构迁移子步骤的新执行记录，不表示 R1 综合 CRUD、全能力示例或 Laravel/modules 1:1 目标已完成。

本布局已实际通过 fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、warnings-denied 默认/all-features Rustdoc、deny 和 no-default check。默认 263 passed/22 ignored，全 feature 486 passed/32 ignored；默认不再由独立 example 包隐式强开 auth/grpc 等 feature，不与旧默认测试计数机械比较。根显式 34 个测试 target 保留原框架+demo 的 33 个与原 example auth HTTP 测试；模块注册有/无、普通业务路由、声明错误数据库启动失败回归已执行。底层框架源码、dependencies/features 未改；根锁无新增或升级 registry name/version/checksum，仅移除 20 个不再由根示例 build 使用的工具节点。

根无参数 cargo run 实际验证 /health、公开 HTML、/api 和 /api/example 均 200，业务响应包含 chrono::Utc::now 产生的时间；SIGINT exit 0。现有真实子进程 SIGTERM 退出测试保留。根 CLI config show 转发通过。

PostgreSQL 18.6/MySQL 8.4.13 各执行 cargo test --locked -p openrust --all-features 的 db_connection/db_migration/auth_database/auth_runtime/queue/schedule/auth_mfa/auth_oidc/queue_crash_recovery/schedule_calendar/queue_notifications/example_auth_flows，共 12 targets/48 passed/0 ignored；testing backend database 集成各 1 passed。原 example 的认证/JWT/PAT/入队流程已迁为模块专有测试并通过同一根 Server 装配。第一轮 fixture 未声明 app.name 而沿用默认 OpenRust，预期 openrust 的断言失败；现显式声明 fixture 名称后双库重跑通过，未改框架默认值。使用既有服务和 TestDatabase 隔离库，不修改业务数据或服务配置；测试 helper 的 Drop 清理为 best effort，不能把未能确认归属的历史隔离库批量删除或宣称均已清理。

/tmp 新生成普通/PG auth/MySQL auth 三类应用，均 check/test 通过，双库 auth 各 1 项真实 TCP 流程通过，普通工程公开 resource 宏回归与无参数 cargo run /health 200、SIGINT0 通过。进一步在普通工程仅添加 app/orders、修改 app/mod.rs 注册清单和声明已有 serde/chrono 依赖，src/main.rs 与 bootstrap/app.rs 哈希保持不变；check/test 与真实 /api/orders HTTP、正常退出通过。这是新增模块/第三方业务依赖使用证明，不是完整 CRUD。

根没有示例 build.rs/build-dependencies。模块自有 proto/generator.toml、generator.lock 在 /tmp 临时工具中 cargo run --offline --locked；generate.py --check 实际验证 generated/echo.rs 重生字节一致。生成类型通过 grpc feature 编译，不等于已注册本模块真实 gRPC 服务。

最终 CI 双库命令均通过：每库 cargo test --workspace --features db-<backend> 为 278 passed/22 ignored，auth,db-<backend> 为 324 passed/25 ignored；另显式 cargo test -p openrust --features auth,queue,view,encryption,db-<backend> --test example_auth_flows，各 2 passed/0 ignored。MySQL 使用具备隔离建库权限的测试管理员，未改服务配置。

Rust 1.97.0 的 cargo check --workspace --all-targets --all-features 通过，SQLite/queue consumer 实际 run、gRPC consumer locked check 通过；stable 下八个 consumers locked test/metadata 全通过，metadata 均独立单成员/默认成员。cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项与四类 RPC、realtime 真实 Redis 跨进程 1 项通过。SQLite 仍仅是自定义 driver consumer，不等于完整官方支持。

最终 README 更新后重新执行默认/full workspace fmt/Clippy/tests/docs、deny/no-default，全部通过，计数仍为 263/22 与 486/32；根裸 run/HTTP/退出再次通过。额外干净生成 gRPC 应用 check/test 通过（无业务测试，因此仅证明模板/Proto 编译，不代替真实 RPC）；生成项目锁独立解析，未修改源码根锁。模块 proto --locked 离线重生字节再次一致。代码独立审查的 proto 锁定/教程导入修复已落实；不同模型 gpt-6-sol 最终静态源码复审无 blocker/should-fix，未参与CI编辑的代理独立复核也无 blocker/should-fix，限定提交收口中。

最终确认无 Cargo/rustc/server 进程后，对独占 /tmp/openrust-r1-root-target 先 cargo clean --dry-run 再 cargo clean，实际回收 44051 files/14.1 GiB，磁盘剩余 118 GiB（约13%）。本任务在软限时也只于安全阶段清理该独占目录；未知归属根 target 与预存 consumer 缓存不清。/tmp 生成项目和日志保留作本轮证据。后继未挂载 app/example/extensions.rs 与未审 R2 contract 是独立 WIP，不计本次已验证功能。

以上仅为 ADR-073 过渡路径的 R1-S 结构子交付。最终 modules/example 目录迁移依 ADR-074/R2-M；统一 Provider、完整 CRUD、全能力示例、供应商实发及完整集群尚未交付或不在本子步骤范围，不能转记为这些目标通过。

## R1 ADR-072 历史验收事实

以下是 14c143f 及其前置布局的实际记录，保留原命令和目录以追溯当时证据；它们不是当前可直接运行的路径或本次 ADR-073 验收。

按 [ADR-072](../decisions/adr-072-framework-and-application-projects.md)，根 openrust/src 是框架库，examples/demo 是根 workspace 成员，共用根 Cargo.lock，无嵌套 workspace。根 default-members=["examples/demo"]，根与 demo 目录无参数 cargo run 默认 server；全部原 tests/server 迁入 examples/demo/tests/server。CLI 模板是唯一骨架来源；本轮迁移实现与适用验收已完成。

本轮同一 workspace 已实际通过：cargo fmt --all --check，默认/all-features workspace all-target Clippy，默认/all-features workspace test，默认及 warnings-denied all-features Rustdoc，cargo deny check。默认 361 passed / 26 ignored；all-features 481 passed / 32 ignored。另显式 cargo test -p openrust --lib 为 146 passed / 0 ignored。根锁图仅把原 application 本地节点变为 demo，注册表 name/version/checksum 集合完全不变。

现有 server 进程回归已从 workspace 根启动并验证应用公开 HTML 首页、ready 与 SIGTERM 退出。Cargo 保留调用 cwd，故 demo 薄入口仅在从 workspace 根启动时切换到应用目录；其余 cwd 保持既有部署行为，框架配置 API 不变。

根与 demo 目录均实际执行无参数 cargo run，/health 200、/ 为应用公开 HTML 且 200、SIGINT exit 0。CLI 从 workspace 根按 Cargo metadata 唯一默认成员选择 demo，新增回归测试及实际 config show 转发通过。

真实 PostgreSQL 18.6/MySQL 8.4.13：各以 cargo test --locked -p demo --all-features 执行 db_connection/db_migration/auth_database/auth_runtime/queue/schedule/auth_mfa/auth_oidc/queue_crash_recovery/schedule_calendar/queue_notifications，共 11 targets/46 passed/0 ignored；testing 的 backend database 集成各 1 passed。另按最终 CI 执行 cargo test --workspace --features db-<backend> 与 auth,db-<backend>：各后端分别 373 passed/26 ignored 与 392 passed/26 ignored，包含 demo 与 example 的真实隔离库/HTTP 测试。MySQL 这些入口使用有建库权限的测试管理员；不修改共享服务配置或业务数据。真实复制/完整集群不在本轮验收范围。

/tmp 干净创建普通、PG auth、MySQL auth 三类用户应用，均 check/test 通过；双库 auth 各 1 项真实 TCP/隔离库流程通过，普通工程临时资源宏测试与 cargo run /health 200、SIGINT0 通过。metadata 确认普通工程仅自身、数据库工程仅自身与迁移两个成员，均有自己的 workspace/lock 与单独默认应用，不继承源码 demo 或工具成员；该生成能力不改变框架与 demo 同仓同 workspace 的维护方式。

根 openrust 与 demo 的 no-default check 通过；cargo +1.97.0 check --workspace --all-targets --all-features 通过；同一 MSRV 下独立 SQLite/queue consumer 实际 run 与 gRPC consumer locked check 通过。8 个公开 consumers 均通过 stable locked test/metadata；cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项测试与四类真实 RPC 通过，realtime 1 项真实 Redis 跨节点测试通过。SQLite consumer 仅证明明确拒绝框架迁移锁后用自有 Migrator、表名 scope、连接/事务/回滚，不代表 R6 完整支持。

最终不同模型 gpt-6-sol 只读复审无 blocker/should-fix，核对 6+27 tests、28 个 feature 映射、根默认启动/CLI 生成隔离与 CI；审查未代替以上实际执行。所有当前 Markdown 本地链接/锚点、旧路径与锁图核对通过。本任务 root target 与临时生成项目已清理；root target 随后被后台重新生成约 344 MiB，归属未确认，连同预存且非本任务独占的 consumer 缓存保留。

### 中间布局的历史行为证据

以下记录对应根框架与独立 demo 的中间布局，仅保留命令行与真实服务行为证据，不作为同一 workspace 已验收。

中间布局实际通过：框架 fmt、default/all-features all-target Clippy、tests、warnings-denied docs、deny；workspace 默认 344 passed / 26 ignored，all-features 387 passed / 30 ignored。独立 demo 默认与 all-features Clippy/test/doc 通过，分别 16 passed / 0 ignored 与 93 passed / 2 ignored。忽略和因环境缺失提前返回的用例不算真实服务通过。

真实 PostgreSQL 18/MySQL 8.4 各执行 `cargo test --locked --manifest-path examples/demo/Cargo.toml --all-features --test db_connection --test db_migration --test auth_database --test auth_runtime --test queue --test schedule --test auth_mfa --test auth_oidc --test queue_crash_recovery --test schedule_calendar --test queue_notifications -- --nocapture`：各 11 targets / 46 passed / 0 ignored；另 testing package 的 backend database 集成各 1 passed。使用既有服务、TestDatabase 隔离数据，不修改共享容器/配置。复制 lag 分支未提供拓扑，不算真实复制验收。

干净生成普通、PostgreSQL auth、MySQL auth 三工程，均 check/test 通过；认证两工程各有 1 项真实 TCP/隔离库测试。普通工程以临时下游资源宏测试验证公开 openrust 路径，普通 cargo run 后 /health 200、SIGINT exit 0。metadata 确认普通工程仅自身一个成员、数据库工程仅自身与迁移两个成员，workspace 根均为生成目录，不继承开发 exclude；框架本地路径指向仓库根。

独立 demo 应用根无参数 cargo run 后真实 TCP /health 200、SIGINT exit 0。上述命令与 all-features server 子进程测试串行执行，无二进制覆盖。根/独立 demo 的 no-default check、Rust 1.97 all-target/all-features check 与 demo cargo deny 均通过，deny 仅保留既有依赖重复告警。8 个独立 consumers 均 locked metadata/test 通过；cache/communication/SQLite/queue/gRPC 实际 run 通过，gRPC 15 项测试与四类真实 TCP RPC 完成，realtime 1 项 Redis 跨节点测试通过。SQLite 原示例错误期待框架迁移互斥支持 SQLite，现仅修 consumer：断言明确拒绝后执行自有 Migrator/表名 scope；连接/事务/回滚仍通过，不算 R6 官方支持。gRPC 多 bin 的 CI/README 已明确选 grpc-example，应用默认 cargo run 不受影响。

不同模型独立审查发现 MySQL CI 普通用户缺 CREATE DATABASE 权限，新增 demo 整组已改 root；该记录为中间状态；最终同一 workspace 已复验修后双库 feature 命令，结果见本节开头。

## R1 旧布局验收事实（ADR-071）

以下保留旧 ADR-071 根 application/server 与 framework/openrust 布局的实际验收；不能作为 ADR-072 新布局通过。目录与分发依据旧 ADR-071。根为 application/server，框架库为 framework/openrust，默认成员只有根应用；apps/example 已迁为 examples/example。下文保留的原专项记录是迁移前行为证据，含 `cargo test -p openrust --test ...` 的旧命令；当时应用集成 target 使用 `-p application`，框架单元测试仍用 `-p openrust --lib`。

实际执行：fmt，默认/all-features workspace all-target Clippy，默认/all-features workspace test，默认及 warnings-denied all-features Rustdoc，cargo deny；MSRV `cargo +1.97.0 check --workspace --all-targets --all-features`，no-default-features check。依赖审计仅保留既有重复版本告警。最终默认 workspace 349 passed / 26 ignored，all-features 480 passed / 32 ignored；忽略或缺服务导致 skip 不计真实服务通过。

真实 PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p application --all-features --test db_connection --test db_migration --test auth_database --test auth_runtime --test queue --test schedule -- --nocapture`，各 6 targets、31 passed、0 ignored；从现有服务读取授权测试连接，使用 TestDatabase 隔离数据，不改共享容器/配置。数据库连接 target 中依赖真实复制的分支仍有独立环境门禁，本次未提供复制拓扑；完整复制/集群不属于 R1 新验收。

干净 `/tmp` 默认生成应用 check/test 通过，普通 cargo run 后真实 TCP `/health` 为 200、SIGINT 正常退出。PG/MySQL 认证生成应用分别 check/test 通过，`generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 各真实执行 1 项；使用当前模板、database/migrations 与 resources/views。发现模板 fake 配置缺 view 段后已同步修复模板/综合示例并重新生成验收。公开 `tests/consumers/cache-custom-driver` 已通过 locked 实际运行，所有 consumer lock 同步本地框架依赖图。

根普通 cargo run（无 --bin/--package）真实 TCP `/health` 返回 200，SIGINT exit 0。

验证隔离：默认 cargo run 与 all-features 子进程测试禁止交叉占用同一 target/debug/server。首次 all-features metrics 子进程超时发生在并行默认 cargo run 覆盖二进制期间；停止交叉运行后整组重跑通过。此项不是新语义验收或放宽超时。原真实供应商/复制/集群、SQLite、最低后端版本与 R2 以后 contract 缺口继续保留。

## M5 WebSocket 与 SSE 跨节点路由

按 [ADR-059](../decisions/adr-059-realtime-transport.md) 启用 `realtime` / `realtime-redis`。在本机已有 Redis 8.10.2 上通过独立示例执行：

```bash
OPENRUST_REALTIME_TEST_REDIS_URL=redis://127.0.0.1:6379/ \
  cargo test --locked --manifest-path tests/consumers/realtime/Cargo.toml --test cluster -- --nocapture
```

2026-10-27 实际通过：两个独立 API 子进程经 Redis Pub/Sub 路由 WebSocket 和 SSE；真实 TCP 客户端验证 Origin allowlist、跨节点用户撤销、SIGTERM 时两类会话关闭与进程退出。测试随后启动一个 Redis 地址为 `127.0.0.1:1` 的 API 进程，确认在线发布返回 `realtime_transport_unavailable`，不存在本地静默成功。另 `cargo test --manifest-path examples/demo/Cargo.toml --features realtime --test realtime -- --nocapture` 通过 AuthLayer、Origin 与 SSE 响应头聚合 HTTP 测试；实时 service 单元测试 4 项覆盖过期、载荷/Origin 限制、连接投递/撤销、慢消费者上限。

本簇门禁实际通过：workspace fmt、默认/all-features all-target Clippy、默认/all-features workspace tests、warnings-denied all-features workspace Rustdoc、根与独立示例 cargo-deny、独立示例 all-target/all-features Clippy 及 `git diff --check`；真实跨节点命令如上。cargo-deny 的 advisories/bans/licenses/sources 均通过，保留既有重复依赖与未匹配 MPL allowance 警告。整组验证结束时根 `target/` 与 `tests/consumers/realtime/target/` 合计约 13 GiB，所在卷可用 86 GiB（低于 10%）；无构建进程、Cargo dry-run 分别预览根 target 12.5 GiB 与示例 target 2.7 GiB 后，对两处 target 执行 Cargo clean，回收约 15.2 GiB；清理后仓库 target 均为空，卷可用空间为 99 GiB（约 10.7%）。软告警剩余量由仓库外磁盘占用造成，未删除其他文件。

该证据不代表 Redis 断开期间 Pub/Sub 可恢复消息，也不代表 PostgreSQL/MySQL 全栈集群、负载均衡、worker/scheduler、滚动升级或性能门禁通过；M6 仍需复验。用法与边界见 [tests/consumers/realtime/README.md](../../../tests/consumers/realtime/README.md)。

## M5 gRPC G1 基础与生成

按 [ADR-040](../decisions/adr-040-grpc-integration.md) 增加可选 `grpc` feature、配置校验、服务注册/冲突检测、默认及具名共享 Channel、`grpc serve` 命令、应用自有 proto/build.rs 生成和 loopback TCP 示例。默认 feature 不启用 tonic/prost/protoc；G1 只接受明文 `http://` client endpoint，TLS/认证/health/配额由 G2 交付。

服务端集成测试 3 项通过（注册与真实 TCP 生命周期、重复名称拒绝、未注册服务失败）；独立示例实际执行 unary、server stream、client stream、bidirectional stream、半关闭与断连清理。CLI `--grpc` 模板测试通过；在无 `.git`、无 target 的干净源码副本生成应用后 `cargo check` 和 `cargo test --locked` 通过，无绝对路径或 registry patch。默认 workspace 测试与 `RUST_TEST_THREADS=1 cargo test --workspace --all-features` 通过；all-features 包含真实 Redis targets。all-target/all-features workspace 与独立示例 Clippy、warnings-denied 默认/all-features Rustdoc、workspace 与独立示例 `cargo deny`、fmt/diff check 通过。deny 仅保留既有重复依赖版本及未匹配 MPL allowance 告警。`cargo +1.97.0 check` 的框架 gRPC feature 与独立示例均已通过；默认依赖树不含 tonic/prost/protoc。未覆盖 G2 安全语义和 G3 多节点/联合运行。

## M5 gRPC G2a TLS/mTLS

G2a 已在真实 loopback TCP 上验证服务端 TLS、具名客户端 CA/域名校验、双向 TLS 客户端身份以及错误 CA、错误域名、过期服务端证书、缺失/不受信客户端证书的拒绝。示例测试由 `rcgen` 临时生成证书，不依赖外部凭据。配置单测覆盖明文/HTTPS 与 TLS 材料不匹配、客户端身份证书配对、默认具名端点校验和 Debug 脱敏；`examples/demo/tests/server/grpc.rs` 覆盖未显式 opt-in 时明文 server 拒绝。应用生成模板只在开发配置显式允许 loopback 明文。

本簇已通过 `cargo test --workspace`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、独立示例 Clippy、默认/all-features warnings-denied Rustdoc、workspace 与独立示例 `cargo deny`、Rust 1.97 默认与 gRPC feature check、独立示例 `--locked` check/test/run、干净源码副本生成应用 check/test、`cargo fmt --all --check` 和 `git diff --check`。`cargo deny` 的重复版本与未命中 MPL allowance 为已有提示，advisories/bans/licenses/sources 均通过。未覆盖身份/授权、deadline/health/shutdown 或多节点验收。

## M5 gRPC G2b 身份与错误映射

`tests/consumers/grpc/` 的真实 TCP RPC 测试覆盖已签名 JWT、数据库 PAT 和测试替代 Authenticator 路径，包含缺失/无效/重复 Authorization、Cookie Session 不参与 RPC、JWT/PAT `CurrentUser` 与 PAT abilities、Policy 授权、缺 ability 拒绝及 PAT 撤销后拒绝。PostgreSQL 18 与 MySQL 8.4 的隔离数据库各自真实执行 `database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp`，通过 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --features db-<backend> database_auth_tests::database_bearer_and_method_policy_work_over_real_tcp -- --nocapture`；测试贯通真实 `AuthPlugin`、JWT 验签、PAT 读写/撤销、Policy 表、gRPC TCP handler 和 Status。错误映射单测覆盖 validation、permission 与内部错误脱敏；网络用例确认状态码为 `UNAUTHENTICATED`/`PERMISSION_DENIED`，不回传消息或凭据。

通过 `cargo fmt --all --check`、`cargo clippy -p openrust --features 'grpc auth' --all-targets -- -D warnings`、独立示例 all-target/all-features Clippy、默认与 all-features workspace tests、独立示例 all-features tests、warnings-denied 默认/all-features Rustdoc、workspace 与独立示例 `cargo deny`、Rust 1.97 gRPC+auth check、独立 consumer locked check/test、PostgreSQL/MySQL 真实 TCP 测试及 `git diff --check`。全功能 workspace 与 cargo-deny 保留既有明确记录的重复 crate 版本和未命中 MPL allowance 提示；advisories/bans/licenses/sources 通过。未覆盖 deadline、全局限额、health/reflection、有界停机或集群。

## F8-A CSV/XLSX 解析与转换

- 定向入口：`cargo test -p openrust --features data-exchange --lib data::exchange::tests`、`cargo test --manifest-path examples/demo/Cargo.toml --features data-exchange --test data_exchange`。
- 独立 consumer：`cargo check --manifest-path tests/consumers/data-exchange/Cargo.toml --locked` 与 `cargo test --manifest-path tests/consumers/data-exchange/Cargo.toml --locked`；示例通过相对 path 依赖使用公开 API，不含 registry patch 或开发者机器绝对路径。
- 核心测试 8 项通过，覆盖 CSV BOM/类型/逐行错误/重复表头/输入上限/行与全局错误上限/callback 失败/公式注入，XLSX 类型保真/公式拒绝/shared string 声明上限/宏拒绝/ZIP 展开上限，以及 根目录集成宿主 feature 接入 1 项通过。
- `cargo fmt --all --check`、默认和 all-features workspace clippy/tests、warnings-denied 默认与 all-features Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过。cargo-deny 保留既有 duplicate dependency 与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。
- F8-B 的双库任务化验收见下文“F8-B 数据交换任务”。

## F8-B 数据交换任务

`examples/demo/tests/server/data_exchange_tasks.rs` 使用 PostgreSQL 18 和 MySQL 8.4 的隔离测试库、真实 `DatabasePlugin`/`QueuePlugin` 与本地 Storage driver。两个后端均实际执行 `import_batches_are_atomic_idempotent_and_export_is_streamed` 并通过。测试覆盖 CSV 分批事务、重复请求回放、第二批失败后的部分报告与重试恢复、取消后的报告和续跑、字段错误脱敏、tenant 作用域隔离、导出流式落盘、已取消导出不发布文件、attempt 路径隔离、过期代际保护、清理时对象锁忙拒绝删除，以及 QueuePlugin 自动注册并执行报告清理 job。对象上传完整返回后才写保留标记并排期清理；SharedLocks 租约每 5 分钟续期、单次续期有 30 秒超时。续租失败会令任务返回错误，但不会丢弃可能已提交的 Storage future，故不保证数据库长时间不可用时外部 Storage exactly-once。已通过命令：

```bash
cargo test --manifest-path examples/demo/Cargo.toml --features 'data-exchange-tasks storage-local db-postgres' --test data_exchange_tasks -- --nocapture
cargo test --manifest-path examples/demo/Cargo.toml --features 'data-exchange-tasks storage-local db-mysql' --test data_exchange_tasks -- --nocapture
```

独立 consumer `tests/consumers/data-exchange/` 启用可选 feature，通过 `cargo check --manifest-path tests/consumers/data-exchange/Cargo.toml --locked` 与 `cargo test --manifest-path tests/consumers/data-exchange/Cargo.toml --locked`。consumer 演示应用从受保护查询生产行并交给有界导出 sink；框架不代替应用授权，也不提供下载路由。事务只保证单个导入批次内业务写入与幂等完成标记原子；已提交批次在取消或后续失败时保留，导入副作用至少一次，外部副作用仍须由应用自行幂等。`RequestIdempotency` 现有上限为 24 小时；报告保留默认 7 天、最多 30 天。

## M5 可选外部身份：OIDC（F7 子簇）

`cargo test -p openrust --features auth-oidc auth::oidc::tests --lib` 覆盖 ID Token 签名、issuer/audience/nonce/`at_hash`、未知 `kid`、不安全授权 URL 拒绝与 Debug 脱敏。`examples/demo/tests/server/auth/oidc.rs` 的浏览器绑定、回调单次消费、显式绑定/解绑、邮箱冲突与审计事务测试在 PostgreSQL 18 和 MySQL 8.4 各真实通过 1 项。启用 `auth-mfa` 后，同一真实数据库用例还贯通本地账户注册/验证、TOTP enrollment、外部身份绑定、关联 MFA challenge、完成第二因子后自动绑定 refresh family、关联 Session 及 RP logout 撤销；完成 MFA 前确认没有 refresh family。provider fake 的隔离库测试还验证未登记 post-logout redirect 在本地撤销前被拒绝、拒绝后 refresh 仍可轮换；IdP 未提供 logout endpoint 时只完成本地 refresh/Session 撤销并返回无远程 URL。分别运行 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa db-postgres' --test auth_oidc -- --nocapture` 与 `OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa db-mysql' --test auth_oidc -- --nocapture`。测试 URL 需要具备创建/删除隔离库权限。

真实 IdP 基础验收使用 Keycloak 26.8.1 镜像和 `examples/demo/tests/server/fixtures/keycloak/openrust-realm.json`，通过同一浏览器 cookie 完成两个独立 client 的 SSO：先交互式登录主 client，再对次 client 执行授权码、S256 PKCE、token exchange、RS256/JWKS、issuer、nonce 与相同 subject/email 验证。启用退出验收时还需设置管理员凭据与可创建/删除隔离库的 `OPENRUST_TEST_DATABASE_URL`。详细多副本强制验收命令与结果见下段；一般 workspace 测试在未设置 issuer 时会显式 skip，不计为真实服务通过。JWKS 最长缓存 5 分钟；过期刷新失败时 fail closed。

OIDC 子簇及并发 storage 回归修复的门禁：`cargo fmt --all --check`、默认与 all-features `cargo clippy --workspace --all-targets --... -D warnings`、`cargo test --workspace`、`cargo test --workspace --all-features`、默认/all-features warnings-denied workspace Rustdoc、`cargo deny check` 均通过。`cargo deny check` 输出仅有重复依赖版本告警，advisories/bans/licenses/sources 均为 ok。all-features 聚合 server 测试 10 项通过；workspace 测试中无 IdP 环境的 Keycloak target 属于显式 skip，不计入上面的真实服务证据。

SSO 强制验收使用真实 Keycloak 26.8.1、PostgreSQL 18/MySQL 8.4、Redis 8 和两个隔离应用（每个应用两个 API 子进程）。callback、MFA 与 logout handler 经真实 loopback TCP round-robin；双库均通过双 client SSO、callback/logout 并发（挂起 callback 在 logout 后返回 401）、MFA completion/logout 竞态（challenge 撤销后返回 401）、refresh/logout 竞争、每个子节点拒绝已撤销 Session/refresh、重启节点后 Token 重放幂等，以及 15 秒 access token 在退出后仍可验签并按 `exp` 到期拒绝。OIDC 双库测试还在真实 Redis Session store 上注入一次 delete error，确认同一通知重放可完成清理；此项与物理 Redis 断连分开记录。真实断连验收与结果见本节 2026-10-27 F7 记录。复跑需 `OPENRUST_REQUIRE_SHARED_SESSION=1`、`OPENRUST_TEST_SESSION_REDIS_URL`、`OPENRUST_REQUIRE_OIDC_IDP=1`、`OPENRUST_OIDC_ISSUER`、Keycloak 管理员变量、双库 `OPENRUST_TEST_DATABASE_URL` 与 `RUST_TEST_THREADS=1`；命令见本节上方。现有 PostgreSQL 18、MySQL 8.4、Redis 8 服务未停止。

## M5 ADR-054 TOTP MFA 与恢复码（F7 子簇）

`cargo test -p openrust --features auth-mfa auth::mfa::tests --lib` 覆盖 RFC 6238 SHA-1 向量、6 位格式及允许时间步边界。`examples/demo/tests/server/auth/mfa.rs` 的真实 TCP/数据库生命周期测试在 PostgreSQL 18 与 MySQL 8.4 各通过 1 项：`OPENRUST_TEST_DATABASE_URL=... cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-mfa auth-oidc db-<backend>' --test auth_mfa --test auth_oidc -- --nocapture`。覆盖 password 与 OIDC challenge、Session 登录/既有 Session 阻断、挑战来源绑定/过期/五次失败、TOTP replay、恢复码单次消费、enrollment/禁用/重置、密钥轮换、审计与 refresh 撤销；服务端测试还验证此前密码登录 helper 无法绕过 MFA。

CLI 新增 `openrust new --db <backend> --auth --auth-mfa --encryption`。把当前 tracked/untracked 源码复制到干净临时 workspace（无 registry patch、无开发者绝对路径），分别生成 PostgreSQL 与 MySQL 应用；两个生成包均实际通过 `cargo check` 和 `cargo test`，其中生成的完整 auth HTTP/fake-mail 测试各执行 1 项并使用 PostgreSQL 18 / MySQL 8.4 隔离数据库。`cargo test -p cli` 18 项通过。

MFA 子簇门禁：fmt、默认/all-features workspace all-target Clippy、默认/all-features workspace tests、默认/all-features docs（all-features warnings denied）、no-default-features check、`cargo deny check` 均通过。deny 仅有既有重复 crate 版本与未匹配 MPL-2.0 allowance 警告；advisories、bans、licenses、sources 均通过。Twilio 真实发送仍受外部授权凭据/目标约束，与 MFA 验收无关；Keycloak 单 client OIDC 真实验收见上一节，完整 SSO 为后续独立 F7 子簇。

## M5 缓存驱动与命名实例

`cargo test -p openrust cache::tests` 通过 memory namespace/TTL/remove、file key hash/过期/替换/并发、命名 stores 和外部 driver 注册；`cargo test -p openrust --features cache-redis cache::tests::redis_store_roundtrips_ttl_on_live_service -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --all-features --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact` 均在本机 Redis 8.10.2 真实服务通过。CLI 实测复用可达 loopback Redis，不触碰 Compose；干净 `/tmp` 源码副本生成默认应用后 `cargo check`、`cargo test` 通过，独立 custom-driver 示例 `cargo test/run --manifest-path tests/consumers/cache-custom-driver/Cargo.toml` 通过。默认与 all-features workspace tests、all-target/all-features clippy、fmt、warnings-denied 默认/all-features docs、cargo deny 与 no-default-features check 通过。workspace server 的数据库集成 target 因未设置 `OPENRUST_TEST_DATABASE_URL` 跳过；本缓存簇不修改数据库路径，不以该 skipped target 代替双库验收。Redis 启动 Compose 的冷启动分支尚未在现有可用 Redis 环境下触发；已验证的是复用路径。Redis 最低兼容版本仍待 v1 发布前专项确认。

## M5 F4a 缓存 tag generation

`cargo test -p openrust cache::tests` 覆盖 memory generation 失效、命名空间隔离、旧 entry 写入拒绝、无能力 file driver 拒绝；Redis 真实服务测试使用两个独立 Redis 连接验证共享读取、代际失效和旧 entry 回填被拒绝，`根目录集成宿主` 聚合测试通过真实 Redis 验证应用集成。Redis tag generation 不设置 TTL，tagged value 必须有 TTL；未创建真实跨进程双节点测试，F4c 最终矩阵仍需补足。F4a 的 workspace default/all-features tests、clippy、warnings-denied docs、fmt、cargo deny 和 no-default check 均通过；数据库集成测试因未配置 `OPENRUST_TEST_DATABASE_URL` 跳过，与本簇无关。

## M5 命名数据库连接实例（ADR-045）

`cargo test -p openrust --features db-postgres named_instance -- --nocapture`、`database_selector` 与 `migration_action` 定向测试通过。`cargo test --manifest-path examples/demo/Cargo.toml --features db-postgres --test db_migration named_database_instances_keep_migrations_and_transactions_isolated -- --exact --nocapture` 和 MySQL 对应命令各通过 1 项，使用 PostgreSQL 18 与 MySQL 8.4 真实隔离库。测试同时配置 default、analytics、reporting 三个独立实例，验证连接身份、默认句柄兼容、具名副本读取、prefix、显式 migration 目标、未迁移表隔离及事务数据不串用。具名副本使用对应实例自己的隔离数据库 URL，验证的是实例绑定边界，不是物理复制延迟；复制语义由读写分离专项另行覆盖。

干净源码副本生成 `apps/named_database_sample` 后直接 `cargo check -p named_database_sample` 与 `cargo test -p named_database_sample` 通过；扫描应用及根 manifest 未发现临时 registry patch 或机器绝对路径。默认/all-features workspace tests、all-target clippy、warnings-denied docs、fmt、cargo deny 与 no-default-features check 通过。`cargo deny` 保留既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未覆盖跨数据库事务、自动查询路由或生产集群负载。

## M5 Redis Session driver

启用 `session-redis` feature，通过 `RedisSessionStore::connect(dsn, namespace)` 与 `SessionPlugin::with_store` 显式使用 Redis；数据库 Session store 仍为默认。真实 Redis 8.10.2 测试命令：`cargo test --manifest-path examples/demo/Cargo.toml --features session-redis --test auth_runtime redis_sessions_are_shared_between_apps_and_revoke_across_instances -- --exact --nocapture`。PostgreSQL 18 和 MySQL 8.4 分别运行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend>,session-redis --test auth_database --test auth_runtime`，各 14 项通过（auth_database 7、auth_runtime 7）；两个独立 App 使用两个 Redis connection manager，共享同一 namespace。验证真实 Redis 创建/读取/TTL 过期/删除、重复创建拒绝、删除后 save 不复活、不同 namespace 隔离、ID 轮换清除旧记录、跨 App 登出撤销、CSRF 缺失拒绝与有效 token 放行；namespace 校验、URL/Debug 脱敏及不可达 endpoint 的启动失败也覆盖。用例走访现有 Redis 8 服务，没有修改或停止容器。

`cargo +1.97.0 check -p openrust --no-default-features --features session-redis` 与 server feature check 通过。默认/all-features workspace clippy、test、warnings-denied docs、fmt、no-default check 与 `cargo deny check` 通过；deny 仍有既有重复版本与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 均通过。两个 App 在同一测试进程内，未代表多进程集群门禁；Redis 6.2 最低版本兼容和在线 Redis 故障期间的端到端恢复留待发布/集群验收。

## M5 Redis Queue driver（ADR-046）

本次使用已运行的真实 Redis 8.10.2 服务。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis_driver` 3 项通过：四个独立连接 worker 对 40 个任务竞争、活动唯一键、失败保留/重试/幂等 retry ID/进度、独立子进程强杀后租约到期恢复、非法 namespace 与不可达服务失败关闭。`cargo test --manifest-path examples/demo/Cargo.toml --features queue-redis --test queue_redis` 2 项通过：Redis QueuePlugin 无需 DatabasePlugin；存在数据库 scheduler 注册时 build 失败。另用 `examples/queue` Redis-only 启动 API，真实 TCP `POST /jobs` 返回任务 ID，再由独立 worker 使用同一 namespace 运行；两个任务进程均正常响应 Ctrl-C。

`cargo fmt --all --check`、默认及 all-features workspace clippy、默认及 all-features `cargo test --workspace`、warnings-denied 默认及 all-features docs、`cargo check -p openrust --no-default-features` 和 `cargo deny check` 通过；all-features tests 运行 Redis targets，workspace 其他 PG/MySQL integration tests 在未提供 URL 时依项目约定 skip。deny 输出有既有重复版本与 unmatched MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。Redis driver 不访问 SQL，故没有把数据库队列既有 PG18/MySQL8.4 验收冒充为 Redis 双库测试；Redis Cluster 分片、Redis 6.2 最低版本、持久化/故障切换及生产多节点负载仍待 v1 总门禁。

## M5 HTTP 有界运行与就绪切换

2026-10-26 定向测试：`cargo test -p openrust serve::tests -- --nocapture` 覆盖未绑定/绑定就绪、drain 中停止接入、在途请求完成、非合作请求到期取消及 bind 失败；`cargo test --manifest-path examples/demo/Cargo.toml --test server serve_process_exits_successfully_after_sigterm -- --nocapture` 通过真实 TCP readiness 和真实子进程 SIGTERM，all-features 下同一测试额外注入仅供测试的加密 key 后通过。全量门禁：fmt、diff check、all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check` 和 `cargo check -p openrust --no-default-features` 均通过。deny 输出既有重复 crate 版本与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。此项未改数据库/队列协议，不需要双库测试；未覆盖集群依赖 readiness 或跨节点 drain。

## ADR-039 源码交付验收

当前 workspace packages 为根 `openrust` 与 `tools/` 下的 `cli`、`macros`、`testing`、`performance`；根 package 内含 `server` bin，所有 package 均显式禁用发布，内部依赖通过 workspace path 解析。仅根 package 是 `default-members`，以保证裸 `cargo run` 选择 `server`；完整验证显式使用 `--workspace`。2026-10-27 目录收纳后，Cargo metadata、CLI fixture 与 workspace 检查确认这五个 package 及 `db_routing` bench target；PostgreSQL 与 MySQL feature 转发的 benchmark 编译检查通过，真实数据库 benchmark 未因目录调整重跑。CLI 保留 `openrust` 命令，并默认生成 `apps/<name>`，应用与 migration 加入现有 workspace。生成器保留根 manifest 其他内容，拒绝重复名、现存目录和不支持的外部目标；dev/serve 按目标应用定位二进制和配置。

2026-10-26 干净源码副本（无 `.git`、临时 registry patch 或开发机绝对路径）生成默认、PostgreSQL、MySQL 与加密认证应用。默认应用 `cargo test` 通过；PostgreSQL 18 和 MySQL 8.4 的加密认证应用各通过 `cargo test`，真实数据库 HTTP auth flow 均执行 1 项通过。两个 workspace 应用以不同配置及端口分别启动，HTTP 根路由返回各自应用名，证明配置和启动目标隔离；CLI 单测覆盖重复/非法/外部目标失败无半成品。

本仓库最终门禁：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、`cargo deny check`、`git diff --check` 均通过。另在真实 PostgreSQL 18 与 MySQL 8.4 服务分别执行 openrust/testing/server 的数据库、认证、队列、崩溃恢复与调度 feature 集成测试并通过。`cargo deny` 仅有既有重复依赖版本和未使用 MPL allowance 警告；advisories/bans/licenses/sources 均通过。C1–C3 文档整改不属于本簇。

以下 `-p openrust-*` 命令只代表历史验收；当前命令使用新包名。历史临时 patch 不作为 ADR-039 黄金路径证据。

## 本地 workspace 门禁

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
cargo deny check
```

按 feature 独立检查框架包，避免只依赖 `--all-features`：

```bash
cargo check -p openrust --no-default-features
cargo check -p openrust --no-default-features --features db-postgres
cargo check -p openrust --no-default-features --features db-mysql
cargo check -p openrust --no-default-features --features auth
cargo check -p openrust --no-default-features --features mail-smtp
cargo check -p openrust --no-default-features --features encryption
cargo check -p openrust --no-default-features --features test-util
cargo check --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```

应用数据加密专项：`cargo test -p openrust --features encryption security::tests`；生成器/密钥命令：`cargo test -p cli`；`根目录集成宿主` 接线：`cargo check -p openrust --bin server --features encryption`。在干净源码副本中生成的 `openrust new --auth --encryption` PostgreSQL/MySQL 应用均通过直接 `cargo test`，未添加临时 registry patch。

根 package 示例：`cargo run -p openrust --example hello`；应用配置可用 `APP_SERVER__PORT` 指定隔离端口，HTTP smoke 访问 `/` 应返回统一 JSON 200。

## PostgreSQL / MySQL

CI 数据库服务作业分别启用 PostgreSQL/MySQL 的 `queue` feature，并显式运行 `db_migration`、`queue`、`queue_crash_recovery`、`schedule` 和 `schedule_calendar` targets。普通 all-features 作业无数据库 URL 时仍可能跳过真实集成；需以数据库 job 命令实际执行结果作为证据。

暂定最低支持为 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常开发与 Compose 基线为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；常规 CI 数据库基线为 PostgreSQL 18、MySQL 8.4。每次数据库/缓存集成测试前，先盘点当前可用的 Docker 容器和本机服务，确认版本、健康状态、监听端口及测试所需连通性，然后优先复用合适的现有服务。不要因使用项目 Compose 更方便就重复创建基础服务。

M4 队列集成测试使用 `OPENRUST_TEST_DATABASE_URL`，由 `testing::TestDatabase` 建立隔离数据库并在测试后清理。命令：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres' --test db_migration --test queue --test queue_crash_recovery`；MySQL 将 `db-postgres` 换为 `db-mysql`。2026-10-26 在现有 PostgreSQL 18 与 MySQL 8.4 容器实测迁移、并发 worker/dispatch、事务派发提交与回滚、进度、失败与幂等重试、单任务超时后继续处理，以及独立子进程强杀后的 lease 到期恢复。`queue_crash_recovery` 当前只验证崩溃恢复；旧 worker 与新 token 并发接管 fencing 的专项竞态断言及完整跨节点验收仍未完成；调度的独立进程证据见下段。凭据由容器环境变量读取，没有输出到日志；既有服务未重建或停止。

2026-10-26 Worker 合作停机修正：先运行真实 PostgreSQL red test，观察到 SIGTERM 后合作 handler 未完成确认；实现后在 PostgreSQL 18 与 MySQL 8.4 分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'db-<backend>,queue' --test queue_crash_recovery`，各 5 项通过；并执行 `--test queue`，各 1 项通过。真实 worker 子进程 SIGTERM 覆盖合作任务完成并确认、不合作任务有界超时并释放 lease、同队列 pending job 不再领取；测试子进程关闭其 SeaORM pool 模拟连接不可用，worker 返回 `internal_error` 且保留 owner lease；共享数据库并发测试让新 worker 接管过期任务，旧 token 不能确认新 worker 已完成的任务。旧 queue target 的 PG 首次复跑发现连接池关闭与隔离库删除竞争；测试现在显式关闭 SeaORM pool，随后双库 targets 均通过。临时服务 `openrust-m4-auth-tx` 为本任务创建并保留供后续数据库簇复用。

M4 scheduler 集成入口：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres' --test schedule`，MySQL 使用 `db-mysql`。2026-10-26 两库各通过 4 项测试：两个独立进程经同步屏障竞争同一 UTC interval slot、强制异常退出后保持已提交 slot、`schedule run --once` 命令、常驻命令 SIGTERM 退出；均验证隔离数据库只产生一条 schedule run 和一条 job。另有 interval/daily slot 边界单测。scheduler 使用共享数据库时钟和唯一键；该验收不替代生产负载资源测量或最终集群部署验收。

F1 日历调度：`cargo test -p openrust --features queue schedule::tests` 的 4 项通过，覆盖春季 DST gap、秋季重复 wall minute、闰日、语法/时区拒绝及补跑配置上限；`queue::tests::cron_registration_rejects_invalid_schedule_configuration` 另验证公开注册入口拒绝无效配置。PostgreSQL 18 与 MySQL 8.4 分别运行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'db-<backend>,queue' --test schedule_calendar`，各 4 项通过：独立进程同刻竞争同一 cron 槽、表达式及 IANA 时区版本变更、实际 DST 数据库槽（春季跳过、秋季重复分钟仅存一条）、跨 26 小时停机后按每 tick 两项上限补齐精确积压数。集成目标显式执行且未跳过；不替代生产负载或完整集群验收。

2026-10-26 队列唯一键改动后，PostgreSQL 18 与 MySQL 8.4 分别重跑 queue migration 和 queue 集成测试通过。额外验证同 key 并发 dispatch 返回同一活动 job ID、完成后可再次创建、以及同一业务事务中重复 dispatch 只创建一个 job。唯一键只抑制活动任务重叠，不提供完成后持久化的请求幂等历史。

2026-10-26 M4 任务链/批次簇：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 与 `git diff --check` 通过。PostgreSQL 18 和 MySQL 8.4 各自重跑 `db_migration`、`queue`、`queue_crash_recovery` 集成测试；覆盖 chain 顺序、事务派发、Stop/Continue、batch 并发上限、失败语义与输入边界。cargo-deny 仅报告既有重复依赖版本和未使用 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未执行生产资源或多节点集群测试。

2026-10-26 M4 队列化通知簇：PostgreSQL 18 (`queue,db-postgres`) 与 MySQL 8.4 (`queue,db-mysql`) 分别执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-<backend>' --test queue_notifications`，各 1 项通过。真实隔离库验证多渠道业务事务回滚不留任务、提交后两个渠道分别得到稳定活动唯一任务、worker 从应用状态取 Mailer/SmsSender 并分别处理；模拟邮件供应商已接受但确认丢失，验证队列立即不重入、按既有退避重试且外部副作用可能重复。PG18 另执行 SMTP 专项：`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-postgres,mail-smtp' --test queue_notifications queued_notification_is_accepted_by_configured_smtp_sink -- --exact`，Mailpit v1.32.0 接受实际 SMTP 发送；其 API 回读 recipient、subject、text、HTML 均与固定入队内容一致。SMTP sink 由本簇临时容器提供，运行于隔离端口；不是外部邮件供应商验收。Twilio 实发未授权且未执行。默认与 all-features workspace tests、all-target/all-features warnings-denied clippy、warnings-denied all-features docs、fmt 和 `cargo deny check` 通过；deny 仅报告既有重复版本与 unused allowance 警告，advisories/bans/licenses/sources 均通过。all-features workspace run 未设置数据库 URL，其数据库用例不替代上列双库显式运行。未执行生产负载或完整集群验收。

2026-10-26 目录收尾：迁移前后 `cargo metadata --no-deps` 均列出 5 个 workspace packages；all-features `cargo test -- --list` 均为 265 个条目、target/test 名保留。迁移后 workspace test、all-target clippy、warnings-denied docs、`cargo deny check`、fmt 和 `cargo check -p openrust --no-default-features` 通过。PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p openrust --all-features`，40 项真实 server 集成测试通过。CLI 生成的 PostgreSQL/MySQL `--db --auth` 项目各自经本地 `openrust`/`testing` path patch 通过 fmt、check 和真实数据库 `auth_flows` HTTP test；默认生成项目 check/test 也通过。hello 示例作为真实 TCP HTTP 服务返回 200 JSON。queue/Scheduler 双库命令已加入 CI；未宣称生产集群或最低版本总验收。

2026-10-26 M3 Mailer/Policy 生成器：CLI 单测 13 项通过，覆盖命令解析、输出结构、CRUD 权限名、模块注册、多模块追加与稳定排序、非法输入、缺少 `auth` 和不覆盖现有文件。PG18/MySQL8.4 `--db --auth` 新项目各连续生成多个 mailer/policy，使用仅限 `/tmp` 的本地 `openrust` / `testing` path patch 分别通过 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；两库 `auth_flows` 各 1 项通过真实隔离数据库和 loopback HTTP。MySQL TestDatabase 从容器内已有 `mysql` 管理库连接，未创建或修改现有服务。Workspace fmt、default/all-features clippy/test、warnings-denied all-features docs 和 `cargo deny check` 通过；deny 仅有既有 duplicate dependency 与 unused MPL allowance 警告。没有外部短信投递或集群验收声明。

2026-10-26 M3 HTTP route rate limit：`cargo test --manifest-path examples/demo/Cargo.toml --test http_router --test http_resource` 通过（35 router、16 resource），all-features route limit 目标测试通过；同 scope/key 超限得到 429 与 `Retry-After`，不同自定义 key 隔离，auth-enabled 缺失主体拒绝。通过 `cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test db_migration migration_command_applies_repeats_reports_and_rolls_back -- --exact --nocapture` 与 MySQL 对应命令，在现有 PostgreSQL 18 / MySQL 8.4 容器中的隔离数据库各通过：真实 DatabaseRateLimitStore 多实例并发，以及 `RateLimitLayer::by_client_ip` HTTP 请求遇到伪造 XFF 仍按未信任 socket peer 限额。数据库用户/密码只在运行器进程环境内传递，未输出；容器未被创建、修改或停止。

只有找不到符合版本和测试要求的现有服务，或现有服务无法用于该测试时，才创建隔离的临时服务。使用独立 Compose project/端口，先用 `TestDatabase` 或独立测试库隔离数据；不得擅自停止、删除、重建或改动已有容器、卷、数据和配置。新建资源只清理本任务明确创建且确认无其他使用者的部分。检查命令不得输出连接密码或其他凭据。

```bash
docker ps --format 'table {{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}'
lsof -nP -iTCP -sTCP:LISTEN
```

确认没有合适服务后，才使用项目 Compose；需要隔离端口时：

```bash
OPENRUST_TEST_POSTGRES_PORT=55432 OPENRUST_TEST_MYSQL_PORT=53306 docker compose -p openrust-ci -f compose.ci.yaml up -d --wait postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:55432/openrust_test cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-postgres --test auth_database --test db_connection --test db_migration
OPENRUST_TEST_DATABASE_URL=mysql://root:root@localhost:53306/openrust_test cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-mysql --test auth_database --test db_connection --test db_migration
```

MySQL `testing` 隔离测试需具备建库/删库权限的管理 URL；一般应用集成测试使用最低权限 `openrust` 用户。MySQL driver 强制 Rustls TLS，并在新连接设置 `+00:00` 时区。两个独立数据库不能替代真实复制拓扑。

M2 读写路由真实复制测试使用 `OPENRUST_TEST_DATABASE_URL`、`OPENRUST_TEST_READ_REPLICA_URL` 配置主库/副本，并设置 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1` 启用副本暂停/追平断言。测试暂停 PostgreSQL WAL replay 或 MySQL SQL thread，验证副本旧读、两个独立 `App` 的 `with_primary` 主库新读与 scope 隔离，再恢复复制等待追平。2026-10-26 在 PostgreSQL 18.6 和 MySQL 8.4.13 临时主从服务通过。

可运行 `cargo bench -p performance --bench db_routing --features db-routing-bench,db-postgres` 或 `cargo bench -p performance --bench db_routing --features db-routing-bench,db-mysql` 对主库与显式副本分别进行 20 次预热、500 次串行 `SELECT 1` 查询，记录吞吐及 p50/p95/p99；`OPENRUST_DB_ROUTE_BENCH_SAMPLES` 可设为至少 100 的样本数。2026-10-26 Apple arm64、16 核 / 64 GiB、Docker 29.9.2、Release profile 下最新结果：PostgreSQL 18.6 primary 3658.0 req/s (p50 265.875µs, p95 332.292µs, p99 368.708µs)，replica 3452.1 req/s (270.333/424.625/541.75µs)；MySQL 8.4.13 primary 3735.9 req/s (p50 256.958µs, p95 315.333µs, p99 362.041µs)，replica 3709.8 req/s (261.416/303.5/484.333µs)。此 `SELECT 1` 单并发微基准只记录路由和往返基线，不代表业务吞吐或性能提升；完整 CPU/内存/业务负载测试仍按性能计划执行。

2026-10-26 根模块迁移后，上述 DB/auth tests 在 PostgreSQL 16 和 MySQL 8.4 均实际通过（分别 4 项 auth、3 项连接/事务、1 项迁移）。`testing` 数据库隔离测试也分别通过。

## 生成项目

CLI 模板快照测试：`cargo test -p openrust-cli`。生成 PostgreSQL、MySQL `--auth` 项目后运行 `cargo fmt --all --check`、`cargo check`、`cargo test`。旧模板的历史集成验证使用 Cargo patch 指向本地根 `openrust` 与 `testing`。新的源码交付验收按 ADR-039，在干净源码副本中生成 apps 后直接运行，不继续以 patch 作为默认操作入口。

2026-10-26 根模块迁移后，两种生成项目都以真实 PostgreSQL 16 / MySQL 8.4 服务运行 `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 并通过。测试覆盖实际 HTTP、JWT/PAT 认证、数据库流程、可信代理 client IP 与 fake mail。

2026-10-26 当前日常基线复验：`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-postgres --test auth_database` 在现有 PostgreSQL 18 容器通过 4 项；`--features auth,db-mysql` 在现有 MySQL 8.4 容器通过 4 项。按当前 CLI 模板新生成的 `--auth` 项目分别以本地 Cargo patch 指向未发布 workspace，并在真实 PostgreSQL 18 / MySQL 8.4 服务上通过 `cargo fmt --all --check`、`cargo check` 和 `cargo test --test auth_flows`（每库 1 项真实 TCP HTTP 流程，使用隔离数据库与 fake mail）。生成目录位于 `/tmp/openrust-auth-pg-resume` 与 `/tmp/openrust-auth-mysql-resume`，没有纳入仓库。

2026-10-26 ADR-031 出站 HTTP：历史基线为 `cargo test -p openrust --features http-client http::client::tests` 15 项及 `根目录集成宿主` 聚合接入 1 项；CLI `--http-client` 生成项目 check/test 通过，证据保留于原验收记录。

2026-10-27 ADR-051/052 出站策略与韧性：`cargo test -p openrust --features http-client http::client::tests -- --nocapture` 21 项通过，涵盖目标策略、DNS 固定、重试上限/抖动/Retry-After、单次 body 限制；`cargo test --manifest-path examples/demo/Cargo.toml --all-features --test http_client -- --nocapture` 6 项通过，聚合 HTTP consumer 验证目标拒绝、依赖隔离、容量与等待上限、取消释放、熔断打开/半开恢复。总门禁 `cargo fmt --all --check`、`cargo test --workspace`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features`、default/all-features workspace all-target clippy、warnings-denied default/all-features Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check`、`git diff --check` 全部通过。`cargo deny` 仅输出既有未匹配 MPL-2.0 allowance 与重复版本警告，advisories/bans/licenses/sources 均通过。无需外部服务；只访问回环 TCP 测试服务，不访问公网目标、云 metadata 或供应商。

2026-10-26 ADR-033 邮件/SMS：`cargo test -p openrust --features 'mail-smtp,sms-twilio,test-util' mail::tests:: -- --include-ignored` 通过，含真实本机 SMTP 对 Mailpit v1.32.0 的投递；`curl --noproxy '*' http://127.0.0.1:18025/api/v1/messages` 确认 `user@example.test` 收到 `Welcome`。Twilio transport tests 覆盖 POST URL、Basic auth、form/template fields、2xx receipt、4xx脱敏、timeout及单次调用不重试；测试不访问 Twilio。CLI 9 项测试与 `--sms-twilio` 生成项目 `/tmp/openrust-sms-generated-20261026` 的 check/test 通过；default/all-features workspace test、all-features clippy、warnings-denied docs、cargo deny 与 diff-check 通过。未提供实际 Twilio 账号凭据与授权号码，真实 SMS 投递仍未验收。

2026-10-26 ADR-032 请求级国际化：6 项 `i18n` 单测与 5 项 `examples/demo/tests/server/i18n/runtime.rs` 通过，覆盖 locale 规范化、Accept-Language q 值、TOML config、JSON resource 合并/参数/fallback、请求并发隔离、错误/validator message、稳定 code/字段路径、未翻译应用错误保留与显式 locale override。新生成项目 `/tmp/openrust-i18n-generated-20261026` 使用本地 Cargo patch 通过 `cargo check`、`cargo test`；其真实 server 进程收到 `Accept-Language: zh-CN` 后对 404 返回中文 message。default/all-features `fmt --check`、workspace clippy/test、warnings-denied docs、`cargo deny check` 通过。无数据库行为，不要求双库集成；邮件/SMS 的实际供应商验收留在 M3 通信簇。

## 迁移验收记录

- `cargo metadata --no-deps --format-version 1` 确认根 package 为 `openrust`，workspace 共四个成员：根 package `openrust`、`cli`、`macros`、`testing`。
- 无默认、默认、关键单独 feature、all-features 编译已检查；workspace default/all-features clippy 与 test、warnings-denied rustdoc、`cargo deny check` 均通过。`cargo deny` 仍会输出重复版本依赖提示，但无 advisory、license、ban 或 source 失败；JWT 使用 `aws_lc_rs`，不再解析到受 RUSTSEC-2023-0071 覆盖的 `rsa 0.9`。
- M2 数据库读写分离的 PostgreSQL/MySQL 真实复制拓扑已单独通过（见上文）；其余尚未验收的范围包括 Redis 最低版本、生产负载下多 worker/旧 token 竞态、M6 全栈集群、S3 bucket-version 专项与完整性能门禁，继续按 `todo.md` 和各专题计划推进。实时在线跨节点单项验收见本文件 M5 ADR-059。
- PostgreSQL 14 / MySQL 8.0 结果是历史兼容性证据；当前暂定下限已采用 PostgreSQL 14+ / MySQL 8.0+，但这些结果不替代发布前对完整承诺能力的最低版本复核。Redis 6.2+ 的真实兼容性在 M5 driver 落地后验证。

2026-10-26 ADR-034 应用数据加密/本地轮换：`cargo test -p openrust --features encryption security::tests` 8 项通过，包含 RFC 8452 AES-256-GCM-SIV 向量、二进制/空明文、随机 nonce、context、active key 写入/历史 key 解密、篡改/格式/越界、配置脱敏与多 Encryptor 互解；`cargo test -p openrust-cli` 11 项通过，包含 32-byte Base64 key generation 与 `--encryption` 模板；`cargo check -p openrust --bin server --features encryption` 通过。实际 CLI 生成项目 `/tmp/openrust-encryption-generated-20261026` 使用本地 Cargo path patch 通过 `cargo check`、`cargo test`，并用环境 key 执行 `cargo run -- config show`；输出中的 active key ID 与 key map 值均为 `********`。最终 default/all-features workspace tests、all-features clippy、warnings-denied default/all-features docs、`cargo deny check` 和 `git diff --check` 通过；cargo-deny 保留既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。该簇无数据库读写，不要求新增双库集成验收。测试应用 all-features 时显式注入测试 key ring；生产默认配置不含 key。

2026-10-26 ADR-035 审计簇：`cargo test -p openrust --features test-util audit::tests` 6 项通过，覆盖 allowlist、敏感详情/Debug 脱敏、错误传播、database/custom plugin 和公开 fake；PostgreSQL 18 与 MySQL 8.4 的 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 5 项通过。新增测试使用 `TestDatabase` 隔离库和完整 audit 表名映射，验证权限变更与审计同事务提交、外层回滚、审计 insert 失败导致权限回滚，以及最多 N 行的重复清理。CLI 新生成的 PostgreSQL 与 MySQL `--auth` 项目均通过 `cargo fmt --all --check`、本地 Cargo patch `cargo check` 和真实 `auth_flows` HTTP/TCP 数据库流程测试。workspace default/all-features tests/clippy、warnings-denied default/all-features docs、no-default-features check、`cargo deny check` 通过；deny 仅有既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。

2026-10-26 ADR-036 signed links：`cargo test -p openrust --no-default-features --features signing signing::tests` 4 项通过，覆盖用途/subject/TTL、Debug 脱敏、shared store 撤销与保留旧 key 的轮换；PostgreSQL 18、MySQL 8.4 完整 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database` 各 6 项通过，覆盖两个 signer 实例跨库撤销、自定义完整表名映射、过期记录有界清理及应用插件注册。新生成 `--auth` 项目分别使用本地 Cargo patch，通过 `cargo fmt --all --check`、`cargo check`、真实数据库迁移及 TCP HTTP auth flow。最终 `cargo fmt --all --check`、`git diff --check`、workspace default/all-features clippy/test、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过；deny 仅有已存在的重复依赖与 unused MPL allowance 警告，advisories/bans/licenses/sources 通过。没有新增依赖。

2026-10-26 M3 认证/安全总验收：本机预置 PostgreSQL 18 / MySQL 8.4 容器凭据与仓库 CI 默认值不同，未触碰预置数据库；使用仓库 `compose.ci.yaml` 的无持久卷 PostgreSQL 18 / MySQL 8.4 服务、独立测试库后，`cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database --test auth_runtime` 各 12 项通过。按当前模板生成的 PG/MySQL 项目分别用真实数据库执行 `cargo test --test auth_flows`，各 1 项真实 TCP HTTP 流程通过。覆盖注册/重复注册、邮箱验证、JWT 与 PAT、refresh 单次轮换/重放撤销、密码重置、refresh logout、Session 登录/退出与 CSRF、共享限流、可信代理、Policy/RBAC 与 PAT ability 交集。生成项目各自还通过 `cargo fmt --all --check` 与 `cargo check`。临时 Compose 服务已停止并移除，既有 PG/MySQL/Redis 服务保持运行。

2026-10-26 M3 数据输出/查询边界：`cargo test -p openrust http::sort::tests` 覆盖排序解析、字段 allowlist、重复/未知/格式错误输入；CLI generator 单测覆盖 `--uuid-id` / `--soft-delete` 和生成 feature 注册。临时生成项目（本地 patch 到当前 workspace）分别在现有 PostgreSQL 18 与 MySQL 8.4 上通过格式检查、`cargo check` 和真实隔离数据库 test：迁移 UUID 主键及 UTC `deleted_at`，插入生成 UUIDv7，显式 `not_deleted()` 隐藏软删除行，普通 `find_by_id` 仍能读取，清空 `deleted_at` 后恢复。HTTP README 补充严格输入 DTO、响应字段 allowlist、显式 SeaORM Condition、关联预加载示例；没有引入 ORM 自动过滤或动态查询 DSL。测试只使用现有服务与临时目录，未输出凭据，未改动容器。

2026-10-26 M4 typed events/queued listener：`cargo test -p openrust --all-features events::tests` 与 `cargo test -p openrust --features queue --lib events::queued_tests` 通过，后者覆盖 mapper 成功、mapper error 与 Queue dispatch error 传播/短路。外部下游 consumer crate 在 `/tmp/openrust-events-consumer` 以 `queue,db-postgres` 通过离线 `cargo check`。真实双库命令均运行成功：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在现有 PostgreSQL 18.6 通过；`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-mysql --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在临时 MySQL 8.4 通过。覆盖事件 transaction context、事务回滚不留任务、提交后 worker 消费、普通 dispatch 消费；测试显式释放 Events/Queue/Db/App 后 `TestDatabase::close` 正常删除隔离库。预置 MySQL 8.4 容器因容器内 socket PID lock 错误持续重启，未改动；仅使用 `compose.ci.yaml` 在空闲 53306 端口创建 `openrust-ci` 临时 MySQL，测试后 `docker compose ... down -v` 仅清理本次创建的容器/网络/卷。修复后的最终门禁通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、default/all-features workspace tests、warnings-denied default/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。未触碰预置 PostgreSQL/MySQL/Redis 数据，无凭据输出。

2026-10-26 ADR-041 事务与 AuthFlows 事件：在任务专属 Compose project `openrust-m4-auth-tx`（PostgreSQL 18、MySQL 8.4，端口 55432/53306）分别执行 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test db_connection transaction_events_rollback_before_commit_and_ignore_after_commit_errors -- --exact` 与 `cargo test --manifest-path examples/demo/Cargo.toml --features auth,db-<backend> --test auth_database auth_flow_events_share_account_and_refresh_transactions -- --exact`，四项均实际运行通过。验证提交前拒绝回滚、提交后错误不掩盖已提交结果、注册校验拒绝不建账户、泛化登录失败事件，以及登录成功事件失败无 refresh token 遗留。临时服务由本任务创建；预置服务未改动。

2026-10-26 ADR-042 共享锁与请求幂等：在任务专属 `openrust-m4-auth-tx` PostgreSQL 18 / MySQL 8.4 服务，对每个 backend 实际执行 `cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_coordination -- --exact shared_locks_and_request_idempotency_are_database_scoped`，均 1 项通过。测试使用 `coord_` 自定义表前缀的 TestDatabase 隔离库及两个独立 SeaORM pool，覆盖迁移索引、共享锁 owner 竞争、过期接管/fencing、旧 owner 释放与事务内检查、同键并发仅一 owner、scope 隔离/指纹冲突、事务回滚同时撤销业务写入和完成快照、成功完成后的 JSON replay、过期 lease 接管及每次最多 N 行清理。测试运行时没有临时 services 以外的依赖；没有声称进程强杀、真实 HTTP middleware 或外部副作用 exactly-once。

2026-10-26 F2 国际化：`cargo test -p openrust --lib i18n::tests::` 9 项通过，新增覆盖结构化 plural/select、英文与波兰语 CLDR 类别、无效/缺失参数 fallback、按实际 fallback 资源 locale 选择复数规则、精确小数、英中日期差异及显式时区跨日；`cargo test --manifest-path examples/demo/Cargo.toml --test i18n` 6 项通过，包含并发请求 locale 及按请求语言选复数。干净源码副本 `/tmp/openrust-f2-source-wgP07b/source`（无 `.git`、无 registry patch/开发机路径）使用 `cargo run -p cli -- new i18n_sample --db postgres` 生成到 `apps/i18n_sample`，其 `cargo check` 与 `cargo test` 通过。工作区 `cargo fmt --all --check`、default/all-features workspace tests、default/all-features clippy、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`、`cargo +1.97.0 check -p openrust`、`cargo deny check` 通过；deny 输出为既有重复依赖版本警告，advisories/bans/licenses/sources 均通过。F2 无数据库读写行为，不要求 PostgreSQL/MySQL 集成验收；F1 已有双库证据不由此替代或重做。

2026-10-26 F3 安全邮件模板：新增可选 `mail-templates`（MiniJinja 2.24）、静态 `include_str!` catalog、locale 分层回退/布局继承、HTML 自动转义、严格缺参、100,000 fuel 与 256 KiB 有界输出、主题换行/长度校验及 text/html MIME alternative。`cargo test -p openrust --features 'mail-templates test-util' mail::template::tests` 4 项通过（含 FakeMailer）；`cargo test --manifest-path examples/demo/Cargo.toml --features mail-templates --test mail_templates` 1 项通过；CLI 邮件生成器定向测试通过。干净源码副本 `/tmp/openrust-f3-final-aKylsZ/source`（无 `.git`、无 registry patch/开发机绝对路径）生成 PostgreSQL 应用及 mailer，`cargo fmt --all --check`、`cargo check -p mail_sample`、`cargo test -p mail_sample` 通过。Compose Mailpit v1.32.0 使用 SMTP/UI 11025/18025（默认 1025 端口被占用），显式运行 `smtp_submits_rendered_template_to_mailpit` 成功；Mailpit `/api/v1/message/<id>` 确认 Subject `Greeting`、Text `Hello, Mailpit`、HTML `<p>Hello, Mailpit</p>`，随后停止本次启动的 Mailpit。default/all-features workspace tests、default/all-features clippy、warnings-denied all-features docs、`cargo +1.97.0 check -p openrust --features mail-templates`、`cargo deny check`、fmt 与 diff check 通过；deny 有既有重复依赖及未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。本簇不改数据库行为，不需要新增双库测试；队列派发/通知重试边界留给后续功能簇。

2026-10-26 M4 生成与集成收口：CLI `generate job` 单测覆盖数据库前置、重复保护、多任务插件注册、queue feature 与 `QueueMigration` 登记、显式失败 handler。无 `.git` 的干净源码副本中分别生成 PostgreSQL 与 MySQL 应用并生成一个/多个 job；应用及 migration 的直接 `cargo check`、`cargo test` 均通过，manifest 无 registry patch 或开发机绝对路径。`queue-demo` 示例分别以 `queue,db-postgres` 与 `queue,db-mysql` 通过编译；PostgreSQL 18 隔离库中实际执行迁移，真实 TCP `/health` 返回 200，HTTP `POST /jobs` 派发，`schedule run --once` 派发 heartbeat，独立 `queue work demo` 进程消费两项任务。数据库服务为已存在的 `openrust-m4-auth-tx` PostgreSQL 18 容器，仅创建并删除本次隔离数据库；curl 明确绕过本机 SOCKS 代理。PG18/MySQL8.4 的 `queue_notifications` targets 本次各 1 项真实服务测试通过。`cargo fmt --all --check`、all-target/all-features clippy、default/all-features workspace tests、warnings-denied all-features docs、`cargo deny check`、queue-only feature check 与 diff check 通过；deny 只有既有重复依赖与未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。M4 功能簇完成；仍未宣称生产负载或完整集群验收。

2026-10-26 M5 通用插件公开契约：新增外部 integration consumer target，只通过 `openrust` 公共 API 组合 Provider/Consumer 插件，验证 section 配置、服务、命令、HTTP 路由与异步 `finish`；缺依赖、重复服务/命令、无效配置值脱敏和重复路由启动错误均有覆盖，兼容 `insert`/`command` 的后注册替换也有测试。配置 section 错误保留完整字段路径且不输出配置值；应用路由组装将 Axum 注册 panic 转为 `http_route_registration_failed`，直接 Axum API 边界写入 Rustdoc/README。`cargo test --manifest-path examples/demo/Cargo.toml --test plugin_contract` 8 项通过；`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 通过。deny 保留既有重复依赖与未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 检查通过。本簇不修改数据库功能，不新增双库集成要求；后续 Db driver contract 需要独立 PostgreSQL/MySQL 验收。

2026-10-26 M5 Db 外部扩展契约：`DatabaseTableNames` 增加 SQLite 双引号引用与通用 `Db::initialize[_with_replicas]` backend 校验；保留 64-byte 通用标识符上限及 PostgreSQL 63/MySQL 64 backend 限制，未知 SeaORM backend 明确以 `unsupported_database_backend` 拒绝。v1 正式后端与 `DatabasePlugin` URL/feature 检查保持 PostgreSQL/MySQL。独立 `tests/consumers/db-custom-sqlite/` 使用相对 path 依赖、独立 workspace 和 `sqlx-sqlite`；`cargo +1.97.0 check` 及 `cargo +1.97.0 run --locked` 实际通过，真实 SQLite 连接、backend、重复初始化错误、表名 quoting、应用自有 SeaORM migration、事务提交/回滚均有运行断言；独立 package 的 stable fmt/clippy 通过，CI MSRV job 已添加运行入口。PostgreSQL 18 与 MySQL 8.4 分别实际执行 `db_connection::plugin_opens_pool_and_executes_a_query` 和 `db_connection::transaction_commits_success_and_rolls_back_callback_errors`，四项均通过。Workspace fmt、all-target/all-feature clippy、默认/全 feature tests、warnings-denied all-feature docs、workspace `cargo deny check` 与 `cargo deny --manifest-path tests/consumers/db-custom-sqlite/Cargo.toml --config deny.toml check` 通过；deny 报告既有重复依赖及未匹配的 BSD-2-Clause/MPL allowance 警告，advisories/bans/licenses/sources 检查通过。SQLite 不进入官方 v1 数据库矩阵，认证/队列/共享锁/限流和 `TestDatabase` 等专属能力不由本簇宣称兼容。

2026-10-26 M5 Queue 外部扩展契约：`JobRegistry` 与 `JobContext` 通过公开 API 支持外部 driver 注册/执行 typed Job、注入取消信号、进度 reporter 和 AppState；未配置 reporter 明确返回 `queue_progress_unsupported`，ownership token 保持内置数据库 driver 私有。`cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，覆盖无需数据库的 await 内直接执行、typed Job/进度 reporter、合作取消、未知任务、重复注册、无 reporter 显式失败及上下文边界；README 与 Rustdoc 明确该入口不派发/持久化、不提供事务、自动重试或任务组。`cargo check -p openrust --no-default-features` 通过。独立 nested workspace `tests/consumers/queue-custom-driver` 通过 `cargo run --locked` 实际派发并运行 typed Job，all-targets clippy 通过；无绝对路径或临时 registry patch。PostgreSQL 18 与 MySQL 8.4 分别实际运行 `db_migration`、`queue`、`queue_crash_recovery`、`schedule`、`schedule_calendar`、`queue_notifications`，各 16 项通过。Workspace fmt、all-target/all-features clippy、default/all-features tests、warnings-denied default/all-features docs、cargo deny 与 diff check 通过；cargo deny 仅报告既有重复依赖版本和未匹配 MPL allowance 警告，advisories/bans/licenses/sources 通过。外部 memory driver 明确不提供持久化/跨节点/事务保证，不代表 Redis driver 或集群验收。

2026-10-26 M5 跨协议验证与调用作用域（验证通过，尚未提交）：新增公开 `validation::Validate` / `validate` 并让 HTTP Validated extractor 复用原字段路径/错误转换；新增 task-local `ExecutionContext`、白名单 `ExecutionMetadata`（safe request ID/Locale）、层级 cooperative cancellation 和不超过父调用的 deadline budget。HTTP middleware 建立 request/locale/deadline/cancel scope，非法 request ID 移除后重新生成；`App::run_command` 建立匿名 scope 与默认 locale；typed Job 与 `JobRegistry::execute` 建立每次执行 scope，JobRegistry 执行入口强制 deadline/cancellation，数据库 worker 保留已有 timeout 与停机宽限控制，缺省 locale 使用应用默认值。数据库 queue 用有版本的 payload envelope 持久化 request ID/locale，legacy raw payload 保持可读、任务组继承 metadata、failed-job retry 原样保留；保留字段 `__openrust_execution` 在含 metadata 新派发中被拒绝，完整且合法的版本 envelope 作为框架格式，畸形旧结构按原 payload 保留；自定义旧 driver 收到非空 metadata 默认显式返回 unsupported。worker span 记录 job id/name/attempt/request_id，不记录 payload；request ID 关联不代表 W3C/Otel trace link。定向 HTTP timeout/cancellation 测试 1 项、queue external driver contract 4 项、JobRegistry deadline/scope 测试 1 项、metadata malformed/roundtrip 单测已实际通过。PostgreSQL 18：`cargo test --manifest-path examples/demo/Cargo.toml --features queue,db-postgres --test db_migration --test queue --test queue_crash_recovery -- --nocapture` 实际运行 migration 1、queue 1、crash recovery 5 项通过；MySQL 8.4 同一命令实际运行 1+1+5 项通过。两库 queue target 含持久化 worker 和任务链中的 request ID/locale round-trip；均使用既有 `openrust-m4-auth-tx` 健康服务及 TestDatabase 隔离库，未改容器或输出凭据。最终门禁已通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、默认/all-features workspace tests、warnings-denied 默认/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。`cargo deny check` 返回成功，仅报告既有重复依赖及未匹配 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。本地 scoped commit 尚未创建。

2026-10-26 M5 F4b 单进程同键加载合并：新增有界 `CacheStore`/`Cache` `get_or_load_with`，键按 store、namespace、logical key、返回类型和稳定 load ID 隔离；配置校验 key/waiter 上限与 loader timeout。单测覆盖 8 路合并、类型/load ID 隔离、leader 取消重选、容量/等待者拒绝、超时与错误广播回收。`cargo test --workspace` 与 `cargo test --workspace --all-features` 通过；其中 all-features openrust 184 项通过、2 项 ignored；`cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --nocapture` 在真实 Redis 上通过，覆盖 Redis tag 失效和应用聚合 singleflight。`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、warnings-denied all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features` 通过；deny 保留既有 MPL allowance 未匹配和重复依赖告警，advisories/bans/licenses/sources 均通过。此单进程功能无数据库写入，不要求双库测试；PostgreSQL/MySQL 集群一致性不由此记录宣称，跨节点 SWR 留待 F4c。干净源码副本中当前 CLI cache 模板生成 check/test 已于同轮执行记录（未使用 registry patch 或机器绝对路径）。

2026-10-26 M5 F4c stale-while-revalidate 与应用后台任务生命周期：定向 SWR 测试覆盖 fresh/stale/最终过期、tag 失效、刷新失败与 stale 上限、并发刷新限制、取消及停机 join；真实 Redis 双连接 server 测试通过。PostgreSQL 18 与 MySQL 8.4 分别以两个独立 App/数据库 pool/Redis 连接实际运行 `db_coordination::f4c_swr_uses_shared_locks_across_apps_and_redis_connections`，各 1 项通过，验证共享锁降低重复刷新并观察共享刷新结果；此为单进程多 App 专项，不代表多节点集群验收。干净源码副本生成应用直接 `cargo check`、`cargo test` 通过，无 registry patch 或开发机绝对路径。`cargo test --workspace`、`cargo test --workspace --all-features`、default/all-features all-target clippy、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check`、fmt 与 diff check 通过；deny 仅有既有未匹配 MPL allowance 与重复依赖警告，advisories/bans/licenses/sources 均通过。完整集群、多节点缓存负载及外部通知未由本簇覆盖。

2026-10-26 显式同步任务执行：复用 ADR-043 的 `JobRegistry::execute`，没有增加重复 API。定向 `cargo test --manifest-path examples/demo/Cargo.toml --features queue --test queue_driver_contract` 5 项通过，新增纯进程内 await 行为断言，不配置数据库/Redis；明确进度需 reporter、取消为合作语义，且直接执行不提供事务派发、持久化、自动重试或任务组。workspace fmt、clippy、tests、warnings-denied docs 与 `cargo deny check` 均通过；deny 的重复 crate 版本与未匹配 MPL allowance 为现有告警。

2026-10-26 M5 F5 资源查询授权（ADR-047）：`cargo test -p openrust --features 'auth db-postgres' auth::query::tests` 验证非空范围构造、空 OR 组与规则错误 fail-closed，以及嵌套 OR 筛选仍位于 owner scope 的 AND 内。真实隔离库 HTTP 测试 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-postgres' --test auth_query_authorization` 与 MySQL `db-mysql` 对应命令均通过 PostgreSQL 18 / MySQL 8.4：owner 只能读取匹配行；越权 ID 返回空；分页只计授权行；混合授权 ID 批量修改/删除只影响本人行；关联查询校验父记录并用子实体规则过滤子集合。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml` 通过。裸 SeaORM、直接关联加载、原生 SQL、自定义 driver 不被自动拦截；完整业务 CRUD、导出组合和集群压力仍待相应验收。

## M5 ADR-048 稳定键有界分批读取

`cargo test -p openrust --features 'auth db-postgres' batch_options_enforce_the_documented_row_bound` 验证默认 100、最小 1、最大 1000 及越界拒绝。PostgreSQL 18 与 MySQL 8.4 分别使用已有 loopback 服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_query_authorization -- --nocapture`，各实际执行 1 项通过：owner scope 分页读取、取消后用最后完整主键游标续读、无越权/重复行、复合主键升降序和跨页顺序、callback 错误停止及连接继续可用。测试凭据从既有容器配置在进程内构造，不输出凭据、不创建或更改服务。独立 consumer `cargo check --manifest-path tests/consumers/query-authorization/Cargo.toml --locked` 通过。

## M5 ADR-049 数据库迁移互斥

在既有 PostgreSQL 18 与 MySQL 8.4 loopback 服务上，分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features db-<backend> --test db_migration -- --nocapture`，4 项均通过。真实隔离数据库和独立子进程验证首次迁移竞争只运行一个 migration callback、第二个 up 等待并读到最新状态、短等待预算返回 `migration_lock_timeout`、持锁进程强杀后恢复，以及既有 up/down/status 与具名数据库隔离。凭据从容器配置在进程中构造，不输出、不更改或停止服务。fmt、default/all-features all-target warnings-denied Clippy、default/all-features workspace tests、warnings-denied default/all-features docs、no-default-features check 与 `cargo deny check` 通过；workspace 运行时不配置的 DB 集成 targets 不代替上述显式双库执行。未宣称生产多节点迁移演练。

## M5 ADR-050 文件存储

`cargo fmt --all --check`、default/all-features all-target Clippy、`cargo test --workspace`、`cargo test --workspace --all-features`、warnings-denied default/all-features Rustdoc、`cargo check -p openrust --no-default-features` 与 `storage-local`/`storage-s3` feature checks 均通过；`cargo deny check` 通过，仍报告既有 duplicate versions 与未匹配 MPL-2.0 allowance。all-features 中 `openrust::storage` 本地文件测试、`server/tests/storage/driver_contract.rs` 独立公开 API consumer、`server/tests/storage/http.rs` 真实 TCP 授权上传/下载均执行并通过。CLI 生成的 `apps/storage_demo` 在无 `.git` 的源码副本中直接 `cargo check -p storage_demo` 与 `cargo test -p storage_demo` 通过；生成 manifest 使用 workspace 相对 path，无临时 registry patch 或开发机绝对路径。

被 workspace 测试忽略的真实服务专项已显式运行：`NO_PROXY=127.0.0.1,localhost OPENRUST_TEST_S3_ENDPOINT=http://127.0.0.1:19000 OPENRUST_TEST_S3_ACCESS_KEY=<本机测试值> OPENRUST_TEST_S3_SECRET_KEY=<本机测试值> OPENRUST_TEST_S3_VERSIONED_BUCKET=openrust-storage-versioned OPENRUST_TEST_S3_PLAIN_BUCKET=openrust-storage-plain cargo test --manifest-path examples/demo/Cargo.toml --features storage-s3 --test storage_s3 -- --ignored --nocapture`。测试连接 MinIO `RELEASE.2025-10-15T17-29-55Z`，versioned/plain buckets 均为真实服务，验证 metadata、12 MiB 多段上传、检查器拒绝、延时上传取消与目标不可见、覆盖后旧 version body 与 presigned URL 仍读取旧内容、短期 URL 过期及未启用版本控制时 fail-closed。结束后通过 MinIO client 核实两个 bucket 均无未完成 multipart，也无本测试遗留对象。服务使用本任务创建的临时容器与 tmpfs 数据盘，非生产多节点集群。

`StorageDisk::put_body` 限制交给 inspector/backend 的数据块并保留背压，但不能限制 Axum 上游构造单个 body frame 时已经发生的分配；直接 `put_stream` 的调用方也须控制输入 chunk。filesystem 仅本机可见；API 节点 A 上传、B 下载仍在 v1 集群门禁验证，不能由单服务 MinIO 测试代替。真实 provider 进程强杀后的 multipart 回收依赖 bucket 生命周期策略。

并发应用启动回归：`cargo test -p openrust --all-features storage::opendal_driver::concurrent_filesystem_driver_builds_reserve_distinct_staging_directories --lib -- --exact` 使用 64 个并发 filesystem driver 初始化同一 root，验证 staging 路径不会因时钟刻度碰撞而失败；`cargo test --manifest-path examples/demo/Cargo.toml --all-features --test server` 聚合测试 10 项并发运行通过。根因是旧 staging 名仅使用 PID 与纳秒时钟，所有同时初始化的 server App 共用 filesystem root；现改为进程内原子序号并对 `AlreadyExists` 做最多 16 次重试。

2026-10-27 F7 SSO 真实 Redis 故障与恢复：PostgreSQL 18、MySQL 8.4 均以 Keycloak 26.8.1、Redis 8 独立容器、两个隔离应用及每应用两个 API 子进程执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth-oidc auth-mfa session-redis db-<backend>' --test auth_oidc_keycloak -- --nocapture`。测试真实停止专用 Redis 容器，确认已验证的 Back-Channel Logout 返回 503；恢复 Redis 后重启两个应用的四个 API 子进程，以相同 Logout Token 重放并获得成功，随后验证共享 Session/refresh 状态。运行结果：PostgreSQL 18 通过 1 项（17.51 秒），MySQL 8.4 通过 1 项（18.11 秒）。另以既有 Redis 8 服务复跑双库正常双应用 SSO/退出路径（各 1 项）及 OIDC 注入一次 Session 删除失败后重试（各 1 项），均通过。真实连接故障后 Redis driver 连接管理器未在存活进程内恢复；当前验收语义要求恢复服务后重启 API 副本，再重放通知。M6 完整集群门禁仍需另行验收。

2026-10-27 F6-A 租户上下文与查询隔离：`cargo test -p openrust tenant::tests --lib` 3 项通过，覆盖 TenantId 校验/Debug 脱敏、task-local 租户隔离、嵌套切租拒绝与 resolver 失败关闭。PostgreSQL 18/MySQL 8.4 分别执行 `OPENRUST_TEST_DATABASE_URL=<service-url> cargo test --manifest-path examples/demo/Cargo.toml --features 'auth auth-mfa db-<backend>' --test auth_query_authorization -- --nocapture`，各 2 项通过：原 F5 授权查询回归 1 项和 F6 tenant HTTP 专项 1 项。新专项在每个 backend 启动两个真实 TCP API listener、共享隔离库，使用 AuthLayer 校验身份后由 TenantLayer resolver 将两个用户映射到不同 tenant；伪造 `x-tenant-id` 不改变 tenant；用户/tenant 双 scope 限制列表、关联、批量更新/删除，交叉 tenant 行保持不变，直接调用受保护规则且缺上下文时拒绝。workspace fmt、default/all-features clippy、default/all-features tests、warnings-denied default/all-features docs、no-default check 与 cargo deny 均通过；cargo deny 仅有既有重复依赖与未匹配 MPL allowance 警告。

F6-B 租户资源传播已完成：Cache key/singleflight/tag generation、跨后台 SWR 任务固定 tenant 物理键、持久 Queue metadata/worker task scope、Storage tenant 物理路径、Audit tenant_id 与损坏 envelope fail-closed。单元测试 133 项通过；PostgreSQL 18/MySQL 8.4 各自 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth,auth-mfa,queue,db-<backend>' --test auth_database` 7 项、`--test queue` 1 项、`cargo test --manifest-path examples/demo/Cargo.toml --features 'queue,db-<backend>' --test queue_crash_recovery` 7 项均通过。独立 worker 进程首次尝试失败，另一 worker 实例重试后完成并恢复租户上下文。真实 Redis 8 配合两个独立 Cache App/连接运行 `db_coordination`，PostgreSQL/MySQL 各 2 项均通过：验证共享 SWR 锁、跨租户 key/tag 隔离与 tag 失效边界；本地 filesystem 双 App 和 custom Storage driver 使用相同逻辑路径的隔离测试通过。fmt、default/all-features all-target warnings-denied Clippy、default/all-features workspace tests、warnings-denied default/all-features Rustdoc、no-default-features check 与 `cargo deny check` 均通过；deny 仅有既有重复依赖和未匹配 MPL-2.0 allowance 告警。

## M5 gRPC G2c 调用预算、资源限制与健康停机

真实 TCP 示例 `cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --all-features` 11 项通过：完整 server-stream deadline、全局并发超限/许可释放、消息大小限制、reflection descriptor 白名单及缺 descriptor 拒绝、health Watch 停机状态、非终止流的 shutdown deadline，以及既有 JWT/TLS/mTLS 用例。`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 6 项通过，覆盖共享 readiness、明文安全默认和真实子进程 SIGTERM。SIGTERM 子进程测试清除继承的 `RUST_TEST_THREADS`，避免串行 workspace runner 影响子进程测试调度。

通过 `cargo fmt --all --check`、默认与 all-features workspace all-target Clippy（`-D warnings`）、默认 `cargo test --workspace --quiet`、串行 `RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认/all-features workspace Rustdoc、workspace 与独立示例 `cargo deny check`、独立示例 all-target/all-features Clippy、Rust 1.97 根 gRPC feature check、独立示例 Rust 1.97 locked all-target/all-features check、no-default-features gRPC check 与 7 项 gRPC unit tests。独立示例锁定依赖并通过完整 all-features 测试；根与示例 cargo deny 均通过 advisories/bans/licenses/sources，保留既有 duplicate crate versions 与未匹配 MPL allowance 警告。G2c 只变更 gRPC runtime，不新增数据库行为；G2b JWT/PAT/Policy 的 PostgreSQL 18/MySQL 8.4 证据仍见前文，G2c 不重复声称双库认证验收。HTTP+gRPC 联合 supervisor、真实多节点路由、滚动升级和 proto 新旧互通仍未验收，列入 G3。

## M5 gRPC G3a HTTP/RPC 联合生命周期

示例 `cargo test --manifest-path tests/consumers/grpc/Cargo.toml --locked --all-features` 15 项通过，包含联合 HTTP `/ready` 与 gRPC unary 的真实 TCP 调用、双 listener readiness/停机、gRPC 绑定冲突后 HTTP listener 自动释放、`combined_shutdown_timeout_aborts_and_joins_both_listeners` 超时回收，以及插件服务注册前置条件和既有 G2 TLS/auth/deadline/health 运行治理；`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 7 项通过，其中 `server_host_composes_http_and_grpc_under_one_lifecycle` 通过真实 TCP 调用 HTTP health 与 gRPC health，并验证统一停机关闭两个端口；`cargo test -p openrust --features grpc runtime::tests --lib` 2 项验证早期 listener 退出会报错、故障 listener 会通知并 join sibling。该组合入口先构建 router/TLS、再绑定两个 listener；`Readiness` 只有两个 listener 都 active 才为 ready，supervisor 统一 stop、deadline、abort/join 与 app-owned background task 回收。没有模拟真实节点故障或 HTTP/2 代理，不能作为 G3c/集群证据。

本簇通过 `cargo fmt --all --check`、默认/all-features workspace all-target Clippy（`-D warnings`）、默认 `cargo test --workspace --quiet`、串行 `RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied 默认/all-features workspace Rustdoc、`cargo deny check`、Rust 1.97 根 gRPC feature check 与独立示例 locked all-target/all-features check。示例单独运行 all-features Clippy 与 `cargo deny check`；deny 的 advisories/bans/licenses/sources 通过，仅保留既有重复 crate 版本和未匹配 MPL allowance 警告。无数据库行为变化，不重复 PostgreSQL/MySQL 数据验收；proto/trace 传播已在 G3b 验收；完整双节点路由留待 G3c。

## M5 gRPC G3b Proto 兼容与 trace 传播

当前 Echo schema 保留原有 package、service、RPC 路径和字段 tag 1，在 request 增 `client_hint`、response 增 `api_revision` tag 2；旧 schema 单独生成到示例 build output 的 `legacy/`，不污染根输出。`cargo test --manifest-path tests/consumers/grpc/Cargo.toml --all-features old_and_new_proto_clients_interoperate_over_real_tcp -- --nocapture` 实测旧 client→当前 server（旧响应 decoder 忽略 tag 2）与当前 client（发送未知 request tag 2）→旧 server（旧 server 忽略字段、新 client 收到新 response tag 默认值）均通过。

trace 仅传播经严格校验的 W3C version 00 `traceparent`。`cargo test -p openrust --features grpc --lib` 147 项通过，含 serde round-trip、零 ID/版本/大小写/注入格式拒绝、HTTP middleware 接受有效值并忽略非法值、typed client 只写入 traceparent 的边界测试；`cargo test --manifest-path examples/demo/Cargo.toml --features grpc --test grpc -- --nocapture` 7 项通过，其中联合 HTTP/RPC 的应用集成宿主经真实 TCP `/trace` 读取 `ExecutionContext`。示例 all-features 16 项通过；`combined_runtime_serves_both_protocols_and_drains_together` 发起真实 HTTP 请求并由 handler typed RPC 返回同一 traceparent，证明 HTTP→RPC 传播。没有 OTel feature 依赖，不传播 tracestate、baggage、Authorization、Cookie 或任意 headers。

默认与 all-features workspace all-target Clippy、默认 workspace tests、串行 all-features workspace tests、warnings-denied 默认/all-features Rustdoc、workspace 与示例 cargo deny、示例 all-target/all-features Clippy、Rust 1.97 gRPC feature check、no-default-features check 与 `git diff --check` 均通过；cargo deny 的 advisories/bans/licenses/sources 通过，仅保留既有重复 crate 版本和未匹配 MPL allowance 警告。双库不是本簇行为依赖；all-features 示例里的数据库认证用例因未设置 `OPENRUST_TEST_DATABASE_URL` 按其既有条件跳过，G2b 双库认证证据不在此重复计入。G3c 在该次 G3b 验收时未覆盖，后续真实双节点代理证据见下节。

## M5 gRPC G3c 双节点 HTTP/2 路由与健康摘流

`tests/consumers/grpc/scripts/test-g3c.sh` 使用 Docker Compose 启动两个真实 Linux OpenRust 节点和 Envoy 1.40.1。单一持久 `tonic::Channel` 的 40 次 unary RPC 由同一个 Envoy downstream HTTP/2 connection ID 发出，access log 显示两个 upstream 容器；长流运行时向其所属节点发送 SIGTERM，标准 gRPC Health `Check` 观测到 `NOT_SERVING`，Envoy gRPC active health checker 将节点标记为 unhealthy，10 次后续 unary RPC 全部抵达存活节点。既有长流因节点退出而中断，同 Channel 新调用成功；重启节点后 40 次 RPC 再次覆盖两个 upstream。末次实测 Envoy 约 0.54% CPU、19.78 MiB，单次观测不作为容量基线。Compose 脚本会清理该项目的容器、网络和本地节点镜像。

停机健康状态行为由 `cargo test --manifest-path examples/demo/Cargo.toml --all-features --test grpc` 8 项验证，其中 Health `Watch` 收到 `NOT_SERVING`、宽限期等于 shutdown deadline 被插件拒绝；`cargo test -p openrust --features grpc --lib grpc::tests::health_drain_grace_must_fit_inside_shutdown_deadline` 1 项通过。独立示例 `cargo test --locked --manifest-path tests/consumers/grpc/Cargo.toml --all-features` 16 项通过；本轮没有设置数据库 URL，条件数据库认证路径未构成新的双库证据。

本簇门禁通过：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --quiet`、`RUST_TEST_THREADS=1 cargo test --workspace --all-features --quiet`、warnings-denied workspace/示例 all-features Rustdoc、workspace 与独立示例 `cargo deny check`、独立示例 all-target/all-features Clippy、Rust 1.97 根 `grpc` 和独立示例 check、shell syntax、`git diff --check`，以及上述真实 Envoy 双节点验收。cargo deny 的 advisories/bans/licenses/sources 通过，保留既有 duplicate crate 与未匹配 MPL allowance 提示。G3c 不涉及数据库行为，不以本簇重复数据库验收；完整 API/worker 集群、多 worker/scheduler、共享依赖、跨节点上传和实时通信留在 M6。

## M5 ADR-060 密码哈希容量

使用 `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test -p openrust --features auth auth::password::tests --lib`，7 项通过；`cargo test -p openrust --features auth --lib`，160 项通过；`cargo test -p openrust --all-features --lib`，258 项通过、2 项忽略。`cargo test --manifest-path examples/demo/Cargo.toml --features auth --test auth_runtime auth_plugin_rejects_invalid_password_capacity_during_startup -- --exact`，1 项通过。PostgreSQL 18 和 MySQL 8.4 复用已运行容器，分别在 `OPENRUST_TEST_DATABASE_URL` 指向容器本地管理库时执行 `cargo test --manifest-path examples/demo/Cargo.toml --features 'auth db-<backend>' --test auth_database auth_flows_run_registration_login_refresh_reset_and_logout_on_the_configured_database -- --exact --nocapture`，各 1 项通过；URL 凭据从容器配置在测试进程内构造，未输出，数据由 `TestDatabase` 创建隔离库并在结束时删除。

门禁：`cargo fmt --all --check`、`git diff --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps`、auth feature 下根库/`根目录集成宿主` 定向 Clippy 与 Rustdoc、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过。首次对仅 `auth` 的根包 all-target Clippy 出现已有的 `CurrentUser::test_user` dead_code 警告，随后匹配范围的库 Clippy 和完整 workspace all-target Clippy 均通过。cargo deny 的 advisories/bans/licenses/sources 通过，保留既有重复 crate 与未匹配 MPL allowance 警告。默认 workspace 未配置 DB URL 的其他集成 targets 按条件跳过；双库 AuthFlows 用例已显式执行。无 all-features workspace 矩阵记录；此子簇匹配 `auth` feature、默认 workspace 及双库用例均已覆盖。

## M5 ADR-061 依赖就绪探针

`Readiness::register_probe`/`dependencies_ready` 与 HTTP `/ready` 已实现；最多 64 个稳定命名探针，整轮 2 秒、最多 16 并发且同一时刻只运行一轮。探针失败仅在服务日志记录探针名与稳定错误码，HTTP 返回通用 503。探针注册/重复名、失败恢复、并发上限/单飞及 ready 503 脱敏单测通过；根 `cargo test -p openrust --lib` 在新增单飞测试前 139 项通过，随后相关 readiness 5 项与 health 1 项再次定向通过。PostgreSQL 18/MySQL 8.4 均使用现有容器，经本地 TCP 故障代理实测真实 DB pool 断连时探测失败、代理恢复后 pool 重新连通并恢复 readiness；健康 pool 探针各另通过 1 项。Redis 8 现有服务运行 `cargo test --manifest-path examples/demo/Cargo.toml --features cache-redis --test server cache_plugin_uses_the_configured_shared_redis_store -- --exact --nocapture`，真实 CachePlugin store 探针通过。`cargo clippy -p openrust --all-targets --features 'db-postgres,cache-redis' -- -D warnings`、`cargo clippy -p server --test db_connection --features 'db-postgres,db-mysql' -- -D warnings`、`RUSTDOCFLAGS='-D warnings' cargo doc -p openrust --no-deps --features 'db-postgres,cache-redis'`、`cargo deny check`、fmt 与 diff check 通过；deny 的 advisories/bans/licenses/sources 通过，保留既有 duplicate crate 与未匹配 MPL allowance 提示。随后完整 workspace 默认 Clippy 与 tests 已通过；此记录仅描述 ADR-061 专项证据，不代表当前 M5 全部验收完成。

## M5 ADR-067 低基数指标与 OpenTelemetry（WIP）

2026-10-27：可选 `metrics` feature 提供显式启用的 Prometheus `/metrics`，HTTP 指标使用固定 method/status 类标签；真实 TCP 子进程测试确认默认服务不提供该路由、配置开启后可抓取，正文无 request ID 或路由标签。HTTP middleware 有界并发超限快速返回 503。可选 `otel` feature 通过 OTLP/HTTP batch exporter 导出 spans；本机临时 Collector 0.120.0 实际收到 HTTP spans，并通过真实跨协议 consumer 观察到 HTTP 与 typed gRPC spans 保留同一个 W3C Trace ID。同步 exporter 在 Tokio blocking pool 中 flush，测试覆盖 server lifecycle drain。HTTP span 不写 URI/request ID/traceparent；OTel 属性只含协议字段。PG18/MySQL8.4 真实隔离库登录突发在 2 connection pool 下运行 16 次请求，两个后端均观察到密码哈希容量拒绝并在突发后成功登录；样本结果见当前任务记录，尚不足以建立正式回归阈值或分离 API CPU/内存基线。

当前轮门禁：`cargo fmt --all --check`、`git diff --check`、默认与 all-features `cargo clippy --workspace --all-targets`、默认与 all-features `cargo test --workspace`、`RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --all-features` 与 `cargo deny check` 均通过；default workspace 中未设置 `OPENRUST_TEST_DATABASE_URL` 的集成测试按条件跳过。all-features workspace 测试 270 项通过、2 项忽略；默认 230 项通过。deny 的 advisories/bans/licenses/sources 通过，仍有既有 duplicate crate 与未匹配 MPL allowance 警告。PG18/MySQL8.4 突发单次样本分别为：PG pool=2、16 请求、4 成功/12 容量拒绝、623.1 req/s、p50 10.9ms、p95/p99 17.8ms、恢复 7.8ms；MySQL pool=2、16 请求、4 成功/12 拒绝、488.7 req/s、p50 16.8ms、p95/p99 23.9ms、恢复 8.7ms。单次本机小样本不能作为回归阈值；`/usr/bin/time -lp` 包含编译与测试进程，不能分离服务 CPU/内存。仍缺稳定重复压力模型、服务资源分项及过载下真实 API 吞吐/错误率报告，因此 ADR-067 保持 Proposed，M5 继续 in_progress。

后续对 HTTP response body 持有并发许可的工作区 WIP 做定向复验：`cargo test -p openrust --features 'metrics,otel,grpc' http::middleware::tests --lib` 4 项通过；`cargo clippy -p openrust --all-targets --features 'metrics,otel,grpc' -- -D warnings` 通过。测试验证许可在 body 读完/丢弃后释放，并保持并发超限立即返回 503；这部分 WIP 未包含在前述全 workspace 门禁或已提交实现中。

构建资源：代码门禁结束后 dry-run 再 clean，Cargo 报告主 target 回收 23.5 GiB、consumer 1.2 GiB、默认生成应用 1.8 GiB、双库生成验收 3.9 GiB；磁盘余量由约 101 GiB 恢复至 125 GiB。主 target 实际磁盘占用峰值约 17.95 GiB，合计 consumer 约 19.1 GiB。不清理主仓 target 或他人服务。生成应用源码与 /tmp 验收日志暂留供主控核对。
