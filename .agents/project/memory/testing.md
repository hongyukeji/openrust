# 测试基线

> 受众：AI Agent
> 更新：2026-10-26
> 摘要：根 package 模块迁移、双库与生成项目当前可复验入口。

## ADR-039 源码交付验收

当前 package 为 `openrust`、`cli`、`macros`、`testing`、`server`；均显式禁用发布，内部依赖通过 workspace path 解析。CLI 保留 `openrust` 命令，并默认生成 `apps/<name>`，应用与 migration 加入现有 workspace。生成器保留根 manifest 其他内容，拒绝重复名、现存目录和不支持的外部目标；dev/serve 按目标应用定位二进制和配置。

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

应用数据加密专项：`cargo test -p openrust --features encryption security::tests`；生成器/密钥命令：`cargo test -p cli`；`apps/server` 接线：`cargo check -p server --features encryption`。在干净源码副本中生成的 `openrust new --auth --encryption` PostgreSQL/MySQL 应用均通过直接 `cargo test`，未添加临时 registry patch。

根 package 示例：`cargo run -p openrust --example hello`；应用配置可用 `APP_SERVER__PORT` 指定隔离端口，HTTP smoke 访问 `/` 应返回统一 JSON 200。

## PostgreSQL / MySQL

CI 数据库服务作业现分别启用 PostgreSQL/MySQL 的 `queue` feature，并显式运行 `db_migration`、`queue`、`queue_crash_recovery` 和 `schedule` targets。普通 all-features 作业无数据库 URL 时仍可能跳过真实集成；需以数据库 job 命令实际执行结果作为证据。

暂定最低支持为 PostgreSQL 14+、MySQL 8.0+、Redis 6.2+；日常开发与 Compose 基线为 PostgreSQL 18、MySQL 8.4 LTS、Redis 8；常规 CI 数据库基线为 PostgreSQL 18、MySQL 8.4。每次数据库/缓存集成测试前，先盘点当前可用的 Docker 容器和本机服务，确认版本、健康状态、监听端口及测试所需连通性，然后优先复用合适的现有服务。不要因使用项目 Compose 更方便就重复创建基础服务。

M4 队列集成测试使用 `OPENRUST_TEST_DATABASE_URL`，由 `openrust_test::TestDatabase` 建立隔离数据库并在测试后清理。命令：`cargo test -p openrust-server --features 'queue,db-postgres' --test db_migration --test queue --test queue_crash_recovery`；MySQL 将 `db-postgres` 换为 `db-mysql`。2026-10-26 在现有 PostgreSQL 18 与 MySQL 8.4 容器实测迁移、并发 worker/dispatch、事务派发提交与回滚、进度、失败与幂等重试、单任务超时后继续处理，以及独立子进程强杀后的 lease 到期恢复。`queue_crash_recovery` 当前只验证崩溃恢复；旧 worker 与新 token 并发接管 fencing 的专项竞态断言及完整跨节点验收仍未完成；调度的独立进程证据见下段。凭据由容器环境变量读取，没有输出到日志；既有服务未重建或停止。

M4 scheduler 集成入口：`cargo test -p openrust-server --features 'queue,db-postgres' --test schedule`，MySQL 使用 `db-mysql`。2026-10-26 两库各通过 4 项测试：两个独立进程经同步屏障竞争同一 UTC interval slot、强制异常退出后保持已提交 slot、`schedule run --once` 命令、常驻命令 SIGTERM 退出；均验证隔离数据库只产生一条 schedule run 和一条 job。另有 interval/daily slot 边界单测。scheduler 使用共享数据库时钟和唯一键；该验收不替代生产负载资源测量或最终集群部署验收。

2026-10-26 队列唯一键改动后，PostgreSQL 18 与 MySQL 8.4 分别重跑 queue migration 和 queue 集成测试通过。额外验证同 key 并发 dispatch 返回同一活动 job ID、完成后可再次创建、以及同一业务事务中重复 dispatch 只创建一个 job。唯一键只抑制活动任务重叠，不提供完成后持久化的请求幂等历史。

