# API-F01 邮件验证 HTTP

2026-11-20。本批接通正式邮件验证接口，定向／整包、严格检查和恢复后的根 HTTP／Pail／Artisan 均通过，本批核范围本地提交。API-F01 仍待密码重置、安装及三语整体验收。

## 来源与实现

固定 Fortify 1.43.0（`3db382a3d5d6a0d537de32a190ce3b945e621713`）的 VerifyEmailRequest、三个邮件验证控制器、响应契约、RedirectAsIntended 和 routes/routes.php。查阅本地 `v0.x`（`6d1a81924451e15f595f68c8dfb520ad6d6c44cf`）的 Fortify HTTP／api_management，旧版 authority 状态协议不适合当前 Laravel 模型／Session 契约，未复制。

- 正式 verification.notice／verification.verify／verification.send；auth、signed、默认 throttle:6,1 和可配置 limiter，功能／视图开关、前缀及嵌套 RoutePath 配置。签名由原生 URL／signed 中间件验证，不在 Fortify 重写。
- VerifyEmailRequest 使用原生 FormRequest，在控制器前按恒定时间字节比较模型主键和小写 SHA1 邮箱摘要；空规则接受额外输入。主键按字符串保留前导零，不使用认证标识代替。
- `registerEmailVerification::<T>()` 注册实现 Model + MustVerifyEmail + Clone 的编译模型；适配器 Clone 保留真实对象，保存后装回原生 Guard。注册表属于当前应用；未注册时明确失败。提示页和发送通知通过已有 MustVerifyEmail 能力执行，无需可变模型注册。
- 未验证时发送通知；已验证 JSON 204，Web intended。验证幂等，只有 markEmailAsVerified 返回 true 才发原生 Verified；保存或事件失败传播，保持已发生的数据库副作用。原生 Verified 载荷为 AuthIdentifier，沿用框架已有事件契约，没有宣称 PHP 用户对象载荷等价。
- VerifyEmailResponse JSON 204；Web intended 默认 email-verification 重定向加 ?verified=1。发送成功 JSON 202，Web back + verification-link-sent。三个响应契约和视图可由应用替换。
- 通知沿用原生用户方法，不接管投递渠道。原生默认 VerifyEmail URL 使用用户认证标识；认证标识与主键不同时，应用应通过已有 VerifyEmail::createUrlUsing 提供按主键签名的 URL。本批授权检查坚持主键语义，不声称原生默认通知已补齐任意模型主键反射。测试通知只计数，实际 SMTP／邮件投递不在范围内。

## 验收

独立 archive 基线 `5e7248f6`，证据 `/private/tmp/openrust-fortify-email-20261120/`；同期协作者改动保留，不代表 V17 最终主线组合验收。

- 四个新增实际 TCP／MySQL 8.4.13 函数：访客拒绝、视图、发送／重复发送、已验证响应；错误主键（001 与 1）、邮箱摘要、篡改签名和过期；重复验证只发一次事件，数据库 Session 登录恢复；Web intended／verified 参数和状态闪存；prefix／paths／views=false、真实 CSRF、限流、功能关闭；BEFORE UPDATE 失败、事件监听器失败，以及响应替换读取已更新 Guard 用户。
- 固定原 [PHP 请求／控制器／响应对照](fixtures/email_verification.php)：五组授权输入、19 组 JSON／Web、初始验证状态、mark 返回 false／抛错、intended 与通知场景。19 组结果与 Rust 实际状态、重定向路径、闪存、通知计数、数据库状态和事件逐项比较；四组非空授权参数经真实签名 HTTP 比较。空 id 仅原请求观察，不冒称已穿过原生必需路由参数。
- 对照 PHP 替代 Request／模型／Redirect／事件和通知边界，不执行完整 PHP HTTP、签名、SQL 或邮件渠道。十份固定源 SHA 门禁；PHP 8.5.12 固定镜像，输出 SHA256 `9dff4d727512af9289b2060149df78b57ef89c36378d97f84be3606ebfa4e4a8`；重复一致、源码改动负样本拒绝。
- 共用账户夹具抽至 tests/support/accounts.rs，原账户四函数重新通过；整包 Fortify 43 个不同函数及原生 Session／验证／事件 12 个函数回归。Rust 1.94.0，workspace／all-targets features pail,passport,fortify 严格 Clippy 和全仓格式通过，根 main 构建通过；运行实际 HTTP／Pail／恢复／关停／独立 Artisan 前，阶段边界磁盘读数 48,683,792 KiB 触发停线，因此本批根入口验收未执行，不能沿用上一批结果冒充通过。见 regressions.log。
- 68 份输入与独立源码快照一致，见 [SHA 清单](source-sha256-email-verification.json)。首次编译夹具导入／FormRequest 参数、配置路径和重定向边界断言失败均保留日志；修正后重新执行。过期夹具采用正数历史时间戳 1，保留原生／PHP 对 expires=0 的假值语义。
- 未运行 SQLite、其他数据库、SMTP 服务或发布矩阵；未改产品站点，三语正文及浏览器随 API-F01 全簇交付。

复现：在独立快照并指定专属 target，执行 `python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-fortify --tests`；固定对照运行 `python3 -B .agents/project/audits/api-f01/fixtures/prepare_email_verification.py /private/tmp/fortify-reference /private/tmp/email.json`。

## 磁盘停线与恢复

根构建后的阶段边界读数 48,683,792 KiB，即 49.852 GB／46.428 GiB，低于项目 48,828,125 KiB 停线；未开启 Pail 检查或新模块。核对专属 target 无进程占用后，保留构建完成的 main 到 `/private/tmp/openrust-fortify-email-20261120/retained-binary/debug/main`，其余专属编译产物直接清理；df 增加 5.976 GB，剩余 57,200,628 KiB，即 58.573 GB／54.551 GiB。Docker 数据卷前后一致；见 [清理记录](cleanup-email-verification.json)。源码、固定对照、失败／成功日志和全部共享资源保留。

按项目停线要求等待用户确认恢复；未暂存／提交本批。恢复后在独立 source 工作目录设置 `CARGO_TARGET_DIR=/private/tmp/openrust-fortify-email-20261120/retained-binary`，执行 `python3 -B tests/mysql_acceptance.py -- python3 -B tests/pail_auth_http.py`，复查输入 SHA 和工作区差异后收尾本地提交，再继续密码重置、安装与三语整体验收。该烟测无需重新构建；以后编译另设新专属 target。

2026-11-20 用户明确“确认恢复”。复查磁盘超过 50 GB；68 份输入 SHA 与原独立快照一致，复用保留二进制完成 MySQL 8.4.13 下实际 HTTP／Pail／恢复／超时清理／SIGINT／端口释放及独立 Artisan，见 root-smoke-resumed.log。无需重新构建；此前未执行烟测的记录保留为历史。核范围本地提交，不推送。
