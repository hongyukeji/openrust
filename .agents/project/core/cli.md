# cli

> 受众：AI Agent
> 更新：2026-11-02
> 摘要：源码内 `openrust` 命令提供中文帮助、项目生成、开发启动与运行期命令代理；生成项目遵循根 workspace 和简体中文说明规范。


当前宿主：显式模块注册与宿主API前缀已完成[限定验收](../memory/testing.md#显式模块注册与宿主-api-前缀限定验收)：10项主门禁、6个真实server进程、fresh minimal9/MFA真实MySQL3、生成模块持久启停1及只读空目录负例通过；首批8页mdBook构建/链接、112生成源码归档、不同模型独审和target清零完成，本地提交以Git为准。registered为应用约定，库file默认保留；完整Modules/Routing与全量文档未关闭，下一SQLite及CLI后继按唯一todo。 新应用不再生成根modules_statuses.json，显式file/file-live仍调用原发现/编译绑定。全部CLI使用方式对应仍按R13。

标准生成应用默认MySQL、Redis缓存与Redis队列编译依赖，队列默认database，连接在实际使用时创建；`--minimal`明确选择无数据库/队列模板。框架迁移由FrameworkMigrator统一提供，生成任务不重复登记QueueMigration，生成迁移目录测试检查框架前缀和重复名。四种fresh的check/tests、MySQL/PG/Redis实际任务与默认MySQL认证/MFA、精简consumer和配置缓存重启已通过[限定验收](../memory/testing.md#标准依赖与队列开箱限定验收)；未据此关闭完整Artisan/R13。

## 通用 DB 门控（限定验收）

模型生成不依赖官方驱动文字；普通new登记FrameworkMigrator而不建迁移package，有DB骨架仍使用自有Migrator避免重复贡献。普通UUID模型只为应用及已有迁移清单启用db-uuid，Job依真实迁移结构预检。根/生成/SQLite及真双库、普通两种模型与UUID已按[限定范围验收](../memory/testing.md#db-通用能力与驱动分层限定验收)；UUID先预检两清单再写，坏输入真实生成调用保持原字节。写阶段不保证跨文件事务回滚；完整CLI与标准队列默认体验仍开放。

## 当前实现

- C04-D1生成模板接入公开Session契约：认证项目装配正式数据库SessionProvider，plain测试才显式追加memory；请求共享句柄、flash/now/null与opaque Cookie往返由生成的request_session/request_cookie helper验证。Session表通过AuthMigration纳入，不能再重复注册独立SessionMigration；本簇最终重验结果见[testing](../memory/testing.md)。

- 应用端口使用中性的 `SERVER_PORT`，生成 `.env.example` 在 `APP_URL="http://localhost:${SERVER_PORT}"` 前声明 `SERVER_PORT=3000`；内部字段仍为 `server.port`，自定义应用命名空间不改变该环境键。缓存选择使用 `CACHE_STORE`/`cache.default`，默认具名 redis；开发服务发现须使用所选 store 的实际连接，其他具名 Redis store 不套用全局 `REDIS_*`。R2-ENV已通过生成、真实HTTP/Redis与适用工作区门禁，限定证据见memory/testing.md；完整CLI范围仍按todo验收。

- `openrust new <目标目录>` 从源码 workspace 创建独立应用，支持相对与绝对目标，保留独立 Cargo.lock。应用清单以相对路径解析同一框架、工具与驱动补丁，Compose 用相对构建上下文复现目录关系。两份源码可一起迁移。限定证据见[生成应用 Docker Compose](../memory/testing.md#生成应用-docker-compose)。
- C04-C普通与认证模板均接入CookieServiceProvider和`tests/support/request_cookie.rs`，由正式Provider/Router测试读取、队列、直接响应及删除，使用opaque wire回送；加密缺失仅显式plain选择，不暗降级。普通项目fmt/check/3项测试、认证PG18/MySQL8.4项目各2项及完整CLI20项已实际通过，证据见[testing](../memory/testing.md)。
- `openrust dev` 和 `serve` 通过 Cargo metadata 找到当前应用，再从其目录运行 `cargo run`。开发入口复用或准备适用的 Redis 服务。根应用和生成应用的 `./artisan dev` 调用 `openrust dev --watch`，在 macOS 和 Linux 上通过 Watchexec 监听应用和本地 Cargo 依赖。成功构建的独立产物在旧进程收到 SIGTERM 并退出后启动，编译失败期间旧应用继续服务。开发配置沿用应用的 `--env` 和 dotenv 选择规则。

- package `cli` 位于仓库 `tools/cli/`，二进制命令 `openrust` 保留；源码交付与应用隔离按 历史决策 039 已完成。
- `package:discover --preview` 输出候选 Rust 引用。`--install` 写入按目标和应用特性分开的清单。根应用和生成应用的构建脚本核对锁文件、相关 manifest 与项目 Cargo 配置，然后把对应清单交给同一 Provider 启动槽。
- `openrust --help` 及子命令帮助使用简体中文标题、选项说明和帮助/版本说明；命令、参数、配置键和协议值保留技术原名。
- 应用二进制的 `list` 和 `help <命令>` 使用同一 Provider 命令目录。`Command::usage` 提供可选参数说明，帮助入口不执行目标命令；生成项目复用同一入口。
- 新应用的 `AGENTS.md`、`.env.example` 注释、模块入口 Rustdoc，以及 Model/Migration/Mailer/Policy 生成 Rustdoc 使用简体中文；模板说明不改变应用邮件等运行时语言策略。

- `openrust generate model` 生成 SeaORM entity 并注册 module；`openrust generate migration` 生成建表迁移并注册到 `Migrator`，支持基础标量、UTC 时间、JSON、nullable 与 references 字段 DSL。两者可一致启用 `--uuid-id`（UUIDv7）和 `--soft-delete`（显式 `Entity::not_deleted()` scope，非全局过滤）。`generate mailer` 生成实现 `Email` 的纯文本类型并注册邮件模块；`generate policy` 生成基于既有 `auth::Policy` 的资源 CRUD 授权包装，要求项目启用 `auth`。
- 其他命令转发为当前项目的 `cargo run --quiet -- <args>`，并保留子进程退出码。
- 默认模板快照覆盖 manifest、主入口、应用、路由、基础配置、测试与 `AGENTS.md`；双库模板单测覆盖 backend feature、migration crate、插件和 seed 文件。数据生成器测试覆盖 entity fields、UTC/JSON columns、references 和多迁移 registry 更新。
- `tests/application.rs` 通过读取生成的 `Cargo.toml` 并与 `CARGO_PKG_NAME` 比较验证包名，避免不同项目名长度改变 rustfmt 排版；PG18 与 MySQL 8.4 的新生成认证项目格式、check 与 HTTP 集成测试均通过。
- 数据库模板以稳定 `openrust_migration` dependency alias 引用独立 migration package，生成应用名长短不会改变 `app.rs` 格式。
- `openrust new` 支持 `--http-client`、`--sms-twilio`、`--encryption` 可选项，可与 `--db` / `--auth` 组合；`openrust key generate` 生成 32-byte OS 随机密钥并输出 Base64，不写配置文件。
- `openrust new --grpc` 生成可选 gRPC 应用：转发 `grpc` feature，创建应用自有 `proto/echo.proto`、vendored-protoc `build.rs`、typed proto module 和 `[grpc] listen_addr` 配置，并安装 Echo、`GrpcServiceProvider` 与 `HttpGrpcServiceProvider`。默认入口仍是 HTTP `serve`；配置 TLS 证书/私钥后可显式运行 `serve all`，以同一应用在两个端口提供 HTTP 与 typed Echo。默认模板不增加 gRPC 依赖或代码生成。
- `--auth` / `--fortify` 生成官方Web默认管线；`--profile jwt-api` 显式生成无Session/Cookie的JWT API与因子管理测试。`--grpc` 的真实二进制验收通过子进程部署环境配置HTTP/TLS，保留信任/拒绝与信号停机断言。当前四类实际生成证据见[组合验收](../memory/testing.md#fortify-官方默认与显式-jwt-组合验收)，旧`--auth-mfa`选项已移除。
- 生成源码采用内置模板文件与 Rust 字符串拼装，当前未使用 MiniJinja；生成器包含 Model、Migration、Mailer、Policy 和 Job。UUID 主键默认关闭，启用时应用和 migration package 均添加可选 `db-uuid` feature。

- C04-B2在生成的`tests/support/request_input.rs`共用公开Request/Fluent测试入口；普通`test_app`与认证`auth_flows`均调用，认证路径复用真实隔离DB配置和原清理流程。模板根路由从最终应用配置读取`app.name`，不以包名替代；`app.timezone`默认UTC。本簇最终普通与双库生成认证项目均实际通过check/tests，共用helper获独立复审；证据见testing，未以历史快照代替。

## 后续边界

- controller/scaffold/command 等生成器、只读 `doctor`、诊断和开发编排属于 M6 待补项。具体顺序以 v1 计划 与活跃待办为准。
- 出站 HTTP 客户端与生成 feature 的已有验收见 [测试基线](../memory/testing.md)；外部邮件/SMS 投递属于各通信 driver 的单独验收边界。

目录/分发以 历史决策 073 和 [architecture](../architecture.md) 为准；独立生成应用不修改源 workspace。根 package 为 openrust library；同一根 package 提供唯一默认 server bin。

后继 R13 补全命令/生成器、doctor、watch 和编排；当前 generate API 暂保留，未宣称 Artisan 全命令完成。

## 翻译来源生成

普通、MySQL、Web 认证、JWT 与 gRPC 生成应用均包含可编辑 `translation_sources` 配置、文件来源和同一 Provider 安装机制；数据库扩展显式启用，首次迁移与后续数据库/组合模式分开。JWT 不增 Session/Cookie。五种真实生成与适用来源安装验收见[来源测试基线](../memory/testing.md#翻译来源限定验收)，完整官方翻译接口仍开放。