2026-10-26 M4 任务链/批次簇：`cargo fmt --all --check`、`cargo clippy --workspace --all-targets --all-features -- -D warnings`、`cargo test --workspace --all-features`、warnings-denied all-features docs、`cargo deny check` 与 `git diff --check` 通过。PostgreSQL 18 和 MySQL 8.4 各自重跑 `db_migration`、`queue`、`queue_crash_recovery` 集成测试；覆盖 chain 顺序、事务派发、Stop/Continue、batch 并发上限、失败语义与输入边界。cargo-deny 仅报告既有重复依赖版本和未使用 MPL allowance 警告，其余 advisories/bans/licenses/sources 通过。未执行生产资源或多节点集群测试。

2026-10-26 目录收尾：迁移前后 `cargo metadata --no-deps` 均列出 5 个 workspace packages；all-features `cargo test -- --list` 均为 265 个条目、target/test 名保留。迁移后 workspace test、all-target clippy、warnings-denied docs、`cargo deny check`、fmt 和 `cargo check -p openrust --no-default-features` 通过。PostgreSQL 18 与 MySQL 8.4 各执行 `cargo test -p openrust-server --all-features`，40 项真实 server 集成测试通过。CLI 生成的 PostgreSQL/MySQL `--db --auth` 项目各自经本地 `openrust`/`openrust-test` path patch 通过 fmt、check 和真实数据库 `auth_flows` HTTP test；默认生成项目 check/test 也通过。hello 示例作为真实 TCP HTTP 服务返回 200 JSON。queue/Scheduler 双库命令已加入 CI；未宣称生产集群或最低版本总验收。

2026-10-26 M3 Mailer/Policy 生成器：CLI 单测 13 项通过，覆盖命令解析、输出结构、CRUD 权限名、模块注册、多模块追加与稳定排序、非法输入、缺少 `auth` 和不覆盖现有文件。PG18/MySQL8.4 `--db --auth` 新项目各连续生成多个 mailer/policy，使用仅限 `/tmp` 的本地 `openrust` / `openrust-test` path patch 分别通过 `cargo fmt --all --check`、`cargo check` 与 `cargo test`；两库 `auth_flows` 各 1 项通过真实隔离数据库和 loopback HTTP。MySQL TestDatabase 从容器内已有 `mysql` 管理库连接，未创建或修改现有服务。Workspace fmt、default/all-features clippy/test、warnings-denied all-features docs 和 `cargo deny check` 通过；deny 仅有既有 duplicate dependency 与 unused MPL allowance 警告。没有外部短信投递或集群验收声明。

2026-10-26 M3 HTTP route rate limit：`cargo test -p openrust --test http_router --test http_resource` 通过（35 router、16 resource），all-features route limit 目标测试通过；同 scope/key 超限得到 429 与 `Retry-After`，不同自定义 key 隔离，auth-enabled 缺失主体拒绝。通过 `cargo test -p openrust-server --features queue,db-postgres --test db_migration migration_command_applies_repeats_reports_and_rolls_back -- --exact --nocapture` 与 MySQL 对应命令，在现有 PostgreSQL 18 / MySQL 8.4 容器中的隔离数据库各通过：真实 DatabaseRateLimitStore 多实例并发，以及 `RateLimitLayer::by_client_ip` HTTP 请求遇到伪造 XFF 仍按未信任 socket peer 限额。数据库用户/密码只在运行器进程环境内传递，未输出；容器未被创建、修改或停止。

只有找不到符合版本和测试要求的现有服务，或现有服务无法用于该测试时，才创建隔离的临时服务。使用独立 Compose project/端口，先用 `TestDatabase` 或独立测试库隔离数据；不得擅自停止、删除、重建或改动已有容器、卷、数据和配置。新建资源只清理本任务明确创建且确认无其他使用者的部分。检查命令不得输出连接密码或其他凭据。

```bash
docker ps --format 'table {{.Names}}\t{{.Image}}\t{{.Ports}}\t{{.Status}}'
lsof -nP -iTCP -sTCP:LISTEN
```

确认没有合适服务后，才使用项目 Compose；需要隔离端口时：

