# Webhook 原生初始化入口

根应用的 `init` 接受显式的 `webhook-client`、`webhook-server` 及联合选项。沿用现有 `[features].default` 源码编辑、本地包发现、配置清理、密钥生成和迁移装配。标准 `all` 组合保持原有范围。两包继续通过 Provider 注册原生编译迁移；配置模板通过已有 `vendor:publish --tag=webhook-client-config` 或 `--tag=webhook-server-config` 显式发布。应用自行登记回调路由、验证配置及业务处理器，发送侧由应用提供投递策略及 Worker。固定 Spatie 包使用 Composer 发现与 Laravel 资源发布，OpenRust 的 Cargo 特性和编译迁移责任对应其安装步骤。旧版 `6d1a81924` 无这两个独立包的安装入口。 [固定 Laravel 包开发](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/packages.md) · [固定 Spatie 接收包](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/README.md) · [固定 Spatie 发送包](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/README.md)

应用自管的[迁移发布](migration-publication.md)已补齐同名文件的构建、账本和回滚身份。

## 实际验收

- `tests/webhooks_install_acceptance.py` 在隔离源码使用无 Webhook 特性的正式入口启动 Artisan。预演单包、双包和 `all`，配置和 Cargo 清单保持原字节，数据库无 Webhook 表。真正初始化、重新构建后，只有所选包具备配置发现与数据库表。配置和路由缓存生成后，实际 HTTP `/up` 返回 200。
- SQLite 分别验证接收、发送、双包初始化及重复初始化。PostgreSQL 18.7 和 MySQL 8.4.13 验证双包初始化及重复初始化。重复执行保留 `APP_KEY`、数据库迁移行数、现有模型、路由、自定义 stub 和已发布配置。两包的配置发布重复执行保留自定义内容。
- 从同一隔离源码编译的消费者分别在 SQLite、PostgreSQL 及 MySQL 通过原生 Artisan 队列和独立 Worker，校验 Spatie 与 Standard Webhooks 两个签名协议以及 Worker 自身凭据的解析。正式应用安装后的健康路由和独立消费者的业务回归分开记录。

变更文件的格式检查和框架库严格 Clippy 通过。全工作区格式检查仍报告[三处既有差异](backend-interruption.md)。

## 已安装应用收发往返 {#installed-roundtrip}

`tests/webhooks_install_acceptance.py --profile both --roundtrip` 从未启用包的正式制品启动，完成两个包的初始化、配置发布、迁移和重复初始化检查。之后在隔离应用接入发布后的两个配置模块、应用 Provider、接收路由和发送 Artisan 命令，重建正式 `main`。接收包的命名配置、真实路由、数据库队列，发送包的 Builder、签名及持久投递均通过应用入口装配。职责对照固定 Spatie 的应用 Job 与异步派发示例。[固定接收包](https://github.com/spatie/laravel-webhook-client/blob/ff886d1bae33d35ad98b6c91403f280df158129d/README.md#processing-webhooks) · [固定发送包](https://github.com/spatie/laravel-webhook-server/blob/6da6be3a721190bcdedcdef32d31d5980b77eb0e/README.md#usage)

分别测无缓存和配置／路由缓存。每个阶段通过同一个安装制品的独立 Artisan 进程提交 Spatie 和 Standard Webhooks 投递，独立出站 Worker 把签名后的原字节发送到应用的真实回调，独立接收 Worker 处理持久接收记录。分别验证入队前后、持久终态、签名错误无落库、原始正文及处理确认，最终两类队列均空且无失败任务。让生产者带不同凭据生成配置缓存并提交任务，由出站 Worker 在运行时解析正确凭据。接收 Worker 可从已保存记录完成处理，运行时无签名凭据。配置缓存、投递快照和两侧队列均检查无明文凭据。

隔离源码基线 `bcbd85b31`，Rust 1.94.0。内嵌 SQLite、PostgreSQL 18.7、MySQL 8.4.13 各通过 4 次双向协议投递和 2 次 HTTP 无效签名校验。完成三库验收后，最终生成应用通过全工作区 `cargo fmt --all -- --check` 和正式二进制 `cargo clippy --locked --bin main -- -D warnings`。首次 SQLite 验收中，夹具误把 64 位十六进制投递 ID 当作 UUID，按投递与接收记录各自的 ID 格式校验后重测通过。

夹具只授权固定回环地址和两条路径。正式应用的默认骨架和安装器均保持原有装配。跨主机部署、真实远端供应商和生产 TLS 仍走各自验收门禁。
