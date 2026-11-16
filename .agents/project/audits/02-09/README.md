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

## 惰性集合比较与替换

对照固定 Laravel 的 [差集](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L363)、[交集](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L644)、[合并](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L852) 和 [replace](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1054)。旧版未发现同契约的惰性操作。

- 差集、交集、关联、键及自定义比较变体在首次取值时实体化来源并复用实体集合的类型化比较。`merge` 重排数字键并覆盖同名字符串键，`union` 保留既有键值。
- `replace` 先读完替换来源，在主来源中每个键第一次出现时替换，最后按顺序追加未用的替换项。每次取值最多推进一条主来源记录，来源错误按读取顺序传播。
- PHP 8.5.12 固定源码对照、41 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性按键筛选与投影

对照固定 Laravel 的 [only](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L951)、[except](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L435)、[keyBy](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L567) 和 [投影方法](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L775)。旧版未发现同契约的惰性操作。

- `only` 按来源顺序获取每个请求键的首项，匹配完后立即停止。`except` 在首次取值时按键实体化后排除。
- `keyBy`、`pluckWithKeys` 与 `mapWithKeys` 保留产出时的重复键。实体化结果时最后一个值生效。Rust 用类型化回调表达 PHP 的字段路径，分组复用实体集合并返回已完成的组。
- PHP 8.5.12 固定源码对照、44 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性查找与相邻值

对照固定 Laravel 的 [get、has 与 hasAny](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L539)、[last](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L755) 和 [search、before 与 after](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1100)。旧版未发现同契约的惰性操作。

- `get` 和 `search` 返回首个匹配，`before` 与 `after` 按输入出现次序取相邻项。`last` 读完来源。未读到的来源错误不影响已经得出的结果。
- `hasAll` 对应 PHP 的多键 `has`，空键列表时仍检查来源是否非空。Rust 查找沿用规范化的 `ArrayKey` 相等规则，键 `"08"` 和 `8` 保持独立。PHP `get` 的宽松等值差异已在对照中复现。
- PHP 8.5.12 固定源码对照、47 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性值与字段过滤

对照固定 Laravel 的 [where 系列](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L655) 与 [两类集合共用测试](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/tests/Support/SupportCollectionTest.php)。旧版未发现同契约的惰性操作，沿用当前实体集合的类型化比较和 JSON 路径读取。

- `r#where`、成员过滤和区间过滤保留输入键、重复出现和顺序。区间含两端，区间外要求小于下界或大于上界，NaN 两者都不匹配。严格成员变体共享 Rust 类型比较。
- 空值过滤支持 `Option` 与 JSON。JSON 路径缺失与显式 null 等价，false 保留为非空。`whereInstanceOf` 使用 Rust `Any` 向下转换，不要求克隆值。
- PHP 8.5.12 固定源码对照、50 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。

## 惰性分页拆分与填充

对照固定 Laravel 的 [forPage 与 partition](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L529)、[split](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1316)、[splitIn](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1448) 与 [pad](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1837)。旧版未发现同契约的惰性操作。

- `forPage` 按源次序跳过至目标页，保留键和重复项。`split` 在首次取值时按键实体化并均衡分组。`splitIn` 在调用时先计数，再以保留键的块按需遍历。
- `partition` 单次遍历，返回两个实体集合。同一个键可分别存在于通过与未通过组。`pad` 正数延迟右补，负数复用实体集合左补。Rust 对生成整数键超出范围返回错误，PHP 生成器会回绕。
- PHP 8.5.12 固定源码对照、53 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 2026-11-17 01:52（北京时间）周额度剩余 74%，北京时间 2026-11-23 09:31 重置，无第二窗口读数。PHP 临时容器已回收，清理本批 2.7 GiB Cargo 产物及文档缓存。项目卷可用 77,010,220 KiB（78.86 GB／73.44 GiB）。证据目录 /private/tmp/openrust-lazy-partition.BbGsJBgw。02-09 接续序列化和其余公开入口。

## 惰性集合序列化

对照固定 Laravel 的 [序列化](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L1011)、[首个路径值](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L366) 和 [dot 与 undot](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1717)。旧版未发现同契约的惰性集合入口，复用现有实体集合的 Serde 和数据路径实现。

