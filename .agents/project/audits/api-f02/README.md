# API-F02 Fortify Passkeys 生命周期

2026-11-20。范围：挑战、注册、原生关系列表、删除、无密码登录及密码确认；对接固定官方 JS。补充模型定制和 AAGUID 展示不计入本批生命周期结论。最终命令结果在本文件验收节登记；失败记录保留。

## 固定来源与旧版检查

- Fortify 1.43.0 `3db382a3d5d6a0d537de32a190ce3b945e621713`，Framework 13.40.0 `1449bf241017087fbd38c118ecaabef3f6797fdc`，文档 `960eedf0911155ad16326ade49c0ea5d18eda0b8`。
- Fortify composer 的 `laravel/passkeys ^0.3.0` 对应 **laravel/passkeys-server**，本批固定 0.3.0 `2e7f893429d35acff45ac16d05ddcf883372a20e`。**laravel/passkeys** 则为 JS 仓库，固定 `@laravel/passkeys` 0.4.1 `c4ee7572993aadb40c630dedc349fe74cca0c89f`，按其原 package-lock 安装并独立打包。实际传递版本、archive／文件 SHA 见[上游清单](fixtures/upstream-manifest.json)。初次混淆两仓库的下载未作为正式 PHP 基线。
- Rust 密码学使用 `webauthn-rs-core =0.6.2`（MPL-2.0，源提交 `ed367d0316a946c86dd402404a9abbafbcc2165d`，声明 MSRV 1.89），公开包装限制安全策略。未改写该 crate 的源码。上游 MIT 许可随本包保留。97 个新增锁定包、无旧版本删除，见[锁增量](dependencies.json)。主线已有 RSA／签名依赖与新 crypto-glue 依赖的版本资格由 Cargo 正常消歧，没有覆盖旧包版本。
- 旧版 `v0.x` `6d1a81924451e15f595f68c8dfb520ad6d6c44cf`：查阅 `vendor/openrust/framework/src/auth/fortify/{credentials.rs,credentials/challenge.rs,authority.rs}` 及对应测试。旧实现为 authority／凭据与 SeaORM 框架，没有符合本次固定契约的 WebAuthn 实现；只提取挑战绑定、并发与账户归属原则，不迁入旧 API／表结构。

## 实现与语言适配

- 新增独立 `openrust-passkeys` crate，由 Fortify 依赖并通过 `openrust_fortify::passkeys` 导出。安装器发布原生迁移，静态接入 User 能力与应用 Provider；重复安装保留自定义源码，关闭 Cargo feature 后仍可构建。
- PHP `PasskeyUser` 契约与 Trait 默认方法合并为 Rust `PasskeyAuthenticatable`（`PasskeyUser` 的公开别名）；应用实例内显式 `useUserModel::<T>()` 保留真实 Guard 模型类型。用户句柄是表名和主键的 HMAC-SHA256 十六进制，共 64 字节；没有把用户 ID 改为 UUID。
- 高层 webauthn-rs 强制 UUID 句柄，因此使用 core 层。`new_unsafe_experts_only` 被封装在受限构造器内：只接受 HTTPS 或 HTTP localhost、RP 子域约束、精确 canonical Origin／端口、禁止 cross-origin，固定 ES256／RS256、UV required、resident required、attestation none。独立真实签名负例验证 Origin／端口、UV 和计数器，而不只修改签名后断言失败。
- 服务器注册／认证状态独立类型，可序列化进 Session，只向客户端返回 `options()`。Session 保存有效期、随机 challenge nonce；客户端不能提交服务器状态。先做请求形状校验，再消费状态；原子 cache `add` 排除并发载入同一个 Session 快照后的重放。服务端强制截止时间，并非只给浏览器一个 timeout 提示。
- 原生 Eloquent Passkey、HasMany／BelongsTo、JSON／日期 cast、外键级联与唯一 credential_id。注册在 fill 后写入服务器拥有的 user_id，保留上游 fillable。验证在事务内 `refreshForUpdate` 后验签和检查当前计数器、更新 last_used_at／备份标记再提交；任何失败回滚。可发现凭据登录要求并匹配已存用户句柄，确认操作另核账户所有者。
- 三个原生 Controller 与七条正式路径／路由名、Fortify 前缀／域名／Guard／中间件及命名限流。上游没有列表 GET 路由；应用通过用户原生关系实现列表。管理默认 `password.confirm`，可关闭确认开关；删除其他账户凭据 403，不存在 404。
- 登录复用原生 StatefulGuard、remember 与 Session／CSRF 轮换；确认复用 passwordConfirmed。可替换四个响应契约，Rust `toResponse` 显式借用可选 Passkey 代替 PHP 的可变 `withPasskey`。`authorizeLoginUsing` 的异步回调增加真实用户参数，应用内保存、锁只保护克隆，调用时不持锁。三个原生事件携用户标识、模型快照与真实借用上下文。
- 配置／回调／路由开关随应用实例保存，避免 PHP 静态配置在长生命周期 Rust 进程跨应用泄漏。Fortify 的配置映射优先读取其配置组，Provider 默认源读取 `PASSKEYS_USER_HANDLE_SECRET` 并回退 app.key。standalone Provider 可装配默认路由；本批不将独立资源发布列为已交付。

## 验收与可重现入口

独立批次 `/private/tmp/openrust-passkeys-20261120/`，最初 git archive 基线 `682cdfafb`；最终整合快照改用 `ad58593d250b6e987224fefdc9ea68930d27fe13` archive，保留同期 Socialite／OIDC 已提交实现，叠加本批核定输入。全部 Cargo 命令在该源码快照及独占 target 运行；Rust 1.94.0，`CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`。

