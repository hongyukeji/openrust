# 07-01 Helpers 与 Strings

依赖 02-08。按固定版 [Laravel Helpers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/helpers.md) 和 [Strings](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/strings.md) 核对数据路径、数组查询和字符串函数。检查 v0.x 的 Support 数据路径、字符串实现、测试与 PHP 8.5 夹具。数据写入沿用当前 Fluent 的有界实现，转写复用旧版 portable-ascii 2.0.2 数据、来源清单和许可证。

| 函数族 | Rust 契约 |
| --- | --- |
| 数据路径 | 区分显式 JSON 空值与缺失路径。通配读取保持遍历顺序，存在性检查按字面路径。写入验证深度，失败保留原值。 |
| 数组与集合 | Arr 接收 JSON 数组与对象，完整键优先。类型化数组用切片和迭代器，集合流水线沿用 Collection 与 LazyCollection。 |
| 字符串 | 切片借用输入，长度和负索引以 Unicode 标量为单位。保留 Stringable 链式操作。Base64 解码严格校验并返回字节。 |
| 应用服务 | 通过应用的类型化接口和 Facade 获取。URI 和概率判定沿用 Uri 与 Lottery。 |

初批覆盖数据路径、基础 Arr、字符串匹配、截取、转换和默认转写。各后续批次和剩余范围见下文。

## 验收

- Rust 1.94.0 锁定离线构建。格式、严格 Clippy 和并行工作区测试通过，495 项通过，3 个忽略项为集成测试启动的子进程入口。并行回归包含 Redis 限流及调度验收。
- 新增 5 项定向测试，覆盖通配与首末路径、显式空值、稀疏数组、深度失败不修改数据、Unicode 负边界和旧版 PHP 转写夹具。
- 六份三语文档 Rust 示例实际编译运行。三语站点构建通过，12 项通用浏览器检查及 6 项 Helpers 至 Strings 导航通过。

## Markdown 与英语词形变化

- 固定 Laravel 的 `Str::markdown` 使用 GFM，行内入口省略段落包装。Rust 沿用现有 pulldown-cmark，提供表格、删除线、任务列表及类型化 HTML 和链接策略。默认转义 HTML 并过滤链接协议。Rust 行内入口以已解析 Markdown 省略常用块包装，PHP InlinesOnly 解析器和扩展对象作为适配差异。与邮件共用链接过滤，并拒绝控制字符和反斜线。 [固定 Str](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Str.php)
- `plural`、`singular`、`pluralStudly`、`pluralPascal` 和 `counted` 使用现有 Rust 英语词形库。核对负一、零、不可数词、尾部标点、驼峰及数字分组。Doctrine 规则全量和可变 locale 仍待验收。旧版 Support Strings 已复用转写数据与夹具，未发现可复用的 Markdown 或词形实现。 [固定 Pluralizer](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Pluralizer.php)
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

## 尚未验收的范围

- Strings 仍需逐项核对 多语言词形及 COMB 标识符。
- Helpers 仍有 Arr 余下方法、日期及 Benchmark、Pipeline、Sleep、Timebox 等工具的实现或 Rust 映射验收。
- 02-08 依赖仍在账本开放。07-01 继续记录为部分验收。

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
- RBNF 整数拼读覆盖锁定数据包的语言，小数拼读目前支持英语。locale 数据版本和 Intl 解析的宽容规则存在差异。未覆盖语言和数值返回类型化错误。 [Rust RBNF 数据范围](https://docs.rs/intl/0.6.5/intl/spellout/index.html)
- PHP 8.5.12 与 ICU 76.1 实跑固定 Laravel Number，核对 21 项输出。7 项 Rust 定向测试及严格 Clippy 通过。实际 Artisan 运行三语示例，实际回环 HTTP 核对德语小数、USD 金额、英文序数和 i64 上界。三语 VitePress 构建通过。
