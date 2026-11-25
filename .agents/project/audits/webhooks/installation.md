# Webhook 原生初始化入口

根应用的 `init` 接受显式的 `webhook-client`、`webhook-server` 及联合选项。沿用现有 `[features].default` 源码编辑、本地包发现、配置清理、密钥生成和迁移装配。标准 `all` 组合保持原有范围。两包继续通过 Provider 注册原生编译迁移；配置模板通过已有 `vendor:publish --tag=webhook-client-config` 或 `--tag=webhook-server-config` 显式发布。应用自行登记回调路由、验证配置及业务处理器，发送侧由应用提供投递策略及 Worker。固定 Spatie 包使用 Composer 发现与 Laravel 资源发布，OpenRust 的 Cargo 特性和编译迁移责任对应其安装步骤。旧版 `6d1a81924` 无这两个独立包的安装入口。 [固定 Laravel 包开发](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/packages.md) · [固定 Spatie 接收包](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/README.md) · [固定 Spatie 发送包](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/README.md)

应用自管的[迁移发布](migration-publication.md)已补齐同名文件的构建、账本和回滚身份。

## 实际验收

- `tests/webhooks_install_acceptance.py` 在隔离源码使用无 Webhook 特性的正式入口启动 Artisan。预演单包、双包和 `all`，配置和 Cargo 清单保持原字节，数据库无 Webhook 表。真正初始化、重新构建后，只有所选包具备配置发现与数据库表。配置和路由缓存生成后，实际 HTTP `/up` 返回 200。
- SQLite 分别验证接收、发送、双包初始化及重复初始化。PostgreSQL 18.7 和 MySQL 8.4.13 验证双包初始化及重复初始化。重复执行保留 `APP_KEY`、数据库迁移行数、现有模型、路由、自定义 stub 和已发布配置。两包的配置发布重复执行保留自定义内容。
- 从同一隔离源码编译的消费者分别在 SQLite、PostgreSQL 及 MySQL 通过原生 Artisan 队列和独立 Worker，校验 Spatie 与 Standard Webhooks 两个签名协议以及 Worker 自身凭据的解析。正式应用安装后的健康路由和独立消费者的业务回归分开记录。

变更文件的格式检查和框架库严格 Clippy 通过。全工作区格式检查仍报告[三处既有差异](backend-interruption.md)。
