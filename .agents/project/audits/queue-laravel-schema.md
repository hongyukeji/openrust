# 队列存储对齐 Laravel 与工具链文件移除

2026-11-20，用户要求彻底删除额外的队列恢复机制，按 Laravel 机制实现，不保留可选开关。另要求移除 dee8ae7fb 引入的根 rust-toolchain.toml，保留 Cargo.toml 的 rust-version。

## 契约与来源

- 固定 Laravel framework `1449bf241017087fbd38c118ecaabef3f6797fdc` 的 `src/Illuminate/Bus/DatabaseBatchRepository.php`：批次状态、失败 ID、options 保存在单一批次表，计数更新使用事务与行锁。应用基线 `06d016a364a37430eec9cbc52209adce3d7667ce` 的默认队列迁移为 jobs、failed_jobs、job_batches。
- 实现前核对本地 v0.x 的 `vendor/openrust/framework/src/queue.rs`、`queue/redis.rs`。旧版将组任务封装为内部任务，使用不同的 SeaORM/执行接口；未直接复用其公开接口或组调度器，保留当前 Laravel 批次仓库和队列驱动结构。

## 实现与边界

- 删除七条配套迁移、五张配套表的框架读写、SQL 发布 outbox、追加发布账本、逐投递完成账本、跨连接回执、Redis 发布／转发去重标记及两个专有恢复命令。新库队列只创建 jobs、failed_jobs、job_batches；根应用还有认证、缓存与 SMS 的独立迁移，共六条默认迁移文件。
- 普通批次、失败／重试、取消、追加、链内批次沿用现有公开 API。批次计数在 job_batches 行锁内更新；Rust 类型化续链及回调名称序列化进 options，后继任务直接按载荷中的连接／队列／延迟派发。Rust 无 PHP 序列化闭包，故保存稳定回调名和类型化续链载荷。
- 同连接数据库批次共享事务，Redis／独立 SQL／外部服务分别提交；发送错误返回调用方并回滚批次 SQL，但已被外部服务接收的任务可能保留。回调失败记录日志，不提供 outbox 补发或逐投递恰好一次保证；业务处理器负责幂等。
- PostgreSQL 真实失败路径暴露既有时间戳文本绑定和放弃失败事务后连接复用问题：按参数类型绑定 TIMESTAMP，未结束的 PostgreSQL 事务连接关闭后再归还池。失败任务清理改用已有查询构造器的可移植限量删除。
- 删除根工具链文件，保留 Cargo.toml 最低 Rust 1.94 声明；文档改为显式 cargo +1.94.0 验收。
- 不连接开发库执行删表。已有安装须先用旧版本处理完待发布／转发数据再升级，旧表与数据本批保留；确认闲置并备份后再清理。历史验收记录只作历史留证，不能用来声称已删除的机制仍然可用。

## 验收

独立 git archive 源码及本批修改：`/private/tmp/openrust-queue-laravel-20261120/source`；专属 target、CARGO_INCREMENTAL=0、CARGO_BUILD_JOBS=3、dev/test debug=0，显式 Rust 1.94.0。初始来源 HEAD 为 `88436e19312cf9f38b8246ac1d67b7b49ca08bc8`，追加本提交的源码改动。本批未在活跃工作树构建，未运行日常以外的 MySQL／SQLite／SQL Server 或真实 SQS／Beanstalkd 发布矩阵。

