# 07-01 Helpers 与 Strings

依赖 02-08。按固定版 [Laravel Helpers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/helpers.md) 和 [Strings](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/strings.md) 核对数据路径、数组查询和字符串函数。检查 v0.x 的 Support 数据路径、字符串实现、测试与 PHP 8.5 夹具。数据写入沿用当前 Fluent 的有界实现，转写复用旧版 portable-ascii 2.0.2 数据、来源清单和许可证。

| 函数族 | Rust 契约 |
| --- | --- |
| 数据路径 | 区分显式 JSON 空值与缺失路径。通配读取保持遍历顺序，存在性检查按字面路径。写入验证深度，失败保留原值。 |
| 数组与集合 | Arr 接收 JSON 数组与对象，完整键优先。类型化数组用切片和迭代器，集合流水线沿用 Collection 与 LazyCollection。 |
| 字符串 | 切片借用输入，长度和负索引以 Unicode 标量为单位。保留 Stringable 链式操作。Base64 解码严格校验并返回字节。 |
| 应用服务 | 通过应用的类型化接口和 Facade 获取。URI 和概率判定沿用 Uri 与 Lottery。 |

初批覆盖数据路径、基础 Arr、字符串匹配、截取、转换和默认转写。后续批次与整单元验收见下文。

## 验收

- Rust 1.94.0 锁定离线构建。格式、严格 Clippy 和并行工作区测试通过，495 项通过，3 个忽略项为集成测试启动的子进程入口。并行回归包含 Redis 限流及调度验收。
- 新增 5 项定向测试，覆盖通配与首末路径、显式空值、稀疏数组、深度失败不修改数据、Unicode 负边界和旧版 PHP 转写夹具。
- 六份三语文档 Rust 示例实际编译运行。三语站点构建通过，12 项通用浏览器检查及 6 项 Helpers 至 Strings 导航通过。

## Markdown 与英语词形变化

- 固定 Laravel 的 `Str::markdown` 使用 GFM，行内入口省略段落包装。Rust 沿用现有 pulldown-cmark，提供表格、删除线、任务列表及类型化 HTML 和链接策略。默认转义 HTML 并过滤链接协议。Rust 行内入口以已解析 Markdown 省略常用块包装，PHP InlinesOnly 解析器和扩展对象作为适配差异。与邮件共用链接过滤，并拒绝控制字符和反斜线。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- `plural`、`singular`、`pluralStudly`、`pluralPascal` 和 `counted` 使用现有 Rust 英语词形库。核对负一、零、不可数词、尾部标点、驼峰及数字分组。八语言规则及显式 locale 见下文的多语言词形变化。旧版 Support Strings 已复用转写数据与夹具，未发现可复用的 Markdown 或词形实现。 [固定 Pluralizer](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Pluralizer.php)
- 字符串套件 4 项通过，定向严格 Clippy 和格式检查通过。三语同一示例由实际 Artisan 命令执行。实际回环 HTTP 核对 Unicode 行内输出、恶意协议过滤、HTML 转义和计数结果。三语站点构建通过。

## 链式 URI 与查询串

