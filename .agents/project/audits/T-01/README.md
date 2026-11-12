# T-01 测试入口与可复用测试应用

对照固定 [Laravel Testing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/testing.md) 的测试启动和环境机制，沿用固定 [Goravel](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的编译型应用装配思路。核对旧版 `6d1a8192` 的 `testing/app.rs`，保留使用生产 Provider 启动测试应用、实例独立配置及不修改进程状态的原则。当前使用常规 Builder 和 Cargo 集成测试，临时目录由 `TempDir` 管理。

`TestApp` 在独立目录中创建应用，允许启动前写入本地夹具。默认 `app.env=testing`，可用 Builder 接入该测试所需的配置和 Provider。用常规引导启动，测试完成后按字段析构顺序释放应用并清理临时目录。HTTP 和 Artisan 测试入口的进一步覆盖归 T-02。

## 验收

- 同一进程内并行引导两个独立应用，从同名文件读取不同设置，变更其中一个应用的配置不会影响另一个。两个根目录在各自释放时清理。默认测试环境另有独立用例。
- 从三语文档提取相同的 Rust 示例，在干净源码上编译并运行通过。
- Rust 1.94.0 锁定离线工作区 535 项通过，5 项忽略夹具由集成测试启动。格式及严格 Clippy 通过。
- 三语站点构建、12 项通用浏览器检查和 6 项测试指南侧栏链接及页面检查通过，覆盖根路径及 `/docs/`。

下一单元 T-02 HTTP 与 Artisan 测试指南。
