# API-S01 本地账户归属与HTTP登录示例

2026-11-20，独立基线 `5c366c96539cdde1815ae8370bc9f3cda3587610`。接续[OAuth1](oauth1.md)，API-S01本机计划范围至此闭合；真实供应商授权仍缺各供应商测试应用、凭据、登记回调与账户。完整OIDC RP／双应用SSO仍属于API-O01／02；本批不提升其状态，最终并行模块主线整合仍待。

## 职责与示例

沿固定Socialite5.33.0的职责：驱动返回远端用户，应用决定本地账户与Session登录。包不自动建用户表、注册产品路由或按邮箱创建／合并账户。新增编译并执行的应用动作范例 `vendor/openrust-socialite/tests/support/local_login.rs`，三语正文逐字使用该源码。动作只按可信provider namespace和远端subject查询已有绑定，再通过原生`Auth::sessionGuard(...).loginUsingId`读取Eloquent用户并登录，轮换Session标识和CSRF token；未知绑定返回false，存储错误正常传播。

配置表使用`VARBINARY`唯一键`(provider,provider_user_id)`，使大小写／前导零／字节语义独立于MySQL默认collation；原生`whereExactText`负责精确查询。外键指向本地users.id，示例采用默认BIGINT UNSIGNED，删除本地用户时级联移除绑定。远端标识只接受非空字符串或整数，字符串保留原样；不存在的ID、数组／布尔／空ID拒绝。邮箱不参与归属，远端凭据不存入Session。绑定／注册由应用另行通过认证与验证流程完成，本批不提供自动绑定端点。

真实验收应用包含两个隔离schema、同名但不同密钥的加密Cookie、数据库Session、原生Eloquent UserProvider，以及受控GitHub HTTP token／profile／已验证邮箱。固定github/work两套路由，跳转时保存供应商namespace，回调前消费并核对，避免回调选择本地归属namespace。默认state仍由Socialite验证与消费，失败后重试需要重新开始授权。退出使用带真实CSRF的POST和原生logoutAndInvalidate；供应商撤销及联邦退出不混入本地Session退出。

旧版v0.x没有Socialite扩展；旧`auth/oidc.rs`的issuer／subject归属与拒绝邮箱自动合并原则已复核。其SeaORM／旧AuthFlows接口与当前原生Eloquent／Session契约不符，本批未复制旧API或持久化结构。完整RP机制留后继单元独立提取。

## 实际验收

`python3 -B tests/mysql_acceptance.py -- cargo +1.94.0 test --locked -p openrust-socialite --test local_login` 实际MySQL8.4.13通过。该函数在两个schema执行全部真实HTTP流程：

- 登录已有绑定、后继请求从数据库Session恢复、SessionID／CSRF轮换；登录前固定Cookie失效，退出后旧Cookie重放失效。
- verified同邮箱仍按github/work各自subject归属；`007`与数值7、`ABC`与`abc`分别映射到不同本地用户。
- 未绑定／非法远端ID、供应商回调不匹配、错误及已消费state、远端401，均不能登录；错误state／namespace在token HTTP前拒绝。
- 两个独立Session同时登录同一已有绑定，均归属同一用户，用户数不增加；数据库唯一约束拒绝冲突绑定。
- 另一应用无法恢复本应用Cookie；本应用删除用户／级联移除绑定或绑定表故障，不影响另一应用的独立schema登录。
- 本地退出实际拒绝错误CSRF并保留登录，正确CSRF失效Session；删除账户拒绝新登录，数据库失败返回500且保留脱敏和游客状态。

整包在同一MySQL8.4 wrapper下运行 **35个Socialite函数**，其中上述新函数实际执行，不是缺少URL跳过；根纯配置 **1个函数**通过。没有增加新的PHP oracle：供应商协议未改，已提交的固定PHP驱动测试仍全过，新增内容是应用账户归属策略与原生认证组合，不冒称Laravel提供自动账号绑定策略。

最低Rust1.94.0严格`clippy --workspace --all-targets --locked --features pail,passport,socialite -- -D warnings`、`fmt --all --check`通过。feature根main构建，保存二进制后在MySQL8.4 wrapper内复验根HTTP 200/404／内容类型、Artisan inspire与Passport帮助、原始./artisan inspire、SIGINT关闭／端口释放通过。三语`npm run build`，`check_browser.mjs` **12项**与`check_socialite.mjs` **6项**，覆盖`/`及`/docs/`，8个h2保留；新增本地动作、表结构和原生Guard方法检查。

独立快照不包含同期Fortify安装改动，未声称本批完成所有并行模块的最终主线整合。无生产Rust源码、依赖、Cargo.lock或默认路由修改。三语包含可复用动作／DDL／回调接入、已有模型注册、供应商namespace、注册与绑定职责及退出边界说明。

## 证据与收尾

证据在`/private/tmp/openrust-socialite-local-20261120/`。`tests-accepted.log`／`configuration.log`／`clippy.log`／`clippy-delivery.log`／`fmt.log`／`root-build.log`／`root-smoke.log`／`docs-*`保留，初次编译适配错误留在mysql-first.log，修正后的mysql.log、mysql-delivery.log及整包regression.log通过。源码SHA和accepted-overlay.tgz保存本批精确输入。所有进程和批次容器结束后删除自有source／target／root-main；保留数据库和全部卷、共享缓存／镜像／源码证据，实测释放见local-login-cleanup.json。本地核范围提交，不推送。