```bash
OPENRUST_TEST_POSTGRES_PORT=55432 OPENRUST_TEST_MYSQL_PORT=53306 docker compose -p openrust-ci -f compose.ci.yaml up -d --wait postgres mysql
OPENRUST_TEST_DATABASE_URL=postgres://openrust:openrust@localhost:55432/openrust_test cargo test -p openrust-server --features auth,db-postgres --test auth_database --test db_connection --test db_migration
OPENRUST_TEST_DATABASE_URL=mysql://root:root@localhost:53306/openrust_test cargo test -p openrust-server --features auth,db-mysql --test auth_database --test db_connection --test db_migration
```

MySQL `openrust-test` 隔离测试需具备建库/删库权限的管理 URL；一般应用集成测试使用最低权限 `openrust` 用户。MySQL driver 强制 Rustls TLS，并在新连接设置 `+00:00` 时区。两个独立数据库不能替代真实复制拓扑。

M2 读写路由真实复制测试使用 `OPENRUST_TEST_DATABASE_URL`、`OPENRUST_TEST_READ_REPLICA_URL` 配置主库/副本，并设置 `OPENRUST_TEST_VERIFY_REPLICA_LAG=1` 启用副本暂停/追平断言。测试暂停 PostgreSQL WAL replay 或 MySQL SQL thread，验证副本旧读、两个独立 `App` 的 `with_primary` 主库新读与 scope 隔离，再恢复复制等待追平。2026-10-26 在 PostgreSQL 18.6 和 MySQL 8.4.13 临时主从服务通过。

可运行 `cargo bench -p openrust-server --features db-postgres --bench db_routing` 或 `cargo bench -p openrust-server --features db-mysql --bench db_routing` 对主库与显式副本分别进行 20 次预热、500 次串行 `SELECT 1` 查询，记录吞吐及 p50/p95/p99；`OPENRUST_DB_ROUTE_BENCH_SAMPLES` 可设为至少 100 的样本数。2026-10-26 Apple arm64、16 核 / 64 GiB、Docker 29.9.2、Release profile 下最新结果：PostgreSQL 18.6 primary 3658.0 req/s (p50 265.875µs, p95 332.292µs, p99 368.708µs)，replica 3452.1 req/s (270.333/424.625/541.75µs)；MySQL 8.4.13 primary 3735.9 req/s (p50 256.958µs, p95 315.333µs, p99 362.041µs)，replica 3709.8 req/s (261.416/303.5/484.333µs)。此 `SELECT 1` 单并发微基准只记录路由和往返基线，不代表业务吞吐或性能提升；完整 CPU/内存/业务负载测试仍按性能计划执行。

2026-10-26 根模块迁移后，上述 DB/auth tests 在 PostgreSQL 16 和 MySQL 8.4 均实际通过（分别 4 项 auth、3 项连接/事务、1 项迁移）。`openrust-test` 数据库隔离测试也分别通过。

## 生成项目

CLI 模板快照测试：`cargo test -p openrust-cli`。生成 PostgreSQL、MySQL `--auth` 项目后运行 `cargo fmt --all --check`、`cargo check`、`cargo test`。旧模板的历史集成验证使用 Cargo patch 指向本地根 `openrust` 与 `testing`。新的源码交付验收按 ADR-039，在干净源码副本中生成 apps 后直接运行，不继续以 patch 作为默认操作入口。

2026-10-26 根模块迁移后，两种生成项目都以真实 PostgreSQL 16 / MySQL 8.4 服务运行 `generated_auth_routes_run_over_http_with_isolated_database_and_fake_mail` 并通过。测试覆盖实际 HTTP、JWT/PAT 认证、数据库流程、可信代理 client IP 与 fake mail。

2026-10-26 当前日常基线复验：`cargo test -p openrust-server --features auth,db-postgres --test auth_database` 在现有 PostgreSQL 18 容器通过 4 项；`--features auth,db-mysql` 在现有 MySQL 8.4 容器通过 4 项。按当前 CLI 模板新生成的 `--auth` 项目分别以本地 Cargo patch 指向未发布 workspace，并在真实 PostgreSQL 18 / MySQL 8.4 服务上通过 `cargo fmt --all --check`、`cargo check` 和 `cargo test --test auth_flows`（每库 1 项真实 TCP HTTP 流程，使用隔离数据库与 fake mail）。生成目录位于 `/tmp/openrust-auth-pg-resume` 与 `/tmp/openrust-auth-mysql-resume`，没有纳入仓库。

