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

## 磁盘缓存驱动

- 对照固定 Laravel [StorageStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/StorageStore.php) 和 CacheManager。openrust-v0.x 没有对应驱动，复用现有磁盘管理器、本地与 S3 适配器、TTL 和缓存值契约。
- `storage` 使用具名或默认磁盘，按 SHA-1 哈希和 Laravel 的双层分片规则生成相对对象键。缓存范围由配置目录指定，指定空目录时清理所选磁盘的文件。S3 递归列表按磁盘根与目录前缀分页，本地非空目录清理后重建。
- Rust 使用 JSON 记录和毫秒 TTL，按类型验证计数。读取过期或损坏记录时返回未命中，保留文件供覆盖或显式清理，避免读取过程中误删并发更新。符合 Laravel 的磁盘驱动边界，读改写没有跨进程原子性，此驱动不支持标签与锁。
- 基线 `0301207c7`。固定 Laravel PHP 在受控磁盘和时钟上对照键形、TTL、计数和清理范围。Rust 使用真实本地磁盘、RustFS 1.0.5、MySQL 8.4.13 与 Redis 8.10.3 验收。S3 分页读取 1002 个对象并保留相邻前缀；实际 Artisan 清理测试覆盖本地、S3、删除单键与缓存范围。隔离二进制通过实际 HTTP 写入、读回及另一个 Artisan 进程清理。
- macOS arm64、Rust 1.94.0，27 项缓存、驱动、事件和 memo 回归通过，本地空路径清理额外复验。变更文件格式、严格 Clippy、三语 VitePress 和 18 项浏览器检查通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412`。临时容器已回收，清理 4.1 GiB 本批 Cargo 产物和文档缓存后，项目盘可用 60.11 GB（55.99 GiB），周额度剩余 70%。证据目录 `/private/tmp/openrust-03-06-storage-cache.jLfFS5pO`。

## 会话缓存

- 对照固定 Laravel [Session Cache](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/session.md#session-cache)、[SessionStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/SessionStore.php) 及 [CacheManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/CacheManager.php#L382)。openrust-v0.x 仅记录待实现。复用现有缓存仓库、会话驱动与会话属性的点路径操作。
- `request.session()?.cache()?` 解析具名的 `session` 仓库，应用级仓库每次操作按所属应用和当前请求取会话。缓存沿用会话持久化、加密和生命周期，记录使用 OpenRust 的 JSON／字节格式与毫秒时间。缓存命名空间单独保存在会话载荷中，Rust 的普通会话属性仍可借用引用。过期读取移除条目，计数保留过期时间，清理当前缓存空间保留其他空间和普通属性。续期、清理与计数的变更在当前会话内同步完成。无活跃 HTTP 会话返回 `SessionUnavailable`，缓存标签及分布式锁沿用不支持边界。
- 隔离基线 `bed2da324`，macOS arm64、Rust 1.94.0。固定 Laravel PHP 受控时钟验证过期、计数、续期、点路径及会话和命名空间隔离。`cargo test -p openrust-framework --test session_http --locked --offline -- --test-threads=1` 通过 8 项真实 HTTP 用例，其中两项通过实际 Cookie 验证 array／MySQL 8.4.13 跨请求读取、相同键隔离、二进制缓存、会话 ID 轮换与失效，以及命名空间清理。MySQL 与 Redis 8.10.3 的会话阻塞回归通过。
- `cargo test --test cache --test cache_drivers --test cache_events --locked --offline -- --test-threads=1` 和 `cargo test -p openrust-framework --test cache_memo --locked --offline` 覆盖 27 项缓存用例，其中两个条件用例另用双 Redis 和 RustFS 夹具实跑。实际 `main artisan inspire` 成功，`main artisan cache:clear session` 在没有 HTTP 会话时明确失败并返回退出码 1。
- 变更文件格式和严格 Clippy 通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412`。三语 VitePress、12 项站点浏览器检查、6 项缓存页到会话页链接导航及三语 Rust 示例编译通过。
- 临时容器全部回收。清理本批 4.2 GiB 隔离 Cargo 产物和本批文档缓存后，项目盘可用 60.05 GB（55.93 GiB），周额度剩余 69%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-06-session-cache.PN9qbnef`。

## Memcached 驱动

- 对照固定 Laravel [Memcached 配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#memcached)、[MemcachedStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemcachedStore.php)、[MemcachedConnector](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemcachedConnector.php) 及 [MemcachedLock](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/MemcachedLock.php)。openrust-v0.x 未发现对应实现和测试，复用当前 Repository、TagSet、锁等待和应用配置。
- 支持 TCP、Unix 套接字、SASL PLAIN、带权节点分配、应用内持久连接池、批量读取、原子计数、标签及锁。节点分配使用加权 rendezvous 哈希，Rust 连接参数采用具名选项。缓存值采用 OpenRust JSON／字节编码，非负整数用 Memcached 原生计数格式，PHP 序列化条目和节点哈希无法互换。连接超时、认证和协议故障返回类型化缓存错误。
- 锁写入、续期和释放使用服务端条件更新。Rust 在释放锁和取走条目时用 CAS 条件删除，避免读写间的更新被旧请求删除。缓存清理按 Laravel 行为清空所有配置节点，覆盖所有前缀。
- 隔离基线 `1be6a66e4`。以 PHP 8.4.27、ext-memcached 3.4.0 和真实 Memcached 1.6.45 运行固定 Laravel 存储和锁接口。Rust 1.94.0 在真实双 TCP 节点、SASL 节点和 macOS arm64 Unix 套接字通过 4 项用例。覆盖批量顺序与缺失、长 TTL、两连接竞争、标签、锁恢复及所有者校验、凭据错误、加权路由、连接池配置冲突，以及实际 Artisan 删除与清空。
- 隔离工程的实际二进制通过 HTTP 写读、暂停 Memcached 时返回 500、恢复后的下一次请求读回、独立 Artisan 清理及再次 HTTP 未命中。实际 `main artisan inspire` 通过。MySQL 8.4.13 和 Redis 8.10.3 下通过缓存、驱动、事件及 memo 回归。双 Redis 锁服务器和 S3 条件回归本批未重跑。
- 定向格式、工作区严格 Clippy、三语 VitePress 和 18 项浏览器检查通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412`。Unix 夹具为本地构建的 Memcached 1.6.45，构建时绕过当前 SDK 已移除的 Darwin sandbox 探测，缓存协议和运行方式使用原版。
- 临时容器和 Unix 夹具均已关闭，清理 4.3 GiB 本批隔离 Cargo 产物、文档缓存及本地 C 构建产物后，项目盘可用 58.77 GB（54.73 GiB）。周额度剩余 69%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-06-memcached.geg4c4Mp`。

## DynamoDB 驱动

- 对照固定 Laravel [DynamoDB 配置](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/cache.md#dynamodb)、[DynamoDbStore](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/DynamoDbStore.php) 和 [DynamoDbLock](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/DynamoDbLock.php)。openrust-v0.x 未找到对应实现。使用 Rust 1.94.0 可编译的 AWS SDK DynamoDB 1.116.0，现有共享 AWS 依赖版本保持不变。
- 支持标准凭据链、会话令牌、自定义端点与属性名。以 Unix 秒处理 TTL，读取时过滤过期记录。锁默认 24 小时，续期和释放使用服务端所有者条件。原子条件删除的 `pull` 保证仅一个请求获得旧值，AWS SDK 不自动重放这类返回旧值的写操作与原子计数。
- Rust 存储 JSON、原始字节及原生数字，保留数字字符串类型。计数的范围条件防止溢出后写坏记录。固定 Laravel 的 `forever` 会把五年后的绝对时间再次加到当前时间。OpenRust 直接使用五年后的到期时间，同样适用于无 TTL 的原子 `add`。不支持标签或整表清空。
- 隔离基线 `32a41c761`。固定 Laravel PHP 通过 DynamoDB Local 3.3.2 实测批量读写、计数、缺失与续期、所有者操作、整表清理拒绝及上游 `forever` 时间计算。Rust 3 项专项覆盖默认与自定义键、批量件数和请求尺寸、并发条件更新、TTL、锁恢复、Artisan 删除和清理保护。故障夹具实测只重试未处理的批量项，以及限次耗尽错误。
- 独立工程实际二进制通过 HTTP 写读、独立 `artisan cache:forget` 及 HTTP 未命中，`main artisan inspire` 通过。实际服务为 DynamoDB Local，AWS 托管服务和 IAM 角色部署凭据留待发布验收。
- MySQL 8.4.13、Redis 8.10.3 的缓存、事件和 memo 回归通过。Memcached、双 Redis 锁服务器、S3 条件用例沿用之前的验收。变更文件格式、严格 Clippy、三语 VitePress 和 18 项浏览器检查通过。全仓格式仅报告既有的 `sanctum_authentication.rs:1412`。
- 临时进程和容器已回收。清理 4.4 GiB 本批隔离 Cargo 产物和文档缓存后，项目盘可用 57.67 GB（53.71 GiB）。周额度剩余 69%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-06-dynamodb.gBg0l79j`。

