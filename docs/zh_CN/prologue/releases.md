# 发布说明

## 版本与上游基线

当前源码在 `Cargo.toml` 中声明开发版 `0.0.0`。构建时使用已提交的 `Cargo.lock`，部署时记录源码提交号。OpenRust 版本号描述 OpenRust 自身的版本。Laravel v13.40.0 是兼容基线，应用启动与装配参考 Goravel v1.18.0。 [Laravel 源码](https://github.com/laravel/framework/tree/1449bf241017087fbd38c118ecaabef3f6797fdc) · [Goravel 源码](https://github.com/goravel/goravel/tree/31651f646ccedf1701471bf01285111270a9d370)

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

依赖审计仍报告 chrono、chrono-tz、paste 和 pin-utils 的维护状态公告，与已修复的漏洞分别跟踪。应用发布仍须执行自身依赖审计和目标环境验证。
