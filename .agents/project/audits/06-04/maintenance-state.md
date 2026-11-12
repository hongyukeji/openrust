# 06-04 应用维护状态与 Worker 门禁

## 基线

对照固定 Laravel [Maintenance Mode](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/configuration.md#maintenance-mode)、[MaintenanceMode 契约](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Contracts/Foundation/MaintenanceMode.php)、[MaintenanceModeManager](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/MaintenanceModeManager.php)、[Worker](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Worker.php) 与 [WorkCommand](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Queue/Console/WorkCommand.php)。参考固定 [Goravel 维护模式](https://github.com/goravel/framework/blob/a554ced468cada50a269780692a0491e5f9e3a11/foundation/console/maintenance.go) 的配置与存储接线。v0.x 未找到可复用的应用维护状态。复用应用容器、缓存与 Worker 信号监控。

## 实现与 Rust 适配

- `Application::maintenanceMode` 解析当前应用的状态后端，`isDownForMaintenance` 读取激活状态。文件默认位于 `storage/framework/down`，在同目录原子替换。缓存驱动使用指定仓库与 Laravel 键，数组驱动按应用隔离。
- 守护 Worker 在取件前观察状态，Artisan 单次 Worker 在维护时等待并返回。监听器的子进程执行同一检查，`--force` 允许取件。状态和 I/O 错误沿用现有框架错误报告。
- 后续依赖为 HTTP 维护响应与内置 `down`／`up` 命令，统一使用本批契约。

## 验收

- SQLite、MySQL 文件状态与 Redis 缓存状态的实际 Worker 和监听器验证等待、强制运行及解除后恢复。独立进程确认共享状态。工作区用例覆盖文件原子写、错误数据和数组隔离。
- 三语示例包含应用状态激活与 Artisan 强制取件。三语示例均已编译并运行实际 Artisan。格式、严格 Clippy、文档构建与 18 项浏览器检查通过。工作区全量测试（含真实 SQLite、MySQL 8.4、Redis 8 及 Artisan 子进程）579 项通过、7 项忽略、0 项失败。

[English](../../../../docs/en/getting-started/configuration.md#maintenance-mode) · [简体中文](../../../../docs/zh-Hans/getting-started/configuration.md#maintenance-mode) · [繁體中文](../../../../docs/zh-Hant/getting-started/configuration.md#maintenance-mode)
