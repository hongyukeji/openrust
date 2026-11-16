# 配置读取与 Artisan 配置缓存验收

- 固定 Laravel `1449bf2410` 的 `Config\Repository::get`、`getMany` 和类型化读取用于核对缺键默认值。显式空值保留，类型不符返回错误。固定 Goravel `a554ced468` 的 `UnmarshalKey` 对应 Rust 的 `Config::deserialize`，由 Serde 读取同一仓储。原始及默认值读取、应用隔离、重绑定、HTTP 和 Artisan 门面共五项回归通过。第一批本地提交为 `0f04c9048`。
- 固定 Laravel 的 `config:cache`、`config:clear`、`config:show` 和 Boot 缓存命中语义已核对。沿用 `v0.x` 的 JSON 快照、原子替换及错误脱敏。缓存从本次无缓存引导后的仓储生成，缓存命中时无需解析环境文件。缓存位于 `bootstrap/cache/config.json`，进程环境 `APP_CONFIG_CACHE` 可替换路径。Rust 在完整结果可写入时原子替换，刷新失败保留原快照。
- 两项进程级回归使用真实编译后的 Artisan 和 TCP HTTP。验证缓存、修改环境文件后 CLI 与 `GET /up` 仍读旧值、刷新读取新值、清除后重启读取源配置。额外验证显式环境和自定义路径、无效环境文件时缓存仍生效、失败刷新保留快照、清除损坏缓存、损坏内容脱敏、缺失键退出码 1 及 Unix 缓存权限。环境文件加解密的三项进程回归通过。
- 三语 VitePress 构建及两种路径的十二项浏览器检查通过。全仓格式检查报告既有 Sanctum 断言换行差异。
