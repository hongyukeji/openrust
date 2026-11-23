# 项目上下文

AI 代理在新对话、任务交接或上下文压缩后接续开发时，先阅读本文件和[当前交接](current.md)。

- 保存后续仍会用到的项目背景、用户已确认的决定及其原因，以及需要继续跟进的待确认事项。适用时附日期和出处。
- 决定变更时更新原条目。把会影响后续工作的结论写下来，便于其他代理接手。
- 当前任务、停点和当轮授权见[当前交接](current.md)，长期约束见[项目规范](conventions.md)，历史验收见[项目进度](progress.md)。

## 已确认的决定

| 决定 | 原因 | 出处 |
| --- | --- | --- |
| 默认开发分支为 `master`，旧版开发历史保存在 `v0.x`。新主线的设计和复用按项目规范执行。 | 让 Laravel 用户能沿用原有知识，同时保留旧版积累的实现和回归用例。框架与应用骨架独立定版，更新时分别核对。 | 用户确认从零重建、先定正式骨架，并要求旧版复用注明「仅作参考」。[固定基线](plans/laravel-rebuild-baseline.md) · [骨架与开发顺序](conventions.md#骨架与开发顺序) · [旧版实现复用](conventions.md#旧版实现复用) |
| 有成熟、契约适配的 Rust 包时，可以集成或按需二开。 | 复用稳定的底层能力，把开发投入留给 Laravel 契约和 Rust 装配。 | 用户确认成熟 Rust 包可引入、集成或二开。[移植原则](conventions.md#laravel-到-rust-的移植原则) |
| 源码和生产共用同一套 Artisan 命令。开发用项目的 `artisan`／`artisan.bat`，发布二进制直接启动应用，追加 `artisan` 进入命令模式。同进程启动适合默认部署，单独运行 Worker 等仍可选。 | 小团队只需部署一个应用，同一命令体系可用于不同环境。需要单独管理后台进程时仍有独立入口。 | 用户确认入口借鉴 Goravel、命令契约按 Laravel 对齐。运行器本地提交 `7f3fdd1be`。[命令与发布入口](../../README.md) · [独立发布包实测](audits/08-04/README.md#主线独立发布包与交付对照复验) |
| Docker唯一配置为docker/docker-compose.yml，默认启动五服务（含Nginx）。原生Compose只加载根.env，Shell插值优先；DOCKER_DB_HOST／DOCKER_DB_PORT／DOCKER_REDIS_HOST可覆盖容器连接，缺省复用普通参数；镜像取APP_IMAGE_NAME:APP_IMAGE_VERSION，缺省openrust:latest，不增加启动脚本。MySQL默认3306，Redis端口统一监听／映射／健康检查／连接，app等待依赖healthy。持久化数据归docker/data/，public保持只读挂载。 | 方便备份和迁移，发布目录可以整体搬动。 | 用户确认使用真实目录和相对路径，支持公共目录映射。提交 `b894b541c`。[默认 Compose](../../docker/docker-compose.yml) · [部署验收](audits/08-04/docker.md) |
| 文档用可直接在 GitHub 阅读的三语 Markdown，同一批文件供 VitePress 构建。站点可相对路径部署，切换语言保留章节。 | 保留源码阅读和静态站两种入口，让同一份产物可部署到站点根目录或子目录。 | 用户确认用 VitePress 和原生 `vitepress preview --port 1313`。[文档使用与部署](../../docs/README.md) |
| `.env.example` 面向中国开发者提供简体中文和 `Asia/Shanghai` 示例。Redis 缓存库在未显式配置 `REDIS_CACHE_DB` 时回退到 `REDIS_DB`，示例省略前者。 | 常见项目只改必要的环境变量。简体中文和时区是应用示例的选择，Redis 需要单独分库时仍可覆盖。 | 用户确认只改示例语言偏好和 Redis 回退。提交 `9daca1683`、`23df079bf`、`84f329241`、`a065d4e4e`、`b95e0a079`。[环境示例](../../.env.example) · [数据库配置](../../config/database.rs) |
| `config/api.rs`、API 路由和受保护的 `/user` 由 `install:api` 按需生成，保留 `API_PREFIX`。Sanctum 按固定版本单独验收，`--passport` 随 Passport 就绪后接入。 | 保留 Laravel 的按需安装流程，同时允许部署环境直接配置 API 前缀。 | 用户确认 API 配置随安装命令生成，并保留环境变量覆盖。[Sanctum 与 API 安装](plan.md#sanctum-与-api-安装) · [扩展对账](audits/core-closure-ledger.md#扩展与发布) |
| Fortify 的后续设计以 Laravel 官方认证机制为默认。API＋Bearer JWT 显式启用，TOTP 由用户自愿绑定。Passport／OIDC 作为独立扩展，目标是自建授权服务器和跨应用 SSO。 | 保留 Laravel 的默认体验，按需支持前后端分离和跨应用认证。 | 用户先后确认「Laravel 官方机制默认、API＋Bearer JWT 显式可选」、TOTP 可选和 OIDC SSO。当前主线的交付范围见[扩展对账](audits/core-closure-ledger.md#扩展与发布)，旧版实现见[复用参考](plan.md#旧版复用参考)。 |
| 日常数据库覆盖 PostgreSQL 18、MySQL 8、SQLite，缓存覆盖 Redis、database、file；按受影响的共享行为分别验收。数据库其他版本、SQL Server 原生 Linux x86_64 和商业嵌入端点归发布验收。 | 2026-11-21 用户要求加强三库与三种缓存驱动的日常兼容性，覆盖此前仅 PostgreSQL 或仅 MySQL 的验收约定；历史证据保留，单库通过不代表三库通过。 | 用户本次明确调整。[验收清单](conventions.md#验收清单) · [日常验收](conventions.md#日常数据库与缓存验收) · [发布验收](audits/database-dialects.md#发布验收) |
| `artisan init` 是唯一的完整初始化入口，取消 `install:full` 命令及别名，保留单项安装器；默认交互式选择配置，支持 `--force`、只读计划及无交互模式。 | 用一套引导完成配置、完整安装、构建与迁移，最终可启动；默认保留配置、凭据、密钥及迁移历史。 | 用户于 2026-11-19 在侧对话明确采用新入口并取消旧命令，覆盖此前同时保留两个入口的建议；详见[初始化计划](plan.md#artisan-init)。 |

## 当前语言标准：BCP 47（2026-11-22）

用户明确要求全项目采用 BCP 47，默认三语为 `en`、`zh-Hans`、`zh-Hant`；覆盖旧“内部下划线、Web 连字符”的约定。配置、接口、资源、文档目录及 URL 一起迁移；地区标签与文字标签不视为天然等价。详细范围、迁移决策、第三方边界及验收条件见[开发方案](audits/03-07/locale-bcp47-2026-11-22.md)。用户后续已授权实施与隔离验收；当前代码已迁移，结果与边界见[验收记录](audits/03-07/locale-bcp47-2026-11-22.md)。后续“全部处理好”已落实本地 .env 三个语言值迁至 zh-Hans，其他内容及权限保留；Seeder并行夹具隔离和全量回归已闭合。开发／生产数据及旧队列仍由部署者按升级说明处理，没有自动修改；用户随后授权暂存和本地提交，不包含推送或发布。早期国际化验收只代表当时旧标识源码。

## 待用户下令

初始化的最终体验已由用户于 2026-11-19 再次确认：执行 `artisan init` 并跟随必要引导后，直接 `cargo run` 即可正常启动；内部安装、密钥、构建和迁移由命令串联，失败可修正，真正就绪才报告成功。具体契约和端到端验收统一见[初始化计划](plan.md#artisan-init)。用户要求主对话按此最新方案继续开发，此侧对话只调整计划和交接，不修改功能实现、不暂存、提交或推送。

- **URL Monitor 示例。** 网址由管理员配置，准备使用定时探测、连续失败告警、去重、恢复通知和发送失败后的补发。用户已明确框架主线先按自身契约完善调度和邮件等通用能力，示例的配置细节继续讨论，等用户下令后再建分支和实现。生产 SMTP、真实供应商投递和部署调度按各自环境记录验收。出处为用户在主对话中的暂停指令，以及 [OpenRust · URL Monitor](codex://threads/01a1e2ee-7453-70e1-b9fd-2c04b4dbf084) 的需求交接。
