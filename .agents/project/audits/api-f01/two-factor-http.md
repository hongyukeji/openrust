# API-F01 双因素管理与登录挑战

本批接通 Fortify 正式双因素 HTTP 路由、原生 SessionGuard 登录管线、显式 Eloquent 模型装配及本地 QR SVG。此前账户动作和加密恢复码见 [账户动作验收](two-factor-model.md)。API-F01 尚缺注册、密码重置、邮件验证、资料／密码更新和安装；三语产品正文仍为空白占位。

## 固定来源与复用

- 固定 Fortify 1.43.0，`3db382a3d5d6a0d537de32a190ce3b945e621713`。逐项核对 Actions/RedirectIfTwoFactorAuthenticatable、TwoFactorLoginRequest、五个管理控制器、TwoFactorAuthenticatedSessionController、响应和 routes/routes.php。
- 本地旧版 `v0.x`（`6d1a81924451e15f595f68c8dfb520ad6d6c44cf`）的 `vendor/openrust/framework/src/auth/fortify/qr.rs` 直接复用矩阵／SVG 渲染；阅读 login.rs 与 credential_challenge 测试，按固定上游保留 Session 消费顺序，没有引入旧版 authority／CAS 存储协议。
- QR 使用固定 `qrcode = 0.14.1`，关闭默认 features，无新增运行时依赖树；许可 MIT OR Apache-2.0，依赖声明最低 Rust 1.67.1，本项目仍用 Rust 1.94.0 验收。
- Bacon/BaconQrCode v3.0.1 `src/Writer.php` SHA256 `b3f968284dafcb6b010287028eacca7f4e39a7c0d2e7852309596fa9121fbeb0`，确认省略参数时使用纠错级别 L。原 Fortify 指定 192px、零外边距、白底及 RGB(45,55,72)。Rust 保留这些可观察参数；二维码库的分段、mask 和 SVG 路径字节可不同，以独立解码验证同一完整 URI，不声称输出逐字节相同。

## 接口与机制

- 新增 `Fortify::registerTwoFactorAuthenticatable::<User>()`，要求 `User: TwoFactorAuthenticatable + Clone`。这是 Rust 对 PHP 动态 trait／类名查找的显式适配；用户仍按原生 AuthManager 注册模型名。注册表属于单个 Fortify 应用，没有进程级模型表。Clone 保留用户对象自身状态；管理后用原生 `Guard::set_user` 安装保存后的对象，不额外发 Authenticated 事件。
- 原生 UserProvider 新增可选 `getModel() -> Option<TypeId>`，EloquentUserProvider 返回实际编译模型；SessionGuard 暴露 `getProvider()`。挑战恢复通过该类型按模型**主键**查询，独立于认证标识列。未使用 trait 的用户按上游继续普通密码登录，管理接口在未注册模型时明确失败。
- 默认流水线的 RedirectIfTwoFactorAuthenticatable 采用原生 provider 及 200ms Timebox，密码验证／rehash 只传密码 credentials；回调路径保留自身时序。未发生挑战时仍执行后续 AttemptToAuthenticate，不合并上游两次认证。失败沿用原生 Failed 事件、候选用户和借用 credentials、Fortify 失败限流。
- 挑战写入 `login.id`（模型主键）与 `login.remember`，发送 TwoFactorAuthenticationChallenged；JSON 200 `{"two_factor":true}`，Web 跳转命名路由。`RedirectsIfTwoFactorAuthenticatable` 提供对象安全容器替换；泛型账户动作按 transient 绑定，响应契约和视图可替换。
- TwoFactorLoginRequest 使用原生 FormRequest 验证 `nullable|string`；控制器在单次请求内持有挑战账户，避免重复查询，并承接上游 request 的用户／消费职责。恢复码先于 OTP，按恒定时间字节比较；合法后先移除 login.id，再替换并保存。OTP 合法也先移除 login.id。验证失败发失败事件并保留挑战；成功发 ValidTwoFactorAuthenticationCodeProvided，pull remember，经原生 guard.login 和 Session regenerate 完成登录。JSON 204，Web intended；默认失败响应按 filled(recovery_code) 选择字段，JSON 422／Web 错误包。
- 正式管理路由包括启用、确认、禁用、二维码、secret key、读取／重建恢复码；匹配固定 RoutePath 键、命名路由、guest／auth guard 及可选 password.confirm／命名两因素 limiter。Features 选项仍要求严格 bool true。解除 Provider 对已实现 two-factor-authentication 的启动拒绝；其余未实现默认 feature 仍明确拒绝，不将部分安装称为默认完整安装。
- QR SVG／URL、secret key 和恢复码只通过受保护管理端点输出；SVG 不把 URI／密钥插入 XML 文本。secret 不存在时 QR／恢复码为 []，secret-key 为带上游消息的 404。保持加密 buffer 清零及原 PHP 非原子的恢复码替换机制，不宣称并发 CAS 消费。

