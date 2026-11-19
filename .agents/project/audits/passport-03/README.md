# Passport-03 授权码与 PKCE

2026-11-20：骨架、实现及本单元行为验收完成。V06 整体仍部分验收；后续刷新兑换、资源 Guard、设备、PAT、可选 grant 和安装共存继续按依赖推进。

## 固定参考和复用

- Passport 13.10.0：`f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7`。
- League oauth2-server 9.4.1：`9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c`，独立固定协议参考，不冒充新 Composer lock。
- League URI 7.5.1／interfaces 7.5.0、PSR HTTP Message 2.0，固定引用及压缩包 SHA 见本目录清单。Passport Controller／Provider／路由／Bridge 和 League Grant／Exception／RedirectValidator 的实际源码 SHA 分别见 `upstream-sha256.json`、`league-sha256.json`。
- 只读核对同级 `openrust-v0.x` 的 `6d1a81924`，提取 PKCE、参数／错误、scope HTML 转义、重定向及其公开回归；不复制 SeaORM、Figment、旧服务接线或旧内联测试。保留原 MIT 许可。新授权服务器接当前 Provider、Session、Auth、Encrypter、SQLx/Eloquent 和应用容器。
- 隔离源码以 `ef430b465b323ef21beb0360c76a022d4c59ff83` 为基线，叠加本批核定输入；不叠加协作者同期 Pail／Precognition／翻译工作。33 份输入 SHA 与实际验收副本逐一一致，见 `source-sha256.json`。

## 实现与语言适配

- 实际 GET／POST／DELETE `/oauth/authorize` 和限流 POST `/oauth/token`；支持前缀、包中间件和 ignoreRoutes 开关。仅注册已实现端点。
- 浏览器沿用配置 SessionGuard、web CSRF 和请求 Session 锁。原始已验证请求及认证主体保存在 Session，批准／拒绝一次性消费 nonce；不信任 POST 替换客户端、回调或 scope。
- prompt none／login／consent、已有有效 grant 的 scope 同意判定，以及应用提供的原生 authorizationView 回调。Rust 编译函数替代 PHP closure／序列化对象，应用负责实际同意界面。
- 公开客户端要求 PKCE；plain／S256、43–128 PCRE 表达式、PHP trim、常量时间比较与降级拒绝。请求 state 原样返回，由客户端核对。
- 回调先验证再产生 Location。非 loopback 精确匹配，HTTP loopback 忽略端口，兑换始终绑定原始可选字符串；PHP URI 的 Unicode 转义、百分号大小写和 dot segment 保留。
- 十分钟授权码以现有应用认证加密保护，包含应用 storage 域上下文；不与 PHP 密文互通。即使双应用共享 APP_KEY、密钥和镜像行也拒绝跨应用兑换。
- 当前访问／刷新默认十二个日历月；访问 JWT 为 RS256，在线签名使用 AWS-LC。首次只签发刷新凭据，其兑换、期限定制和资源验证由 Passport-04 完成。
- 客户端与授权码锁、同一数据库事务覆盖 access／refresh 保存和 code 撤销。自定义令牌模型沿用实际连接、cast、事件；取消保存或 SQL 错误时回滚并保留可重试授权码。相比 PHP 分步保存，这是当前 Rust 显式事务的失败关闭适配。
- 不启用 Fortify 动作，不将 OAuth2 服务端称为 OIDC Provider。

## 实际验收

证据目录：`/private/tmp/openrust-passport-authcode-20261120/`。所有 Cargo 命令均在该目录的 source 副本执行，独立 target，Rust 1.94.0，`CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0`。

- `python3 -B tests/mysql_acceptance.py -- python3 -B ../acceptance.py`：实际 MySQL 8.4.13，70 个 Rust 测试函数通过；目标 authorization_code 8、client 9、client_contracts 3、client_lookup 12、client_management 11、lifecycle 10、mysql 1、pkce 14、redirect_uri 2。日志 `mysql8.log`。
- 真实 TCP HTTP：公开与机密客户端、真实用户登录／Session／CSRF、auth identifier 与主键不同、授权→同意→兑换、独立 RSA 公钥验签、拒绝／跨 Session／nonce 重放、PKCE 缺失／错误、错误秘密、Basic 表单、已有 grant、scope 扩大、回调篡改、过期认证信封、客户端撤销、批准前回调变更／撤销、双应用镜像行、竞争兑换恰好一胜、真实 SQL 失败回滚与自定义模型事件取消后重试。
- 过期用例由测试权威解密／回填过去时间并认证加密，不声称实际等待十分钟。公钥验签直接检查 RS256 JWT，不用生产签名函数自验。
- 固定 PHP 8.5.12 + 真实 League URI 源码生成 1369 行：1323 个布尔结果、46 个配置异常。重复生成字节一致；改变上游源码后 SHA 门禁退出 255。生成器、输入和固定 SHA 随 `tests/reference/redirect` 交付；保留 repeat、negative-source 日志与 PHP 源码。
- `cargo +1.94.0 clippy --workspace --all-targets --locked --features pail,passport -j 6 -- -D warnings`、`cargo +1.94.0 fmt --all --check`：通过，见 clippy-final／fmt-final。
- 实际根二进制 features pail,passport：HTTP `/`／`/up`、API／Web／Accept JSON 404、OAuth token 协议 400／no-store、Artisan inspire／passport:keys 帮助、SIGINT 和端口释放通过。另原始 `./artisan inspire` 默认 feature 构建通过；未将默认不启用包的二进制当作 Passport 端点证据。见 root-passport-smoke／root-smoke。
- 三语 `npm run build`、原有浏览器 12 项及 Passport 正文 6 项（`/` 和 `/docs/`）：通过。正文回调使用与真实 HTTP 夹具相同的 Request／Session／Response 接口；同步描述当前接入前置和后续范围。

## 失败与边界

首次测试发现测试 PEM 多一个尾随换行，已恢复旧版固定 PEM 字节，不放宽生产解析；AuthCode scopes 没有 JSON cast，改为上游原始 JSON 文本。真实 URI oracle 发现旧版错误限制 65535 端口和重写 Unicode／dot segment，已修正。旧 lifecycle 夹具使用未注册 custom 中间件并断言端点 404；现注册真实中间件、配置 array cache 并验证嵌套前缀及协议响应。首次新测试 Value／Observer 类型导入编译错误、格式和一项 Clippy collapsible-if 已修正；原失败日志保留。

本轮不执行 SQLite／其他数据库及其他平台协议实机验收；不称为完整第一代发布，不推送／打 tag／部署。保留协作者工作与全部数据库卷。清理及本地提交见末尾补记。

## 收尾补记

33 份核定输入与隔离副本 SHA 一致，源码覆盖包和全部验收日志保存在上述证据目录。交付生成器经重新下载、源码／压缩包校验后完整复现 1369 行；调整为可传目录时发现 PHP autoloader 变量捕获／同名循环变量冲突，已修正并重验，原失败产物保留。最终变更源码负样本退出 255。确认 source／target 无活动进程后直接删除，df Available 从 57,620,807,680 增至 63,966,126,080 字节，实际释放 6,345,318,400 字节；移除本批无容器引用的 PHP 镜像。数据库容器和卷清单保持不变，未操作共享缓存。本批核范围本地提交，不推送。

暂存检查另发现生成的 HTML 实体表末尾多一个空白行；仅移除该空行，更新最终 SHA 与覆盖包，保留验收前 SHA。该清理不改变任何 Rust token 或表内容。
