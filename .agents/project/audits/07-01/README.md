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