2026-10-26 ADR-031 出站 HTTP：`cargo test -p openrust --features http-client http::client::tests` 通过 15 项，覆盖本机真实 TCP JSON/status/body limit/redirect/timeout/response stream，fake 不访问网络、JSON/query/form/multipart/stream body、transport 与 response body timeout、取消、retry transport/status 与 attempt cap、不可重放 request 拒绝、配置拒绝及 proxy 凭据 Debug 脱敏；`cargo test -p openrust-server --features http-client --test http_client` 通过 1 项聚合应用接入。CLI `--http-client` 生成项目 `/tmp/openrust-http-client-generated-20261026` 经 Cargo patch 指向本地未发布 workspace，通过 `cargo check`、`cargo test`。最终通过 `cargo fmt --all --check`、default/all-features workspace clippy/test、warnings-denied default/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`、默认 normal dependency tree 无 reqwest、`git diff --check`。cargo-deny 仅保留既有 MPL-2.0 unused allowance 与 duplicate dependency warnings，advisories/bans/licenses/sources 均通过。

2026-10-26 ADR-033 邮件/SMS：`cargo test -p openrust --features 'mail-smtp,sms-twilio,test-util' mail::tests:: -- --include-ignored` 通过，含真实本机 SMTP 对 Mailpit v1.32.0 的投递；`curl --noproxy '*' http://127.0.0.1:18025/api/v1/messages` 确认 `user@example.test` 收到 `Welcome`。Twilio transport tests 覆盖 POST URL、Basic auth、form/template fields、2xx receipt、4xx脱敏、timeout及单次调用不重试；测试不访问 Twilio。CLI 9 项测试与 `--sms-twilio` 生成项目 `/tmp/openrust-sms-generated-20261026` 的 check/test 通过；default/all-features workspace test、all-features clippy、warnings-denied docs、cargo deny 与 diff-check 通过。未提供实际 Twilio 账号凭据与授权号码，真实 SMS 投递仍未验收。

2026-10-26 ADR-032 请求级国际化：6 项 `i18n` 单测与 5 项 `apps/server/tests/i18n/runtime.rs` 通过，覆盖 locale 规范化、Accept-Language q 值、TOML config、JSON resource 合并/参数/fallback、请求并发隔离、错误/validator message、稳定 code/字段路径、未翻译应用错误保留与显式 locale override。新生成项目 `/tmp/openrust-i18n-generated-20261026` 使用本地 Cargo patch 通过 `cargo check`、`cargo test`；其真实 server 进程收到 `Accept-Language: zh-CN` 后对 404 返回中文 message。default/all-features `fmt --check`、workspace clippy/test、warnings-denied docs、`cargo deny check` 通过。无数据库行为，不要求双库集成；邮件/SMS 的实际供应商验收留在 M3 通信簇。

## 迁移验收记录

- `cargo metadata --no-deps --format-version 1` 确认根 package 为 `openrust`，workspace 共五个成员：`openrust`、`openrust-cli`、`openrust-macros`、`openrust-test`、`openrust-server`。
- 无默认、默认、关键单独 feature、all-features 编译已检查；workspace default/all-features clippy 与 test、warnings-denied rustdoc、`cargo deny check` 均通过。`cargo deny` 仍会输出重复版本依赖提示，但无 advisory、license、ban 或 source 失败；JWT 使用 `aws_lc_rs`，不再解析到受 RUSTSEC-2023-0071 覆盖的 `rsa 0.9`。
- M2 数据库读写分离的 PostgreSQL/MySQL 真实复制拓扑已单独通过（见上文）；其余尚未验收的范围包括 Redis、生产负载下多 worker/旧 token 竞态、跨节点实时通信、S3 与完整集群，继续按 `todo.md` 和各专题计划推进。
- PostgreSQL 14 / MySQL 8.0 结果是历史兼容性证据；当前暂定下限已采用 PostgreSQL 14+ / MySQL 8.0+，但这些结果不替代发布前对完整承诺能力的最低版本复核。Redis 6.2+ 的真实兼容性在 M5 driver 落地后验证。

