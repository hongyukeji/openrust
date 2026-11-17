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

## 独立 Redis Cluster 清锁

- 固定 Laravel 的 `RedisStore::flushLocks` 要求独立锁库，再对锁连接执行 `FLUSHDB`。在旧版 `v0.x` 未找到 Redis Cluster 清锁实现。复用现有锁连接、仓库事件和 Artisan `cache:clear --locks`。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L290)
- 使用集群节点 ID 拒绝同一个集群的不同种子连接和指向其中节点的直连别名。拓扑查询出现错误时终止清理。确认锁库独立后，Redis 对锁连接执行 `FLUSHDB`，具名集群连接会覆盖各主节点。三语 Redis 和缓存文档记录作用范围及 ACL 所需命令。
- 在隔离基线 `a73b475da` 用 Redis 8.10.3 验证同集群别名与直连别名拒绝、两个独立集群之间清锁、集群和直连的两种组合，以及分片锁与数据哨兵。实际双进程 HTTP 和独立 Artisan 验证别名拒绝和成功清理，缓存值均保留。单机 Redis、原有集群槽迁移与故障切换、`artisan inspire` 通过。
- 改动文件格式、严格 Clippy、三语站点、12 项通用浏览器检查及 6 项 Redis 页面检查通过。全仓格式仍报告既有的 `sanctum_authentication.rs:1412`。临时 Redis 与本批隔离缓存已回收。项目卷可用 52.79 GB（49.17 GiB），周额度剩余 67%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-locks.dEsERJoU`。
- 03-09 待集群标签和其他原始命令的重定向覆盖，核心账本保持 51/66。

## Redis Cluster 标签与过期索引清理

- 固定 Laravel 的 RedisTagSet 用有序集合跟踪标签成员，RedisTaggedCache 在集群上按标签分批清除，Artisan 使用 `cache:prune-stale-tags`。核对旧版 `v0.x` 的 `redis_tags.rs` 和标签身份测试，复用代际核验、物理索引、分批退役及失败后续做机制。 [Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php) · [Laravel 集群标签清理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L210)
- Rust 根据连接与缓存存储前缀确定一个标签槽，显式声明 Lua 访问的键，并在写入前校验快照代际。标签可跨组合清理且按顺序区分身份。单槽让值写入与索引登记保持原子，热点可按不同前缀分散。标签修剪按注册索引分页，仅移除已无对应值的索引引用。Redis 直连原有键格式及普通集群缓存的分片机制继续使用原实现。 [Redis EVAL 键声明](https://redis.io/docs/latest/commands/eval/)
- 隔离基线 `bdfec17e5`，Rust 1.94.0、Redis 8.10.3。实际集群脚本覆盖有序和重复标签、跨客户端读取、普通键与不同前缀隔离、计数、续期、过期引用、超过一批的物理清除、无效引用导致清理中断及可恢复。现有集群连接、锁、槽迁移和故障切换全部复验。Redis 直连标签与缓存入口回归通过。
- 两个实际 HTTP 进程分别写入和读回标签值。独立 Artisan `cache:prune-stale-tags redis` 修剪过期引用并保留有效值，`cache:clear redis --tags=artists` 清除匹配标签并保留普通值和其他标签，`artisan inspire` 正常退出。
- 改动文件格式、严格 Clippy、三语站点、12 项通用浏览器检查和 6 项标签文档链接检查通过。全仓格式仍报告既有的 `sanctum_authentication.rs:1412`。临时 Redis 与本批隔离产物已回收。项目卷可用 117.29 GB（109.23 GiB），周额度剩余 67%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-tags.lo4O4vrg`。
- 03-09 整单元继续跟踪其他原始 Redis 命令的重定向，以及真实 TLS 和无主机名重定向；标签脚本跨迁移窗口的专门回归待补。本批不调整核心账本闭合数。

## 原始命令重定向与标签迁槽