- `toArray`、`jsonSerialize`、`toJson` 和 `toPrettyJson` 按最后一个键值实体化。连续数字键生成数组，稀疏键与命名键生成对象。读取失败与编码失败分别返回原错误。Serde 支持嵌套惰性集合，JSON 解码构造可重播来源。
- `value` 在第一个包含路径的条目处停止，匹配 null 时保留该值。`dot` 与 `undot` 在首次取值时转换。`join` 与 `implode` 使用实体集合的可显示类型与键规则。
- PHP 8.5.12 固定源码对照、56 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 2026-11-17 01:58（北京时间）周额度剩余 74%，北京时间 2026-11-23 09:31 重置，无第二窗口读数。PHP 临时容器已回收，清理本批 2.7 GiB Cargo 产物及文档缓存。项目卷可用 73,804,060 KiB（75.58 GB／70.39 GiB）。证据目录 /private/tmp/openrust-lazy-serialization.V5dKzjLK。02-09 接续其余公开入口。

## 惰性来源构造

对照固定 Laravel 的 [range](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L98) 和 [wrap、unwrap 与 times](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L132)。旧版未发现同契约的惰性来源构造，复用实体集合的类型定义和键规则。

- `range` 按端点决定方向，步长可达到终点时包含终点。Rust 的 `i64` 和 `NonZeroUsize` 在类型层约束整数范围与正步长，整数边界无需构建中间列表。
- `times` 每次遍历从 1 开始调用回调，结果键从 0 开始且按需生成。`wrap` 以 `Option` 表达单值或空来源。`fromValue` 复用 JSON 键规则，`unwrap` 实体化条目并传播读取错误。
- PHP 8.5.12 固定源码对照、59 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 2026-11-17 02:02（北京时间）周额度剩余 74%，北京时间 2026-11-23 09:31 重置，无第二窗口读数。PHP 临时容器已回收，清理本批 3.0 GiB Cargo 产物及文档缓存。项目卷可用 71,768,020 KiB（73.49 GB／68.44 GiB）。证据目录 /private/tmp/openrust-lazy-sources.krgaXiZU。02-09 接续流程控制和其余公开入口。

## 惰性集合流程控制

对照固定 Laravel 的 [ensure](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L385)、[空值分支](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L600) 和 [管道](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L805)。旧版未发现同契约的惰性入口。

- `when`、`unless` 和 `tap` 在调用时运行所需回调。空值分支最多探测一项，失败直接传播。具名 Else 版本对应 Laravel 的可选备用回调。
- `pipe` 可改变输出类型，`pipeInto` 通过 `From` 转换，`pipeThrough` 串接相同集合类型的步骤。`ensure` 通过 Rust 谓词表达验证，遇首个不合格值停止。`reduceWithKeys` 和 `reduceInto` 逐次保留原键与重复项。
- PHP 8.5.12 固定源码对照、62 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 2026-11-17 02:08（北京时间）周额度剩余 74%，北京时间 2026-11-23 09:31 重置，无第二窗口读数。PHP 临时容器已回收，清理本批 3.0 GiB Cargo 产物及文档缓存。项目卷可用 67,441,124 KiB（69.06 GB／64.32 GiB）。证据目录 /private/tmp/openrust-lazy-flow.LZjByIcO。02-09 接续其余公开入口。

## 惰性记录与嵌套行转换

对照固定 Laravel 的 [flip](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L519)、[select](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L986) 和 [展开与类型映射](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L428)。旧版未发现同契约的惰性操作，递归更新及重复使用现有实体集合实现。

- `select` 按需选择 JSON 字段并重排条目键，保留显式 null。`flip` 使用 `ArrayKey` 约束输入值的类型，重复键在实体化时最后一次生效。
- `mapSpread` 与 `eachSpread` 用切片及独立键传参。`mapInto` 使用 `From`，需按键构造时可用 `map`。`mergeRecursive`、`replaceRecursive` 和 `multiply` 首次取值时读取完整输入。
- PHP 8.5.12 固定源码对照、65 项 Rust 集合测试、实际 Artisan、三语共用示例、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式仍仅报既有 Sanctum 排版。
- 2026-11-17 02:14（北京时间）周额度剩余 74%，北京时间 2026-11-23 09:31 重置，无第二窗口读数。PHP 临时容器已回收，清理本批 3.0 GiB Cargo 产物及文档缓存。项目卷可用 62,375,324 KiB（63.87 GB／59.49 GiB）。证据目录 /private/tmp/openrust-lazy-transforms.hujSnpfi。02-09 接续组合与抽样。

