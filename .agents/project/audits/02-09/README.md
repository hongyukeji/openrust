# 02-09 惰性集合

核对固定 Laravel 13.x 的[惰性集合章节](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/collections.md#lazy-collections)、[LazyCollection 源码](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php)和[回归测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportLazyCollectionTest.php)。Goravel 固定版本的[类型化集合](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/support/collect/collection.go)使用实体切片，仅参考其编译期类型化 API。旧版 `6d1a8192` 的框架源码及测试无惰性集合，沿用 02-08 从旧版提取的 `ArrayKey` 归一化规则。

- 接通迭代器工厂、带键和可失败的输入、`Enumerable`、`Collection::lazy()`、过滤与映射、分块、正负 `take`、跳过、首次匹配和副作用。组合后的来源在取值时打开，`take(n)` 不提前读取第 n+1 个产出。负数 `take` 需要遍历到末尾，仅保留指定长度的尾部。
- `remember()` 共用一个源迭代器，按位置缓存已读条目。多次遍历可交错并保留重复键。实体化时同键的最后值覆盖前值，可先调用 `values()` 保留各次出现。失败立即终止，记忆模式保留源错误并向后续遍历重放。
- Rust 使用线程内的迭代器和 `Result`，允许回调借用局部状态。按位置重放要求值可克隆。分块输出已实体化的 `Collection`。期限与心跳使用单调时钟，`throttle` 在同步调用线程等待，期限检查发生在两次读取之间。PHP 的动态回调模式、完整扩展方法表及异步数据库游标衔接仍属后续范围。

## 验收

macOS arm64、Rust 1.94.0。在 `/private/tmp/openrust-02-09.vzJ6p7/app-under-test` 以 02-08 候选副本为底运行。

- `cargo +1.94.0 fmt --all --check`、`cargo +1.94.0 test --workspace --locked --offline`、`cargo +1.94.0 clippy --workspace --all-targets --locked --offline -- -D warnings` 通过。测试报告 369 项通过，含本批 6 项回归。真实 MySQL 未在本批复跑。
- 用 PHP 8.5 CLI 运行固定源码，13 项对照通过。验证惰性打开、过滤短路、普通遍历重新打开、带重复键的缓存、双迭代器和负数截取。独立 Rust 程序用真实文件验证按需读取、缓存重放及来源删除后的重新读取错误。`RUSTUP_TOOLCHAIN=1.94.0 ./artisan inspire` 正常退出。
- 三语 Markdown 的惰性集合示例均使用 Rust 1.94.0 编译并执行。`npm run build`、12 项常规浏览器组合及 6 次集合页正文、侧栏和 `remember()` 锚点检查通过。
- 与隔离副本逐文件核对本批实现、测试和文档。工作区锁定依赖解析及 `git diff --check` 通过。

下一单元为 03-01 本地文件系统与公共磁盘，依赖 01-01 的配置和应用基础。

## 异步数据库游标

对照固定 Laravel 的 [Connection 游标](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Connection.php#L456) 和 [Query Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Query/Builder.php)。Rust 的 Connection、Transaction 和 Query Builder 通过 `DatabaseCursor` 返回可失败的异步 Stream。首次轮询才编译 SQL 或取得连接，复用现有语句准备、绑定检查、驱动解码、读写路由和按组限额。SQLx `fetch` 逐行交付，`async-stream` 0.3.6 封装拥有局部状态的流，无需后台任务，新增宏包同为 MIT 且最低 Rust 1.65。提前停止释放非事务租约；事务内提前结束仍按现有不完整命令策略标记中断，丢弃事务触发回滚。旧版找到的 BatchCursor 是键集分批位置，未找到同契约的数据库行流。固定 Goravel 的连接与查询入口作为装配参考。

隔离基线 `bfe3c3c1`。MySQL 8.4.13 和 SQLite 验证未轮询不占连接、绑定检查、单连接池提前结束后可继续查询、事务读完继续使用、提前结束中断和回滚。SQLite 验证先产出第一行再传播数据库错误。真实 SQLite HTTP 验证只读连接、写后粘滞、显式使用写库及跨应用隔离。9 项查询、7 项数据库、6 项分页回归、全目标严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 排版差异只在隔离副本处理。容器已回收，清理本批隔离 Cargo 产物 1.9 GiB 及站点产物。

提交前周额度剩余 25%，项目卷可用 52 GiB。按当前交接记录收尾本批并等待恢复确认。下一批接 Eloquent 模型逐行水合与关系游标，再复核 05-02 和 05-06。同步惰性集合的其他差异仍按本单元跟踪。

## Eloquent 模型游标

固定 [Laravel Eloquent Builder](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Database/Eloquent/Builder.php) 的 `cursor` 应用查询作用域，逐行构造模型并经过模型集合，再返回单条结果。Rust 复用数据库行流和现有模型水合、属性转换与读取事件。游标不执行关系预加载，读取事件内查询需要另一个可用连接。v0.x 的 BatchCursor 表达键集分页位置，固定 Goravel 的 ORM 查询接口作为装配参考。

隔离基线 `38d96054`。MySQL 8.4.13 与 SQLite 通过 TCP 请求验证转换、读取事件、关系预加载边界、提前结束和读取到第三行时传播事件错误。另验证软删除作用域。夹具先启动应用再注册读取事件，以确保数据库管理器已绑定。21 项查询、关系和分页回归，Rust 1.94.0 严格 Clippy、实际 `./artisan inspire`、三语示例编译、VitePress 和 18 项浏览器检查通过。既有 Sanctum 排版仅在隔离副本处理。清理本批 1.9 GiB Cargo 产物及文档构建产物。提交前周额度剩余 99%，项目卷约 119 GiB。关系游标接线仍待验收。

## 惰性分组和滑动窗口

对照固定 Laravel 的 [LazyCollection](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1196) 及 [两类集合共用测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。旧版未找到同契约。继续采用现有懒迭代器、键规则和实体块类型，Goravel 的集合仅参考编译期类型。

- `chunkWhile` 用一项前瞻识别下一组，回调可读取当前组，保留键与顺序。
- `sliding(size, step)` 支持重叠、连续和带间隔的完整窗口，校验正数参数。重复键沿用 Laravel 的覆盖和顺序规则。由 Rust 实体窗口持有输出值，因此值需实现 `Clone`。
- 定向集合测试 20 项，通过延迟读取、边界复用、重复键、短窗口、无效参数和读取错误传播。PHP 8.5.12 运行固定 Laravel 源码对照。三语共享 Rust 示例、实际 `./artisan inspire`、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 本批针对同步迭代器，不需要数据库夹具。其余同步方法继续逐项对照。

## 惰性选取与追加

对照固定 Laravel [LazyCollection 的 keys、nth、concat 和 slice](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L737)、[chunkBy](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L941) 和 [两类集合共用测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。旧版没有同契约的惰性方法，复用现有迭代器、实体集合切片和键规则。

- 正向 `slice` 保留键并按需读取。负偏移或负长度按 Laravel 先实体化并去重，再按位置切片。`nth` 禁止零步长，选取结果重新编号，选取间隔中的来源错误依然传播。
- `keys` 产出原键并保留重复项。`concat` 接受另一个同值类型的惰性集合，读完第一来源才打开第二来源，合并结果重新编号。Rust 的 `chunkBy` 使用回调提取待比较的值，保留组内原键。PHP 字符串字段路径由 Rust 回调表达。
- PHP 8.5.12 固定源码对照、23 项集合测试、三语共用 Rust 示例、实际 Artisan、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性嵌套集合展开

对照固定 Laravel 的 [collapse 与 collapseWithKeys](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L206)、[flatten](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L497) 和 [flatMap](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L464)。旧版未发现同契约，沿用当前实体集合到惰性集合的转换。

- `collapse` 展开一层，重排索引。`collapseWithKeys` 保留内部键和顺序，重复键在实体化前可逐项读取。`flatMap` 先映射到分组，再展开。Rust 用类型约束接受惰性集合、实体集合、向量和数组。
- `flatten` 对 JSON 按指定层级展开数组和对象，对标量直接产出。每次遍历复用一个栈，逐项移动已拥有的 JSON 值。嵌套来源按需打开，错误立即结束遍历。
- PHP 8.5.12 固定源码对照、26 项 Rust 集合测试、三语共用示例、实际 Artisan、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性来源配对

对照固定 Laravel 的 [zip](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1816)、[combine](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L885) 和 [两类集合共用测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。旧版未发现同契约的惰性操作，本批沿用现有实体集合的 Rust 行类型和惰性输入转换。

- `zip` 和 `zipMany` 按位置读取到最长来源结束，以 `Option` 表达空位。Rust 用 `zipMany` 表达 Laravel 的可变参数重载。
- `combine` 从第一来源的值生成键，复用 PHP 数字字符串键规则。来源不等长时，迭代器在检测到差异的那次读取返回错误。已产出的成对值保持有效。
- PHP 8.5.12 固定源码对照、29 项 Rust 集合测试、三语共用示例、实际 Artisan、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性去重和终端判定

对照固定 Laravel 的 [unique](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1738)、[sole、hasSole 和 firstOrFail](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1336) 及 [hasMany](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L344)。旧版未发现同契约的惰性操作。

- 去重沿用实体集合的类型化相等比较，`uniqueBy` 每轮遍历独立保留已见的投影值，输出第一次出现的原条目和键。Rust 的 `uniqueStrict` 与默认类型比较相同。
- `contains` 与 `firstOrFail` 在首个匹配时停止。`hasSole` 和 `hasMany` 最多读取两条匹配。`sole` 在前两条同键时返回后一个值，复现 Laravel 先按键实体化的结果。来源错误只在实际读到时返回。
- PHP 8.5.12 固定源码对照、32 项 Rust 集合测试、三语共用示例、实际 Artisan、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性排序与重复项

对照固定 Laravel 的 [lazy passthru](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1935)、[排序接口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1498) 和 [两类集合共用测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。旧版未发现同契约的惰性操作。

- 首次取值时实体化来源，再用现有实体集合排序、反转或找重复项。键收敛和稳定排序沿用实体集合，负向 `slice` 复用同一路径。来源读取失败时传播错误，无部分结果。
- Rust 通过类型化投影或比较回调表达 Laravel 的动态排序参数。`duplicatesBy` 输出重复的投影值及原键，`duplicatesStrict` 使用同样的类型化相等比较。
- PHP 8.5.12 固定源码对照加载 Symfony 1.43.0 的 `SortDirection` polyfill。35 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性集合聚合

对照固定 Laravel 的 [枚举聚合](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L202)、[中位数与众数](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L185) 及 [countBy](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L336)。旧版未发现同契约的惰性操作，复用现有数值转换和实体中位数实现。

- `sum`、`avg`、最值和折叠按次序处理每条输入。空来源保留求和的零值，平均值与最值返回 `None`。`every` 在首次不满足时短路。`reduceSpread` 复用实体集合的二元元组契约。
- `countBy` 在首次读取时逐条汇总，按组首次出现顺序产出，保留数字字符串键规则。中位数与众数先按来源键实体化。众数使用类型化 `Eq + Hash` 分组，同频值按出现顺序返回。动态投影通过 Rust 回调表达。
- PHP 8.5.12 固定源码对照、38 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