2026-10-26 ADR-034 应用数据加密/本地轮换：`cargo test -p openrust --features encryption security::tests` 8 项通过，包含 RFC 8452 AES-256-GCM-SIV 向量、二进制/空明文、随机 nonce、context、active key 写入/历史 key 解密、篡改/格式/越界、配置脱敏与多 Encryptor 互解；`cargo test -p openrust-cli` 11 项通过，包含 32-byte Base64 key generation 与 `--encryption` 模板；`cargo check -p openrust-server --features encryption` 通过。实际 CLI 生成项目 `/tmp/openrust-encryption-generated-20261026` 使用本地 Cargo path patch 通过 `cargo check`、`cargo test`，并用环境 key 执行 `cargo run -- config show`；输出中的 active key ID 与 key map 值均为 `********`。最终 default/all-features workspace tests、all-features clippy、warnings-denied default/all-features docs、`cargo deny check` 和 `git diff --check` 通过；cargo-deny 保留既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。该簇无数据库读写，不要求新增双库集成验收。测试应用 all-features 时显式注入测试 key ring；生产默认配置不含 key。

2026-10-26 ADR-035 审计簇：`cargo test -p openrust --features test-util audit::tests` 6 项通过，覆盖 allowlist、敏感详情/Debug 脱敏、错误传播、database/custom plugin 和公开 fake；PostgreSQL 18 与 MySQL 8.4 的 `cargo test -p openrust-server --features auth,db-<backend> --test auth_database` 各 5 项通过。新增测试使用 `TestDatabase` 隔离库和完整 audit 表名映射，验证权限变更与审计同事务提交、外层回滚、审计 insert 失败导致权限回滚，以及最多 N 行的重复清理。CLI 新生成的 PostgreSQL 与 MySQL `--auth` 项目均通过 `cargo fmt --all --check`、本地 Cargo patch `cargo check` 和真实 `auth_flows` HTTP/TCP 数据库流程测试。workspace default/all-features tests/clippy、warnings-denied default/all-features docs、no-default-features check、`cargo deny check` 通过；deny 仅有既有重复依赖与未使用 MPL allowance 警告，advisories/bans/licenses/sources 通过。

2026-10-26 ADR-036 signed links：`cargo test -p openrust --no-default-features --features signing signing::tests` 4 项通过，覆盖用途/subject/TTL、Debug 脱敏、shared store 撤销与保留旧 key 的轮换；PostgreSQL 18、MySQL 8.4 完整 `cargo test -p openrust-server --features auth,db-<backend> --test auth_database` 各 6 项通过，覆盖两个 signer 实例跨库撤销、自定义完整表名映射、过期记录有界清理及应用插件注册。新生成 `--auth` 项目分别使用本地 Cargo patch，通过 `cargo fmt --all --check`、`cargo check`、真实数据库迁移及 TCP HTTP auth flow。最终 `cargo fmt --all --check`、`git diff --check`、workspace default/all-features clippy/test、warnings-denied default/all-features docs、`cargo check -p openrust --no-default-features`、`cargo deny check` 均通过；deny 仅有已存在的重复依赖与 unused MPL allowance 警告，advisories/bans/licenses/sources 通过。没有新增依赖。

2026-10-26 M3 认证/安全总验收：本机预置 PostgreSQL 18 / MySQL 8.4 容器凭据与仓库 CI 默认值不同，未触碰预置数据库；使用仓库 `compose.ci.yaml` 的无持久卷 PostgreSQL 18 / MySQL 8.4 服务、独立测试库后，`cargo test -p openrust-server --features auth,db-<backend> --test auth_database --test auth_runtime` 各 12 项通过。按当前模板生成的 PG/MySQL 项目分别用真实数据库执行 `cargo test --test auth_flows`，各 1 项真实 TCP HTTP 流程通过。覆盖注册/重复注册、邮箱验证、JWT 与 PAT、refresh 单次轮换/重放撤销、密码重置、refresh logout、Session 登录/退出与 CSRF、共享限流、可信代理、Policy/RBAC 与 PAT ability 交集。生成项目各自还通过 `cargo fmt --all --check` 与 `cargo check`。临时 Compose 服务已停止并移除，既有 PG/MySQL/Redis 服务保持运行。

