# T-03 数据库测试

- 按固定 [Laravel Database Testing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/database-testing.md) 核对迁移、Seeder 和数据库断言。参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的应用装配，复用旧版 `6d1a8192` 中独立 SQLite 文件和正常迁移路径。
- `TestApp::sqlite` 为每个测试应用创建独立 SQLite 文件，经应用 Provider 配置连接。迁移和 Seeder 继续走实际 Artisan。三个断言复用查询构建器，支持 SQL NULL。每次创建独立夹具实现测试间隔离。Laravel 的 `RefreshDatabase` 事务策略及其他驱动的测试库管理仍待接入。
- Rust 1.94.0 锁定离线工作区 537 项通过，5 项已有测试夹具忽略，格式和严格 Clippy 通过。定向用例并行执行两个应用的 `migrate --seed`，验证空列和行数、缺失 Seeder 退出码 1、`migrate:fresh --seed` 重试、另一应用数据和临时文件释放。
- 英文、简体中文、繁体中文示例提取执行通过。三语站点构建、12 项通用浏览器检查、6 项数据库测试页面定向检查通过。
