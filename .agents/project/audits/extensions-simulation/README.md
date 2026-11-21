# 扩展模拟生产环境验收

2026-11-21，按用户要求重新执行当前扩展入口，并补齐支付入站回调 HTTP 故障恢复。源码归档为 `244842943baeb03214467bef70a5c9aa1b59aec4`，叠加本主题的两个 `callback_http.rs` 测试及 Passkeys 验收夹具修正；不复制活跃工作区 SMTP、activitylog 等其他任务的改动。隔离源码、生成应用和日志位于 `/private/tmp/openrust-passkeys-simulation-20261121/`，独占编译目录 `/private/tmp/openrust-extension-simulation-20261121/target`，Rust 1.94.0、jobs=2、增量关闭、dev/test debug=0。

## 来源及验收方法

复用项目既有本地供应商服务器、原生 HTTP 客户端、数据库容器入口、Chrome CDP 虚拟认证器及固定 Laravel Passkeys 客户端，不引入新运行时或生产抽象。原有 Socialite／OIDC／Pay／Cashier 的固定官方源码参考及差异继续见各自主题记录。

本次联网核对 [Laravel Socialite AbstractProvider](https://github.com/laravel/socialite/blob/5.x/src/Two/AbstractProvider.php)、[W3C WebAuthn](https://www.w3.org/TR/webauthn-3/)、[Stripe Webhook](https://docs.stripe.com/webhooks)、[Paddle Webhook 测试](https://developer.paddle.com/webhooks/test-webhooks)。搜索工具没有返回条目，实际通过官方 URL 获取正文，副本及下载结果保留在临时 `references.json`。采用真实浏览器签名和原始 HTTP 字节验证；支付原始回调只在事务提交后应答，失败时由投递方重试，避免把直接调用验签函数误计为完整入站验收。

## 本次实际覆盖

| 范围 | 已执行的模拟环境与行为 | 外部生产边界 |
| --- | --- | --- |
| Socialite | GitHub、Google、Facebook、X/Twitter OAuth2、Twitter OAuth1、Bitbucket、GitLab、LinkedIn、LinkedIn OIDC、Slack、Slack OIDC、微信网站/公众号、QQ、小程序；本地 TCP 供应商、会话 state/PKCE/签名与错误矩阵；MySQL 本地身份绑定、真实 HTTP 登录与 Session 轮换/隔离 | 未登录真实供应商账户，未验证真实应用审核、权限、网络、设备端 code 或用户数据 |
| OIDC RP/SSO | 本地 OP 与两个独立 RP，真实 HTTP code/PKCE、三种客户端认证、密钥轮换和故障；MySQL 共享 Session 双副本单次认领；Chrome 双 RP SSO、强制再认证、CSRF、未知身份拒绝、登出与访问令牌边界、并发登出回调 | OP 为协议模拟器；未接企业 IdP、跨站部署、真实租户或外部密钥轮换服务 |
| Passkeys | Chrome 虚拟 CTAP2 认证器；ES256 与 RS256；原生注册/断言、UV、身份、挑战、来源、计数器回退与重放负例；生成应用的完整浏览器结果见下文 | 未使用物理安全密钥、Touch ID、跨设备同步或手机混合传输 |
| 微信/支付宝 | 本地 TCP 支付网关、请求与响应 RSA/AEAD；下单、查单、关单、部分退款/退款查询、真实超时只发送一次；新增入站原始回调篡改400、业务失败500回滚、重试提交、重复原生 ACK，三库事务恢复 | 无真实商户、真实二维码/App/JSAPI 调起、公开回调或资金收付 |
| Stripe/Paddle | 本地供应商 HTTP Checkout/门户/订阅生命周期、付款方式/账单/退款/分页、归属与敏感信息保护、超时与未知结果；新增入站 Webhook 错误回滚、重试、重复投递及迟到通知查当前状态，三库并发/取消恢复 | 无真实 Checkout/门户页面、Stripe.js/Paddle.js、SCA 挑战、真实产品目录/支付账户或扣款 |

`cargo +1.94.0 test --locked --offline -p openrust-socialite -p openrust-pay -p openrust-cashier -p openrust-passkeys --all-targets` 最终退出 0，日志 `package-final2.log`。Socialite 无 URL 时 `local_login` 会返回，故该次不计为数据库登录通过；已另用新的 MySQL 8.4.13 容器真实执行该函数，`socialite-mysql-http.log` 退出 0。Cashier 的官方签名函数常规为 ignored，已在禁网只读 PHP 8.5 容器调用固定官方 Stripe/Paddle SDK 生成当前时间签名后单独运行，通过 `cashier-official-reference.log`；不把 ignored 计作常规已验。微信官方 AESGCM 与支付宝 RSA2 也重新生成样本，与已有固定样本解析后相等。

新增 Pay 与 Cashier 两个入站 TCP 函数分别在 SQLite、PostgreSQL 18.6（180006）、MySQL 8.4.13 通过；后两库同时重新执行既有持久化函数，覆盖业务事务回滚、future 取消后恢复、八并发只处理一次、内容冲突及 Cashier 乱序与客户删除。命令为正式 `tests/postgres_acceptance.py`／`tests/mysql_acceptance.py` 包装器下运行已编译的精确测试二进制，日志 `pay-cashier-postgres.log`／`pay-cashier-mysql.log`，每库四函数退出 0。SQLite 的四函数在 `package-final2.log`，数据库运行版本见相应日志。

OIDC 在新的 MySQL 8.4.13 容器下依次运行 `flow`、`sso` 两个二进制，2+1 函数实际通过，日志 `oidc-mysql-browser.log`，包括完整 Chrome 输出。不将该结果扩展为 PostgreSQL 的 OIDC Session 专项。

浏览器核心命令为 `OPENRUST_TEST_PASSKEY_ALGORITHM=-7|-257 node tests/passkeys_core.mjs <ceremony_driver>`，分别退出 0，日志 `passkeys-core--7.log`／`passkeys-core--257.log`。客户端固定 `@laravel/passkeys` 0.4.1，沿既有锁离线 `npm ci --ignore-scripts`，Vite 8.3.6 重新 bundle（`client-install.log`／`client-build.log`）。

## 初轮问题及收尾

新回调夹具首轮捕获 `Arc<AtomicBool>` 不符合可重试 `Fn` 闭包契约，已在每次调用内部 clone；支付宝 HTTP 初轮错误覆盖表单 Content-Type 导致400，已保留原始表单类型并重跑通过。均为本次测试接线问题，没有放宽产品校验。原始失败日志 `package-tests-with-callbacks.log`／`package-final.log` 保留。

Passkeys 生成应用本次以 `OPENRUST_TEST_PASSKEYS=1` 启动正式数据库 wrapper 下的 `python3 -B tests/fortify_install.py`：PostgreSQL18 默认模型退出0（`passkeys-postgres-http.log`），MySQL8.4 自定义模型重跑退出0（`passkeys-mysql-retry-http.log`）。两次均包含完整 Fortify 原生 HTTP/SMTP/CSRF/2FA/reset/限流/关停，以及固定官方 JS 的注册、确认、可发现登录、Session/用户隔离、重放、期限、删除、计数器并发、级联删除，随后生成应用严格 Clippy 与 feature-off 构建通过。MySQL另外实际检查自定义表/主键/mutator/hidden/appends/动态关系/观察者和类型事件。PG初次浏览器日志的两句历史固定文案写着MySQL，实际数据库由wrapper版本和Python最终pgsql结果证明；本次已将JS输出修为数据库中性表述，不改断言。

MySQL自定义模型首轮失败原因是旧夹具替换 `table.id();`，迁移追加注释链后没有命中，生成表仍用id而模型声明pk。修正为精确替换`.id()`为`.idWith("pk")`并断言恰有一个主键声明，重新生成后完整通过；不降低模型行为断言、不改产品代码。`passkeys-mysql-http.log`／`generated-mysql-fixture-failed`／`mysql-fixture-failed-http.log`保留初轮失败，`passkeys-retry-results.json`明确记录重跑退出0，不覆盖首轮结果。

本次仅修改两个扩展测试文件和两处根验收夹具，未更改扩展生产代码或依赖。范围内 Rust 格式、Python AST语法、Node语法和差异空白检查通过；生成应用严格Clippy覆盖实际应用，不能据此声称新增测试目标的Clippy已单独执行，整仓最终严格检查由核心统一完成。

所有本批Cargo/浏览器/临时数据库进程退出且专属target无人占用后直接删除专属target、本次client/node_modules与acceptance-dist，保留源码/生成应用/失败证据/日志/官方参考及共享缓存。阶段先删除31个已验且无人运行的Mach-O测试二进制，可用33,297,336→36,365,116KiB；最终清理可用33,108,064→39,134,092KiB，净回收6,026,028KiB（并行磁盘活动影响净值）。清理前后同一个匿名Docker数据卷完整保留，删除0卷；清理清单`cleanup-final.json`，日志校验及两库结果`validation-final.json`。
