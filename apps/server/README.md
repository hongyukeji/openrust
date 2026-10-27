# server 集成宿主

`server` 是 workspace 内不发布的集成宿主，正式依赖只有根 `openrust` package。它验证框架门面、启动流程、数据库 feature 转发和进程行为，不承载业务资源。

从应用目录启动可让配置按当前应用定位：

```bash
cd apps/server
cargo run -p server
cargo run -p server -- config show
cargo run -p server -- help
cargo test -p server
```

`Server::build` 默认安装 `HttpPlugin`、`I18nPlugin`、`MailerPlugin`、`SmsPlugin`；启用 `encryption` 时增加 `EncryptionPlugin`。默认服务提供 `/` 与 `/health`，不依赖外部服务。`config/default.toml` 和环境配置位于 `apps/server/config/`。应用默认从当前工作目录的 `config/` 读取文件；`APP_CONFIG_DIR` 可显式指定其他目录，`APP_ENV` 选择运行环境。环境变量覆盖文件值，`APP_SERVER__PORT=4000 cargo run -p server` 可修改端口，`PORT=4000` 是通用简写；若同时设置，应用前缀变量优先。`config show` 会遮蔽疑似密钥。

启用 `auth` 只开放编译能力，不会自动把认证插件加入默认 server。可运行门面认证集成测试：

```bash
cargo test -p server --features auth --test auth
```

需要 `AuthPlugin::new()` 的应用须先注册 `DatabasePlugin`，并把 `AuthMigration` 加入现有 SeaORM migrator。自定义 `Authenticator` 不要求数据库。认证集成测试也验证不注册 `SessionPlugin` 时的 JWT 路径，使用测试密钥配置，不会写入仓库配置。

API 响应使用 `ApiResponse<T>` JSON envelope；健康状态位于 `data.status`。`[api] http_status` 默认 `always_ok`，可用 `APP_API__HTTP_STATUS=semantic` 启用语义状态码。响应正文的 `request_id` 与 `x-request-id` 一致。

`[i18n]` 中的 `default_locale` 与 `fallback_locale` 设置默认语言和回退。请求可传 `Accept-Language: zh-CN`；应用可通过 `I18nPlugin` 注册 JSON catalog，并在处理器中注入 `RequestLocale`。普通 `{name}` 文本保持兼容；计数消息使用带 `plural` 的对象，有限选项使用 `select` 对象，两者都要求 `other` 回退分支。应用可用 `I18n::format_number(&locale, "1234.50")` 格式化精确小数；格式化日期时间前，调用方应先把 Chrono 值转换到明确的业务时区。

默认邮件与短信驱动只记录经过脱敏的日志内容；生产环境拒绝使用日志驱动。`sms-twilio` feature 启用 Twilio adapter；SMTP 本机 Mailpit 验收步骤见 `src/mail/README.md`。启用 `encryption` 可注册 `EncryptionPlugin`；密钥应通过受保护的环境变量或 secret store 提供，不要提交真实密钥。

启用 `storage-local` 会由 `Server::build` 安装 `StoragePlugin` 并使用本机 filesystem disk；启用 `storage-s3` 会同时启用 `storage-local` 与 S3-compatible driver。对象存储配置、流式 API 和授权边界见 [文件存储指南](../../src/storage/README.md)。

`grpc` feature 将 gRPC 运行时转发到 server 集成宿主；应用仍需显式注册服务和 `GrpcPlugin`，不会在启用 feature 时自动开放端口。服务端默认要求 TLS；本机开发或可信 TLS 终止代理必须显式设置 `allow_insecure_server = true`。插件组装、明文拒绝和 TCP 生命周期通过 `cargo test -p server --features grpc --test grpc` 验证；四类 Protobuf 网络调用与 TLS/mTLS 见 [`examples/grpc/`](../../examples/grpc/README.md)。

数据库、认证、队列与调度能力由各集成测试显式组装。开启 Cargo feature 不会让默认 `Server::build` 自动注册这些服务。详见 [ADR-007](../../.agents/project/decisions/adr-007-aggregate-server-entry.md)。
