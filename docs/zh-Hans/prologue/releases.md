# 发布说明

## 版本与上游基线

框架与同步发布的官方包从根 `Cargo.toml` 的 `workspace.package.version` 继承版本。当前源码版本直接读取该清单：

<<< ../../../Cargo.toml#framework-version{toml}

准备发布时只修改此值，执行 `cargo metadata --format-version 1` 刷新 `Cargo.lock`，检查锁文件差异，再运行验收并更新变更说明。发布构建使用已提交的锁文件，并记录源码提交号。历史发布说明保留当时的版本。

修改包版本后需要重新构建；使用配置缓存的应用还需重新生成缓存，以刷新默认的 `app.version`。显式设置的 `APP_VERSION` 仍由应用自行管理。

仓库默认应用也继承此版本。业务应用可以单独声明自己的 `[package].version`；`app.version` 默认取该应用的 Cargo 包版本，也可由 `APP_VERSION` 覆盖。`App::version()` 和 Artisan `--version` 返回框架包版本。第三方 vendor 包保留上游版本。文档的 `master`／`v0.x` 菜单表示源码分支，不是应用版本。

各包目前声明 `publish = false`，统一版本不会自动启用注册表发布。源码安装保留 workspace 清单；Cargo 生成的 `.crate` 归档会把继承元数据解析为自身清单中的值。单独复制成员目录不会包含其 workspace 元数据。容器镜像标签仍通过 `APP_IMAGE_VERSION` 配置，见[部署指南](../getting-started/deployment.md)。

## 环境兼容范围

源码已在 macOS ARM64、Linux ARM64／x86_64 和 Windows ARM64 上运行验证。须针对目标操作系统与架构构建，二进制不能跨平台通用。Windows Docker Desktop 需要可用的 Linux 容器后端。

现有驱动检查覆盖 SQLite 3.51.3、MySQL 5.7.44／8.0.46／8.4.13、MariaDB 10.3.39／11.8.9、SQL Server 2017／2022 和 PostgreSQL 10–18。这仅描述特定版本和场景的检查范围，不代表所有平台、补丁版本和功能组合均可用。驱动配置及版本限制见[数据库](../database/getting-started.md)。Redis 队列和 Reverb 集成已在 Redis 8.10 上运行验证。

## 部署要求

按[部署指南](../getting-started/deployment.md)构建和运行独立产物。MySQL 5.7 须配置[支持 ECDHE 的 TLS](../database/getting-started.md#mysql-57-tls)；Oracle Linux 7 镜像需使用[派生镜像](../database/getting-started.md#mysql-57-oracle-linux-7)。移植数据库写入逻辑时，参阅[影响行数](../database/queries.md#mysql-affected-rows)。

生产 TLS 由反向代理或负载均衡器终止，须核对部署主机名和证书链。托管 SQS 需要正确的区域、队列 URL 和 IAM 凭据，本地协议测试不能确认 AWS 侧权限。凭据按队列授予 `sqs:SendMessage`、`sqs:ReceiveMessage`、`sqs:DeleteMessage`、`sqs:ChangeMessageVisibility` 和 `sqs:GetQueueAttributes` 权限，临时凭据同时配置会话令牌。参阅[队列](../digging-deeper/queues.md)。

## 变更

应用开发者可在仓库的[更新日志](https://github.com/hongyukeji/openrust/blob/master/CHANGELOG.md)查看变更。迁移已有应用前请查阅[升级指南](upgrade.md)。

## 依赖安全

锁定构建使用 lettre 0.11.24、由 libsqlite3-sys 0.39.1 提供的 SQLite 3.54.1 和稳定版 RSA 0.10.0。SQLx SQLite 适配器保留现有 0.9.0 实现，改用修复后的 SQLite 依赖；WebAuthn 密码适配器更新 RSA feature 声明与类型导入。上游源码及许可证保留在 `vendor/`。Passport 保留密钥生成与 PEM 格式，包括显式生成 1024 位密钥；在线签名仍使用 AWS-LC。

日期辅助方法使用 Jiff 0.2.46 原生类型和内置 IANA 2025b 时区数据，SQL 驱动使用官方 `time` 集成。构造与日历运算见[辅助方法](../digging-deeper/helpers.md)。本地图片及 AWS 传输依赖采用上游对 paste 和 pin-utils 的替代实现，并保留源码与许可证。应用发布前须审计实际依赖并验证目标环境。
