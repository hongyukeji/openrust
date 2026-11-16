# 00-01 Config 门面的类型读取、写入和重绑定

固定 Laravel `Config` 通过 `config` 容器键委托配置仓库，支持点分键、类型读取与运行时写入。旧版 `v0.x` 沿用点分键仓库，本批复用主线的仓库和 Facade。基础门面已在 `74b73ed17` 提交，本批保留 `Config::get(&str) -> Result<Option<Value>, BindingResolutionError>`。缺失用 `None` 表达，JSON null 保持显式值，调用方可通过 Option 组合回退。 [固定 Laravel 配置文档](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#accessing-configuration-values) · [固定 Laravel 门面](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Support/Facades/Config.php)

新增 `has`、`all`、`set`、`push`、`prepend` 和严格类型读取。字符串、整数、浮点数、布尔值按 JSON 类型检查。Laravel 的 PHP 顺序数组与关联数组分别映射到 JSON 列表和对象。`array` 保持两种形态，`collection` 保留键。门面同步与异步访问都遵循无缓存设置，`swap` 后容器再次绑定时仍可在同一作用域读到新仓库。

基于 `baf8b0508` 的隔离副本通过 11 项门面、仓库及 TestApp 回归。两个应用并发 HTTP 和 Artisan 验证各自的名称、写入或重绑定，并检查作用域外错误。真实应用二进制在 HTTP 路由中执行三语正文代码，`./artisan fixture:config-probe` 同样读取本次启动设置的 `app.name`。格式、全目标严格 Clippy、VitePress 构建和 15 项浏览器检查通过。既有 Sanctum 格式差异仅在隔离副本处理。

此记录只验收配置访问入口。配置缓存和环境文件加解密仍由 00-01 后续交付。
