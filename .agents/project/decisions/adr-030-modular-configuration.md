# ADR-030：按能力模块组织配置

- 状态：已采纳，已实现并验证
- 日期：2026-10-26
- 背景：`src/config/mod.rs` 集中放置环境解析、Figment source、框架配置结构与测试，已不利于按配置域定位；数据库、认证和邮件设置又分散在各能力模块。
- 决策：框架配置定义按真实配置域拆到 `src/config/*.rs`，`mod.rs` 保留环境引导、source 合并、通用错误和公开 facade。当前具备的 app、api、auth、database、logger、mailer、server 各有配置模块；未实现能力不创建空配置文件。
- 兼容：已有 `openrust::auth::{JwtConfig, PasswordConfig, SessionConfig}`、`openrust::database::DatabaseConfig`、`openrust::mail::MailConfig` 路径通过 re-export 保留；新增集中路径为 `openrust::config::<domain>::*`。
- 运行配置：生成项目的根 `config/default.toml` 与环境 TOML 保持不变，继续由 Figment 统一分层提取为强类型结构；`src/config/` 只保存 Rust 源码。
- 环境来源：遵循 ADR-016，默认 `APP_` namespace；已存在的 `.env` 仅用于非 production 引导/本地覆盖，外部进程环境优先，`.env.example` 提供安全占位说明。production 不读取 `.env`。
- 取舍：借鉴 Loco 按能力拆分 typed config 结构、Laravel 提供 `.env.example` 并由外部变量覆盖的清晰职责；不引入 PHP facade、任意字符串全局 config accessor、Laravel config cache、环境专属 `.env` 变体或第二套解析器。
- 验收：旧公共路径编译、按模块解构与统一 Figment 加载结果一致；覆盖环境选择、层级优先级、secret 脱敏、production `.env` 隔离，并对默认及 `--db/--auth` 生成模板运行测试。配置功能簇的实际命令结果记录于提交说明和 `memory/verification.md`。
