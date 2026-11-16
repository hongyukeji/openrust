# 00-01 配置缓存与整单元验收

固定 Laravel `1449bf2410` 的 `Config\Repository::get`、`getMany` 和类型化读取用于核对缺键默认值。显式空值保留，类型不符返回错误。固定 Goravel `a554ced468` 的 `UnmarshalKey` 对应 Rust 的 `Config::deserialize`，由 Serde 读取同一仓储。原始及默认值读取、应用隔离、重绑定、HTTP 和 Artisan 门面共五项回归通过。第一批本地提交为 `0f04c9048`。

并行提交 `763c4485b` 提供三条配置命令和真实 HTTP 验收。本次保留 `config:show`、常规文件保护及仓库读取，沿用此前移除 `bootstrap/cache/` 的目录决定。

固定 Laravel 的 `config:cache` 会清除本次选定的旧缓存并引导全新应用，再写入 Provider 参与后的最终配置。旧版 v0.x 的 C022 已验证双阶段路径、缓存屏障和原子写入。本批沿用这些契约与回归思路。 [固定 Laravel 命令](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Console/ConfigCacheCommand.php) · [固定 Laravel 配置文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#configuration-caching)

Rust 将配置序列化为 JSON，默认保存在已有运行时目录 `storage/framework/config.json`，并以 `APP_CONFIG_CACHE` 支持绝对或应用相对路径。主进程按普通 Artisan 启动。刷新时由 `runApplication` 的同一二进制在子进程重建并引导应用，原子发布私有快照。配置缓存命中保留标准 Provider 生命周期，跳过普通配置加载，并在路径可预先解析时跳过 dotenv。损坏的 JSON 阻止引导，错误不显示配置值。缓存创建需要标准可执行文件入口，直接构造的测试应用仍可读取和清除已有缓存。

实际 HTTP 验证 Provider 写入后的配置进入缓存、损坏 dotenv 时仍能使用快照、清除后正在运行的服务不热切换，重启读取新值。实际 Artisan 核对覆盖路径、缓存隔离、错误缓存保留、新建失败无半写及幂等清除。五项配置命令和缓存、三项环境加密、一项启动配置与脱敏测试通过。真实 `./artisan inspire`、Rust 格式、严格 Clippy、三语文档命令执行、VitePress 和 15 项浏览器检查通过。macOS 私有缓存权限为 0600。既有 Sanctum 格式差异仅在隔离副本处理。

00-01 的启动骨架、配置来源和环境选择沿用[原验收](README.md)，[配置门面](config-facade-access.md)和[环境文件加解密](environment-encryption.md)已有分项证据，本批闭合剩余配置缓存。其他平台的二进制验收按发布矩阵。
