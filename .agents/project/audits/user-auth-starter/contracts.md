# API-A01 公开入口核定与验收索引

本表沿用[唯一计划](../../plans/user-auth-starter-kit.md)，证据及源码组合见[实施记录](README.md)。它是入口核定索引，不是另一份开发计划。路径表示逻辑路径：API 组使用 `api.prefix`，原生认证网页遵循 Fortify 的前缀与域名；账户页面采用套件实际注册的 `/user/...`。应用生成的当前用户路由由 `install:api` 装配，原有公开 API 信息路由保留。

共同契约：套件业务 JSON 使用 ApiResponse 与应用语言；原生WebAuthn／SDK协议保持其既有结构，HTML 使用原生重定向、错误包和闪存。204 零正文，凭据响应禁止缓存。Cookie 写操作保持 CSRF；纯 Bearer 客户端不依赖 Cookie。套件因素／账户路由接入 sensitive，秘密不进入 old input、URI或后台诊断。账户 API 核对实际所选模型、令牌归属及能力；需要近期确认的操作绑定用户、当前设备／令牌、用途、凭据状态和有效期。错误不授予令牌或恢复权限。

| 入口 | 身份、开关与正常结果 | 失败边界与主要消费证据 |
| --- | --- | --- |
| `artisan init --starter-kit=full`／`api` | 显式安装；配置、Provider、Action、资源、路由、语言、迁移；重建后执行迁移。API 预设保留最小邮件落地页 | 两预设互斥；未知布局在对应写入前拒绝；重复保留定制，`--force` 仅覆盖明确选中的生成资源；失败保留脚手架并禁止报告就绪。`tests/install.rs`、各单元真实 init／浏览器记录 |
| `GET /api/user` | 实际已安装 Sanctum 认证；默认或应用 JsonResource，200 | 未认证401；其他模型不能访问套件账户入口；自定义资源不整体暴露属性。`starter_responses`、`starter_ui`、真实API-only客户端 |
| `GET /login`、`POST /login`、`POST /logout` | 原生Cookie登录、会话轮换和退出；JSON与HTML分别响应 | 密码422、限流429、CSRF419；额外因素未完成不登录。`starter_ui`及各网页浏览器夹具 |
| `GET /register`、`POST /register`、API `POST /auth/register` | 注册开启；原生应用Action、Registered事件；JSON201，不附送长期PAT | 注册关闭时网页与API入口均不注册；校验422；并发邮箱约束保持数据库事务。`preflight::disabling_registration...`、注册／邮件／资料测试 |
| API `POST /auth/token` | 密码主因素；普通签发201；有已启用因素时200挑战，禁止提前签发 | BodyCredentials拒绝URL秘密；能力取服务端白名单；未验证用户限于允许账户能力。`stateless_password_token_login...`、`stateless_factor_challenges...` |
| API `POST /auth/token/challenge` | 同一用户、用途、设备请求、能力和原始期限；TOTP或恢复码后201 | 过期、重放、改密、跨用户或供应商解绑后401；恢复码原子消费；真实主键改变不能继承旧挑战。`starter_custom_users`、`two_factor_management`、`socialite` |
| `GET /forgot-password`、`POST /forgot-password`、API `POST /auth/forgot-password` | 全网页提供找回表单；两预设提供找回后端；复用Password Broker及本地化状态 | API-only不额外发布完整找回表单（GET405，POST仍可用）；关闭注册不关闭找回；原生无此用户／字段错误422。邮件生命周期及注册关闭回归 |
| `GET /reset-password/{token}`、`POST /reset-password`、API `POST /auth/reset-password` | 两预设邮件落地；重置Action和持久化版本、remember轮换、原生Broker claim／finish | 错误／过期／重放422；领取后身份／凭据改变拒绝；并发一次成功；失败回滚。密码令牌、邮件API、跨节点记录 |
| `GET /email/verify`、`GET /email/verify/{id}/{hash}`、同一原始签名路径的Bearer验证适配 | 邮箱验证开启；原始签名URL及身份验证；原生HTML跳转，JSON204；已验证保持幂等 | 篡改、过期、错误用户／主机／邮箱403；旧邮件不验证新邮箱；无Bearer进入URL。`email_verification_identity`、邮件API与浏览器落地记录 |
| `POST /email/verification-notification`、API `POST /auth/email/verification-notification` | 已认证，允许未验证邮箱；按配置同步或加密排队 | 限流、关闭验证、延迟旧邮箱及重复发送分别按原生语义；队列不存邮件凭据。邮件队列、真实Worker／SMTP捕获及隐私记录 |
| `GET /home`、`GET /user/profile` | 完整预设；本人安全字段；账户首页允许未验证邮箱用于修正资料及完成验证；业务路由另加verified | API-only不提供完整账户首页；不能用伪造verified查询或只读资源绕过认证。网页／验证／头像浏览器记录 |
| `PUT /user/profile-information`及同逻辑API路径 | 本人姓名／邮箱；Cookie与PAT事务Action；邮箱变化取消验证并按策略撤销旧凭据 | 资料修改需对应近期证明；跨用户、过期、Action取消／存储失败不部分提交。`web_sensitive_updates...`、`profile_*` |
| `PUT /user/password`及同逻辑API路径 | 本人确认及替换Action；保留当前允许的设备，撤销其他会话／remember／PAT | 错误密码、旧证明、并发、Action过期／失败回滚；database／file／Redis跨节点证据见生命周期单元 |
| `GET/POST /user/confirm-password`、API `POST /auth/confirm` | 原生会话确认或当前PAT用途证明；无密码使用已有Passkey确认 | 旧会话／Token本身不是重新认证；跨设备、跨用途、过期及因素撤销拒绝。`confirmation_*`、Passkeys网页／API |
| `POST /user/password/recovery` | 已登录但无本地密码；CSRF、限流；退出会话后带本人邮箱进入原生恢复 | 不能通过静默SSO获取近期证明；有密码404；guest保护不放宽。`socialite_refuses_last_primary...` |
| `GET /user/api-tokens`、API `GET /user/tokens` | 本人摘要、能力、到期时间；完整网页或两预设API | 不读取／展示原始令牌、摘要或其他用户数据；默认30天与更短Sanctum全局期限合取。令牌管理、过期与自定义模型单元 |
| `POST /user/api-tokens`、API `POST /user/tokens` | 近期确认；当前凭据能力限制；新明文仅当前201响应交付 | 伪造／撤销父令牌、越权能力、无期限请求、旧证明均不签发。`account_*`、`starter_custom_users`、令牌网页浏览器 |
| `DELETE /user/api-tokens/{token}`、API `DELETE /user/tokens/{token}`、API `POST /auth/logout` | 本人指定令牌或退出实际当前令牌，204／HTML原生跳转 | 对方令牌、伪造ID不能越权；清除请求Guard状态。令牌管理与注销回归 |
| API `POST /auth/logout-all` | 近期证明；持久化凭据版本统一撤销，204 | 包括无本地密码、未访问过的会话及并发签发；不靠扫描Session键冒充跨驱动失效。全设备退出三库／缓存单元 |
| 网页与API `GET /user/sessions`、`DELETE /user/sessions`、`DELETE /user/sessions/{session}` | 显式启用设备列表；原生database Session及本人索引；撤销需近期证明 | file／Redis或自定义Handler须关闭列表，通用全设备失效仍有效；不信任客户端user_id。设备API／网页及跨节点回归 |
| 网页／API `POST/DELETE /user/profile-photo` | 开启头像；真实JPEG／PNG／WebP解码、大小和像素限额；提交后清理旧文件 | SVG、伪装、越界、并发及存储／数据库失败；可返回202清理收据。`avatar`、`resource_cleanup`、真实SIGKILL／Worker及浏览器记录 |
| API `GET /auth/resource-cleanups/{operation}` | 仅受控收据头授权；账户删除后仍可查询；状态与响应不声称外部IO恰好一次 | 收据不放URL、日志或数据库明文；过期、伪造、跨操作拒绝；失败可重试。`resource_cleanup`、`avatar`、`deletion` |
| 网页／API `DELETE /user` | 近期确认；可替换DeleteUser／业务预检；同库撤销；204或持久化清理202 | 业务外键、拒绝／伪成功Action、过期／并发证明回滚；硬／软删除、其他身份保留；外部清理崩溃恢复。`deletion`、`starter_custom_users`及真实进程记录 |
| 启用的2FA网页和API：状态、启用、确认、QR、秘密、恢复码、轮换、停用 | 主体认证、能力及近期确认；复用Fortify原生Action、加密秘密和恢复码 | 关闭时入口不存在；只读令牌不能读秘密；错误TOTP、重放、过期、原子消费、因素改变均复核。`two_factor_management`及真实浏览器 |
| 启用的Passkeys网页和API：选项、注册、登录、确认、列表、删除 | 固定原生服务与客户端；Cookie或S256外的无会话持久挑战；真实ES256协议 | RP／Origin、用途、期限、重放、撤销、其他用户、计数器及最后主因素保护；硬件实机另列。`passkeys`、`passkey_api`、事务与浏览器记录 |
| 启用的Socialite：登录、浏览器握手／回调、客户端交换 | 固定原生Google／GitHub state和PKCE；可信邮箱默认创建Action；客户端S256、精确回跳白名单；回调不签发PAT | 同邮箱不自动关联、非法state／verifier、重放／过期、无可信邮箱拒绝；已启用因素不能绕过。`socialite`、真实双节点协议／浏览器记录 |
| Socialite关联账户：列表、开始、预览、明确确认／拒绝、解绑 | 本人会话或同一实际PAT，近期证明；generation绑定与同用户事务 | 回调本身不绑定；跨PAT／过期／撤销因素失败；无密码不能移除最后主因素；关闭供应商仍清注销遗留。`socialite`及关闭回归 |

