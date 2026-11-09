<h1 align="center">OpenRust — Laravel 的 Rust 实现</h1>

<p align="center">
<a href="LICENSE-MIT">MIT</a> / <a href="LICENSE-APACHE">Apache-2.0</a>
</p>

## 关于 OpenRust

OpenRust 使用 Rust 实现 Laravel 的框架能力与开发体验。目录、概念、默认行为和扩展方式以固定版本的 Laravel 官方文档、源码与测试为依据，让熟悉 Laravel 的开发者能够沿用既有知识编写 Rust 应用。

常用能力的使用入口包括：

- [路由](docs/src/the-basics/routing.md)与 [HTTP 请求](docs/src/the-basics/requests.md)。
- [服务容器与 Service Provider 生命周期](docs/src/architecture-concepts/lifecycle.md)。
- [应用配置](docs/src/getting-started/configuration.md)与[本地化](docs/src/digging-deeper/localization.md)。
- [数据库查询](docs/src/database/queries.md)与 [ORM](docs/src/eloquent/getting-started.md)。
- [认证](docs/src/security/authentication.md)与[哈希](docs/src/security/hashing.md)。
- [模块化扩展](docs/src/modules/registration.md)，对应默认集成的优秀第三方扩展 [laravel-modules](https://github.com/nWidart/laravel-modules)。

**项目处于 pre-alpha 全面重构阶段，尚未完成 Laravel 1:1 等价验收。** 上述链接是能力入口，不代表各项均已完整交付。必要的 Rust 语言适配、已验证范围与剩余缺口分别记录于[功能验收对照](.agents/project/memory/core-functional-acceptance.md)。

## 学习 OpenRust

从 [OpenRust 指南](docs/src/README.md)开始，依次阅读[安装与运行](docs/src/getting-started/installation.md)、[配置](docs/src/getting-started/configuration.md)和[目录结构](docs/src/getting-started/structure.md)。指南按固定 Laravel 13.x 的章节与阅读顺序逐步完善；实际 API 使用 Rust 写法，不直接执行 PHP 源码。

[源码应用与扩展指南](docs/src/getting-started/source-application.md)保留运行、生成应用、事件与执行作用域等详细用法；[唯一综合示例](modules/example/README.md)展示业务模块和公开扩展的实际接入。各专题说明会标明适用条件与尚未完成的边界。

在源码目录用 `cargo run` 启动应用，用 `./artisan list` 查看应用命令，用 `./artisan build` 导出 Linux 发布目录。根应用发布目录用 `./bin/openrust` 启动，包根的 `./artisan` 按固定路径调用同包二进制。Cargo 内部 target 仍为 `app`，开发工具 CLI 仍为 `openrust`；生成应用发布为 `bin/<项目名>`。[Artisan 使用说明](docs/src/getting-started/source-application.md#查找应用命令)

在仓库根目录运行 `docker compose up --build -d --wait` 可启动应用、MySQL 和 Redis。Compose 通过 Artisan 执行一次迁移。持久数据默认写入宿主机的 `./docker-data/`，`./public/` 默认以只读方式挂载。[Docker Compose 部署与旧卷迁移](docs/src/getting-started/source-application.md#用-docker-compose-运行)

## AI 辅助开发

使用 AI 编程工具参与开发前，请阅读 [AGENTS.md](AGENTS.md)。仓库提供[项目技能](.agents/skills/README.md)，用于接口设计、功能实现、故障定位和验证；按任务选择适用技能，并遵守文件归属与验证约定。

## 参与贡献

欢迎参与 OpenRust 开发。环境准备、构建验证、资源管理与提交约定见[贡献指南](CONTRIBUTING.md)。

## 行为准则

为维护友善、相互尊重的协作环境，请阅读并遵守[行为准则](CODE_OF_CONDUCT.md)。

## 安全漏洞

如果发现疑似安全漏洞，请按照[安全政策](SECURITY.md)通过 GitHub Security Advisories 私下报告，避免在公开 issue 中披露利用细节。

## 许可证

OpenRust 为开源软件，您可选择 [MIT](LICENSE-MIT) 或 [Apache License 2.0](LICENSE-APACHE) 授权。

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
