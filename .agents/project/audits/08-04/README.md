# 08-04 安装、部署和发布资料

以 `34a7a497b9fb56ebe1c213403399b1fe0bad7dac` 的干净检出验收。对照固定 Laravel 的 [安装](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/installation.md)、[部署](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/deployment.md)、[发布](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/releases.md)、[升级](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/upgrade.md) 与 [贡献](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/contributions.md)，参考固定 [Goravel 入口](https://github.com/goravel/goravel/blob/31651f646ccedf1701471bf01285111270a9d370/main.go) 的编译型运行。核对旧版 `6d1a8192` 的 `Dockerfile`、资源打包、`.env.example` 和 Artisan 路径。沿用二进制与资源分开发行和最小权限运行，部署根路径采用当前 standalone 契约，升级文档记录旧环境变量到现行配置的映射。

## 验收

- macOS ARM64、Rust 1.94.0，从干净源码执行锁定构建、环境模板、SQLite 文件创建、`key:generate`、`migrate`、`inspire` 和 `cargo run`。首页及 `/up` 返回 200。
- `cargo +1.94.0 build --release --locked --features standalone` 生成 Mach-O arm64。独立产物从源码目录之外启动，迁移及 Artisan 通过，首页、健康路由、网站和公共磁盘文件可读。移动发布目录后从不同工作目录运行，SIGTERM 正常退出并释放端口。
- 同锁文件和 Rust 1.94.0 在同架构 Linux ARM64 Debian Bookworm 容器生成 ELF aarch64。将产物和资源部署在另一目录，以非特权用户运行 SQLite 迁移、Artisan、HTTP、静态文件和优雅关停。此处为 ARM64 容器验收。
- macOS 独立产物连接独立 MySQL 8.4.13 测试库，四项迁移及 `migrate:status` 通过，测试库和凭据已清理。Redis 8.10 的发布支持记录引用 08-02 的真实双进程验收。
- 干净检出执行贡献指南的格式、锁定工作区测试与严格 Clippy，533 项通过，5 个忽略项由集成测试启动。三语站点构建、12 项通用浏览器检查和 30 项发布相关页面／导航检查通过。
- Linux x86_64、Windows 实机和生产 TLS 终止仍需各目标环境验收。现有 v0.x 应用的数据迁移依赖应用自己的 schema、数据格式和迁移历史。本单元更新开发版资料，未创建发布标签。

下一单元 T-01 测试入口与可复用测试应用。
