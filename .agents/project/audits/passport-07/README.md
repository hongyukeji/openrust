# Passport-07 显式可选授权与默认门禁

2026-11-20：骨架、实现及本单元验收闭合，继续 Passport-08／09。V06 仍部分验收，不称完整 Passport 或发布完成。

## 固定参考及复用

Passport 13.10.0 `f5f5e9f64559dd52339daa0d6102cb1c30e4a0d7`、League 9.4.1 `9d2f6fc0a0b5aa1bb02506971d3a4ecff2c6526c`。核对 Passport／Provider 开关、Bridge UserRepository／ScopeRepository、PasswordGrant／ImplicitGrant／AbstractAuthorizeGrant、OAuthServerException、控制器错误 fragment 参数及其固定测试；SHA 与未存在路径分开记录。没有新增 PHP 运行时 oracle。

只读检查同级旧版 `6d1a81924`：authorization_server／grant_config 没有 Password／Implicit 实际协议，不能搬占位声称交付。复用当前主线客户端、参数、RSA、Eloquent事务、Session同意和refresh链。隔离基线 `291dc7d3e53ccb38ad7ae45b92321eb719a51dd4`；16份代码／测试／正文输入与根基线字节一致，SHA见记录；同期Fortify在途改动未叠入验收。

## 实现与 Rust 映射

- passwordGrantEnabled／implicitGrantEnabled 默认 false，enablePasswordGrant／enableImplicitGrant 显式开启，创建客户端不启用协议；状态属于各应用。设为false拒绝新请求，Implicit还拒绝先前待同意状态的完成，这是请求内动态配置的明确原生加强。
- Password 复用 Basic／body 客户端验证，公开客户端不要求秘密；验证已知scope后验证username／password，再应用客户端scope过滤。使用普通访问／刷新期限及refresh轮换；错误密码400 invalid_grant且固定description，无多余hint；参数／客户端／provider／SQL异常分开。
- bridge::UserRepository 选择client.provider，空值回退auth.guards.api.provider。默认编译Eloquent provider通过email查询并验证真实密码hash。PHP模型findForPassport／validateForPassportPasswordGrant映射到原生UserProvider的retrieve_by_credentials／validate_credentials；不调用Session、不rehash。
- PHP findAndValidateForPassport的运行时方法发现替换为应用实例findAndValidateForPassportUsing(provider, compiled fn)。PasswordGrantResolver收到真实客户端／用户名／密码，返回AuthFuture<AuthResult<Option<AuthIdentifier>>>；回调承担真实查询验证，优先于分离Provider路径。没有全局mock、JSON包装或伪造用户。
- Implicit复用GET /oauth/authorize、web Session／CSRF、真实登录用户及单次nonce，同意POST无法改绑response_type/client/user/scope。成功302只在已验证callback fragment返回access_token／token_type／expires_in／state，无authcode／refresh。错误scope、login_required／consent_required／denial也按固定控制器参数放fragment；不可信redirect本地拒绝，不回显任意目标。复查客户端与redirect后事务持久化，事件／模型失败回滚；竞争只有一个同意成功。
- 修正05遗留scope偏差：固定ScopeRepository只允许password／personal_access／client_credentials保留*，设备和Implicit应过滤。共享finalize规则改为该allow-list，设备真实资源write已验证为false。05历史记录中设备保留*的结论由此条更正；新增协议不保留错误行为。

## 实际验收

证据 `/private/tmp/openrust-passport-optional-20261120/`，保留全部脚本、失败／成功日志、源码覆盖包和SHA。

- Rust1.94.0，统一入口核对实际MySQL8.4.13。10个Passport targets共113个普通函数通过，3个ignored Chromium显式通过：116个不同Passport函数。configured guard2、Session扩展5、专用ELOQUENT环境真正SQL Session MySQL1，共124个不同函数。
- 新增10个普通函数：默认禁止／启用／关闭／应用隔离、真实Password access／refresh／provider subject、错误密码／参数／秘密／grant／scope／provider、public／Basic／组合模型钩子／取消回滚、自定义编译UserProvider查询与验证／无rehash、Implicitfragment／scope／资源／单次nonce、拒绝／prompt／无可信回调／公网implicit拒绝、CSRF／关闭待同意／撤销／事件回滚、默认scope／客户端限制／真实表故障、同意竞争及改redirect。
- Chromium新流程实际登录→HTML同意form→回调fragment→Bearer资源，独立HTTP客户端Password签发与Bob资源归属；既有设备／SPA两流程同时复验，未把ignored算作默认执行。
- 最低工具链strict workspace／all-targets features pail,passport Clippy和fmt通过。三语23个h2构建、根及/docs/挂载12项通用＋6项Passport浏览器检查通过。
- 根实际features二进制HTTP／OAuth400及no-store、原始./artisan inspire、keys／purge帮助、SIGINT／端口释放通过；原始Artisan后重建features再核对协议路由。
- 失败记录保留：测试fixture曾把public参数传给confidential位置，错误secret负例揭示问题后修正；Zeroizing／Response／容器错误转换的测试编译错误已修复，再完整验收通过。未运行非MySQL数据库，未新增PHP运行时oracle。

## 收尾

仅核定本批本地提交，不推送。保留Fortify等协作者WIP／提交。专属source／target在源码包和根副本保存后直接清理，实际df及全部数据库卷前后列表见cleanup.json；共享缓存和全部数据卷保留。下一单元install:api --passport／passport:install及生成应用，随后自定义与共存整合。
