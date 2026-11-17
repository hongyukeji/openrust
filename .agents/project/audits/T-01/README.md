# T-01 测试入口与可复用测试应用

对照固定 [Laravel Testing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md) 的测试启动和环境机制，沿用固定 [Goravel](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的编译型应用装配思路。核对旧版 `6d1a8192` 的 `testing/app.rs`，保留使用生产 Provider 启动测试应用、实例独立配置及不修改进程状态的原则。当前使用常规 Builder 和 Cargo 集成测试，临时目录由 `TempDir` 管理。

`TestApp` 在独立目录中创建应用，允许启动前写入本地夹具。默认 `app.env=testing`，可用 Builder 接入该测试所需的配置和 Provider。用常规引导启动，测试完成后按字段析构顺序释放应用并清理临时目录。HTTP 和 Artisan 测试入口的进一步覆盖归 T-02。

## 验收

- 同一进程内并行引导两个独立应用，从同名文件读取不同设置，变更其中一个应用的配置不会影响另一个。两个根目录在各自释放时清理。默认测试环境另有独立用例。
- 从三语文档提取相同的 Rust 示例，在干净源码上编译并运行通过。
- Rust 1.94.0 锁定离线工作区 535 项通过，5 项忽略夹具由集成测试启动。格式及严格 Clippy 通过。
- 三语站点构建、12 项通用浏览器检查和 6 项测试指南侧栏链接及页面检查通过，覆盖根路径及 `/docs/`。

下一单元 T-02 HTTP 与 Artisan 测试指南。

## 测试环境与整单元复核

基于 `2eccba41b` 的干净归档复核，按固定 Laravel 的测试环境与 `.env.testing` 行为补齐测试入口。旧版 `6d1a8192` 的测试应用蓝图沿用正常 Provider 启动，并将配置源绑定到夹具。Rust 沿用独立的临时根和正常应用引导。 [固定 Laravel 测试环境](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md#environment) · [固定 Laravel TestCase](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Testing/TestCase.php)

- `TestApp` 的配置加载器收到应用级 `APP_ENV=testing`，从临时根优先读取 `.env.testing`，缺失时回退到同根的 `.env`。HTTP、Artisan 和显式引导复用同一启动路径。夹具忽略继承的配置与包缓存路径。两个并行夹具分别以 HTTP 和 Artisan 首次引导，读到各自数据。以外部生产环境变量和损坏的配置缓存重跑时，外部两个缓存文件内容保持原样。
- 真实 `artisan test` 从带 `APP_ENV=production` 的父子进程链启动 Cargo，Cargo 测试再调用独立 Artisan。测试读取 `.env.testing` 的应用名称和默认 `array` 缓存、会话驱动。另用独立 Artisan 验证测试环境文件可显式将二者设为 `file`。参数转发、失败退出码 101 与静默输出回归通过。
- 实际启动两个独立 TCP HTTP 服务，首页和 `/up` 均返回 200。独立 Artisan 分别读取相应环境、应用名称和测试驱动。Rust 1.94.0 的 17 项定向回归、三语 Rust 示例各一份、工作区严格 Clippy、改动文件格式、三语站点构建与 18 项浏览器检查通过。全量格式检查报出干净基线已有的 `sanctum_authentication.rs` 差异。隔离构建、验收夹具和站点已清理，项目卷可用 78,056,128 KiB。
- T-01 的测试入口与可复用测试应用已复验闭合。测试库的事务刷新与驱动管理由 T-03 跟踪。
