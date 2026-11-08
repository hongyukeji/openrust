# JWT API 认证应用

本应用显式选择 `--profile jwt-api`，默认 `api` Guard 使用 `jwt-api` driver，复用唯一 Fortify Actions、Provider 与现代因子状态。框架的默认认证机制仍为 Web；本项目用公开 Provider 替换选择 API profile。

API 前缀由 `config/api.rs` 与 `config/fortify_api.rs` 配置，默认 `/api/auth`。本生成应用通过公开 `authenticate_using` 回调选择已验证邮箱业务策略，登录的 `account` 为邮箱；框架本身不强制账户拥有邮箱。登录提交 `account` 与 `password`；开启因子的账户返回短期一次挑战，随后向 `/two-factor-challenge` 提交 `challenge` 与 `code` 或 `recovery_code`。成功凭据为裸 JSON，不包装响应 envelope。

资源 `/user` 使用 `Authorization: Bearer <access_token>`。刷新与注销仅接收 JSON `refresh_token`，不放 URL。管理先向 `/two-factor/authorize` 提交真实 Bearer 与密码取得短期 `management_token`，再使用 `X-Fortify-Management` 请求管理路由。管理令牌不能访问资源。确认后管理授权失效，需重新登录；因子状态变更会使旧代际资源凭据失效。

本模式的认证、挑战与管理请求不使用 Session、Cookie 或 CSRF。不要将管理令牌或密码写入日志。部署前配置 `.env` 的数据库、认证和加密密钥，然后运行标准迁移。

```bash
./artisan list
./artisan help migrate
./artisan migrate
```

源码开发用 `cargo run` 启动应用。发布时将 Cargo 按包名构建的二进制安装为 `app`，并将可执行的 `artisan` 放在同一目录。用 `./app` 启动常驻进程，用 `./artisan` 执行应用命令。直接运行 `./artisan` 默认列出命令。

`tests/auth_flows.rs` 验证真实 API 管理、独立 RFC TOTP、恢复码、凭据轮换和逐响应无 Cookie；`tests/jwt_api_business.rs` 保留注册、邮件验证、密码重置、受管 PAT、刷新重放及注销回归。数据库测试必须真实运行，缺少网络数据库测试 URL 视为错误。