## 惰性组合与抽样

对照固定 Laravel 的 [crossJoin](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L327)、[random](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1041)和 [shuffle](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L1182)。旧版未找到对应惰性实现，本批复用现有实体集合的笛卡尔积、无放回抽样和洗牌。

- `crossJoin` 和 `shuffle` 在首次取值时实体化输入，来源错误按迭代结果传播。`random(count, preserve_keys)` 在调用时选定样本，后续遍历重放同一结果。`randItem()` 对应 Laravel 的单项抽取，空来源返回类型明确的 `ItemNotFound`。Rust 用切片提供笛卡尔积的其他来源，显式指定抽样数量及是否保留键。
- 隔离基线更新至 `da9443208`。PHP 8.5.12 的固定 Laravel 对照及 68 项 Rust 集合测试通过，覆盖空来源和空操作数、数量边界、键保留、同一结果重放及错误传播。
- 三语共用 Rust 示例、`./artisan inspire`、变更文件格式、严格 Clippy、三语站点和 18 项浏览器检查通过。全仓格式只报告既有 Sanctum 排版差异。
- PHP 临时容器已回收。清理本批 3.3 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 64.08 GB（59.68 GiB）。证据目录为 `/private/tmp/openrust-lazy-sampling.F74fzsJi`。

## 惰性匹配占比

对照固定 Laravel [EnumeratesValues::percentage](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L562)。旧版未找到对应惰性集合实现。本批复用实体集合的数值舍入与惰性集合的带键归约。

- `percentage(predicate, precision)` 按源顺序向判定函数传值和键。空来源返回 `None`，来源出错立即停止。Rust 通过同一次遍历统计总数与命中数，避免 Laravel 在惰性源上发起空值检查及两次计数。精度和实体集合保持一致，为显式 `u8`，Laravel 则使用默认 2 的有符号整数。
- 隔离基线 `773400368`。PHP 8.5.12 固定源码对照确认键、空结果、0% 和 100%、半入舍入及 Laravel 三次打开来源。70 项 Rust 集合回归确认单次遍历、一次回调每项和错误后不再读取。
- 三语共用例、`./artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅有既有 Sanctum 排版差异。
- 以固定 `LazyCollection.php` 和 `EnumeratesValues.php` 的显式公开方法名为准，162 项中有 155 项在 Rust 有同名入口。余项包括 Laravel 已弃用的 `containsManyItems`、`containsOneItem`，以及 `dd`、`dump`、`escapeWhenCastingToString`、`getCachingIterator`、`proxy`。此项只核方法名，宏、类型签名和可观察行为需另行验收。
- PHP 容器已回收。清理本批 3.1 GiB 隔离 Cargo 产物及文档缓存后，项目卷可用 61.30 GB（57.09 GiB）。证据目录为 `/private/tmp/openrust-02-09-percentage.oXrJSEl1`。

## 惰性缓存迭代器

对照固定 Laravel [缓存迭代器工厂](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L1061)及[旧计数入口](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/LazyCollection.php#L705)，并实测 PHP 8.5.12 的 SPL 迭代器。旧版未找到对应惰性实现，复用现有迭代器、实体集合键规则和现代计数方法。

- `getCachingIterator()` 为单次遍历提供一项预读和 `hasNext()`。`getCachingIteratorWithFullCache()` 对应 SPL 完整缓存，只保留已经返回的条目，重复键更新缓存值。Rust 通过类型约束只要求完整缓存的值实现 `Clone`，不提供 SPL 的隐式字符串转换标志。预读遇到错误时，Rust 保留当前有效条目，在迭代到缓存的错误结果时返回错误。
- 补齐 `containsOneItem()` 和 `containsManyItems()` 旧名称。带筛选的单项检查继续用 `hasSoleWhere`。固定源码显式公开方法名已对齐 158／162，剩余 `dd`、`dump`、`escapeWhenCastingToString`、`proxy`。名称核对不等同于所有签名和行为验收。
- 隔离基线 `764a5152d`。固定 Laravel PHP 对照和 72 项 Rust 集合测试通过，覆盖预读顺序、缓存覆盖、值无需克隆的前瞻、错误传播和兼容名称。三语共用 Rust 示例、`./artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅有既有 Sanctum 排版差异。
- PHP 容器已回收。清理本批 3.1 GiB 隔离 Cargo 产物与文档缓存后，项目卷可用 61.26 GB（57.05 GiB）。证据目录为 `/private/tmp/openrust-02-09-caching.fCyJkVKJ`。

