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

## 集群标签待回收集合容错

- 对照固定 Laravel v13.40.0 的 `RedisTaggedCache::flushClusteredConnection`、`RedisTagSet::flushStaleEntries` 和 Artisan `cache:prune-stale-tags {store?}`。复核旧版 `v0.x` 的 `redis_tags.rs` 与标签身份测试，沿用分批回收和按代际隔离的思路。OpenRust 的待回收集合用于追踪跨命令的清空进度。 [固定 Laravel 标签清空](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L210) · [固定 Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- 在隔离基线 `9a8520eb4` 的 Redis 8.10.3 集群上，新增回归复现无效 UTF-8 代际使清空失败。同槽 Lua 现在按原始字节移除格式错误的集合项，只返回有效代际供后续分批回收。待回收键类型不符时，脚本在更换当前代际前返回错误，修复键类型后可以重试。
- `sh tests/redis_cluster.sh` 四项通过，单机 Redis 标签回归通过。用实际双进程 HTTP 验证跨进程写读、自然过期及指定标签 `flushStale()`。独立 Artisan 验证 `cache:prune-stale-tags [redis]`、已空退役索引回收、有效退役值保留及重复执行，并用 `cache:clear redis --tags=artists` 验证无效代际和错误键类型的不同处理。根应用二进制的 `artisan inspire` 通过。
- Rust 1.94.0 全仓格式与严格 Clippy、三语站点、12 项通用和 6 项缓存文档浏览器检查通过。已回收临时 Redis 和本批文档产物，保留复用的隔离 Cargo 目标目录。项目磁盘可用 66.30 GB（61.74 GiB），周额度剩余 51%。日志保留于 `/private/tmp/openrust-03-09-retirement.cxlQvfd7`。

## Redis Cluster 标签与过期清理复验

- 标签与清理实现已经随 `f2e8d6248`、`36077918f` 和后续容错修复提交。本批基于 `994f564b7` 隔离复验。固定 Laravel 的 `RedisTaggedCache` 分批清空标签值，`RedisTagSet` 按过期分数修剪引用。OpenRust 延续旧版 `v0.x` 中 `redis_tags.rs` 的分批和代际机制，用同槽 Lua 核对值键是否仍存在，同时回收过期及已删除值的引用。 [固定 Laravel Cache Tags](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-tags) · [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L88)
- 真实 Redis 8.10.3 三主节点双进程 HTTP 验证跨进程读写、TTL、选定标签 `flushStale()` 隔离、`forget` 与 `pull` 后的空索引登记回收。独立 Artisan 验证默认及指定存储的 `cache:prune-stale-tags`、重复运行、无效引用清理与有效值保留。按标签清空遇到错误索引类型返回 1，恢复有序集合后重跑同一命令，确认旧值、索引与待回收记录同时移除。单机 Redis 标签回归与根应用 `./artisan inspire` 通过。
- 首轮完整集群回归在副本刚升主后出现一次 `CLUSTERDOWN`，原测试原样复跑通过。故障切换用例现在同时等待 `ROLE` 为主节点和 `CLUSTER INFO` 的 `cluster_state:ok`，再验证读恢复和单次写入，保留丢回执写入不重放的断言。
- 更新测试后使用两个新建 Redis 夹具，各通过 `sh tests/redis_cluster.sh` 的五项回归。Rust 1.94.0 全仓格式与严格 Clippy、三语站点构建、12 项通用和 6 项缓存文档浏览器检查通过。已回收临时 Redis、4.1 GiB 隔离 Cargo 产物及文档缓存。日志保留于 `/private/tmp/openrust-03-09-head-recheck.2xYqGjo4`。

## Redis Cluster 槽迁移期间的过期标签清理

- 对照固定 Laravel v13.40.0 的 `RedisTaggedCache`、`RedisTagSet::flushStaleEntries`、`CacheRedisTaggedCacheTest` 与 `cache:prune-stale-tags {store?}`。上游按过期分数清理标签引用，OpenRust 沿用旧版 `b62ae7c72` 的分批与代际设计，用同槽 Lua 核对引用的值键是否存在。Redis 在同一批的键分布于两个节点时可拒绝多键操作。本批沿用现有错误传播和手动重试语义。 [固定 Laravel 标签清理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L85) · [固定 Laravel 命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php) · [Redis 多键操作规范](https://redis.io/docs/latest/operate/oss_and_stack/reference/cluster-spec/#multi-keys-operations)
- 隔离基线 `0c8e828d7` 的 `sh tests/redis_cluster.sh` 在真实 Redis 8.10.3 上五项通过。新增回归先迁移有效值，保留源节点的索引与到期引用。指定标签和整个储存的清理均返回 `TRYAGAIN`，失败批次保留两条引用。迁移完整个槽后清理只剩有效引用，随后按标签清空仍能移除有效值。真实单机 Redis 标签回归通过。
- 独立三主节点集群上运行两个 HTTP 进程及独立 Artisan。拆槽期间 `flushStale()` 的 HTTP 调用返回 500，`cache:prune-stale-tags redis` 退出码为 1，两个标签仍各有两条引用。完成槽迁移后，同一 HTTP 请求仅修剪指定标签，Artisan 再修剪另一标签。指定与默认存储的命令都以 0 退出，重复清理成功，跨进程读取有效值正常。最后执行 `cache:clear redis --tags=artists`，旧值从 Redis 删除，普通值与其他标签保留。根应用的 `./artisan inspire` 通过。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建和 18 项浏览器检查通过。系统默认工具链运行的 `./artisan inspire` 有 macOS 链接器 unwind 表大小警告，命令仍正常退出。Rust 1.94.0 构建的二进制直接运行 `artisan inspire` 也通过。临时 Redis、本批 6.3 GiB 隔离 Cargo 产物和文档缓存已回收。项目磁盘可用 61.06 GB（56.86 GiB），周额度剩余 48%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-09-tag-migration.nswsel7B`。

## Redis Cluster 标签登记丢失后的恢复

- 固定 Laravel v13.40.0 的 `RedisStore::currentTags` 通过连接与存储前缀扫描索引，再由 `flushStaleTags` 分批修剪。复核旧版 `6d1a81924` 的 `redis_tags.rs` 与 `tag_identity.rs`，沿用分批清理和前缀隔离。OpenRust 的存储级清理在标签槽主节点执行有序集合的前缀扫描，补录仍存在的合法索引，再用原有同槽 Lua 清理。即使登记集合已经被后续写入部分重建，也能找回漏登索引。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L326)
- 在隔离基线 `598451f45` 上，用真实 Redis 8.10.3 原生命令删除登记集合模拟淘汰，再通过正常写入部分重建。在基线中，当前仍有 160 个有效值的索引保留了全部 320 条引用。修复后仅保留 160 条有效引用，空索引及其登记被回收，另一个前缀的索引和值仍可使用。`sh tests/redis_cluster.sh` 五项通过，真实单机 Redis 标签回归通过。
- 双进程 HTTP 验证自然过期和跨进程读写。独立 Artisan 验证 `cache:prune-stale-tags redis` 找回两个漏登索引、剔除过期引用并保留有效值，默认存储清理可重复执行。另复验槽迁移期间 HTTP 返回 500、Artisan 返回 1，迁移完成后两者重试成功。根应用 `./artisan inspire` 通过。
- Rust 1.94.0 工作区格式、严格 Clippy、三语站点构建、12 项通用和 6 项缓存文档浏览器检查通过。已回收临时 Redis、4.1 GiB 本批隔离 Cargo 产物及文档缓存。证据目录 `/private/tmp/openrust-03-09-registry-recovery.RkCPiCmn`。

## Redis Cluster 索引丢失后的空代际回收

- 固定 Laravel v13.40.0 通过 `RedisStore::currentTags` 查找标签索引、分批调用 `RedisTagSet::flushStaleEntries`，并通过 `cache:prune-stale-tags {store?}` 提供存储级清理。复核 `v0.x` 的 `redis_tags.rs` 和 `tag_identity.rs`，沿用分批回收与代际隔离。OpenRust 的集群实现需要为中断的清空保留旧代际。存储级清理额外在标签槽主节点查找本存储的待回收集合，分批检查代际，在同槽 Lua 中确认索引为空后移除该记录。仍有引用的旧代际留给后续按标签清空。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L326) · [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L88) · [固定 Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- 隔离基线 `448dfc7b2` 的 Redis 8.10.3 回归先复现空代际记录残留。修复后 `sh tests/redis_cluster.sh` 五项及真实单机标签回归通过。
- 使用真实三主节点和两个独立 HTTP 进程。通过 `cache:clear redis --tags=artists` 触发中断，再用 Redis `DEL` 模拟索引和登记键淘汰。独立 Artisan `cache:prune-stale-tags redis` 回收空记录，保留仍有索引的旧代际和当前值。HTTP 验证跨进程读取新代际，省略存储参数再次清理成功。同轮复验自然过期、槽迁移时错误传播及完成迁移后重试成功。根应用 `./artisan inspire` 通过。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建和 18 项浏览器检查通过。核心账本维持 62/66。日志与临时验收脚本保留于 `/private/tmp/openrust-03-09-retired-index.oj0jplyG`。 临时 Redis 和本批隔离编译及文档产物已回收，项目卷可用空间增加 14.41 GB（13.42 GiB），剩余 104.33 GB（97.16 GiB）。周额度剩余 45%，第二窗口无读数。

## Redis Cluster 过期清理回收无效代际标识

- 固定 Laravel v13.40.0 按存储扫描标签索引，通过 `RedisTagSet::flushStaleEntries` 分批清除到期引用。旧版 `v0.x` 的 `redis_tags.rs` 用分批处理回收待清空的旧代际。OpenRust 延续现有单槽代际机制，在存储级清理时分批移除待回收集合中格式无效的原始字节。合法代际仍由同槽 Lua 核对索引，有引用时保留给后续按标签清空。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L300) · [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L79) · [固定 Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- 隔离基线 `ce87ff6e1` 在 Redis 8.10.3 集群上复现。清理已删除无索引的合法代际，待回收集合仍残留格式错误的文本与非 UTF-8 项。修复后 `sh tests/redis_cluster.sh` 五项与真实单机 Redis 标签回归通过，修剪后仅保留仍有值的合法代际。
- 真实三主节点和两个 HTTP 进程复验跨进程读写、自然过期、指定标签修剪、槽迁移报错和迁移完成后的重试。独立 Artisan `cache:prune-stale-tags redis` 移除无效标识与空记录，有索引的旧代际和当前缓存仍可用，省略存储参数重复执行通过。用 Redis 原生命令核对集合、索引和值键。
- 根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、工作区严格 Clippy、三语站点和 18 项浏览器检查通过。临时 Redis 及本批隔离编译和文档产物已回收，卷可用空间净增 2.34 GB（2.18 GiB），剩余 102.53 GB（95.49 GiB）。周额度剩余 44%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-03-09-prune-pending.IweEqab5`。

## Redis Cluster 分批回收旧标签代际

- 固定 Laravel v13.40.0 的 Redis Cluster 标签清空分批删除条目，存储级过期清理扫描标签引用。对照 `v0.x` 的 `redis_tags.rs` 和 `tag_identity.rs`，沿用 `b62ae7c72` 的分批及代际隔离思路。OpenRust 在同槽 Lua 内原子切换代际并登记旧代际，随后通过 `SSCAN` 分批回收登记，按原有 Lua 清理旧值、索引及无效标识。标签过期清理复用分批遍历，只移除没有引用的待回收记录。 [固定 Laravel 集群标签清空](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php#L216) · [固定 Laravel 标签过期清理](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L323)
- 在 `8600153b3` 的隔离快照使用真实 Redis 8.10.3。集群五项和单机标签回归通过，覆盖清空 320 个空代际、仍有值的旧代际及格式无效的二进制和文本标识。
- 真实三主节点、双进程 HTTP 与独立 Artisan 复验标签共享、自然过期、槽迁移报错与完成迁移后重试。Redis 命令统计确认 `cache:clear redis --tags=daily` 扫描了多页待回收记录，`SMEMBERS` 调用数未增加。物理旧键及待回收索引已删除，普通缓存和无关标签依然可读。重复清空、显式和默认存储的 `cache:prune-stale-tags` 及根应用 `./artisan inspire` 通过。
- Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建和两种部署路径下的 12 项通用及 6 项标签浏览器检查通过。核心账本维持 62/66。临时 Redis、本批隔离 Cargo 和文档产物已回收，项目卷可用空间净增 2.44 GB（2.27 GiB），剩余 102.16 GB（95.14 GiB）。提交前周额度剩余 42%，第二窗口无读数。日志保留于 `/private/tmp/openrust-03-09-batched-clear.cijV8Z4H`。

## Redis Cluster 存储级清理分批回收空代际

- 固定 Laravel v13.40.0 通过存储前缀查找标签索引，并按过期分数移除引用。OpenRust 在同槽内核对值是否存在，并维护待回收代际以恢复中断的标签清空。核对旧版 `v0.x` 的 `redis_tags.rs` 与 `tag_identity.rs`，沿用 `b62ae7c72` 限制每批处理量和隔离代际的思路。存储级清理每次 Lua 最多检查 128 个旧代际索引，确认无引用后回收登记及待回收记录，有效引用及值继续保留。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L323) · [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84)
- 在隔离基线 `75620ba1c` 使用真实 Redis 8.10.3 三主节点、双进程 HTTP 和独立 Artisan。由 Redis 原生 `INFO commandstats` 记录，同一组含 320 个空代际及仍有引用的索引，`cache:prune-stale-tags redis` 的 EVAL 调用从 326 次降至 8 次。复验自然过期、指定标签的 `flushStale()`、默认与指定存储、无效二进制及文本标识清理、槽迁移错误传播与迁移后的重试。错误类型索引使 Artisan 退出 1，同批待回收记录保留，修复后重试成功。后续按标签清空仍可删除旧值，普通及无关标签的值仍可通过 HTTP 读取。
- `sh tests/redis_cluster.sh` 五项、真实单机标签回归、根应用 `./artisan inspire`、Rust 1.94.0 全仓格式、严格 Clippy、三语站点构建及两种部署路径下 12 项通用和 6 项标签文档浏览器检查通过。核心账本维持 62/66。临时 Redis 与本批隔离 Cargo 和文档产物已回收，项目卷可用空间净增 14.47 GB（13.48 GiB），剩余 101.89 GB（94.89 GiB）。提交前周额度剩余 41%，第二窗口无读数。日志保留于 `/private/tmp/openrust-03-09-prune-batches.KfrTPpQv`。

## Redis 标签按到期分数清理

- 对照固定 Laravel v13.40.0 的 `RedisTagSet::flushStaleEntries`，按有序集合的到期分数清理引用，永久引用保留其负数标记。沿用 `openrust-v0.x` 的 Redis 服务器毫秒时间与到期分数。OpenRust 的集群清理继续按同槽 Lua 批次执行，并在删除前重新读取索引分数，以保留并发续期后的引用；同时清理已不存在的值键引用。修剪只移除索引引用。 [固定 Laravel 标签索引](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L85) · [固定 Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php)
- 在 `adfd36e75` 的隔离快照用真实 Redis 8.10.3 三主节点和两个 HTTP 进程调用 `flushStale()`，再通过独立 Artisan 调用 `cache:prune-stale-tags redis` 和默认存储形式。以 Redis 原生命令将仍可读取的永久值索引分数设为 0，验证指定标签只删除所选索引，整库清理删除剩余引用，两进程仍能读到值。复验自然过期、槽迁移中退出码 1 和迁移后重试、登记丢失恢复、待回收代际分批清理以及无关缓存保留。
- `sh tests/redis_cluster.sh` 五项、单机 `redis_tags_physically_flush_values_and_prune_stale_indexes` 回归、根应用 `./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy 通过。默认 Debug 构建出现 macOS 链接器的 unwind 表警告，Artisan 正常退出。三语站点及两种部署路径下 12 项通用和 6 项标签浏览器检查通过。核心账本维持 63/66。临时 Redis 与本批隔离 Cargo 和站点产物已回收，项目卷可用空间净增 15.15 GB（14.11 GiB），剩余 96.79 GB（90.15 GiB）。周额度剩余 39%，第二窗口无读数。日志保留于 `/private/tmp/openrust-03-09-score.8wDRD7O2`。

## 过期标签清理的存储选择与主线复验

- 以 `8f1ff4a85` 的干净归档复验。固定 Laravel v13.40.0 先解析指定或默认存储，只在存储提供清理能力时调用清理并报告成功。OpenRust 通过 `Store::flush_stale_tags` 的默认空实现表达同一契约。复核 `openrust-v0.x` 的 `redis_tags.rs` 和 `tag_identity.rs`，当前实现在 Redis 服务器计时、批量处理和代际隔离上继续沿用其思路。 [固定 Laravel 清理命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/PruneStaleTagsCommand.php#L29) · [固定 Laravel 过期索引](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84)
- Redis 8.10.3 三主节点和两个实际 HTTP 进程验证指定标签与整库清理、过期索引删除但保留其仍有效的值、槽迁移中失败和迁移后重试、登记恢复与旧代际回收。独立 Artisan 指定 `array` 退出 0，指定不存在的存储退出 1，两者都保留默认 Redis 的索引。单机 Redis 回归新增显式 `array` 的断言，随后显式 `redis` 确实清理过期引用。
- `sh tests/redis_cluster.sh` 五项、真实单机标签回归、根应用 `./artisan inspire`、Rust 1.94.0 全仓格式及严格 Clippy、三语站点构建、12 项通用和 6 项标签文档浏览器检查通过。核心账本仍为 66/66。另一次从同一基线运行的整仓测试在 Provider 并发生成、stub 数量和事件假冒的回滚断言出现三项失败，交接继续跟踪。日志保留于 `/private/tmp/openrust-integration-current.Bs3P2TXn`。
- 已回收临时 Redis、隔离 Cargo 和文档产物。项目卷可用空间净增 33.67 GB（31.35 GiB），清理后可用 100.57 GB（93.66 GiB）。提交前周额度剩余 38%，第二窗口无读数。

## 索引登记丢失后的 Redis 键类型错误

- 固定 Laravel v13.40.0 的 `RedisStore::currentTags()` 按存储键名扫描，`RedisTagSet::flushStaleEntries()` 直接执行有序集合命令，Artisan 在清理成功后才打印成功信息。对照上游键前缀扫描测试和 `openrust-v0.x` 的 `redis_tags.rs`、`tag_identity.rs`。OpenRust 继续按标签槽、存储前缀及代际键格式限定范围，去掉 Redis SCAN 的类型筛选，让错型索引与待回收键的操作错误返回给调用方。 [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L329) · [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84) · [固定上游测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Cache/CacheRedisStoreTest.php#L196)
- 基于 `7d5a98fc0` 的隔离快照，Redis 8.10.3 三主节点、两个真实 HTTP 进程及独立 Artisan 验证索引登记丢失且键类型异常时 HTTP 返回 500、`cache:prune-stale-tags redis` 退出 1。恢复索引后指定标签清理成功，仍可读取原值。异常类型的待回收键也使默认存储命令退出 1，修复后退出 0。Redis 原生命令验证异常键、备份索引和无关键在对应失败步骤保留。
- `sh tests/redis_cluster.sh` 五项、真实单机标签回归、`./artisan inspire`、Rust 1.94.0 全仓格式和严格 Clippy、三语站点构建、12 项通用和 6 项标签说明浏览器检查通过。已回收临时 Redis、隔离 Cargo 和文档产物。提交前磁盘可用 99.82 GB（92.96 GiB），周额度剩余 36%，第二窗口无读数。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-03-09-index-types.3IoCa2Nv`。

## Redis Cluster 指定标签跨代际清理

- 固定 Laravel v13.40.0 的 `RedisTagSet::flushStaleEntries()` 按标签写入的有序集合及到期分数清理索引。复核旧版 `6d1a81924` 的 `redis_tags.rs` 和 `tag_identity.rs`，沿用 Redis 时钟、批次边界和代际隔离。OpenRust 的集群 `flushStale()` 按连接、存储和标签前缀分批发现现存代际索引，继续由同槽 Lua 校验到期分数及值键是否存在。扫描携带同槽路由键，槽转移完成后可收到 Redis 重定向。 [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84) · [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L304)
- 在隔离基线 `4fd0f036c` 的 Redis 8.10.3 集群上先复现旧代际过期引用残留。中间版本的无键扫描也在槽迁移后首次 HTTP 请求复现漏清。最终回归覆盖代际键及登记丢失、同标签写入新代际、指定范围隔离、旧值保留和其它标签索引保留。
- 真实三主节点和两个 HTTP 进程验证自然过期、指定标签修剪、槽拆分时 HTTP 500、Artisan 退出 1、迁移后首次 HTTP 清理及重复执行。真实 `cache:prune-stale-tags redis` 和省略存储参数的命令均通过，保留普通缓存、其他标签和值键；复验索引错型报错和修复后重试。Redis 原生命令核对索引和物理键。根应用 `./artisan inspire` 通过。
- `sh tests/redis_cluster.sh` 五项、真实单机 Redis 标签一项、Rust 1.94.0 全仓格式和严格 Clippy、三语站点构建及两个部署路径下的 12 项通用和 6 项缓存说明浏览器检查通过。此批使用 Redis 8.10.3，未另跑旧 Redis 版本。核心账本维持 66/66。
- 临时 Redis 与本批隔离编译和文档产物已回收，清理前后项目卷可用空间净增 4.25 GB（3.96 GiB），清理后可用 104.69 GB（97.50 GiB）。提交前周额度剩余 33%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-03-09-scoped.6lN6aK0j`。

## Redis Cluster 指定标签的空代际回收

- 固定 Laravel v13.40.0 的 `RedisTagSet::flushStaleEntries()` 按所选标签清理有序集合中的过期引用。OpenRust 沿用旧版 `v0.x` 的分批代际处理。在 Redis Cluster 上，`flushStale()` 清理所选标签的现存索引后，复用存储级清理的同槽批处理，移除该标签的空代际及无效代际标识。旧代际仍有有效引用时保留，后续 `flush()` 可继续处理。 [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84) · [固定 Laravel RedisTaggedCache](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php)
- 基于 `9f374da0b` 的隔离快照，用真实 Redis 回归先复现仅保留一项的预期与实际 323 项的差异。修复后回归覆盖 320 个无索引代际、无效文本与二进制标识、有引用的旧代际、重复清理及其他标签隔离。独立 HTTP 进程写入和读取缓存，按标签的 HTTP 清理只回收自身记录。独立 Artisan `cache:prune-stale-tags redis` 再回收其他标签的空记录。两轮 HTTP 与 Artisan 复验保留值键和其他标签正常读取，槽迁移继续通过。所选标签无索引时，错型待回收集合仍使 HTTP 返回 500，修复后重试成功。
- `sh tests/redis_cluster.sh`、真实单机缓存测试、`./artisan inspire`、Rust 1.94.0 `fmt --all --check`、全工作区全目标严格 Clippy、三语构建及根路径和 `/docs/` 下 12 项通用与 6 项缓存正文浏览器检查通过。本批使用 Redis 8.10.3，其他 Redis 版本不在本次验收范围。macOS 默认工具链链接 `inspire` 时报告调试产物 compact unwind 容量警告，命令退出 0。核心账本维持 66/66。
- 已回收本批临时 Redis 与隔离编译和文档产物，清理期间项目卷可用空间净增 20.91 GB（19.47 GiB），清理后可用 86.64 GB（80.69 GiB）。提交前周额度剩余 30%，第二窗口无读数。验收日志保留于 `/private/tmp/openrust-03-09-selected-retire.dYkXDi2e`。

## Redis Cluster 指定标签的悬空索引登记回收

- 固定 Laravel v13.40.0 的 `RedisTagSet::flushStaleEntries()` 按所选标签修剪过期索引，`RedisStore::flushStaleTags()` 调用各标签的清理。核对旧版 `6d1a81924` 的 `redis_tags.rs` 和 `tag_identity.rs`，复用当前实现已有的分批索引与代际隔离。OpenRust 另维护同槽索引登记。指定标签清理再分批扫描本存储登记，Lua 在检查索引仍缺失时才移除登记，避免删除并发重建的有效登记。保留其他标签和物理缓存值。 [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84) · [固定 Laravel RedisStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisStore.php#L304)
- 在隔离基线 `9beeb64b0` 先复现指定标签清理后仍有悬空登记。修复后用 Redis 8.10.3 测试 320 条无索引登记、有效登记和其他标签隔离。两个真实 HTTP 进程验证索引丢失且无待回收记录时，只移除所选标签的登记，重复调用成功。独立 Artisan `cache:prune-stale-tags redis` 回收其他标签的悬空登记，保留仍然存活的索引和原有缓存值。复验槽迁移失败及重试、错型错误和显式与默认存储选择。
- `sh tests/redis_cluster.sh` 五项、真实单机 Redis 标签回归一项、`./artisan inspire`、Rust 1.94.0 全仓格式、全工作区全目标严格 Clippy、三语站点构建及 18 项浏览器检查通过。浏览器覆盖根路径和 `/docs/`。本批验收使用 Redis 8.10.3。核心账本保持 66/66。
- 本批临时 Redis、隔离编译产物和站点生成物已回收，数据卷清单一致。清理前后项目卷可用空间净增 6.29 GB，清理后可用 90.27 GB（84.07 GiB）。提交前周额度剩余 29%，第二窗口无读数。日志保留于 `/private/tmp/openrust-03-09-orphan-index.R8QvV5qd`。

## Redis Cluster 指定标签的异常索引登记清理

- 固定 Laravel v13.40.0 的 `RedisTagSet::flushStaleEntries()` 只修剪指定标签的过期引用。对照该方法和 RedisStore 集成测试，复核旧版 `b62ae7c72` 的 `redis_tags.rs` 及 `tag_identity.rs`，沿用分批清理与标签代际隔离。OpenRust 用同槽集合登记索引。指定标签清理按原始字节识别所选标签的摘要，分批从集合移除格式异常的登记，保留其指向的 Redis 键。无法识别归属的登记仍由存储级命令清理。 [固定 Laravel RedisTagSet](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTagSet.php#L84) · [上游 RedisStore 测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Integration/Cache/RedisStoreTest.php#L223)
- 在隔离基线 `6e5e9cfd7` 用 Redis 8.10.3 复现异常登记残留。修复后 `sh tests/redis_cluster.sh` 五项及单机 `redis_tags_physically_flush_values_and_prune_stale_indexes` 回归通过。两个真实 HTTP 进程验证文本与非 UTF-8 异常登记的清理范围和重复执行。独立 Artisan `cache:prune-stale-tags redis` 清除其他标签及归属不明的残留登记，Redis 原生命令确认存活索引、缓存值和被异常登记引用的实际键均保留。槽迁移失败与重试、存储选择及 `./artisan inspire` 复验通过。
- Rust 1.94.0 全仓格式、全工作区全目标严格 Clippy、三语站点构建及两种部署路径下 12 项通用与 6 项缓存文档浏览器检查通过。临时 Redis、本批 Cargo 目标及文档生成物已回收，数据卷清单一致。清理期间项目卷可用空间变化 +4.27 GB（+3.98 GiB），清理后可用 86.85 GB（80.88 GiB）。提交前周额度剩余 27%，第二窗口无读数。核心账本维持 66/66。日志保留于 `/private/tmp/openrust-03-09-malformed-registration.6gBQsijW`。


## 日常缓存三驱动与三库矩阵

2026-11-21，在 `6d9f6f463e1ec80753c4c255ea4d1e02143b35ba` 的独占归档中覆盖本批 `tests/cache.rs`，使用 Rust 1.94.0、jobs=2、关闭增量与 dev debug。复用现有 Laravel 缓存契约和 `exercise`，补充 PostgreSQL database 与 Redis 真实后端入口，为 MySQL、PostgreSQL、Redis 添加必需服务标记，避免验收缺服务时静默成功。未增加运行时依赖或新缓存抽象。

- file、SQLite database、PostgreSQL 18.6 database、MySQL 8.4.13 database、Redis 8.10.3 各一项真实定向测试全部通过。共同覆盖跨应用读取、二进制、过期、并发 add／pull、计数器及锁竞争、owner 恢复／刷新／释放；数据库入口执行真实 Artisan 迁移与重置，文件入口复验独立 Artisan 清理及无关文件保留。
- PostgreSQL、MySQL、Redis 各自设置 `OPENRUST_REQUIRE_CACHE_*` 且缺少 URL 时，精确测试均退出 101，按预期拒绝缺少后端。PostgreSQL 使用 `OPENRUST_TEST_CACHE_POSTGRES_URL`，MySQL 使用 `OPENRUST_TEST_CACHE_MYSQL_URL`，Redis 沿用 `OPENRUST_TEST_FAILOVER_REDIS_URL`；这些会清理缓存的测试须通过专属隔离服务运行并精确筛选。
- `cargo +1.94.0 fmt --all --check` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 全部通过。仅新增验收入口，没有改变缓存产品接口，未重建文档站。本快照早于并行 Schema、SMS、settings 和依赖补丁，不将本结果算作这些改动的最终整合验收。

源码和日志保留于 `/private/tmp/openrust-core-matrix-20261121/`。临时 PostgreSQL、MySQL、Redis 服务已清理，Redis 清理前后数据卷清单一致，全部持久卷保留。独占 target 暂留给同一核心整合批次的依赖回归复用；阶段检查 Available 50,986,508 KiB（52.210 GB／48.625 GiB），高于 32 GB 停线，最终整合后记录实际清理。正式额度读取接口因当前独立 CLI 缺少账户鉴权而不可读；按最新真人解除人为额度停线授权继续，不将历史额度读数当作实时值。