## 验收证据

独立 `git archive` 基线 `526723cac`，覆盖本批文件，证据目录 `/private/tmp/openrust-fortify-challenge-20261120/`。同期 Passport 后续提交没有叠加进此快照，最终主线组合仍留 V17 复验。

- `two_factor_routes` 六个真实 TCP／MySQL 8.4.13 函数：正式路由全流程；认证标识 login-001 与主键 001；默认确认选项、可选未确认模式、confirmPassword、前缀与嵌套路由路径配置；回调调用次数和默认五次锁定；命名两因素 limiter、真实 CSRF 拒绝、容器响应及挑战阶段替换；remember Cookie 恢复、Session／CSRF 轮换；恢复码替换／旧码拒绝／重新生成／幂等禁用。
- 损坏密文保持 challenge；MySQL BEFORE UPDATE 触发器拒绝恢复码保存时，challenge 已消费、未登录且未派发替换成功事件；读取属性／解密失败时尚未消费。账户删除后 GET 返回登录跳转，POST 返回失败，失败不伪造认证事件。DDL trigger 使用原生 unprepared，避免 MySQL 预处理限制。
- [固定 PHP 对照](fixtures/two_factor_challenge.php)直接执行原 TwoFactorLoginRequest 与 TwoFactorAuthenticatedSessionController，20 组请求逐项比较状态、错误字段、Session challenge／remember 消费和事件顺序。PHP 8.5.12 固定镜像，SHA 门禁；只替代 Session／模型查询／加密／provider／response／事件边界，不声称 PHP 全栈 HTTP 或真实密码学验收。实际密码学和 MySQL／Session 来自 Rust 正式实现。对照 SHA256 `451369183f72ef7a9a12aca584f8bfc0fa605c454f76f2f4bc070795028e2727`，重复生成字节一致，修改原源码的负样本被拒绝。
- 正式 QR 管理响应导出 SVG，经系统 Quick Look 真正栅格化，再由独立 Apple Vision QR 解码，所得完整 URI 与正式响应一致。公开合成账户／RFC secret，证据 `qr/` 与 `decode.swift`；未声称 Google Authenticator 人工导入或浏览器验收。
- 整包原有 29 个函数及新增 6 个函数，合计 35 个不同 Fortify 函数通过；原生 Session／验证／事件 12 个回归函数通过。完整日志 `regressions.log`，最终受影响测试复验 `final-checks.log`。
- Rust 1.94.0，全 workspace／all-targets、features `pail,passport,fortify` 严格 Clippy `-D warnings`，全仓 fmt；根 main 真实 HTTP→awaited auth→Pail、恢复 200、超时清理、SIGINT 退出 0、端口释放及独立 Artisan inspire 通过。
- 本批未改产品文档站，三语正文和浏览器门禁随 API-F01 完整交付一起验收。没有运行 SQLite／其他数据库或完整发布矩阵。

复现 PHP 对照：

```sh
python3 -B .agents/project/audits/api-f01/fixtures/prepare_two_factor_challenge.py /private/tmp/fortify-reference /private/tmp/challenge.json
```

在覆盖本批文件的独立快照中，指定专属 target，再运行：

```sh
python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --offline --locked -p openrust-fortify --tests
cargo +1.94.0 clippy --offline --workspace --all-targets --locked --features pail,passport,fortify -- -D warnings
cargo +1.94.0 fmt --all --check
```

## 收尾

55 份编译／测试／固定对照输入与独立快照 SHA 一致，见 [输入清单](source-sha256-two-factor-http.json)。确认专属 target 闲置后直接删除，逻辑 6.201 GB，df 实际增加 5.944 GB；剩余 Available 63,891,000 KiB，即 65.424 GB／60.931 GiB。前后 Docker 数据卷清单一致（均为空）；保留源码／证据／共享缓存／镜像，临时 MySQL 标签容器已由入口回收。详见 [清理记录](cleanup-two-factor-http.json)。本批核范围本地提交，不推送。
