# Passport-06 Scope 与其他默认路径

2026-11-20 骨架、实现及本单元验收闭合。V06 保持部分验收，继续 Passport-07／08／09。

## 固定参考与复用

Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7`、League 9.4.1 `9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c`。核对专用 PersonalAccessGrant、Factory／Result／HasApiTokens、ClientCredentialsGrant、ScopeRepository、Passport、TokenGuard、CookieFactory／CreateFreshApiToken、TransientTokenController、类型化事件、CheckToken／CheckTokenForAnyScope／EnsureClientIsResourceOwner 及固定测试；实际下载 SHA 和未存在旧路径分开见 upstream-sha256.json。没有新增 PHP 运行时 oracle，不把源码核对冒充运行对照。

只读核对同级 openrust-v0.x `6d1a81924`：可参考 scope 查询／测试，但无完整 PAT／SPA／该版本中间件可搬用。复用当前主线协议、RSA、仓储、事务与 Guard，不引入旧 SeaORM 接线。隔离基线 `e6d3ac04b06eef37be733df55aa5f7aca8a6adae`；24 份代码／测试／正文输入与根基线字节一致，接纳 SHA 见 accepted-source-sha256.json。同期 Fortify 等提交未叠入本次隔离验收。

## 实现与原生映射

- 默认客户端凭据授权验证机密客户端、HTTP Basic／body secret、scopes、独立期限（未配置跟随普通访问期限）。JWT sub 为客户端，数据库 user_id 为 null，无刷新令牌；用户 Guard 不把机器凭据当用户。
- PAT 走专用应用内 PersonalAccessTokenFactory，公网 personal_access 仍拒绝。按 provider 选择最新有效客户端，空 provider 仅回退 API provider；实际保存用户／名称／scopes，独立日历期限，无刷新令牌。HasApiTokens 用户模型显式声明 getProviderName，替代 PHP 运行时类名推断。
- PersonalAccessTokenResult 四个 camelCase 字段与脱敏 Debug；getToken 返回真实默认模型的 Arc<Mutex<Token>> 并缓存同一对象，getTokenFor<T> 显式取编译模型；未命中／错误可重试。动态注册模型的自动返回类型属09，未声称覆盖。
- defaultScopes／scopeIds／hasScope／validScopes／scopesFor 按固定 PHP 语义；默认字符串协议规范化、去重与客户端限制。中间件使用原生 using／shared 值，替代 PHP 字符串；all空列表允许、any空列表拒绝，401／403保持。Auth::guardName 仅公开既有请求 guard 选择逻辑，避免依赖全局默认。
- SPA Cookie 默认 openrust_token，HS256 内部 JWT＋既有 Cookie 加密，包含用户／CSRF／exp；属性沿用 Session。Bearer 优先，签名／时间／CSRF／用户检查；X-CSRF-TOKEN 或解密 X-XSRF-TOKEN。API 无 web 解密栈时由 Passport 解密，已解密栈显式 withoutCookieEncryption；序列化／密钥回调／名称与开关属于应用实例，相同密钥与格式可以互读，不虚报天然密码隔离。
- CreateFreshApiToken 在 web Session／Cookie 之后为认证 GET 追加 Cookie，已有同名不覆盖。POST /oauth/token/refresh 使用 Session／CSRF，返回200 Refreshed.。TransientToken 允许 scope，不持久化访问记录。
- PHP Logout 使用全局 Request；Rust 在安装 CreateFreshApiToken 的 web/logout 栈观察前后身份并过期 Cookie。这是明确原生适配，未安装的登出路径不自动清除。真实 Chromium 已验证安装路径。
- AccessTokenCreated／RefreshTokenCreated 分别在真实模型 save 后事务分发，失败回滚，AfterCommit 只观察提交；刷新轮换事务触发 AccessTokenRevoked，直接访问令牌 revoke 不发送该轮换事件。载荷不带秘密。
- actingAs／actingAsClient 使用显式 Request 和编译用户，替代全局 mock；scope 与选定 guard 只对本请求有效。actingAsClient 拒绝已有缓存用户的请求，要求新请求，防用户身份残留。

## 实际验收

完整证据 `/private/tmp/openrust-passport-defaults-20261120/`，保留脚本、失败与成功日志、源码包和 SHA。

- 最低 Rust 1.94.0，统一 tests/mysql_acceptance.py 验证真实 MySQL 8.4.13。regression.log 的10个 Passport targets 共103个普通函数通过，显式 browser-final.log 的2个 ignored Chromium函数通过：105个不同 Passport 函数。configured guard 2、Session扩展5及设置专用 ELOQUENT URL 的真实 Session MySQL1，共113个不同函数。
- 新增10个普通函数覆盖 machine／PAT／scope、provider、真实模型与回滚、typed event载荷与顺序／AfterCommit、Cookie 签名／expiry／CSRF／Bearer优先／失效用户／刷新／登出／开关／key、请求替身隔离、真实 trait createToken与模型缓存。
- Chromium 真正执行 Session登录→GET Cookie→API／CSRF拒绝→PAT→Bearer资源，独立HTTP上下文机器签发与资源／用户拒绝，刷新与登出清理；既有设备表单流程同时通过。
- strict workspace／all-targets features pail,passport Clippy通过；格式检查通过。根features二进制实际 HTTP／OAuth400与no-store、原始 ./artisan inspire、keys／purge帮助、SIGINT与端口释放通过。原始 Artisan 后重建 features 再验证协议路由。
- 三语21个h2，六个新增主题、语言适配和边界同步；构建成功，根与 /docs/ 的12项通用＋6项 Passport Chromium断言通过。仅有既有站点chunk体积提示。
- 首轮格式检查只发现新测试换行，fmt后通过；其余修复过程见保留日志。未运行非MySQL数据库、未新增PHP运行时oracle、未宣称07–09或整包发布完成。

## 收尾

仅本批范围本地提交，不推送；保留协作者WIP。专属闲置 source／target 在覆盖包及根源码保存后直接清理，实际释放与Available见cleanup.json。保留共享缓存和全部数据库卷；临时 MySQL由统一标签入口回收。
