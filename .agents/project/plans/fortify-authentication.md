# Fortify 认证对应契约

> 状态：来源已冻结，R7前置生产源码WIP；应用接线与集中验收尚未完成；不代表 Fortify、Google 客户端或完整 Guard 已验收。
> 活跃任务仅在 [唯一 todo](../todo.md) 的阶段二A领取；本文件不建第二任务队列。

## Spec

1. 固定 Fortify v1.41.0 / `c456642584c102f6a6aafee9932299542da637b5`，兼容既有 Laravel 13.x；核心与文档原基线保持。
2. 使用既有 Container、Provider、ConfigRepository、路由、Session、事件与 Encryptor；不另建认证应用或身份库。
3. 先补 R7 的配置具名 Guard / UserProvider / StatefulGuard 生产契约，再将 Fortify actions 接入该 Guard；不能把 JWT-first AuthFlows 改名当 Fortify。
4. 账户标识由配置的 provider 决定，不能固定为示例 users 表或仅 i64；默认数据库实现与自定义 provider 共同验收。
5. 复用 auth-mfa 的密码算法、TOTP、数据库原子消费及审计资产，生命周期与默认值以固定官方源码重写；不兼容旧偏离契约。
6. 包回退配置与安装模板分开：包内 two-factor 未启用 confirm / confirmPassword；发布模板两者均 true；Rust 生成项目按安装默认，显式配置支持两者关闭。
7. 密码第一因素只保存受限 challenge 与 remember；第二因素成功前不调用正式 Guard login，不发正式 Session 身份或 JWT / refresh。
8. 使用同一公开 two-factor provider 提供随机密钥、otpauth URL 与验证；本地 SVG 二维码及手动 secret 接口经正常认证与密码确认策略保护。
9. 按官方状态存储 encrypted secret、encrypted JSON recovery codes、confirmed_at；8个恢复码可重读，每次成功使用替换为新码；Disable 清三项。
10. 既有哈希一次展示、额外二次凭据、挑战期限、原子重放防护、撤销与审计保留为明确公开扩展；不得强制增加官方默认交互或称其为官方默认。
11. 固定来源→公开API/配置→等价行为测试→modules/example→CLI生成→中文文档→MySQL/PG/SQLite最低版本→独立安全审查，完整闭环后本地提交。
12. 原 SQLite 其余能力、CLI全量对应与 Laravel 导航文档任务不移除；真实 Google Authenticator 导入未执行时明确保留外部验收缺口。

## 来源与两层默认