- 按固定 URI 文档核对组件、查询变更、应用路由生成和重定向。移植旧版 URI 的不可变组件修改和查询规则。复用有界的括号参数解析和 RFC 3986 编码。URI 解析保留字面加号和带点键，普通 HTTP 查询保留表单解析契约。Rust 查询使用 JSON 值，URI 通过显式 `redirect` 生成响应。 [固定 URI 文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/helpers.md#uri)
- 10 项锁定离线用例通过，其中 HTTP 查询和 URL 组覆盖既有请求解析、签名和重定向。定向严格 Clippy 通过。实际 Artisan 从三语示例生成相对 URI，并用 `APP_URL` 生成命名路由。实际回环 HTTP 跟随运行时命名路由重定向，核对 Unicode 字符、加号、参数顺序和片段。三语 VitePress 构建通过。

## Unicode 与字面量编辑

- 对照固定 Str 的边界剪除和替换、位置查找、掩码、截取、指定字符去重和空白折叠。复用现有 Unicode 标量切片与旧版迁移的 trim 字符表。Rust 用 `Option` 表示未找到或偏移越界，用字符切片明确去重对象。静态入口和 `Stringable` 链式入口共用实现。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- 5 项字符串测试和定向严格 Clippy 通过。实跑三语示例的 Artisan 命令，回环 HTTP 核对中文和 emoji 的负偏移索引、循环掩码和空白折叠。三语 VitePress 构建通过。

## Arr 类型读取与编码

- 核对固定 Arr 的类型读取、缺失与 null、精确键优先、`add`、`forget`、`pull`、`wrap` 及 RFC 3986 查询编码。复用 Rust 数据路径、旧版迁移的精确键规则和 URI 编码器。通过同一移除入口修复 URI 按字面量带点键删除。类型读取以明确的 `Result` 表达异常，调用方可选择缺失时的默认值。 [固定 Arr](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Arr.php)
- 定向数据、URI、请求查询与字符串回归共 12 项通过，定向严格 Clippy 通过。实际 Artisan 编译并运行三语示例，实际回环 HTTP 验证布尔值读取、删除和 Unicode 查询编码。三语 VitePress 构建通过。

## 随机字符串与标识符

- 对照固定 Str 的随机字母数字、密码字符池、UUID v4／v7、ULID 和格式检查。随机字符使用锁定依赖的系统熵种子生成器，UUID v4 使用现有 AWS-LC 随机源并设置版本和变体位。复用项目既有 UUID／ULID 依赖。Rust 采用类型化返回值和显式字符池。PHP 全局生成器替换、可传入时间、COMB orderedUuid 和字符串版本别名仍有差异。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- 6 项字符串定向测试和严格 Clippy 通过。实际 Artisan 执行三语示例，回环 HTTP 核对 UUID 版本、ULID 格式和密码长度。验收响应只返回布尔结果和长度。三语 VitePress 构建、五个新章节锚点及 12 项浏览器检查通过。

## 正则与文本排版

- 固定 Laravel Str 和 PHP 8.5.12 实测首捕获、可选捕获为空、替换次数、汉字和 emoji 的展示宽度、整词截断以及多字节换行。Rust 复用锁定的 regex，引入类型化无效模式错误和 Rust 回调。模式及替换采用 Rust 语法，PCRE 分隔符、后向查找和模式内反向引用属于语法差异。旧版字符串代码没有对应实现。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- 宽度数据由 PHP 8.5.12 的 `mb_strwidth` 对每个有效 Unicode 标量枚举，连续双宽标量合并为区间。换行使用标量长度和空格分隔，保留整词时处理基本 HTML 标签。 [PHP 宽度数据](https://github.com/php/php-src/blob/php-8.5.12/ext/mbstring/libmbfl/mbfl/eaw_table.h)
- 8 项 Strings 定向测试、严格 Clippy 和三语 VitePress 构建通过。实际 Artisan 运行三语示例。实际回环 HTTP 核对可选捕获、CJK 截断和多字节换行。


## Slug 与严格转写

- 对照固定 Str 的 ASCII 映射、反向分隔符、按序字典替换和 Unicode 字符过滤。沿用旧版默认转写及其 portable-ascii 2.0.2 许可证，补入该固定版本的 locale 映射、覆盖表和多字序列长度。静态与 Stringable 共用实现。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- 严格路径使用 Rust ICU4X 0.7 的 CLDR 文字转换，依次执行 NFKC、非间距组合符清理、NFKC、按 Unicode 文字分段做拉丁转写、Latin-ASCII 与 portable-ascii 回退。ICU4C 的 Any-Latin 分段及 CLDR 规则版本可能产生不同输出。 [ICU4X](https://docs.rs/icu_experimental/0.7.0/icu_experimental/transliterate/index.html)
- 固定 Laravel 的 slug 由 PHP 8.5.12 对照，严格模式规则由 ICU 77.1 对照。锁定离线测试及定向严格 Clippy 通过。实际 Artisan 执行三语示例，实际回环 HTTP 验收默认 slug、德国映射和混合文字转写。三语 VitePress 构建通过。

## Number

- 核对固定 Number 的小数精度优先级、负零、本地化解析、百分比、货币、拼读与序数、文件大小和数量缩写。复用旧版类型保留的 clamp 规则及整数边界回归。采用 ICU4X 的 CLDR 格式和 Rust RBNF 拼读数据。locale 与 currency 通过实例及回调作用域传递，整数及浮点区间分别使用 `pairs` 和 `pairsFloat`。 [固定 Number](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Number.php)
- 拼读覆盖范围见下文的多语言数字拼读。数字解析的 CLDR 数据版本和 Intl 宽容规则存在差异。
- PHP 8.5.12 与 ICU 76.1 实跑固定 Laravel Number，核对 21 项输出。7 项 Rust 定向测试及严格 Clippy 通过。实际 Artisan 运行三语示例，实际回环 HTTP 核对德语小数、USD 金额、英文序数和 i64 上界。三语 VitePress 构建通过。

## 计时工具

- 对照固定 Benchmark、Sleep、Timebox 及其错误和提前返回行为。核对 v0.x 的 Support 实现及测试，未找到可复用的对应计时模块。Benchmark 使用单调时钟返回毫秒均值和回调结果。Sleep 采用显式 await、类型化 Duration 和实例级替身，Timebox 给异步回调的返回值或错误补足剩余时间。 [固定 Benchmark](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Benchmark.php) [固定 Sleep](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Sleep.php) [固定 Timebox](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Timebox.php)
- 条件重复使用等长间隔，零时长时让出调度。Rust 计时差异包括显式等待、Duration 单位、局部替身和 Future 取消语义。PHP 析构等待映射为显式 await，计时替身由调用者传入。Benchmark 返回值和耗时供调用者输出及决定进程退出。
- PHP 8.5.12 对照 Benchmark 返回值和 Timebox 的异常等待及提前返回状态。22 项锁定离线定向回归、严格 Clippy 和三语 VitePress 构建通过。实际 Artisan 运行三语示例，实际回环 HTTP 验证错误返回达到最短时限、替身记录和回调结果。

## Arr 变换与筛选

- 核对固定 Arr 的集合变换、选择、过滤、随机抽样、排序与 CSS 表达。Rust 使用有序 JSON、类型化回调和键投影，复用数据路径的有序合并。保键筛选与重新编号的 `take` 分别验收。动态 PHP 对象转换、松散比较和混合类型排序使用 Rust JSON 契约。 [固定 Arr](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Collections/Arr.php)
- 固定 Laravel 在 PHP 8.5.12 通过七组对照。Rust 1.94.0 锁定离线定向测试、严格 Clippy、真实 Artisan 与回环 HTTP 和三语 VitePress 构建通过。旧版具有数据路径基础，未发现可直接移植的 Arr 变换模块。

## 日期与区间

- 对照固定 Laravel 的 `now`、`today`、`Carbon::plus/minus` 和八种区间单位，并核对旧版 Carbon 请求日期及区间回归。沿用历法分量与应用时区分开的契约，复用现有 Chrono 和 IANA 时区数据。月份支持溢出或月末截断。日期分量按当地历法，小时以下按经过时间计算。Rust 日期不可变，返回新值并显式处理范围错误。 [固定 Carbon 扩展](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Carbon.php)
- PHP 8.5.12 配 Carbon 3.8.4（Laravel 声明的最低版本）对照闰月、月份倒退及夏令时。6 项 Rust 定向测试、严格 Clippy、真实 Artisan、回环 HTTP 和三语 VitePress 构建通过。自定义 Carbon 工厂及任意 PHP 日期字符串仍属适配范围。

## Pipeline

- 对照固定 Pipeline 的阶段替换与追加、进入和退出顺序、短路、目的回调、原输入清理和事务。类型化 Rust 阶段接收一次性 `Next`，选择显式调用或提前返回。事务复用现有连接及回滚机制，阶段通过 `next.context()` 借用同一事务。服务与方法通过类型化回调显式绑定，PHP 类名字符串及方法反射属适配差异。旧版未找到通用 Pipeline 实现。 [固定 Pipeline](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Pipeline/Pipeline.php)
- 固定 PHP 对照嵌套顺序、短路和原输入清理。Rust 1.94.0 锁定离线定向测试与严格 Clippy 通过。真实 Artisan 和回环 HTTP 各验证一个 SQLite 提交与回滚，事务结束后只剩已提交行。三语示例实际运行，VitePress 构建通过。

## 多语言词形变化

- 按固定 Laravel Pluralizer 和其声明兼容的 Doctrine Inflector 2.1.0 核对词形规则。规则数据保留原顺序、不可变项及不规则替换，附 MIT 许可。通过 `InflectionLanguage` 逐次显式选用语言，默认英语。Rust 使用 Unicode 正则，英语 regatta 的后向例外按后缀判断。旧版未发现多语言规则实现。 [固定 Pluralizer](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Pluralizer.php) [Doctrine 2.1.0 规则](https://github.com/doctrine/inflector/tree/2.1.0/src/Rules)
- PHP 8.5.12 运行固定 Laravel 的八语言 46 词矩阵，核对复数、回转单数和计数边界。10 项 Rust 定向测试及严格 Clippy 通过。真实 Artisan、回环 HTTP 和三语 VitePress 构建通过。

## COMB orderedUuid

- 固定 Laravel `Str::orderedUuid` 使用 Ramsey TimestampFirstCombCodec。Rust 复用既有 UUID v4 随机源和版本、变体位编码，把 Unix 10 微秒刻度写入前 48 位。`orderedUuidAt` 接收显式时间，用类型化错误报告范围或随机源问题。同一刻度内的随机顺序与 PHP 一致。旧版未发现对应生成器。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php) [Ramsey 4.7.6 编码器](https://github.com/ramsey/uuid/blob/4.7.6/src/Codec/TimestampFirstCombCodec.php)
- PHP 8.5.12 实跑固定 Laravel 与兼容的 Ramsey 4.7.6，核对实时时钟前缀、二进制与文本一致以及版本。20 项 Rust 锁定离线定向回归、严格 Clippy、真实 Artisan、回环 HTTP 和三语 VitePress 构建通过。验收临时副本、PHP 对照夹具、站点产物和测试 SQLite 文件按批清理，共享 Cargo 缓存保留。

## 标题、字面量编辑与链式入口

- 固定 Strings 方法表逐项补齐静态标题、首字母、填充、搜换、摘要、子串替换、单词计数和 URL 判断，复用已有 Unicode 切片、URL 解析器、正则和 Crypt。链式入口补齐对应操作、断言、条件回调及显式字符串转换。HTML 标签移除支持允许标签列表，与整词截断共用基本 HTML 匹配。URL 端口和主机验证采用 Rust URL 解析器的语义。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php) [固定 Stringable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Stringable.php)
- PHP 8.5.12 固定 Laravel 运行 12 个对照样例。32 项 Rust 1.94.0 锁定离线回归、定向严格 Clippy、真实 Artisan 与回环 HTTP、三语 VitePress 构建通过。字符串格式扫描及额外 Helpers 入口见后续复核。

## 空值、重试和摘要

- 对照固定 Helpers 的空字符串、null、数字、布尔值和空数组规则。Rust 将 JSON 对象作为键值容器。`Retry` 使用一基尝试序号、固定间隔或退避表及错误谓词，复用异步 Sleep。PHP 任意回调延时属于额外适配边界。`Stringable::hash<D>` 以 Rust Digest 类型提供摘要，保留小写十六进制返回值。核对 v0.x 的同名工具，未找到对应通用实现。 [固定 Helpers](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/helpers.php)
- PHP 8.5.12 固定 Laravel 通过四组对照。15 项 Rust 锁定离线定向回归和严格 Clippy 通过。实际 Artisan 与回环 HTTP 验证三次尝试及成功返回。三语 VitePress 构建通过。独占验收目录清理，共享 Cargo 缓存保留。

## Stringable 格式扫描

- 固定 Laravel `Stringable::scan` 将 PHP `sscanf` 的结果包装为集合。Rust 对照 PHP 8.5.12 的格式验证及字节扫描，支持位置、宽度、跳过赋值、扫描集合、进制、科学计数、文字和 `%n`。首个转换前的输入不足产生空集合，部分匹配保留空槽。格式错误返回 `ScanError`。UTF-8 被宽度切断时保留原始字节，非有限浮点遵循既有集合序列化边界。旧版仅有日期词法扫描器，契约不同。 [固定 Stringable](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Stringable.php) [PHP 8.5.12 扫描器](https://github.com/php/php-src/blob/php-8.5.12/ext/standard/scanf.c)
- PHP 8.5.12 矩阵覆盖 48 组正常及异常格式。12 项 Rust 锁定离线回归、严格 Clippy、真实 Artisan 和回环 HTTP 及三语站点构建通过。

## 多语言数字拼读

- 固定 Laravel `Number::spell` 和 `spellOrdinal` 采用 ICU 的区域数据、默认规则与公开序数规则。Rust 内置 ICU 76.1／CLDR 46 的 102 套规则资源与 869 个区域条目，支持逐位拼读、分母规则、数词复数、区域回退及本地化数字输出。沿用实例作用域和含端点的 `after`／`until`。v0.x 的 Number 实现和金标覆盖类型保留的 clamp，已在初批复用。 [固定 Number](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Number.php) [规则数据来源](../../../../vendor/openrust-framework/src/support/number/spellout/README.md)
- 实跑 PHP 8.5.12／ICU 76.1 的固定 Laravel，核对 13,913 组有限数拼读和序数。另与 ICU 76.1 比对 2,607 组无穷大与 NaN。范围覆盖区域变体、小数前导零、负数、两种端点、64 位整数边界和缺省序数规则。在超出有符号 64 位范围的浮点值触发数字回退时，Rust 保留可往返转换的十进制表示。英文 `1e40` 的数字回退已验证。PHP／ICU 在该范围内的转换溢出结果属于运行时差异。
- Rust 1.94.0 锁定离线执行 Number、Strings 和 Stringable 扫描的 18 项定向测试。工作区全目标严格 Clippy 与变更文件格式通过。工作区格式检查仍报告基线中的 Sanctum 测试排版。真实 Artisan 执行三语共用示例，回环 HTTP 200 核对德语、俄语、台湾繁体中文小数拼读。三语 VitePress 构建、拼读章节锚点及 12 项桌面和移动浏览器检查通过。验收路由和命令仅用于隔离副本。

## 整单元验收

- 前置 [02-08](../02-08/README.md#整单元验收) 已闭合。按固定 Helpers 和 Strings 的函数族复核数据路径、Arr、集合、日期、URI、服务辅助、计时和流水线，以及 Stringable 链式契约。最后完成格式扫描和多语言数字拼读。各批记录包含实际应用入口、三语示例与边界用例。
- 07-01 核心契约闭合。Rust 的类型化输入、错误、回调及实例作用域，以及 PCRE、Carbon、PHP 全局状态和浮点溢出的运行时差异，见相应验收章节。
