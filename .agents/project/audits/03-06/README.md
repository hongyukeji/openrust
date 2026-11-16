# 03-06 缓存、过期与原子锁

## 契约与实现

- 对照 Laravel v13.40.0 的 Repository、ArrayStore、FileStore、DatabaseStore、Lock 和缓存测试。参考固定 Goravel 的服务提供者，在每个应用容器中注册 CacheManager，门面在应用作用域内解析。 [Laravel Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md) · [Goravel CacheServiceProvider](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/cache/service_provider.go)
- 实现 array、file、database 的过期、原子增删和计数、锁获取、等待、恢复、续租与释放。内存存储归属应用。文件写入用进程锁和同目录原子重命名。数据库读写走写连接，原子更新用事务与唯一键。锁和普通条目分开存放。数据库锁无显式租期时使用 24 小时。锁释放与续租检查持有者，过期持有者无法操作后继锁。
- 从 openrust-v0.x 的 `6d1a8192` 提取毫秒时间戳、文件重命名、键命名空间及 `vendor/openrust/framework/tests/unit/cache/flush.rs` 的清空边界回归。缓存支持 JSON 和字节，数据库和磁盘使用 Rust 编码。数据库期限列用 BIGINT 保存毫秒值，键使用前缀与键的摘要。数据库计数器缺失返回类型化错误，对应 Laravel 的失败结果。
- 文件锁首次并发打开曾在 macOS 返回 ENOENT。改为先打开已有文件，缺失时独占创建，竞争者重开同一文件。

## 验收

隔离基线 `21a65e98`，macOS arm64，Rust 1.94.0。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline` 和 `cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。388 项测试通过。本批 4 项缓存测试包括真实 SQLite 与独立 MySQL 8.4.13，覆盖过期、竞争、递增、过期所有者、跨实例读取及 Artisan 清理。其他工作区数据库测试按各自的环境开关执行。
- 在两个独立的 MySQL 8.4.13 schema 启用真实迁移与清库测试。4 项迁移测试通过，含 SQLite、MySQL 迁移与 MySQL `migrate:fresh --drop-views --step`。
- 两个真实 OS 进程通过文件锁竞争、凭令牌恢复与释放。清空文件缓存保留目录内的普通文件、目录和符号链接。
- 三语 Rust 示例分别编译并经回环 TCP 发出 3 次请求。跨应用依次取得 404、204、200，命中正文为 `ready`。实际运行 `./artisan migrate`、`./artisan cache:forget reports/latest file`、`./artisan cache:clear file` 和 `./artisan inspire`，退出码均为 0。
- 文档 `npm run build` 成功。站点在 `/` 和 `/docs/` 通过 12 项浏览器组合及 6 项缓存页面专项检查。

## 后续边界

Redis 连接及缓存锁按 03-09 推进。标签、通用缓存事件、迁移生成器、回调持锁与其他存储驱动的完成情况按核心账本跟踪。Windows 文件锁实机未测。下一单元 03-07 使用已通过的 01-01 配置基础。

## 缓存故障切换

- 对照固定 Laravel 的 [FailoverStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/FailoverStore.php) 和 [Cache Failover](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-failover)。openrust-v0.x 的缓存实现没有 failover 组合驱动，复用当前存储契约、原子操作和应用事件分发。
- 依据配置顺序按次解析具名存储并尝试操作。命中、未命中和成功返回 `false` 都直接结束，所有存储报错时返回最后一个错误。再次操作时从主存储开始。拒绝空存储列表，循环依赖返回类型化错误。
- `CacheFailedOver` 记录新报错的存储。Rust 在异步缓存操作取得结果后分发，`exception` 为错误消息，原操作保留自己的返回值或错误。锁构造沿相同顺序选取可创建句柄的存储，获取、续期和释放使用该句柄。当前同步锁构造和恢复接口不会派发异步事件，`get_prefix` 返回配置前缀。标签、标签清理与批量清锁接口仍待闭合。
- 隔离基线 `1b98fc2c7`，macOS arm64，Rust 1.94.0。固定 PHP 对照通过故障切换、首次成功结果、主存储恢复、事件重发和最后一个异常。MySQL 8.4.13 与 Redis 8.10.3 下，`cargo test --test cache --locked --offline -- --test-threads=1` 的 6 项通过，含实际 Artisan `cache:clear failover` 和 `inspire`。另用真实 Redis 运行 2 项 `cache_memo` 回归，验证 HTTP 请求和队列任务生命周期。
- 三语共用 Rust 示例运行通过，VitePress 构建和 18 项浏览器检查通过。变更文件格式及工作区全目标严格 Clippy 通过。全仓格式检查只报告既有 `sanctum_authentication.rs:1412` 排版，已核对该文件与基线逐字节一致。
- 临时 MySQL、Redis 和 PHP 容器已回收。清理本批 3.5 GiB 隔离 Cargo 产物和文档缓存，项目卷可用 60.79 GB（56.61 GiB），周额度剩余 70%，第二窗口无读数。证据目录为 `/private/tmp/openrust-03-06-failover.3xRqxr8D`。

