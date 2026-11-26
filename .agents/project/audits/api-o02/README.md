# API-O02 双应用浏览器 SSO 与 RP 发起退出

2026-11-20。本机计划范围的骨架、实现和验收完成，接续 [OIDC RP](../api-o01/README.md) 和 [Fortify](../api-f01/install.md)。不提供自建生产 OP，不把 OAuth2 token 共享称作 SSO，不增加跨应用退出通知协议。

## 固定契约、恢复来源与旧版

- Core 1.0 errata set 2、Discovery 1.0 沿用 API-O01 的固定原文；本批固定 OpenID Connect RP-Initiated Logout 1.0，2022-09-12。官方原文 SHA `246432dafaa5556d896fa9b9ae4284b213ff6921b33478ed3b46bd6bd4aab804`，与 API-O01 已保存的 logout 引用一致，见 [来源](references.json)。
- 检查旧 `v0.x` 的 `vendor/openrust-framework/src/auth/oidc.rs`，尤其 begin_logout／complete_logout／end_session_endpoint。提取“先本地撤销、固定回调、短期随机 state、单次消费”的职责；旧 SeaORM、AuthFlows、外部关联撤销表及 back-channel 实现不符合当前原生 Session／Cache 装配，未直接复制，也不将旧功能视作当前已交付。
- 恢复 `/private/tmp/openrust-oidc-sso-20261120/source` 的既有未验收源码（基线 ad58593d2），恢复前该批无活动构建。本批先前草拟的原始字符串 hint API 保存为 superseded-logout.patch，正式代码使用恢复源码中的不透明 LogoutHint，并修正绑定成功前提前保存 hint 的夹具顺序、固定回调 query state 冲突及补充并发／期限／跨租户边界。
- 正式验收以 `9ea308252` git archive 及本批输入建立于 `/private/tmp/openrust-oidc-sso-final-20261120/source`，独占同批 target；包含已提交 Passkeys 定制，不复用旧基线替代主线整合。21 份输入逐字节核对，见 [SHA](source-sha256.json)。Cargo.lock 未修改，未引入新依赖。

## API 与语言适配

- `Authentication` 在签名、nonce、claims、UserInfo 全部通过后捕获私有 `LogoutHint`，其 token 为 Zeroizing，无 Debug／Serialize／Deserialize。`rememberLogout(&authentication)` 显式将已验证 hint 存入服务端 Session；应用必须先完成自己的本地账户绑定，避免未知绑定替换已登录用户的退出上下文。公开 tokens 字段后续修改不影响捕获的 hint，浏览器夹具对此有断言路径。
- `takeLogoutHint()` 在本地失效前消费上下文，校验 issuer／client／授权回调配置指纹。`logoutUrl(&hint)` 核对同一配置，严格校验 Discovery 的 end_session_endpoint 与 origin allowlist，拒绝保留 query 冲突，发送 id_token_hint、client_id、固定 post_logout_redirect_uri 与 32 字节安全随机 state。
- `completeLogout()` 消费匿名 Session 中的退出状态，检查常量时间 state、配置指纹、固定目标、未来／过期时间；原生原子 cache add 保护同 Session 并发副本。错误回调同样消费且不登录用户。默认期限沿用 attempt_ttl=600 秒；多副本需要共享数据库／Redis Session 和原子缓存。
- 根配置增加 `OIDC_POST_LOGOUT_REDIRECT_URI`，None 不阻止现有登录，只在请求远端退出时要求配置。只允许合法 HTTPS（显式 loopback 测试开关除外），禁止凭据、fragment 与预置 state。目标不从请求读入。
- 应用退出示例先执行原生 Guard `logoutAndInvalidate()`（清空 Session、更新 CSRF），再按选择尝试 OP。OP 不支持、离线或元数据冲突时仍返回本地已退出状态；不输出联邦退出成功。该示例在 support/local_logout.rs 编译并真实执行，三语正文逐字采用。

## 实际验收

全部 Cargo 在正式独立快照、Rust 1.94.0、增量关闭、3 任务、dev/test debug=0 下执行。MySQL 统一 `python3 -B tests/mysql_acceptance.py -- ...`，实际版本 8.4.13；未运行 SQLite 等数据库。

- `sso-browser.log` 首次浏览器流程通过；最终 `native.log` 的 OIDC 全包 4 原生＋2 HTTP／MySQL＋1 双 RP 浏览器函数，以及根 configuration 1 函数通过。最终 fixture 使用实际 TCP→Kernel→web／CSRF→原生 Session Guard，两个不同 client ID、密钥、Cookie、隔离 schema 及本地用户101／201连接同一受控 OP。
- Chrome 输入受控 OP 账户一次，第二个 RP 不再次输入；并发两个 RP 授权仍各自成功。强制 prompt=login／maxAge=0 要求重新输入，并验证 auth_time（夹具 leeway=1 秒避免秒边界抖动）。foreign Cookie、未知 subject 的已验证同邮箱、其他租户 issuer、过旧 auth_time 均不替换原本地账户。
- 无 CSRF 本地退出419；有效本地退出不结束 OP／另一个 RP。远端退出检查 hint 签名与注册 client／固定回调；不支持／不可用／query 冲突仍本地退出。返回 state 跨 RP 拒绝，正常一次成功后重放400。两请求通过 handler barrier 同时加载原数据库 Session，原子认领结果200／400；过期、配置／目标变化、错误 state 均消费且无法恢复登录。
- OP 退出后另一个 RP 本地 Session 和已有 bearer token 仍有效；prompt=none 得到 login_required，重新输入后可再次登录。受控 OP 单独撤销 token 后 UserInfo401，本地 RP Session 仍有效。测试 OP 只为协议夹具，不作为生产 OP。
- `native.log`：`cargo +1.94.0 clippy --offline --locked --workspace --all-targets --features pail,passport,fortify,socialite,oidc -- -D warnings` 通过。初次 fmt 发现新增测试行换行，修正后 `native-resume2.log` 的全仓 fmt、五 feature 根 main 构建、真实 HTTP异常／Pail／恢复／超时清理／SIGINT与独立 Artisan 通过。`feature-off.log` 的根 main 关闭功能检查通过。
- 三语新增同路径 SSO／退出章节，均为8个h2；`docs-build.log`、`docs-browser.log`、`docs-oidc.log`：构建及根／子路径三语桌面／移动12组合、OIDC内容6组合通过，保留现有大 chunk 提示。

准备快照时旧目录名称冲突导致一次 Cargo check 在 live 启动并完成，日志另存 non-acceptance-live-check.log；不计正式验收。未清理旧批次不明归属 target，本批只清理新建独占产物。失败和格式恢复日志均保留。

## 精确边界

本机 macOS／Chrome／受控 OP／MySQL8.4。真实第三方身份提供方仍需其测试应用、issuer、客户端凭据、获准回调及账户。未提供 front-channel／back-channel、OP 发起退出或全局单点退出，未宣称退出确认能证明远端所有 token 撤销。真实第三方撤销策略、硬件／浏览器／操作系统矩阵与最终发布整合仍按发布门禁跟踪。本批本地提交，不推送。

## 收尾

全部进程退出后清理独占 target 与隔离站点 dist，[记录](cleanup.json)显示实际释放5,402,480 KiB，剩余76,231,976 KiB（78.062 GB／72.700 GiB）。旧恢复源码批次、全部正式／失败日志、共享缓存和数据卷保留；Docker 卷列表前后相同。