- 固定 Laravel 的 PhpRedisClusterConnection 复用 RedisCluster 的协议路由。旧版 `v0.x` 未找到原生 ASK/MOVED 实现。本批沿用现有显式重定向机制，原始命令需要额外键验证时向 Redis 查询 `COMMAND GETKEYS`。只有返回的全部键都匹配重定向槽，才跟随服务器给出的目的地址。键参数保留原始字节。 [Laravel 集群连接](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/Connections/PhpRedisClusterConnection.php) · [Redis COMMAND GETKEYS](https://redis.io/docs/latest/commands/command-getkeys/)
- ACL 拒绝键查询、键无法核对或命令跨槽时，保留原始响应。跨槽写入可能部分成功。ASKING 与命令继续在同一连接按序发送，分别核对两条命令的结果，使后续明确重定向仍受同一上限约束。连接中断后的写入结果由调用方处理。 [Redis Cluster 重定向规范](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/#moved-redirection)
- 在隔离基线 `f2e8d6248` 使用 Redis 8.10.3 验证二进制键的 HSET/HGET、同槽 MSET、跨槽部分成功、受限 ACL、丢失回执时 INCR 只执行一次、主节点切换。实际分批迁移标签数据发生 `TRYAGAIN` 时保留原值，键迁齐后随 ASK、MOVED 读回，再按标签清除。单机 Redis 回归通过。
- 双进程 HTTP 执行原始 HSET/HGET 并跨越 ASK 和 MOVED，独立 Artisan `cache:forget` 清除该键，`artisan inspire` 正常退出。改动文件格式、严格 Clippy、三语站点、12 项通用浏览器检查及 6 项 Redis 重定向文档检查通过。全仓格式仍报告既有的 `sanctum_authentication.rs:1412`。
- 临时 Redis 与本批隔离产物已回收。项目卷可用 115.27 GB（107.36 GiB），周额度剩余 67%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-raw.HeNbotf2`。真实 TLS 和首次响应无主机名的重定向待补，核心闭合数本批保持原值。

## 03-09 整单元验收

- 对照固定 Laravel 的 PhpRedis 集群 TLS 配置以及 Redis 的空端点规则。旧版支持识别 `rediss://`，未找到原生集群空端点处理。Rust 保留实际响应节点的连接主机，收到空端点时只采用 Redis 指定的新端口。初次连接与重定向沿用相同 TLS 方式和证书校验。修复工作区同时启用两个 Rustls 后端导致首次握手无法选择默认后端的问题，沿用项目的 ring 方案并保留应用已安装的后端。 [固定 Laravel 集群连接](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Redis/Connectors/PhpRedisConnector.php#L46) · [Redis 空端点规则](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/#moved-redirection)
- 在隔离基线 `2f43411d1` 上使用 Redis 8.10.3 启动三主节点 TLS 集群。实际连接验收通过可信 CA、拒绝无关 CA、不可达种子后发现健康节点、原始命令的空主机名 `ASK` 和 `MOVED`。使用真实双进程 HTTP 重现两种响应，独立 Artisan `cache:forget` 删除对应键，`artisan inspire` 通过。
- 复跑单机和明文集群夹具，覆盖具名连接、缓存和锁、标签及过期索引、迁槽、故障切换、丢回执后的写入边界。改动文件格式、严格工作区 Clippy、三语站点、12 项通用浏览器和 6 项 TLS 文档检查通过。全仓格式仍报告既有的 `sanctum_authentication.rs:1412`。
- 项目计划的 Redis 连接、序列化、原始命令、缓存与并发锁已有真实驱动及 HTTP／Artisan 证据，03-09 闭合。TLS 验收覆盖服务端凭证与 CA，使用 Redis ACL 密码认证。临时 Redis、测试证书和隔离编译产物已回收。磁盘可用 115.25 GB（107.33 GiB），周额度剩余 66%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-tls.yIUEEKY5`。

## Redis Cluster 标签主线复验

- 以本地 `f09174548` 的隔离快照使用 Rust 1.94.0 和 Redis 8.10.3 复验。标签实现已随 `f2e8d6248` 提交，整单元已随 `fb2a0bf39` 闭合。再次对照固定 Laravel 的标签索引、集群分批清理和 `cache:prune-stale-tags`，保留单槽 Lua 原子更新。 [Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- `sh tests/redis_cluster.sh` 的 4 项测试通过，覆盖过期索引修剪、按标签分批清理、隔离、迁槽和故障切换。真实单机 Redis 的连接与标签清理用例复验通过。
- 实际双进程 HTTP 验证写入、跨进程读取和自然过期。独立 Artisan `cache:prune-stale-tags redis` 移除过期索引引用并保留有效缓存，`cache:clear redis --tags=artists` 保留普通值和其他标签值。`./artisan inspire` 通过。
- 工作区严格 Clippy、三语站点构建、12 项通用浏览器检查和 6 项标签文档链路检查通过。全仓格式检查仍只报告既有的 `sanctum_authentication.rs:1412` 差异。既有三语文档覆盖本次验证的命令和行为。临时 Redis 与本批隔离产物已回收，日志保留于 `/private/tmp/openrust-03-09-recheck.yJc324Uv`。

## 标签清空中断后的过期索引清理

- 对照固定 Laravel 的有序集合、集群分批清空和 `cache:prune-stale-tags {store?}`。旧版 `v0.x` 的 `redis_tags.rs` 用 Lua 回收清空中断留下的旧代际。本批复用当前单槽 Lua，在移除空索引的同时解除该代际的待回收记录。索引中仍有值时，过期清理保留该记录，后续标签清空仍可继续。 [Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php) · [Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- 在 `8d7a31a6d` 的隔离快照上，新增回归先在原实现中复现待回收记录残留。修复后 `sh tests/redis_cluster.sh` 在 Redis 8.10.3 通过全部 4 项集群回归。单个索引的 320 个引用分批扫描后，仅保留 160 个有效值。另验证清空中断后的旧值和新代际隔离，以及单机 Redis 的连接、缓存、锁与标签清理。
- 实际双进程 HTTP 验证共享缓存、自然过期和新代际写入。独立 Artisan 验证 `cache:clear redis --tags=artists` 的失败与成功路径，验证 `cache:prune-stale-tags` 的默认存储、显式存储、重复运行和旧代际回收。检查 Redis 物理键确认旧索引和待回收记录均已删除。根应用 `artisan inspire` 通过。
- Rust 1.94.0 改动文件格式、工作区严格 Clippy、三语站点构建和 18 项浏览器检查通过。全仓格式仍只报告既有的 `sanctum_authentication.rs:1412` 差异。本批验收日志保留于 `/private/tmp/openrust-03-09-prune.QAkmUtvQ`。

## 显式删除后的空标签索引回收

- 对照固定 Laravel 的有序集合、集群分批清空与 `cache:prune-stale-tags {store?}`。OpenRust 的集群标签将每个存储的索引登记在同槽集合中，供过期清理分批遍历。`forget` 或 `pull` 删除最后一个成员时，在同一个 Lua 操作中移除空索引的登记，同一标签写入新值时重新登记。核对 `v0.x` 的 `redis_tags.rs` 和 `tests/unit/cache/tag_identity.rs`，沿用空索引回收思路与标签组合隔离回归。 [Laravel Cache Tags](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-tags) · [Laravel 集群标签清空](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L210)
- 在 `e46343b50` 的隔离快照上，用 Redis 8.10.3 复现空有序集合已消失、登记仍在。修复后 `sh tests/redis_cluster.sh` 的四项回归与单机 Redis 回归通过。覆盖 `forget`、`pull`、重新登记、多标签共享索引及 320 条引用的分批清理。
- 实际双进程 HTTP 验证删除、取回与重新登记，并用 Redis `SISMEMBER` 核对登记。独立 Artisan 验证默认和显式存储的 `cache:prune-stale-tags`、重复清理、清空中断时保留仍有值的旧代际，以及陈旧代际最终回收。根应用 `artisan inspire` 通过。
- Rust 1.94.0 改动文件格式、工作区严格 Clippy、三语站点构建和 18 项浏览器检查通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。临时 Redis 和隔离编译产物已回收，日志保留于 `/private/tmp/openrust-03-09-empty-index.OJOms75f`。

## 集群标签索引的无效引用清理

- 对照固定 Laravel 的 `RedisStore::flushStaleTags`、`RedisTagSet::flushStaleEntries` 和 `RedisStoreTest`。Laravel 按过期分数移除有序集合成员。OpenRust 在同一个 Lua 操作中核验缓存值是否存在，以兼顾过期、显式删除和其他标签清空留下的引用。继续沿用旧版 `b62ae7c72` 的分批清理和按代际隔离思路。 [Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L88) · [Laravel RedisStoreTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/RedisStoreTest.php#L223)
- 在隔离基线 `791788113` 的 Redis 8.10.3 上复现索引含无效引用时修剪中断。Redis 成员按原始字节处理。当前存储内的合法值键继续作为 Lua 的 `KEYS` 传入，无效成员仅作为 `ARGV` 传入并从索引移除。跨槽或跨存储成员不会作为 Redis 键访问。旧代际仍有有效值时，修剪保留其待回收记录。
- `sh tests/redis_cluster.sh` 复跑四项通过。标签回归验证分批清理 322 条引用后保留 160 条有效引用，覆盖非法 UTF-8、其他存储引用和中断后的旧代际。首次集群拓扑测试在主节点切换后收到瞬时 `CLUSTERDOWN`，新夹具复跑通过。真实单机 Redis 回归通过。
- 实际双进程 HTTP 验证标签值共享、自然过期和删除。独立 Artisan 验证默认与显式存储修剪、按标签清空的失败与恢复、重复修剪，以及其他值保留。修剪后用 Redis 原生命令核对无效引用已移除。根应用 `./artisan inspire` 通过。
- Rust 1.94.0 的改动文件格式、工作区严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412` 差异。本批 Redis 夹具、5.6 GiB 隔离 Cargo 产物和文档缓存已回收。日志保留于 `/private/tmp/openrust-03-09-prune-invalid.EsOuxYdC`。

## 集群标签索引登记容错

- 固定 Laravel v13.40.0 在 `RedisStore::currentTags` 按存储前缀筛选标签索引，再由 `RedisTagSet::flushStaleEntries` 分批移除过期引用。OpenRust 为同槽 Lua 更新维护登记集合，本批在遍历该集合时保留 Laravel 的存储边界。已核对旧版 `redis_tags.rs` 和 `tests/unit/cache/tag_identity.rs`，沿用分批清理与命名空间隔离思路。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L326) · [固定 Laravel RedisStoreTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/RedisStoreTest.php#L223)
- 在基线 `07307c6e1` 的真实 Redis 8.10.3 集群中，非 UTF-8 登记项使修剪提前退出。修复后按字节读取登记项，校验索引前缀、标签摘要和代际标识。无效成员只作为 `SREM` 的数据参数批量移除。原先声明的 Redis 键和同槽更新方式保持原有契约。
- `sh tests/redis_cluster.sh` 四项通过。新增断言验证错误格式、非 UTF-8 和其他存储的登记项被移除，合法索引的 322 条引用清理后保留 160 条，其他存储的索引和值仍可用。同版单机 Redis 的连接、缓存、锁和标签清理回归通过。
- 实际双进程 HTTP 验证标签读写、跨进程共享、自然过期、`forget` 和 `pull`。独立 Artisan 验证 `cache:prune-stale-tags` 默认及显式存储、重复清理和标签清空后的代际隔离。用 Redis 原生命令复核无效登记被移除，实际外部键和有效索引仍在。根应用 `./artisan inspire` 通过。
- Rust 1.94.0 的改动文件格式、工作区严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍只报告基线的 `sanctum_authentication.rs:1412` 差异。已回收临时 Redis、4.7 GiB 隔离 Cargo 产物和文档缓存。验收日志保留于 `/private/tmp/openrust-03-09-registry`。

## 按标签清空时清理无效引用

- 固定 Laravel 的 `RedisTaggedCache::flushClusteredConnection` 分批删除指定标签的值，`RedisTagSet::flushStaleEntries` 单独清理过期索引。对照旧版 `b62ae7c72` 的 `redis_tags.rs` 和 `tag_identity.rs`，沿用分批处理、代际隔离与成员校验。OpenRust 集群的 Lua 继续把当前存储的有效值声明为同槽键。格式错误及跨存储引用作为参数从索引移除，其指向的键不被访问。 [Laravel RedisTaggedCache](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L210) · [Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L88)
- 在 `5cbbce621` 隔离快照的 Redis 8.10.3 集群上复现无效引用导致清空返回 `InvalidValue`。修复后 `sh tests/redis_cluster.sh` 四项和真实单机回归通过，覆盖跨批次删除、非 UTF-8 成员、普通键、跨槽的其他存储和错误索引类型后的恢复。原生 Redis 命令确认缓存值、索引登记和待回收记录的实际状态。
- 双进程 HTTP 验证跨进程读取、自然到期及新代际写入。独立 Artisan 验证 `cache:clear redis --tags=artists` 清空成功、Redis 索引类型错误返回 1，以及 `cache:prune-stale-tags [redis]` 默认与显式存储、失败恢复和重复执行。修剪保留仍有有效值的旧代际，在索引清空后移除待回收记录。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式与严格 Clippy、三语站点和 18 项浏览器检查通过。已回收临时 Redis、4.0 GiB 隔离 Cargo 产物和文档缓存。日志保留于 `/private/tmp/openrust-03-09-drain.JSagLq7c`。
- 现有 `cache:prune-stale-tags` 是存储级清理。固定 Laravel 的 `Cache::tags([...])->flushStale()` 是按指定标签清理索引的公开入口，另待对齐。 [Laravel RedisStoreTest](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/RedisStoreTest.php#L223)

## 按指定标签清理过期引用

- 对照固定 Laravel 的 `RedisTaggedCache::flushStale`、`RedisTagSet::flushStaleEntries` 和 `RedisStoreTest`。新增 `Cache::tags(&["daily"])?.flushStale().await?`，成功时返回 `true`。复核旧版 `b62ae7c72` 的 `redis_tags.rs`，保留分批清理和代际隔离。Laravel 按过期分数清理，OpenRust 的 Lua 核验值键是否仍存在，也能移除显式删除留下的引用。所选标签只扫描当前代际，存储级 Artisan 继续回收旧代际。 [固定 Laravel 公开入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L264) · [固定 Laravel 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/RedisStoreTest.php#L223)
- 空标签列表直接成功，未知标签不创建代际。为共享的 Rust `TaggedCache` 返回明确的非 Redis 驱动错误。固定 Laravel 的 memo 标签装饰器转发是单独的 03-06 配套，直接选用 Redis 仓储的入口已接通。 [固定 Laravel memo 标签测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/MemoizedTaggedCacheTest.php#L225)
- 在 `9a0013a4f` 隔离快照上，Redis 8.10.3 集群四项、单机和 Array 回归通过。双进程 HTTP 调用三语文档中的 `flushStale()`，同一到期值在 `artists` 索引中被移除，在 `daily` 索引中保留到独立 Artisan 执行 `cache:prune-stale-tags redis`。原生 `ZCARD` 核对索引，HTTP 验证有效值仍能读取。实际 Artisan 还通过默认存储、重复清理和按标签清空失败恢复回归。Rust 1.94.0 全仓格式、严格 Clippy、三语站点和 18 项浏览器检查通过。证据目录 `/private/tmp/openrust-03-09-flush-stale.73uNk3Jx`。
