# 03-09 Redis 连接及缓存、锁接入

## 契约与实现

- 对照固定 Laravel RedisManager、连接测试、RedisStore、RedisLock 和 LuaScripts。参考 Goravel CacheServiceProvider 的应用级服务绑定。复用旧版 `6d1a8192` 的异步 ConnectionManager、Redis 原生命令与所有者 Lua 校验，并沿用旧版清空逻辑库的回归场景。 [Laravel Redis](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/redis.md) · [Laravel Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#atomic-locks) · [Goravel CacheServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/cache/service_provider.go)
- RedisManager 按应用缓存命名连接，命令传输按需建立。HTTP、Artisan 和 Cache 复用应用容器中的连接服务。支持字节读写、原始命令和独立订阅连接。原始命令保留 Redis 参数，调用方可通过 `key` 应用前缀。
- 缓存使用 Redis 原生 TTL、NX、批量读写、GETDEL 和整数增减。GETDEL 需要 Redis 6.2 或更高版本。整数使用 Redis 可运算的编码。数字字符串带标记以保留类型，计数时原子转换并保留 TTL。其余数据使用带标记的 Rust JSON，含字节类型。参见[缓存类型及数字字符串复验](../03-06/README.md#类型化读取与批量接口)。Redis 锁用毫秒租期和 Lua 所有者校验，数据与锁可使用独立命名连接。清空使用 FLUSHDB，影响选定逻辑库的全部键。
- 连接可为后续命令重建传输，结果不确定的命令交给调用方判断是否安全重试。重连使用客户端的退避实现。Redis Cluster、其他客户端的退避算法、自动命令重放、缓存标签及 Windows、真实 TLS 尚未验收。

## 验收

隔离基线 `21a65e98` 加已验收的依赖与本批改动，macOS arm64，Rust 1.94.0，专用 Redis 8.10.3。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 394 项通过。本批未启用真实 MySQL 和 S3。
- 真实 Redis 验证 ACL 认证、URL 与配置的逻辑库选择、Pub/Sub、批量 JSON 与字节、原子 add/pull、计数保留 TTL、跨应用竞争、过期锁、旧持有者失效、进程间所有者恢复。精确终止数据连接后读请求恢复。
- 实际 HTTP 验证缓存命中、Artisan 清空后缺失及独立库的锁保留。三语 Redis 文档示例编译后均通过真实 HTTP。实际运行 `./artisan cache:forget artifact redis`、`./artisan cache:clear redis`、`./artisan inspire`，均成功。
- 文档构建成功，根路径与子路径通过 12 项通用浏览器检查和 6 项 Redis、缓存交叉链接检查。

下一批为 03-10 国际化、回退和复数，依赖已验收的 01-01 配置。

## 原生 Redis Cluster

- 对照固定 Laravel [Clusters](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/redis.md#clusters)和 [RedisManager 的具名连接解析](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/RedisManager.php#L99)。旧版未发现原生集群客户端。复用现有具名连接、URL 与凭据合并、前缀、单键锁脚本和应用容器绑定。
- `database.redis.clusters` 接受具名种子列表，直连定义优先。现有 `redis` 1.8.0 启用 `cluster-async`，追加 MIT 授权的 `crc16` 0.4.0。集群只有数据库 0，RESP3 支撑普通与模式订阅的独立连接。保留同名缓存连接复用。集群重试设置来自首个种子，拒绝会让客户端退避计算溢出的参数。
- 缓存值、原子取回和普通持有者锁可跨主节点路由。分片批量读保留输入顺序，批量写逐项执行，各槽之间没有共享事务。普通清库由客户端分发到全部主节点。集群标签、仅清锁及队列的专用阻塞连接暂按各自能力边界返回错误。
- 基线 `4a769cf1a`，Rust 1.94.0、macOS arm64。实际 Redis 8.10.3 三主节点覆盖 16384 槽。验证不可用种子后的发现、密码与 URL、非零库和空列表拒绝、同名直连优先、跨槽读写与顺序、Pub/Sub 和模式订阅、跨实例原子取回、续期、令牌释放和全主节点清库。`sh tests/redis_cluster.sh` 可复验。在带认证集群上通过实际双进程 HTTP，以及独立 Artisan `cache:forget`、`cache:clear`，另复验单机 Redis 入口和 `artisan inspire`。
- 变更文件格式、严格 Clippy、三语 VitePress、18 项浏览器检查和 Rust 示例编译通过。全仓格式仍只报告既有 `sanctum_authentication.rs:1412`。`cargo deny --offline check` 无项目配置，基线和本批都报告默认许可清单拒绝及 `paste`、`pin-utils` 的既有 unmaintained 提示。本批额外的许可提示来自 `crc16`，已核对其 MIT 授权。依赖禁用和来源检查通过。
- 本批集群和单机夹具已回收。清理 4.0 GiB 隔离编译产物和文档缓存后，项目卷可用 57.22 GB（53.29 GiB），周额度剩余 68%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-cluster.4SFbXqqD`。
- 03-09 保持开放。下一步验证槽迁移与主节点故障切换，收口连接中断后结果未知命令的重试边界，并补标签和仅清锁的集群验收。已验收的单机连接行为保持当前契约。

## 2026-11-17 09:36 槽迁移、主节点切换与写入重试

- 固定 Laravel 的 PhpRedis 集群连接将 `command_retries` 默认为零。旧版 `v0.x` 无集群重定向实现。Rust 关闭 redis 1.8.0 的命令自动重试，保留拓扑刷新和后续请求的传输恢复。框架只对已覆盖的单目标操作跟随 Redis 的明确 `MOVED` 和 `ASK`，上限取首个种子的 `max_retries`。ASKING 与原命令在同一连接按序发送。 [固定 Laravel 集群连接](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/Connectors/PhpRedisConnector.php#L46) · [Redis Cluster 规范](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/#ask-redirection)
- 原生路由保留常规批量读取的分片并发。Cache 批量读取收到明确槽重定向时，逐键查询并保持缺失项、重复项和输入顺序。连接错误由调用方处理。其他原始命令的重定向仍需扩展，未携带主机名的重定向和真实 TLS 待补验。
- 在 `85b31347c` 隔离快照上使用 Redis 8.10.3 的三主三副本运行 `sh tests/redis_cluster.sh`。验收覆盖 ASK、MOVED、零重定向上限、迁移期间的跨槽缓存批量读、实际停止主节点后的副本提升、同一应用连接恢复读写。专用回环代理丢弃一次 INCR 成功回执，客户端收到错误，直接读取源节点计数为 1。
- 实际双进程 HTTP、独立 Artisan `cache:forget`、`cache:clear` 和 `artisan inspire` 通过，另复验真实单机 Redis。变更文件格式、工作区严格 Clippy、三语站点构建、12 项通用和 6 项 Redis 页面浏览器检查及集群文档 Rust 示例编译通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412` 差异。
- 本批夹具、隔离 Cargo 产物和文档缓存均已回收。项目卷可用 57.16 GB（53.23 GiB），周额度剩余 67%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-topology.mmkmHPre`。03-09 整单元待集群标签、仅清锁和剩余原始命令边界，核心账本保持 51/66。