2026-10-26 M3 数据输出/查询边界：`cargo test -p openrust http::sort::tests` 覆盖排序解析、字段 allowlist、重复/未知/格式错误输入；CLI generator 单测覆盖 `--uuid-id` / `--soft-delete` 和生成 feature 注册。临时生成项目（本地 patch 到当前 workspace）分别在现有 PostgreSQL 18 与 MySQL 8.4 上通过格式检查、`cargo check` 和真实隔离数据库 test：迁移 UUID 主键及 UTC `deleted_at`，插入生成 UUIDv7，显式 `not_deleted()` 隐藏软删除行，普通 `find_by_id` 仍能读取，清空 `deleted_at` 后恢复。HTTP README 补充严格输入 DTO、响应字段 allowlist、显式 SeaORM Condition、关联预加载示例；没有引入 ORM 自动过滤或动态查询 DSL。测试只使用现有服务与临时目录，未输出凭据，未改动容器。

2026-10-26 M4 typed events/queued listener：`cargo test -p openrust --all-features events::tests` 与 `cargo test -p openrust --features queue --lib events::queued_tests` 通过，后者覆盖 mapper 成功、mapper error 与 Queue dispatch error 传播/短路。外部下游 consumer crate 在 `/tmp/openrust-events-consumer` 以 `queue,db-postgres` 通过离线 `cargo check`。真实双库命令均运行成功：`cargo test -p openrust-server --features queue,db-postgres --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在现有 PostgreSQL 18.6 通过；`cargo test -p openrust-server --features queue,db-mysql --test queue database_queue_dispatch_worker_concurrency_failure_and_retry -- --exact` 在临时 MySQL 8.4 通过。覆盖事件 transaction context、事务回滚不留任务、提交后 worker 消费、普通 dispatch 消费；测试显式释放 Events/Queue/Db/App 后 `TestDatabase::close` 正常删除隔离库。预置 MySQL 8.4 容器因容器内 socket PID lock 错误持续重启，未改动；仅使用 `compose.ci.yaml` 在空闲 53306 端口创建 `openrust-ci` 临时 MySQL，测试后 `docker compose ... down -v` 仅清理本次创建的容器/网络/卷。修复后的最终门禁通过：`cargo fmt --all --check`、`git diff --check`、workspace all-target/all-features clippy、default/all-features workspace tests、warnings-denied default/all-features docs、`cargo deny check`、`cargo check -p openrust --no-default-features`。未触碰预置 PostgreSQL/MySQL/Redis 数据，无凭据输出。

2026-10-26 ADR-041 事务与 AuthFlows 事件：在任务专属 Compose project `openrust-m4-auth-tx`（PostgreSQL 18、MySQL 8.4，端口 55432/53306）分别执行 `cargo test -p server --features auth,db-<backend> --test db_connection transaction_events_rollback_before_commit_and_ignore_after_commit_errors -- --exact` 与 `cargo test -p server --features auth,db-<backend> --test auth_database auth_flow_events_share_account_and_refresh_transactions -- --exact`，四项均实际运行通过。验证提交前拒绝回滚、提交后错误不掩盖已提交结果、注册校验拒绝不建账户、泛化登录失败事件，以及登录成功事件失败无 refresh token 遗留。临时服务由本任务创建；预置服务未改动。

2026-10-26 ADR-042 共享锁与请求幂等：在任务专属 `openrust-m4-auth-tx` PostgreSQL 18 / MySQL 8.4 服务，对每个 backend 实际执行 `cargo test -p server --features db-<backend> --test db_coordination -- --exact shared_locks_and_request_idempotency_are_database_scoped`，均 1 项通过。测试使用 `coord_` 自定义表前缀的 TestDatabase 隔离库及两个独立 SeaORM pool，覆盖迁移索引、共享锁 owner 竞争、过期接管/fencing、旧 owner 释放与事务内检查、同键并发仅一 owner、scope 隔离/指纹冲突、事务回滚同时撤销业务写入和完成快照、成功完成后的 JSON replay、过期 lease 接管及每次最多 N 行清理。测试运行时没有临时 services 以外的依赖；没有声称进程强杀、真实 HTTP middleware 或外部副作用 exactly-once。