## 缓存迁移生成

- 对照固定 Laravel [CacheTableCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/CacheTableCommand.php)、[MigrationGeneratorCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Console/MigrationGeneratorCommand.php) 和 [cache.stub](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Cache/Console/stubs/cache.stub)。openrust-v0.x 未发现对应命令，复用当前 MigrationCreator 的名称、时间戳、查重及写入。
- `make:cache-table` 和 `cache:table` 生成带时间戳的 `create_cache_table.rs`。已有同名迁移时以退出码 1 拒绝重复生成。模板创建 `cache` 和 `cache_locks`，包含主键和过期索引，值字段遵循固定 Laravel 的 `MEDIUMTEXT`。Rust 应用需要重新构建以注册新迁移。
- 隔离基线 `c83d3a00e`，Rust 1.94.0、macOS arm64。实际 Artisan 验证主命令、别名帮助、重复保护及原文件逐字节保留，通用 `make:migration` 回归通过。隔离副本移走自带迁移后用实际二进制生成并重建，在 MySQL 8.4.13 上运行 `migrate` 和 `migrate:status`。核对列类型、两张表的主键及过期索引。应用的 `cache:forget` 与 `cache:clear database --locks` 操作生成的表，随后 `migrate:rollback` 删除两张表并保留 `users`。
- 命令列表与别名、`artisan inspire`、变更文件及模板格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式只报告既有的 `sanctum_authentication.rs:1412`。
- 临时 MySQL 和 3.5 GiB 本批隔离 Cargo 产物及本批文档缓存已回收，项目盘可用 57.64 GB（53.68 GiB），周额度剩余 69%，第二窗口无读数。证据目录 `/private/tmp/openrust-03-06-cache-migration.LFPORtAt`。
