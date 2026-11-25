# Artisan 完整初始化验收

交付契约见[完整初始化计划](../../plans/init-full.md)。该入口复用 OpenRust 的源码安装器、包发现、原生迁移和 Redis 锁。独立 HTTP、Artisan 与 Worker 子进程使用最终应用产物验证基础链路。安装步骤延续[历史初始化验收](init.md)中的配置优先级、资源发布、失败重跑和迁移隔离契约。

`--dry-run` 的只读边界从 Artisan 运行开始。源码入口 `./artisan` 使用既有 `cargo run` 启动命令，首次运行可能生成 CLI 编译产物。预览自身不写应用配置或脚手架，也不连接外部服务。

## 功能清单

| 能力 | 安装及启用条件 | 后续使用入口 |
| --- | --- | --- |
| API／Sanctum、广播频道及 gRPC | 复用单项安装器发布应用注册。 | 应用 API、广播频道及 gRPC 服务注册。 |
| Passport、Fortify、Passkeys、用户套件 | 安装 Passport 配置、迁移及密钥；用户套件选用 `full`，发布 Fortify 接线、REST 资源、页面和语言文件。浏览器设备页读取原生 Redis 会话，设备索引沿用 MySQL。 | `/register`、`/login`、`/user/profile`、`/user/sessions`、OAuth 客户端命令。外部 WebAuthn 使用 HTTPS 和匹配的 Origin。 |
| Reverb、Socialite、OIDC／SSO、Precognition | Reverb 和 OIDC 发布各自配置；Socialite 由特性装配，Precognition 使用核心中间件。 | 广播监听随环境启用；OAuth／OIDC 提供商、回调和本地账号绑定由应用配置；路由自行应用 `precognitive`。 |
| Webhook 接收和发送 | 分别构建两个特性，通过包发现注册 Provider、默认配置与编译迁移。 | 业务添加回调、处理任务和签名配置，配置发送目标及消费者。 |
| Activitylog、Cashier、Horizon、New Relic、Pail、Pay、Pennant、Pulse、Scout、Settings、Telescope | Cargo 特性、已有 Provider 和包默认配置；有编译迁移的包随原生迁移执行。 | 应用按业务使用对应门面、命令、队列及仪表盘；支付、APM 和外部索引按实际环境设置。 |
| 数据库翻译 | 启用可直接引用的 `localization-database` 库。该库的正式契约要求应用显式创建自己的词条表并选择加载时机。 | `DatabaseLoader` 与 `Lang::load_from`。 |
| 内置缓存、会话、队列、锁、调度和其他核心能力 | 复用框架默认装配，完整预设共享 MySQL 与 Redis。 | 应用门面、HTTP、队列 Worker、`schedule:run`。 |

SMTP、真实短信、支付和外部身份供应商按使用场景提供账户及凭据。 验收清单固定于本次集成版本；在验收基线之后新增的机器翻译 `translate` 按自身文档启用，待后续预设版本纳入。测试使用两个独立的本机应用进程共享 MySQL 与 Redis；Redis Cluster、真实生产负载均衡及 TLS、原生 Windows 和 Linux 复用各自的发布验收范围。

## 运行结果

- macOS arm64、Rust 1.94.0、MySQL 8.4.13 和 Redis 8.10.3。首轮在 `1c0dbe8b5` 上叠加本批生产改动。数据库词条扩展改名后，在含 `192ba0f8e` 的干净源码完整复验，并确认命令预览和安装清单使用 `localization-database`。服务由测试夹具在外部独立启动，公开源码入口 `./artisan init:full` 的进程路径拦截 Docker 和 Docker Compose 调用。
- 新项目的只读预览、已有驱动冲突、生产迁移未授权、缺少数据库配置及 Redis 不可用检查均通过。正式命令从干净应用完成构建、迁移和 HTTP／Artisan／Redis Worker 自检。实际浏览器路由完成注册、登录、退出及设备会话读取，两个同时运行的应用进程共享登录态。
- Redis Worker 在保留任务后被强制终止，独立 Worker 在重试窗口后完成该任务。原生迁移锁竞争返回 12，`init:full` 同时报告失败，释放后迁移正常。两个独立调度进程只留下一个 `on_one_server` 任务标记。
- 模拟 Cargo 不可用后保留 `.env`、密钥及自定义路由。锁释放后再次完整初始化成功，重跑前后的既有配置字节、应用清单和迁移文件集合一致，进程传入的数据库 URL 未输出或落盘。实际终端上的原有 `init` 向导可选择扩展及取消，取消后保持文件内容。
- 三语 VitePress 构建通过。根路径及 `/docs/` 下，三语部署指南链接在桌面和手机共 12 组浏览器校验中均打开正确的安装章节。

- `cargo fmt --all --check` 通过。Cargo 回归中原 `init` 的 16 项与原生数据库会话的 2 项通过，覆盖数据库会话加密、元数据和 Guard 归属。 Fortify 浏览器设备的 7 项回归通过，包括不受支持的驱动及同名自定义处理器拒绝。 Rust 1.94.0 的工作区全目标严格 Clippy（`--workspace --all-targets --locked -- -D warnings`）通过。

复现时用项目内干净源码和专属 Cargo 目录。先用 Rust 1.94.0 构建 `main`，从隔离源码运行 `python3 -B tests/mysql_acceptance.py -- python3 -B tests/init_full_acceptance.py`，外部通过 `OPENRUST_TEST_REDIS_HOST` 和 `OPENRUST_TEST_REDIS_PORT` 提供隔离 Redis 服务。运行日志集中于本批 `target/tmp/init-full/`。
