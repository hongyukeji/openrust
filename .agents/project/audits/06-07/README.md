# 06-07 密码重置与邮箱验证

依赖 04-07 限流、05-08 认证、06-01 签名 URL、06-06 通知。对照固定 [Laravel Password Reset](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/passwords.md)、[Email Verification](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/verification.md)、[PasswordBroker](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Passwords/PasswordBroker.php) 和 [AuthPasswordBrokerTest](https://github.com/laravel/framework/blob/v13.40.0/tests/Auth/AuthPasswordBrokerTest.php)。用户能力依托当前 UserProvider，复核固定 Goravel Auth 装配及旧版验证码流程。沿用旧版账户更新与真实数据库用例的思路，继续使用 06-01 已验收的签名和密钥契约。

实现命名密码代理、数据库与缓存仓库、限流、回调、默认邮件和 Artisan 清理。保留 Laravel 翻译状态和响应时间规则。Rust 通过 trait 能力暴露可重置与可验证用户。存储先以原子更新或共享缓存锁预留令牌，成功后移除，回调失败恢复仍属同一请求的预留。请求中断时，预留由过期或再次发链回收。邮箱验证处理器显式接收从当前账号加载的可变模型。注册、发链和验证事件使用共享分派器。验证通知通过用户能力覆写，默认通知使用本地化文本链接。应用定义路由、页面和密码写入回调。

## 验收

- Rust 1.94.0 格式、锁定依赖和严格 Clippy 通过。串行运行 `cargo test --workspace --locked --offline -- --test-threads=1`，488 项通过，3 个子进程入口由集成测试启动。首次并行全量运行发现既有调度测试的临时目录名碰撞，详见本批进度。
- SQLite、MySQL 8.4.13 和 Redis 8.10.3 验证令牌哈希、过期、发链节流、并发预留、回调失败恢复及新版令牌保护。实际 `./artisan auth:clear-resets users` 只清除了过期记录。
- 真实 HTTP 与 Mailpit 验证重置与验证邮件链接、并发重放、登录账号不匹配、篡改、过期、重复验证单次事件、受保护路由的 302/403 和验证重发第七次返回 429。
- 三语 Rust 示例实际运行于同一 HTTP 验收入口。三语站点构建通过，12 项通用浏览器检查与 6 项密码重置到邮箱验证导航检查通过。

下一单元 06-08 Artisan 帮助、命令文档与本地化。依赖 00-03 与 03-10 已提交。

## 密码重置与邮箱验证的默认邮件 {#default-auth-notification-mail}

- 对照固定 Laravel v13.40.0 的 `ResetPassword` 与 `VerifyEmail`。旧版 `openrust-v0.x` 的认证实现使用用途绑定的一次性验证码，当前主线复用已验收的签名 URL、翻译与 06-06 的 `MailMessage`。默认模板渲染 HTML 按钮和纯文本链接。重置邮件采用固定版主题，并按 `auth.defaults.passwords` 和选中代理的 `expire` 翻译有效期提示。邮箱验证保留独立有效期和签名。 [固定重置邮件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Notifications/ResetPassword.php#L69) · [固定验证邮件](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Notifications/VerifyEmail.php#L57)
- 隔离基线 `08f7d7b8a` 上，数组邮件传输与真实 Mailpit SMTP 的两项 HTTP 链路验证双 MIME 正文、重置邮件显示配置的 37 分钟、验证邮件保留动作。使用邮件中的链接复验并发重放、登录用户不匹配、过期、篡改、事件去重及重发限流。
- SQLite、MySQL 8.4.13、Redis 三项令牌仓储回归通过。独立 SQLite 数据库执行 `./artisan migrate --force` 和 `./artisan auth:clear-resets users` 成功。Rust 1.94.0 全仓格式、严格 Clippy、三语文档构建及根路径和 `/docs/` 的 18 项浏览器检查通过。核心账本维持 66/66。下一批核对认证邮件的应用内 URL 和邮件构建回调。
- 本批隔离编译和站点产物已回收，数据卷清单一致。清理前后项目可用空间净增 3.87 GB（3.61 GiB），清理后可用 89.43 GB（83.29 GiB）。提交前周额度剩余 28%，第二窗口无读数。日志保留于 `/private/tmp/openrust-password-notifications.ipg9EhSx`。

## 认证邮件应用级链接与内容回调 {#auth-notification-mail-hooks}

- 对照固定 Laravel v13.40.0 的 `ResetPassword` 和 `VerifyEmail`。旧版验证码路径没有这两组回调，复用主线认证能力 trait、服务容器和通知邮件类型。Rust 的 `createUrlUsing` 与 `toMailUsing` 在应用容器内注册，由通知邮件通道求值。邮件回调可返回 `MailMessage` 或完整 `Mailable`。 [Laravel 重置通知](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Notifications/ResetPassword.php#L55) · [Laravel 验证通知](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Auth/Notifications/VerifyEmail.php#L42)
- 重置通知沿用上游的邮件回调优先级，回调接收用户和令牌。默认用户通过 `sendTo` 传入完整认证能力。Rust 的 `send(email)` 保留直接使用默认邮件的入口。邮箱验证通知先解析链接，再将用户与链接传给邮件回调。自定义链接沿用应用路由和签名策略。
- 隔离基线 `44607ac0d` 上启动三个独立应用，用真实 HTTP 和 Mailpit SMTP 验证 URL 覆写、邮件回调优先级、邮件消息与完整 Mailable、按签名处理的自定义验证链接和应用间隔离。重置和验证均从实际收到的链接回到应用请求。保留原默认流程上的重放、过期、篡改、事件去重和重发限流验收。
- SQLite、MySQL 8.4.13、Redis 8.10.3 三项令牌仓储回归、独立 `./artisan migrate --force` 与 `./artisan auth:clear-resets users`、Rust 1.94.0 格式、严格 Clippy、三语示例注册和站点构建、两种部署路径的 18 项浏览器检查通过。核心账本维持 66/66。下一批核对更换邮箱后旧验证链接和并发验证的边界。
- 本批隔离编译和站点产物已回收，数据卷清单一致。清理前后项目可用空间净增 4.14 GB（3.86 GiB），清理后可用 89.39 GB（83.25 GiB）。提交前周额度剩余 28%，第二窗口无读数。日志保留于 `/private/tmp/openrust-auth-mail-hooks.Yxuhhkt2`。

## 换址与并发邮箱验证 {#verification-after-email-change}

- 固定 Laravel 根据当前用户 ID 与当前邮箱摘要授权。旧版 `openrust-v0.x` 的验证码流程也使用仅在未验证时更新的数据库条件。本批保留主线的签名摘要和原子状态更新，新增 `whereExactText` 匹配当前邮箱。数据库排序规则将两个地址视为相同也不会把旧地址的验证结果写给新地址。应用换址时在同一次更新中清除验证时间。 [固定 Laravel 请求类](https://github.com/laravel/framework/blob/1449bf241017087fbd38c118ecaabef3f6797fdc/src/Illuminate/Foundation/Auth/EmailVerificationRequest.php#L15)
- 隔离基线 `2f9d5dec4` 的 MySQL 8.4.13 回归复现旧模型快照误标记了只有大小写差异的新邮箱。修复后，SQLite 与 MySQL 的旧快照更新均影响零行，旧签名链接返回 403。通过真实 HTTP 请求获取新链接，同时提交两次，新链接均返回 204，`Verified` 只触发一次。重复验证、邮箱不匹配、篡改与过期链接仍覆盖在本批 HTTP 回归内。
- 四项 HTTP 回归含 Mailpit SMTP，三项 SQLite／MySQL 8.4.13／Redis 8.10.3 令牌测试及 SQL 编译与参数绑定回归通过。MariaDB、PostgreSQL 与 SQL Server 仅核对 SQL 生成和绑定。本批执行 `./artisan migrate --force`、`./artisan auth:clear-resets users`、Rust 1.94.0 格式及严格 Clippy、三语站点构建及双路径 18 项浏览器检查。核心账本维持 66/66。下一批对照 06-08 Artisan 帮助和命令本地化契约。
- 本批隔离编译和站点产物已回收，数据卷清单一致。清理前后项目卷可用空间净增 3.94 GB（3.67 GiB），清理后可用 89.30 GB（83.17 GiB）。提交前周额度剩余 28%，第二窗口无读数。日志保留于 `/private/tmp/openrust-email-verify-race.dQmRRrPi`。