- `cargo +1.94.0 fmt --all --check` 及全 workspace/all-targets/locked 严格 Clippy 通过（clippy-verified.log）。
- 隔离 PostgreSQL 18.6 与 Redis 8.10.3：job_batches 的 PostgreSQL 函数实际执行通过，含五张辅助表不存在、两个专有命令已删除、并发计数、失败／重试、回调、取消、追加、批次任务链、链内批次、动态编辑、条件链、延迟和同步后继、Redis 双向跨连接、两个独立 PostgreSQL 数据库批次、发送失败回滚、失败任务清理。
- migrations 的 PostgreSQL 函数实际通过，含六条根迁移的状态／预演／回滚／刷新／重跑；不将缺环境导致的跳过计为成功。
- 框架 PostgreSQL 回归三个函数实际通过：TIMESTAMP 原生／模拟预处理绑定，单连接池丢弃已失败事务后继续使用，数据库队列真实 HTTP／预约／确认。五项顶层函数全部通过，日志 acceptance-final.log。
- 三语内容检查、VitePress 构建、`check_browser.mjs` 的两种挂载／桌面移动端 12 组合、专项队列页面两种挂载三语 6 页通过；正文检查确认没有旧辅助表或恢复命令，保留批次锚点。日志 docs-content.log、docs-build.log、docs-browser.log、docs-queue-browser.log。
- 编译中同步修复测试调用 timestamp 缺省精度参数；最终完整严格 Clippy 重新通过。早期失败日志保留，最终结果以上述命名日志为准。
- 最终整合以 `af4acff95cd1101fd2cd7ff2b03d10f3200d7c06` 的 git archive 加本批暂存改动建立 `integrated-source`，纳入已提交的 SMTP、SQLite／RSA 依赖修复及文档整理。Rust 1.94.0 fmt、五项实际 PostgreSQL／Redis 测试及全 workspace/all-targets/locked 严格 Clippy 再次全部通过（integrated-acceptance.log、integrated-clippy.log）。三语删除残留“投递记录”描述后，内容检查、站点构建和 12 组合浏览器复验通过（integrated-docs-content.log、integrated-docs-build.log、integrated-docs-browser.log）；专项 6 页检查沿用首轮结果。未覆盖完整发布矩阵。

首轮已回收本批闲置 target（3,928,244 KiB）和文档构建产物（61,636 KiB），df 实际可用增加 3,743,920 KiB。最终整合复验后，再回收专属 target（3,923,032 KiB）及文档构建产物（61,580 KiB）；此次 df 实际可用增加 3,739,336 KiB，收尾剩余 75,414,228 KiB。临时 PostgreSQL／Redis 已移除，已有 MySQL／PostgreSQL／Redis 服务及全部数据卷保留；源码、日志和共享缓存保留。复现所需的源码与差异由 Git 保留，原始日志在上述本批临时目录。本批按范围本地提交，不推送。

## 开发库旧表清理收尾

2026-11-20 用户要求继续处理收尾。仅连接本地 `.env` 指定的 PostgreSQL `openrust` 开发库（18.6），核对五张旧辅助表、jobs、failed_jobs、job_batches 全部为零行；Redis 只读检查未发现旧 chain_delivery 转发载荷，未清理已有 Redis 数据。

- 完整数据库私密备份：`/Users/Home/.local/state/openrust/backups/queue-laravel-20261120-234608/development-postgres.dump`；目录 0700、文件 0600，不进入 Git。使用项目 PostgreSQL 隔离夹具实际恢复成功，核对 13 条迁移和五张旧辅助表均存在且为空，随后回收临时容器。恢复须面向独立空库，说明在备份目录 README.txt。
- 单事务对旧辅助表和标准队列表加锁，重新断言无待处理记录；删除 job_batches_jobs、job_batches_outbox、job_batches_additions、job_batches_continuations、job_chain_deliveries，使用普通 DROP TABLE，无 CASCADE。同步删除七条已移除迁移的记录，不改剩余批号。
- 清理后公有 schema 为十张表，其中队列表仅 jobs、failed_jobs、job_batches；六条迁移记录逐项匹配当前 migration 文件。标准三表仍为空。其余业务表、原有进程、Redis／MySQL、全部数据卷保留。日志及脱敏结果在 `/private/tmp/openrust-queue-db-cleanup-20261120/`。本次仅数据库维护和记录，未启动 Cargo 或文档构建；此前源码集成验收结论不变。
