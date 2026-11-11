# 06-07 密码重置与邮箱验证

依赖 04-07 限流、05-08 认证、06-01 签名 URL、06-06 通知。对照固定 [Laravel Password Reset](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/passwords.md)、[Email Verification](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/verification.md)、[PasswordBroker](https://github.com/laravel/framework/blob/v13.40.0/src/Illuminate/Auth/Passwords/PasswordBroker.php) 和 [AuthPasswordBrokerTest](https://github.com/laravel/framework/blob/v13.40.0/tests/Auth/AuthPasswordBrokerTest.php)。用户能力依托当前 UserProvider，复核固定 Goravel Auth 装配及旧版验证码流程。沿用旧版账户更新与真实数据库用例的思路，继续使用 06-01 已验收的签名和密钥契约。

实现命名密码代理、数据库与缓存仓库、限流、回调、默认邮件和 Artisan 清理。保留 Laravel 翻译状态和响应时间规则。Rust 通过 trait 能力暴露可重置与可验证用户。存储先以原子更新或共享缓存锁预留令牌，成功后移除，回调失败恢复仍属同一请求的预留。请求中断时，预留由过期或再次发链回收。邮箱验证处理器显式接收从当前账号加载的可变模型。注册、发链和验证事件使用共享分派器。验证通知通过用户能力覆写，默认通知使用本地化文本链接。应用定义路由、页面和密码写入回调。

## 验收

- Rust 1.94.0 格式、锁定依赖和严格 Clippy 通过。串行运行 `cargo test --workspace --locked --offline -- --test-threads=1`，488 项通过，3 个子进程入口由集成测试启动。首次并行全量运行发现既有调度测试的临时目录名碰撞，详见本批进度。
- SQLite、MySQL 8.4.13 和 Redis 8.10.3 验证令牌哈希、过期、发链节流、并发预留、回调失败恢复及新版令牌保护。实际 `./artisan auth:clear-resets users` 只清除了过期记录。
- 真实 HTTP 与 Mailpit 验证重置与验证邮件链接、并发重放、登录账号不匹配、篡改、过期、重复验证单次事件、受保护路由的 302/403 和验证重发第七次返回 429。
- 三语 Rust 示例实际运行于同一 HTTP 验收入口。三语站点构建通过，12 项通用浏览器检查与 6 项密码重置到邮箱验证导航检查通过。

下一单元 06-08 Artisan 帮助、命令文档与本地化。依赖 00-03 与 03-10 已提交。
