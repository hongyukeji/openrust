<h1 align="center">OpenRust</h1>

<p align="center"><strong>Laravel 的 Rust 实现</strong></p>

<p align="center">
  <a href="CHANGELOG.md"><img src="https://img.shields.io/badge/OpenRust-v0.0.0-blue" alt="OpenRust 开发版 v0.0.0"></a>
  <a href="Cargo.toml"><img src="https://img.shields.io/badge/Rust-v1.94%2B-dea584" alt="Rust v1.94 或更新版本"></a>
  <a href=".agents/project/plans/laravel-rebuild-baseline.md"><img src="https://img.shields.io/badge/Laravel-v13.40.0-ff2d20" alt="Laravel 对照基线 v13.40.0"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue" alt="MIT 许可证"></a>
</p>

<p align="center">
  <a href="docs/en/README.md">English</a> ·
  <a href="docs/zh-Hans/README.md">简体中文</a> ·
  <a href="docs/zh-Hant/README.md">繁體中文</a> ·
  <a href="CHANGELOG.md">更新日志</a>
</p>

## 关于 OpenRust

OpenRust 是一个 Rust Web 应用框架，沿用 Laravel 的项目结构与开发方式。应用通过服务容器和服务提供者完成装配，通过路由和中间件处理 HTTP 请求。Artisan 与 Web 服务共用应用入口，部署时可使用同一个二进制文件。

框架以 Laravel v13.40.0 为基线，参考 Goravel v1.18.0。 [Laravel 基线源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 参考源码](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

## 快速开始

安装 Rust 1.94 或更新版本，在项目根目录运行。

```sh
cargo run
```

访问 <http://127.0.0.1:8000/> 查看欢迎页，`/up` 用于健康检查。监听地址和端口可通过 `APP_HOST` 和 `APP_PORT` 配置。 [环境配置示例](.env.example)

### Artisan 命令

| 环境 | 查看命令 | 查看命令帮助 |
| --- | --- | --- |
| macOS / Linux | `./artisan list` | `./artisan help list` |
| Windows CMD / PowerShell | `.\artisan.bat list` | `.\artisan.bat help list` |
| Cargo | `cargo run -- artisan list` | `cargo run -- artisan help list` |

命令列表按命名空间分组，标有 `Planned` 的命令已预留入口，执行时会提示尚未实现。 [Artisan 指南](docs/zh-Hans/digging-deeper/artisan.md)

### 构建二进制

```sh
cargo build --release --locked
```

构建产物为 `target/release/openrust`，Windows 上为 `target/release/openrust.exe`。在应用运行目录直接执行二进制即可启动服务，追加 `artisan list` 可查看命令。应用从当前目录读取 `.env`。

## 文档

从[简体中文文档](docs/zh-Hans/README.md)查阅应用生命周期、路由、中间件和 Artisan 的用法。站点的预览、构建与部署方式见[文档站指南](docs/README.md)。

## 贡献

开发规范和验收方式见 [AGENTS.md](AGENTS.md)。

## 许可证

OpenRust 采用 [MIT 许可证](LICENSE)。