## 有序缓存标签

- 对照固定 Laravel 的 [Cache Tags](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#cache-tags)、`TagSet`、`TaggedCache`、`ArrayStore` 和 `FailoverStore`。从 openrust-v0.x 的 `cache/tags.rs` 与 `tests/unit/cache/tag_identity.rs` 复用有序代际身份和键边界回归。对照了旧版 `redis_tags.rs` 的索引与分批清理设计，该设计尚需独立验收。
- 为默认和具名 `array`、`failover` 接通 `tags` 与 `supportsTags`。标签顺序及重复次数参与身份，任一匹配标签更新代际即失效，普通键和只有无关标签的键保留。复用现有 Repository TTL 和原子后端操作。Rust 使用 UUID 代际及 SHA-256 键命名，旧代际的数据随原 TTL 到期或由整库清理移除。
- 隔离基线 `7d0afab4c`，macOS arm64，Rust 1.94.0。PHP 8.5 实际运行固定 Laravel ArrayStore 的顺序、重复项、键边界、清理及应用隔离对照。MySQL 8.4.13 与 Redis 8.10.3 下运行根应用 `cargo test --test cache --locked --offline -- --test-threads=1`，8 项通过，含 Redis 作为故障切换后端时的标签失效。
- 真实 HTTP 依次完成未命中、写入、读回、按标签清理和再次未命中，三语共用示例在同一路径执行。实际 `main artisan inspire` 和 `cache:clear failover` 通过。VitePress 构建及 18 项浏览器检查通过。
- 直接 Redis 标签、过期索引、物理清理、批量清锁和通用缓存事件继续由账本跟踪。当前 `failover` 的标签走 Laravel 的通用代际机制。
- 变更文件格式和工作区全目标严格 Clippy 通过。全仓格式检查只报告既有 `sanctum_authentication.rs:1412` 排版，该文件与基线逐字节一致。临时 PHP、MySQL 和 Redis 已回收，清理本批 2.7 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 60.79 GB（56.62 GiB），周额度剩余 70%。证据见 `/private/tmp/openrust-03-06-tags.2Kd3VwBG`。

## Redis 标签与索引清理

- 对照固定 Laravel 的 [RedisTaggedCache](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/RedisTaggedCache.php)、`RedisTagSet` 与 `cache:prune-stale-tags`。参考 openrust-v0.x 的 `cache/redis_tags.rs`，复用旧代际分批清理和索引验证思路，并与当前具名 Redis 连接、TTL 和二进制值编码整合。
- 直接 Redis 支持有序与重复标签、原子写入和索引更新、计数、更新期限、删除，以及任一标签触发的物理清理。Lua 在同一次操作内读取代际并操作值；清理先切换代际，按每批最多 128 个实际键回收旧代际，保留待处理记录供下一次清理。新代际使用独立键，Redis 全库清空或代际元数据缺失后会生成新的标识。
- `cache:prune-stale-tags [store]` 扫描直接 Redis 存储的标签索引并移除指向到期或已删除值的引用。仍有效的值、普通键和其他缓存前缀保留。该命令在非直接 Redis 存储上为空操作。Redis Cluster 由 03-09 跟踪。
- 基线 `359f28604`，Rust 1.94.0、macOS arm64。固定 Laravel 源码经 PHP 8.5、Predis 3.0.0 和独立 Redis 8.10.3，对照顺序、重复项、物理删除与普通键保留。OpenRust 在 MySQL 8.4.13 和 Redis 8.10.3 下通过 9 项缓存回归。直接 Redis 验证 133 个键的分批删除、错误索引中断后逻辑隔离和补清理、全库清空后的代际、过期索引、64 位计数，以及实际 HTTP 和 Artisan。
- 三语文档示例走实际 Redis HTTP，Artisan 实际运行 `cache:prune-stale-tags redis`。VitePress 构建及 18 项浏览器检查通过。批量清锁和通用缓存事件仍待闭合。
- 变更文件格式和工作区严格 Clippy 通过。全仓格式仅报告基线中已有的 `sanctum_authentication.rs:1412`。回收验收容器，清理本批 2.7 GiB Cargo 产物和文档缓存后，磁盘可用 60.74 GB（56.57 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-redis-tags.GfREnIkp`。

## 批量清锁与按标签清理命令

- 对照固定 Laravel [CanFlushLocks](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Cache/CanFlushLocks.php)、[ClearCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/ClearCommand.php)、Repository 和四种存储。openrust-v0.x 只有通用清理，沿用所选 Redis 库的 `FLUSHDB` 和文件清理边界，补上独立批量清锁能力。
- `Cache::flushLocks` 和具名 Repository 只清锁存储。数组锁和普通缓存独立，文件重用既有锁目录与文件门闩，并拒绝与条目目录解析到同一位置。MySQL 清空独立锁表，拒绝可能与条目表同名的配置。Redis 清空专用逻辑库。同编号不同地址时先用 `INFO SERVER` 比对实例标识，共享逻辑库或同实例别名均拒绝。Failover 遍历所有支持批量清锁的底层存储。
- Artisan 支持 `cache:clear [store] --locks` 和 `cache:clear [store] --tags=first,second`，同时传入非空标签和清锁参数返回状态码 1。锁事件与通用缓存事件另行闭合。
- 隔离基线 `fcec8adaf`，macOS arm64、Rust 1.94.0。固定 Laravel PHP 8.5、Predis 和真实 Redis 8.10.3 的 ArrayStore 与 RedisStore 对照通过。MySQL 8.4.13 与两个独立 Redis 8.10.3 下 13 项缓存回归通过，覆盖文件门闩、清锁后重新获取、旧持有者释放保护、MySQL 锁表和数据隔离、Redis 独立库与地址别名、命令选项冲突、实际 HTTP 和 Artisan。实际清锁删除专用锁库中的标记键，普通值与标签值保留。
- 三语文档中的清锁与标签清理命令实际运行通过。VitePress 构建与 18 项浏览器检查通过。
- 变更文件格式和工作区严格 Clippy 通过。全仓格式仅报告基线中已有的 `sanctum_authentication.rs:1412`。临时夹具已回收，清理本批 2.7 GiB Cargo 产物和文档缓存后，磁盘可用 60.71 GB（56.54 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-locks.OuLPjlei`。

## 缓存操作事件

- 对照固定 Laravel [Repository](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Repository.php)、[Cache Events](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#events)、CacheManager 和 Artisan ClearCommand。openrust-v0.x 未提供这套类型化缓存事件。复用现有应用事件分发器和 Fake。
- 普通和具名仓库派发读写、批量读写、忘记、全量清理和锁清理事件，标签仓库使用原始键和顺序标签。配置可禁用指定存储的事件。Failover 外层默认关闭，底层各仓库遵循自身配置。Memoized 层只在访问底层时派发相关事件。Artisan 的 `cache:clearing` 和 `cache:cleared` 包裹数据或标签清理，`--locks` 独立派发锁事件。
- Rust 的 `pull` 维持原子读取删除，两个前置事件均在操作前触发。原生原子 `add`、正 TTL 的 `touch` 和计数操作直接调用底层。同步监听失败通过缓存错误返回，前置失败阻止写入，后置失败保留已完成的写入。底层返回错误时只保留已经派发的前置事件。
- 隔离基线 `4268ee1f4`，macOS arm64、Rust 1.94.0。固定 Laravel PHP 对照验证事件顺序、标签载荷和批量清锁。MySQL 8.4.13 与两个 Redis 8.10.3 下通过 13 项缓存回归、3 项事件专项与真实 HTTP。Artisan 命令内核验证清理事件次序，实际二进制验证各清理命令，文档中的 Fake 断言在测试中执行。
- Redis memo 的 2 项回归、变更文件格式、工作区严格 Clippy、VitePress 和 18 项三语浏览器检查通过。全仓格式仅报告已有的 `sanctum_authentication.rs:1412`。临时夹具已回收，清理本批 3.6 GiB Cargo 产物和文档缓存后，磁盘可用 60.66 GB（56.49 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-events.XhhZSFxI`。

## 自定义缓存驱动

- 对照固定 Laravel [CacheManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/CacheManager.php) 和 [Adding Custom Cache Drivers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#adding-custom-cache-drivers)。从 openrust-v0.x 的 `cache.rs` 复用应用级注册表、具名惰性实例与替换后保留已解析仓库的设计，使用当前 Repository 和 Store 契约。
- `Cache::extend` 在应用作用域注册工厂。工厂收到管理器和完整的具名配置，可解析应用服务并使用 `repository` 包装，以保留事件设置。`store` 与 `driver` 复用同一具名仓库，`build` 和 `resolve` 创建新的仓库。`forgetDriver` 和 `purge` 移除具名仓库，保留工厂及其他实例。
- Rust 保留同步解析入口。工厂在注册表锁外执行，允许解析同一管理器中其他存储。并发首次解析共享已发布实例，检测同一管理器中的线程等待依赖环。工厂失败后可重试，创建中清除会防止旧实例回写缓存。
- 基线 `169287c8e`，macOS arm64、Rust 1.94.0。固定 Laravel PHP 对照通过懒加载、配置传入、独立应用、按需创建、替换和清除。Rust 5 项专项验证并发、重入、循环依赖、失败重试及创建中清除。独立工程二进制使用自定义文件缓存驱动完成真实 HTTP 写入与读回，另一个实际 Artisan 进程运行 `cache:clear reports`，HTTP 再次读取未命中。
- MySQL 8.4.13 与两个 Redis 8.10.3 下共通过 23 项缓存、事件与 memo 回归。变更文件格式、严格 Clippy、三语 VitePress 和 18 项浏览器检查通过。全仓格式仅报告已有的 `sanctum_authentication.rs:1412`。回收临时夹具，清理本批 4.1 GiB Cargo 产物及文档缓存，磁盘可用 60.63 GB（56.46 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-custom-cache.m6ViKZVf`。

## 空缓存驱动

- 对照固定 Laravel [NullStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/NullStore.php)、[NoLock](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/NoLock.php) 和 CacheManager。openrust-v0.x 未提供此驱动，复用当前缓存 Repository、标签代际及锁所有者生成接口。
- `null` 可按名称或 `CACHE_STORE=null` 解析，未配置默认存储的应用按 Laravel 选择 `null`。根应用的数据库默认值仍由配置提供。读取、批量读取和 pull 总是未命中，写入及续期返回 `false`，清理条目和锁成功。标签操作在不保留代际的存储中使用临时代际，保留失败和清理的可观察结果。
- 无状态锁遵循 Laravel `NoLock`，获取、等待、续期和释放成功，同名锁没有互斥。Rust 用类型化 `MissingCounter` 表达 PHP 计数失败的 `false`。故障切换把这类结果视为本次操作的终点，避免改动备用存储。空仓库事件的存储名为空，与固定 Laravel 一致。
- 隔离基线 `feba1f463`。固定 Laravel PHP 对照通过普通、标签、锁及故障切换路径。Rust 使用真实 HTTP 验证写入返回 `false`、后续读取未命中及写入失败事件。独立 Artisan 二进制的 `cache:forget`、按标签清理和批量清锁通过。
- Rust 1.94.0、macOS arm64，MySQL 8.4.13 与 Redis 8.10.3 下通过 25 项缓存、驱动、事件及 memo 回归，另有 1 项限流回归通过。工作区严格 Clippy、变更文件格式、三语 VitePress 和 18 项浏览器检查通过。全仓格式仅报告已有的 `sanctum_authentication.rs:1412`。临时夹具已回收，清理 3.9 GiB 隔离 Cargo 产物和文档缓存后，磁盘可用 60.58 GB（56.42 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-null-cache.6XOGuKSd`。