- `browser-core-final.log`：`tests/passkeys_core.mjs` 驱动原生 `ceremony_driver`，真实 Chrome CTAP2 常驻／UV 认证器，64 字节句柄、none 注册、可发现登录、排除与允许列表、状态重用、challenge／origin／crossOrigin／RP hash／UV／userHandle／signature／type／rawId 错误；另有真实有效签名但不允许 Origin／端口、未 UV、计数器回退的拒绝。
- `contracts2.log`：本包 5 函数通过，含 48 组[固定原 PHP Trait 对照](../../../../vendor/openrust/passkeys/tests/fixtures/user-contract.json)。PHP 8.5.12 容器无网络执行原 Trait，只有 Config 和属性／表／主键读取为小型 stub；不把它当真实 Eloquent 或 PHP 密码学对照。生成程序[保留](fixtures/user_contract.php)。该批没有运行 PHP web-auth/webauthn-lib 验签，密码学证据来自固定 Rust core 和真实浏览器签名。
- `generated8.log`：`OPENRUST_TEST_PASSKEYS=1 python3 -B tests/mysql_acceptance.py -- python3 -B tests/fortify_install.py`（先构建来源二进制，并配置固定客户端 bundle／Chrome／Node 路径）。真实生成应用、默认 Provider／迁移、MySQL 8.4.13、原 API-F01 全流程和三封真实回环 SMTP；固定官方 JS 注册／确认／登录，原生关系列表，CSRF、确认门禁、跨账户拒绝、会话／CSRF 轮换、服务器过期、删除后登录拒绝、10／11 限流、跨 Session 签名拒绝。两独立 Session／挑战提交相同正计数器，MySQL 行锁保证 200／422；删除用户后 FK 级联清空。随后 rollback／migrate、生成应用严格 Clippy 和关闭 feature 构建通过。
- `provider-final5.log`：原生 Artisan route:list 核七条路由的自定义前缀／域名／Guard、确认开关、删除例外与命名限流，配置优先级和应用隔离通过。
- 三语正文同步 API-F02，保留 11 个二级章节；固定 JS 示例由真实生成应用执行，Rust 用户关系由原生路径编译。最终站点／根路径及 `/docs/` 结果在下方补记。

初期真实验收暴露的 user_id 批量赋值问题已修正；其他失败为测试夹具：不存在的 App facade、把 same-origin fetch 误作无 CSRF 负例、可选 feature 的测试路由缺少 cfg、Route facade 无 post 简写、额外负例占用真实凭据限流次数。夹具修正后完整通过，未放宽生产 CSRF／限流。前缀检查改为正式 TestApp／Artisan 入口，未暴露内部 Router。

## 范围边界

本机 macOS／Chrome CTAP2、MySQL 8.4.13；实测浏览器算法为 ES256。RS256 的 core 支持未等同本批拥有真实 RSA 硬件验收。没有真实硬件、生物识别、多设备云同步或 Windows／Linux Passkeys 浏览器矩阵。PHP credential JSON 不是 Rust 存储交换格式。多副本必须共享 Session 和支持原子 add 的缓存；本批 HTTP 用单进程 array 存储，不冒称 Passkeys 已跑过 Redis 双进程。原生事件走真实 dispatch，但本批没有独立监听器上下文断言。自定义响应与授权回调的容器／应用隔离有编译和定向测试，外部业务策略仍由应用负责。

自定义 Passkey 模型选择、AAGUID 名称／图标与独立包资源发布不在本批实现范围；Fortify 默认模型全生命周期与计划 API-F02 的接口验收独立记录。完整主线发布及历史 Windows 初始化补检等继续依交付计划执行，不把本批称作 OpenRust 全项目完成。未推送。


## 最终整合验收

- `native-final.log`：最终 ad58593d2 整合快照中验证器 33、Fortify 55、Passkeys 5，共 93 个不同函数通过；统一 MySQL 8.4.13 夹具。严格 `cargo +1.94.0 clippy --offline --workspace --all-targets --locked --features pail,passport,fortify -- -D warnings`、`cargo +1.94.0 fmt --all --check` 通过。包含同期 OIDC workspace crate。
- 同日志的根 main 三 feature 构建与 `tests/pail_auth_http.py` 通过：真实异常 HTTP、异步访客认证、Pail 输出、恢复、超时／Session 清理、SIGINT／端口释放及独立 Artisan。
- `docs-build-final.log`、`docs-browser-final.log`、`docs-fortify-final.log`：最终整合源码的三语站点构建、根路径／子路径三语桌面／移动 12 组合，以及 Fortify 内容／链接六组合通过。只保留既有大 chunk 构建提示。
- [37 份核定输入 SHA](source-sha256.json) 已逐字节核对 live／source 一致；Cargo.lock 无旧版本删除。新增包的本地 registry 许可和 MSRV 字段附在锁增量中；尚未下载的目标平台专用包不等同已完成该平台构建。
- `generated-final.log`：最终 ad58593d2 快照再次生成应用，完整 Chrome／官方 JS／MySQL／三封 SMTP、独立挑战计数器竞争与外键级联、迁移回滚重建、严格生成应用 Clippy 和关闭 feature 构建全部通过。早期基线的 generated8 结论由本次整合验收复核。
- 全部本批编译／应用／浏览器进程退出后，按归属直接删除独占 target、客户端 node_modules／验收 bundle 与站点 dist，详见[清理记录](cleanup.json)。df 可用增加 **9,946,848 KiB**；剩余 **78,020,644 KiB（79.893 GB／74.406 GiB）**。共享缓存、源码、生成应用、日志和所有数据卷保留；Docker 卷列表前后相同。
