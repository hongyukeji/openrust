# 07-01 Helpers 与 Strings

依赖 02-08。按固定版 [Laravel Helpers](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/helpers.md) 和 [Strings](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/strings.md) 核对数据路径、数组查询和字符串函数。检查 v0.x 的 Support 数据路径、字符串实现、测试与 PHP 8.5 夹具。数据写入沿用当前 Fluent 的有界实现，转写复用旧版 portable-ascii 2.0.2 数据、来源清单和许可证。

| 函数族 | Rust 契约 |
| --- | --- |
| 数据路径 | 区分显式 JSON 空值与缺失路径。通配读取保持遍历顺序，存在性检查按字面路径。写入验证深度，失败保留原值。 |
| 数组与集合 | Arr 接收 JSON 数组与对象，完整键优先。类型化数组用切片和迭代器，集合流水线沿用 Collection 与 LazyCollection。 |
| 字符串 | 切片借用输入，长度和负索引以 Unicode 标量为单位。保留 Stringable 链式操作。Base64 解码严格校验并返回字节。 |
| 应用服务 | 通过应用的类型化接口和 Facade 获取。URI 和概率判定沿用 Uri 与 Lottery。 |

本单元覆盖数据路径、基础 Arr、字符串匹配、截取、转换和默认转写。PHP 动态对象访问、ICU 严格转写、Markdown、单复数和其他 Laravel 字符串扩展尚无对应验收。

## 验收

- Rust 1.94.0 锁定离线构建。格式、严格 Clippy 和并行工作区测试通过，495 项通过，3 个忽略项为集成测试启动的子进程入口。并行回归包含 Redis 限流及调度验收。
- 新增 5 项定向测试，覆盖通配与首末路径、显式空值、稀疏数组、深度失败不修改数据、Unicode 负边界和旧版 PHP 转写夹具。
- 六份三语文档 Rust 示例实际编译运行。三语站点构建通过，12 项通用浏览器检查及 6 项 Helpers 至 Strings 导航通过。

下一单元 07-02 HTTP Client，前置 01-01 已提交。

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