## 惰性集合调试输出

对照固定 Laravel [EnumeratesValues::dump](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L259)。旧版未找到对应惰性集合入口，复用现有 `all()` 的实体化及键规则。

- `dump()` 读取并输出带键快照并返回原始集合。`dumpWith` 在其后写入附加值，`dumpTo` 允许指定写入器。Rust 用 `Debug` 格式作为 Symfony VarDumper 的语言适配，并通过 `CollectionResult` 传播来源和写入错误。
- 隔离基线 `e0d4ccc46`。PHP 8.5.12 用固定 Laravel 和输出替身验证传给 `dump` 的参数、完全遍历、重复键及返回原集合。74 项 Rust 集合测试通过。实际示例验证标准输出的快照、附加值和后续链式调用，回归覆盖来源失败及断管。
- `./artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅有既有 Sanctum 排版差异。固定源码的显式公开名称已对齐 159／162，余项为 `dd`、`escapeWhenCastingToString` 和 `proxy`。
- PHP 容器已回收。清理本批 3.1 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 61.23 GB（57.02 GiB）。证据目录为 `/private/tmp/openrust-02-09-dump.gupfkT1n`。

## 惰性集合字符串转义

对照固定 Laravel [字符串转换和转义开关](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L1071)。旧版未找到对应惰性实现。复用现有 JSON 序列化，并提取 Blade 原有的 HTML 字符转义作为共享实现。

- `escapeWhenCastingToString(bool)` 在当前惰性集合上设置转义开关，设置时不遍历来源。`toString()` 返回普通 JSON 字符串或经 HTML 转义的字符串，并传播来源和序列化错误。Rust 显式调用该方法，保留错误类型。`toJson()` 和 `toPrettyJson()` 仍返回 JSON。
- 隔离基线 `ba5f1ac2b`。PHP 8.5.12 固定源码对照、76 项 Rust 集合测试和 5 项视图测试通过。覆盖现有 HTML 实体的再次转义、键与值、克隆和转换产生的新集合、来源错误及 Blade 原有转义。
- 实际三语共用示例、`./artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅有既有 Sanctum 排版差异。固定源码公开名称对齐至 160／162，余项为 `dd` 和 `proxy`。
- PHP 容器已回收。清理本批 3.3 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 61.20 GB（56.99 GiB）。证据目录为 `/private/tmp/openrust-02-09-escape.TfZ78Ud2`。

## 惰性集合调试终止

对照固定 Laravel [EnumeratesValues::dd](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Traits/EnumeratesValues.php#L248) 和 [Symfony VarDumper 8.0.0 的运行入口](https://github.com/symfony/var-dumper/blob/v8.0.0/Resources/functions/dump.php)。旧版未找到对应惰性集合入口，复用当前 `dump` 的快照和输出格式。

- `dd()` 和 `ddWith` 返回带调试快照的终止结果。Rust 用 `?` 在正常结果链上终止当前命令或请求。Artisan 把快照写向标准输出并返回状态码 1。HTTP 返回纯文本 500，当前 HTTP 服务可继续处理后续请求。来源失败在生成快照前传播。
- 隔离基线 `455f4b20d`。实际 Artisan 验证状态码 1、快照及附加信息出现在标准输出、后续命令代码未运行。实际回环 HTTP 验证当前请求 500、输出内容及随后另一条请求 200。PHP 8.5.12 使用固定 Laravel、Symfony 的真实 `dd` 函数和格式化替身，验证快照参数、附加信息和状态码 1。
- 77 项 Rust 集合测试和 3 项 HTTP 异常渲染测试通过。`./artisan inspire`、变更文件格式、严格 Clippy、三语站点及 18 项浏览器检查通过。全仓格式仅有既有 Sanctum 排版差异。固定源码显式公开名称已对齐 161／162，余项为 `proxy`。
- PHP 容器已回收。清理本批 3.6 GiB 隔离 Cargo 产物和文档缓存后，项目卷可用 61.21 GB（57.01 GiB）。证据目录为 `/private/tmp/openrust-02-09-dd.sQbyvzLF`。