## 可选因素路径明细

以下明细继承上表的权限、因素、期限与诊断边界，全部由相应Feature控制，未生成的视图不代表禁用原生后端接口。

| 类型 | 方法与路径 | 消费实现／证据 |
| --- | --- | --- |
| Cookie 2FA登录 | `GET/POST /two-factor-challenge` | 原生Fortify控制器，套件登录快照与two_factor_management Cookie回归 |
| 2FA管理（网页及API） | `GET/POST/DELETE /user/two-factor-authentication`；`POST /user/confirmed-two-factor-authentication`；`GET /user/two-factor-qr-code`；`GET /user/two-factor-secret-key`；`GET/POST /user/two-factor-recovery-codes` | 原生Action及starter/two_factor.rs；管理网页、API与恢复码／TOTP消费回归 |
| Cookie Passkeys | `GET /passkeys/login/options`、`POST /passkeys/login`；`GET /passkeys/confirm/options`、`POST /passkeys/confirm`；`GET /user/passkeys/options`、`POST /user/passkeys`、`DELETE /user/passkeys/{passkey}`；完整预设`GET /user/passkeys` | 原生Passkeys controller与套件Actions；passkeys.rs网页／ES256／近期证明回归 |
| Token Passkeys | `POST /auth/passkeys/options`、`POST /auth/passkeys`；`POST /auth/passkeys/confirm/options`、`POST /auth/passkeys/confirm`；`GET/POST /user/passkeys`、`POST /user/passkeys/options`、`DELETE /user/passkeys/{passkey}` | starter/passkeys/api.rs；无Cookie跨进程／节点、超时／重放、真实协议和客户端浏览器 |
| Passkeys客户端 | `GET /vendor/user-starter/passkeys.js`及按需发布的JS／NOTICE | 版本固定的本地浏览器资产；文件不存在时使用同一内置资产；无远程脚本依赖 |
| Cookie Socialite | `GET /auth/{provider}/redirect`（完整预设）；`GET /auth/{provider}/callback`、`GET /auth/{provider}/handoff`；`GET /user/connected-accounts`、`POST /user/connected-accounts/{provider}`、`POST /user/connected-accounts/confirm`、`DELETE /user/connected-accounts/{provider}`（完整预设账户管理） | starter/socialite/{flow,web}.rs；真实state／PKCE、明确确认／拒绝、双节点浏览器 |
| Token Socialite | `POST /auth/providers/{provider}`、`POST /auth/providers/exchange`；`GET /user/connected-accounts`、`POST /user/connected-accounts/{provider}`、`POST /user/connected-accounts/exchange`、`POST /user/connected-accounts/confirm`、`DELETE /user/connected-accounts/{provider}` | starter/socialite/api.rs；客户端S256、同一PAT、回调只返回一次性code、预览／确认和generation回归 |

## 支持范围与另列项

- 正常网页、Cookie SPA与无Cookie Token协议是不同消费路径，不能以其中一条替代另一条。生产环境仍须明确配置可信Origin、stateful来源、Cookie域、Secure／SameSite、CORS及CSRF。
- Google／GitHub使用本地真实协议端点；其他供应商需应用自己的可信资料／创建策略。真实授权账号、OAuth同意页及最终供应商投递另列。
- Mailpit等本地SMTP、实际新进程Worker和强制终止场景已有证据；生产SMTP凭据、真实供应商及手机收信不是本机捕获验收。
- 浏览器移动视口不是实体手机，软件认证器的ES256协议不是物理硬件认证器。生产TLS、云存储ACL／CDN和完整部署版本矩阵归发布验收。
- 自定义模型通过明确的原生Provider／Action／资源注册；自定义主键和连接须匹配令牌表、同库清理队列及相应迁移，不宣称自动理解任意业务模型布局。
