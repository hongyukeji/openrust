# Passport-04 令牌生命周期与资源 Guard

2026-11-20：本单元骨架、实现和行为验收闭合，V06 整体仍部分验收。下一单元为 Passport-05 设备授权，随后继续 PAT／机器令牌／scope 中间件／SPA Cookie／事件、显式可选 grant 及安装共存。

## 参考和复用

固定 Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7` 和 League oauth2-server 9.4.1 `9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c` 的 TokenGuard、AccessToken、Passport、TokenRepository、PurgeCommand、ScopeRepository、RefreshTokenRepository Bridge、RefreshTokenGrant、BearerTokenValidator 和 ResourceServer。实际下载源码的 SHA 见 upstream-sha256.json；未把固定 Passport 中不存在的旧 RefreshTokenRepository API 加回。此批没有新增 PHP 运行时 oracle，不把 Rust 测试描述为 PHP 对照。

只读核对同级 openrust-v0.x 的 `6d1a81924` 中 resource_server.rs、access_signer.rs、grant_config.rs、grant_clock.rs，复用 Bearer 的 PCRE scheme／PHP trim、无 issuer／audience 预设、LooseValidAt 和应用期限原则。日历运算复用主线 Date，而非复制旧日期内核；权威数据接实际 Eloquent 模型和容器，不搬旧存储。

独立快照基线 `0fa6759dd9b9b166dc3197e5cf6d05b5f53153dd`，只叠加 source-sha256.json 的 20 份输入。同期 Precognition／Fortify 等提交不计入此批快照验收，后续最终整合另验。

## 实现与 Rust 映射

- POST /oauth/token 支持 refresh_token；验证客户端秘密／grant、密文及应用上下文、客户端绑定、到期、撤销和 scope 子集。轮换以客户端锁及单个事务撤销旧访问／刷新并保存新的一对；竞争一个成功，事件 veto 或数据库错误回滚并保留旧凭据。访问被单独撤销或过期仍可刷新；刷新被撤销不连带撤销访问。
- tokensExpireIn／refreshTokensExpireIn 按应用分别保存，默认一个日历年。Rust 的 Option 参数合并 PHP getter／setter；GrantLifetime 显式表示秒、日历年或框架有符号 DateInterval。日期保留应用时区、DST 与 PHP 月份 carry；落库 expiry 使用应用时区。
- scoped ResourceServer 使用应用 PublicKey，验证 RS256、可选 iat／nbf／exp、有效 jti／aud 和权威撤销记录；只取首 audience，不约束 issuer。NumericDate 经过有限数、六位小数与 i64 秒字段验证；在线 AWS-LC RSA 范围为 2048–8192 位，已在三语正文说明。
- AuthManager::extend 把守卫名和配置交给原 RequestGuard；保持真实 provider 用户类型。TokenGuard::token 按请求和守卫保存轻量 AccessToken，替代 PHP 动态 withAccessToken；can／cant／transient／revoke 可用。客户端失效、provider 不匹配、用户缺失及用户查询异常成为 guest；权威 token 存储故障为服务错误。
- provider 身份比对接受整数与其规范十进制字符串；拒绝 077、+77、77.0、77e0 和 SQL 可截断的 77junk，防止 SQL 数字强制转换混同身份。既有字符串主键 001 的 Session／Token／viaRequest MySQL 回归通过。
- passport:purge 的 revoked／expired 按 OR，默认保留到期后 168 小时；未使用 expiry 条件时不解析 hours。访问行若仍关联未撤销且期限为 null 或晚于当前时间的刷新行则保留。按实际选定模型、表与连接查询；同时清理 auth code／refresh，设备开关启用时清理已发布设备表。年月／周日时分秒输出沿当前 Date 的日历意义处理。
- Passport::hasScope 接受 *；授权码／刷新 finalize 时删除 *，已有同意按已配置 scope 解析后检查，与固定 Bridge／Controller 对齐。

## 实际验收

证据目录 `/private/tmp/openrust-passport-lifecycle-20261120/`，源码覆盖包和 SHA 保留。

- Rust 1.94.0、真实 MySQL 8.4.13，经 tests/mysql_acceptance.py 统一入口执行；没有执行 SQLite。regression.log 完成 81 项 Passport 函数、2 项 configured extension、5 项内存 provider Session 扩展。新增非空 scope 子集用例后 mysql-final.log 的 18 项全部通过，合并为 82 个不同 Passport 函数。
- 核心 Session MySQL 首轮因缺专用环境变量未执行 SQL，不计入实际验收；core-mysql.log 设置 OPENRUST_TEST_ELOQUENT_MYSQL_URL 和 OPENRUST_REQUIRE_ELOQUENT_MYSQL 后实际重跑通过。因此共 90 个不同函数通过，含 18 个授权码／刷新／资源／清理真实 HTTP/MySQL 用例。
- 覆盖刷新并发、保存 veto 回滚、scope 扩大拒绝及缩减后不可恢复、双应用密文、过期／篡改、RS256 错误、可选时间与超大值、首 audience、整数／字符串身份、provider 不匹配、用户缺失／查询故障、token 存储故障、真实 AccessToken revoke、CSRF、1001 行清理、有效与 null 刷新关联保留、设备开关、自定义不同表与上海时区。另有原生日历闰日、DST、月份、负 interval 和溢出验证。
- 最低工具链完整 workspace／all-targets features pail,passport 的严格 Clippy 及 fmt 通过（clippy-final2.log／fmt-final.log）。三语静态构建及根挂载／docs 子挂载的 12＋6 个 Chromium 检查通过。
- 根应用显式 features 构建，真实 HTTP /、/up、API／Web／Accept JSON 404、OAuth unsupported_grant_type 400／no-store、Artisan inspire、原始 ./artisan inspire、passport:keys／passport:purge 帮助、SIGINT 与端口释放通过。purge 实际数据操作由拥有应用内的 Artisan::call 验证，未把 --help 当成数据库行为验收。
- 初始 mysql／mysql2 失败记录保留；修复 canonical identity、回调 state 夹具、PKCS#1 夹具解码与空 scope 期望。偶发包缓存锁等待如实保留。误在共享根启动的一次 check 已停止，不计入隔离验收。

## 范围与收尾

三语正文与浏览器断言一并同步；后续设备协议、PAT／其他 grant／Cookie／安装共存尚未验收，不称 Passport 整体交付。核定本地提交，不推送；保留共享工作区及其他协作者内容。专属 source／target 清理和实际空间结果见 cleanup.json。所有数据库容器由验收入口按本批标签回收，全部数据卷和共享镜像／缓存保留；本批未拉取 PHP 镜像。