| 来源 | 固定引用及职责 |
| --- | --- |
| Fortify | [v1.41.0](https://github.com/laravel/fortify/tree/c456642584c102f6a6aafee9932299542da637b5)，2026-10-06 发布；composer 的 Illuminate console/support 允许 `^13.0` |
| 官方文档 | [fortify.md](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/fortify.md)，同核心固定文档；不是浮动站点版本 |
| TOTP 引擎参考 | Fortify composer 要求 `pragmarx/google2fa ^9.0`；本次冻结兼容 v9.1.0 / `f00bc788c555adfb6765c437ff3538e59cd88af1`，不是声称上游有已解析 lock |
| 互操作 | [RFC 6238](https://www.rfc-editor.org/rfc/rfc6238)、[Google Key URI Format](https://github.com/google/google-authenticator/wiki/Key-Uri-Format) 仅补充协议与客户端，不能覆盖 Fortify 生命周期 |
| 测试来源 | codeload 归档排除 tests；已由同 commit Git tree / raw blob 补齐21个 tests 文件，逐个核 Git blob SHA1；原始树、SHA256清单及归档保留于本地证据 |

安装默认见 `stubs/fortify.php`；包回退见 `config/fortify.php`。`guard=web`、`username=email`、`home=/home`、`middleware=web`、路由 prefix 空、domain null、views true，以及 login/two-factor limiter 由对应配置与 Provider 注册，不能机械套现 API 前缀。该版本含 passkeys 功能与依赖，完整包范围保留；TOTP先闭环不等于整个包完成。

## 生产行为与现有差距

| 行为 | 固定源码 / 官方测试 | 当前差距及整改方向 |
| --- | --- | --- |
| Guard / Provider | `FortifyServiceProvider::register`、`Actions/RedirectIfTwoFactorAuthenticatable`、`FortifyServiceProviderTest` | 现 Authenticator 解析已选请求凭据（含 JWT/Session/PAT）；缺配置具名 StatefulGuard/UserProvider，不以旧 AuthFlows 的数据库密码查询替代 |
| 登录管线与挑战 | `AuthenticatedSessionControllerWithTwoFactorTest`、`Http/Requests/TwoFactorLoginRequest` | 现 begin_login 返回数据库 token challenge；需官方 Session login.id / login.remember 语义、remember与正常登录共用Guard及响应扩展 |
| 启用 / 确认 | `Actions/EnableTwoFactorAuthentication`、`ConfirmTwoFactorAuthentication`、`TwoFactorAuthenticationControllerTest` | 现600秒pending seed及独立factor表不等同官方已存secret/可选confirmed；既有额外期限只作扩展 |
| 恢复码 | `TwoFactorAuthenticatable`、`RecoveryCodeControllerTest` | 当前10个哈希、只删除；官方8个加密且可重看、使用后替换，生成格式以 RecoveryCode 类为准 |
| Disable | `Actions/DisableTwoFactorAuthentication`、controller test | 需按官方清 secret/recovery/confirmed，近期密码确认受feature option控制；额外 TOTP 确认不可强制覆盖默认 |
| TOTP与二维码 | `TwoFactorAuthenticationProvider`、`TwoFactorAuthenticatable`、Google2FA固定实现 | 引擎SHA1/6位/30秒/window=1；Fortify默认secret长度16个Base32字符；旧20字节secret和最少16字节判断不能挡官方密钥。二维码必须本地生成，不能外传secret |
| HTTP与扩展 | `routes/routes.php`、`Http/Responses/*`、`ResponseBindingTest` | Web/JSON、422字段错误、重定向与事件必须同框架公开扩展；旧统一envelope仅经异常/Response扩展显式选择 |
| 安全 | 官方时间窗/replay cache与现原子数据库/脱敏资产 | 官方 cache以验证码摘要作为键，现按user step；明确差异，不因增强名义伪称等价。并发/取消/审计失败保持failclosed并区分官方默认和增强 |

## 当前 R7 前置源码边界

owner=root；同一main目录按文件交接，主控独占公共接口/manager/文档/索引/Cargo，DatabaseUserProvider与SessionGuard源码各由已交接worker编写。`src/auth/guard/` 已编码对象安全账户/Provider/Guard契约、DatabaseUserProvider、应用级driver工厂与请求级manager缓存、SessionGuard及回归测试代码；这仍是未提交WIP，不以源码存在或定向编译代表闭环。

当前已编码同一AuthServiceProvider的driver/scoped manager、Web/Cookie/Session请求上下文（包括无AuthLayer的普通Web）、具名CurrentUser投影及原始类型ID、根可编辑配置/app-users适配、CLI生成模型/配置/工厂、唯一模块账户安全/浏览器socket/OIDC Session接线和正式指南。数据库Session手动与自动save在I/O门外解析实际默认Guard.id，以Weak scope避免循环；字符串ID需应用兼容sessions.user_id字段。仍待完整组合/生成/实服务验收，不能把定向回归或旧显式Session资产称完整Guard/Fortify交付。

当前源码回归覆盖目标：同scope并发构造一次、不同scope/guard缓存隔离、customcreator按需解析Provider、递归错误与取消后重试；Session轮换、记住我绑定/撤销、事件重入注销、失败/取消不得留下正式身份。已经识别并修复首版事件持锁重入与登录后续失败残留身份的代码风险，测试是否通过以本批实际结果为准。具名DB连接creator已接现Databases选择器，实际专属配置/服务证明仍待本批验收；跨task任意自定义creator依赖环及完整Eloquent尚未闭合，不能把同task环检测冒充全量。

Rust取消/并发适配：Authenticatable以可选原子compare_and_set_remember_token实现候选安装及失败/取消恢复，GenericUser/根模型使用同一账户锁，未实现能力修改前明确拒绝；候选CAS/Guard epoch防旧清理覆盖后继，不承诺回滚已经发出的数据库I/O。既有AuthFlows/OIDC users域桥接要求UserProvider::uses_database_accounts证明同一Db句柄和实际账户表，默认false；不能凭Provider标签或相同整数ID跨账户来源登录/关联。通用Guard不硬拒绝MFA，唯一示例应用middleware保留现增强MFA旧Session拒绝策略，官方挑战成功后登录另在后继Fortify闭合。

保存Cookie顺序差异公开记录：固定StartSession.php在saveSession前调用addCookieToResponse；现Rust保存会异步首次解析默认Guard并可能remember轮换ID，故在发送响应前先完成保存，再从最终Session.id生成Cookie，保证Cookie指向实际已写记录。这是现有失败关闭/可用登录的安全一致性修复，不冒称上游顺序相同，适用普通Web自动save及remember回归。Guard.id保留官方无用户时读Session键的fallback，但授权期限/OIDC桥接必须额外要求guard.user存在；会话元数据不作为授权输入。

默认DatabaseUserProvider不追加业务disabled/租户过滤；业务授权应由配置Provider/模型负责。固定数据库Provider重哈希仅SQL更新，Eloquent模型Provider的对象回写语义另行实现，不能混同。当前Credentials仅表达JSON条件，官方Closure/Arrayable对应仍需公开查询扩展，不豁免。既有Argon2资源与密码输入限制属于现有增强/差距，不冒称完整Laravel Hash。

## 账户身份与密码登录入口

用户明确约束：用户 ID 一定存在，支持整数、字符串/UUID 等标识；用户名一般存在，不据此要求所有应用必填。邮箱与手机号均允许缺失，不能作为所有账户创建、普通用户名密码登录、Provider检索或MFA启用的全局前提。仅启用相应邮件/短信功能与流程时校验所需字段。

身份标识与登录查询字段分别建模。固定 Fortify 的 username 默认 email 保持；应用可通过同一公开 UserProvider / authenticateUsing 对应扩展选择用户名、邮箱、手机号或组合入口，不把组合匹配冒称官方默认。不同入口验证成功都返回同一账户ID与Guard/Provider归属，共用密码、限流和MFA，不创建第三套登录体系。

常规登录界面提供一个“用户名／邮箱／手机号”账号输入框和一个密码框；用户只填其中任一种已绑定账号，不要求同时提交三项。唯一 modules/example、生成项目及浏览器/HTTP验证同源对应；该体验为明确授权的组合入口扩展，固定 Fortify 默认入口仍保持可配置的单一 username 字段。

组合入口配置必须明确：用户名是否大小写敏感、邮箱大小写与数据库collation策略、手机号按应用指定区域规范化（不能猜国家码）、空字段不匹配；多个字段匹配不同账户必须拒绝歧义，不能按查询先后任取一人。同值不同Provider不得串账户。测试覆盖无email/phone账户的用户名密码与MFA、三个入口归一身份、缺失可选字段、恶意输入与跨字段冲突。现有 AuthFlows 的 email/users 固定查询、注册/验证码依赖仍是明确整改项，不因新trait存在就称已关闭。

## 验收边界

当前哈希恢复码无法逆向迁移为官方可读密文；升级必须保留已有保护并提供明确重新生成流程，不能伪造旧明文、静默丢弃或关闭MFA。密码确认使用 Session 的 `auth.password_confirmed_at` 和配置有效期，不能每个管理动作都强制再提交密码。正常登录与第二因素完成必须通过同一 StatefulGuard，含 remember 和 Session regenerate；JWT/refresh属于显式扩展。完整包的注册、密码重置、profile/password更新、邮箱验证、passkeys和安装发布仍在范围内。

对应全部启停/confirm选项组合、已有secret重复启用及 force 强制重建、公开加密器覆写优先级（Fortify显式→Model→默认Crypt）、错误/缺失code、恢复码优先级与替换、无secret QR/secret读取、密码确认超时、guest/auth middleware、remember、重定向与JSON错误。两个具名provider中同标识不得串账户或确认状态；非法/过期challenge不能获得任何正式凭据。

TOTP覆盖固定RFC向量、6位前导零、前后窗口及可配置窗口、secret长度/字符、issuer/account Unicode与冒号/空格编码、二维码解码回读和手动密钥一致；只做RFC向量不算 Google 客户端已实测。数据库原子重放与恢复码竞争、故障/取消/审计回滚、跨实例、SQLite最低3.26及各生成应用必须真实执行。源码与配套收齐再集中运行适用 main 门禁，不逐小改动重复矩阵。
