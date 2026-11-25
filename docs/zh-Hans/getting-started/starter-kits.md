# 入门套件

## 支持状态 {#support-status}

显式用户套件安装原生 Fortify Action、安全资源、Blade 认证及 Cookie／令牌 API。下文说明已经提供的操作；已提供账户注销接口，可选因素管理页面尚未提供。不生成 React、Vue 或 Livewire 前端。

## 可用入口 {#available-entry-points}

```shell
./artisan init --starter-kit=full --extensions=api,fortify
# 仅 API，并保留邮件链接所需的最小网页：
./artisan init --starter-kit=api --extensions=api,fortify
```

先配置应用数据库和邮件。命令发布资源、注册 Provider、重建并处理迁移；自动化可在配置完成后使用 `--no-interaction`。`all` 不会自动安装用户套件，两种预设只能选择一种。

两种预设都启用第一方 stateful API 认证。完整预设增加登录、注册和本人页面；API 预设仅保留邮件链接落地页面。API 路径遵循 `config/api.rs` 和 `API_PREFIX`：受保护用户接口默认为 `/api/user`，设置 `API_PREFIX=api/admin` 后为 `/api/admin/user`。原有公开 API 信息路由保持可用。

Cookie 客户端须先获取 CSRF Cookie，并发送最新的 `X-XSRF-TOKEN`，登录导致会话轮换后也要更新。请求 JSON 不代表签发个人令牌。参阅 [Sanctum](../packages/sanctum.md) 和 [Fortify](../packages/fortify.md)。

## 配置与已有应用 {#scope}

生成的 `config/user.rs` 控制 `USER_STARTER_PROFILE`、注册、邮箱验证和可选因素。注册和验证默认启用，额外因素默认关闭。原有 Guard 和认证前缀保持有效。使用验证与密码找回前，应配置可用的邮件传输。

重复安装保留已发布文件。`--force` 显式替换所选生成资源，执行前应检查业务修改。无法安全识别的自定义 Provider 列表、bootstrap 中间件或 API 用户处理器需要手动接线，不会静默覆盖。标准用户处理器之外的业务路由会保留。

## 可选 JSON 响应 {#json-responses}

生成的应用 Provider 自动安装 `openrust_fortify::starter::StarterKitServiceProvider`。已有 Fortify 应用也可仅注册响应适配，不安装页面：

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

HTML 保留原生重定向和闪存。JSON 登录、注册与双因素登录完成后，通过 `ApiResponse` 返回当前用户，注册使用201。更新省略空数据；退出和邮箱验证保留204空正文。消息跟随应用语言，认证成功响应使用 `Cache-Control: no-store`。

`UserResource` 只读取 `id`、`name`、`email`、`email_verified_at` 及可空的 `avatar_url`，保留字符串主键，不枚举属性或读取凭据。自定义资源可替换原有 Fortify 响应绑定。单独使用响应适配不会签发令牌或改变 Guard。

## 邮件链接与密码会话 {#mail-and-sessions}

套件使用 Fortify 原生的 `POST /forgot-password`、`POST /reset-password`、`PUT /user/profile-information` 和 `PUT /user/password`。JSON 客户端发送 `Accept: application/json`；Cookie 认证须同时携带当前 CSRF Cookie。成功更新返回200，字段或重置凭据无效返回422。验证链接要求本人已认证、当前邮箱哈希匹配、签名有效且未过期。JSON 验证成功使用204空正文，重复验证保持幂等。更换邮箱会清除验证状态并向新地址发送链接。

API 预设也提供最小登录、密码重置与验证网页。未登录浏览器打开验证邮件后，登录仍回到原始签名 URL。验证收据读取数据库状态，添加 `?verified=1` 不能让账户变成已验证。依赖邮件投递前须配置传输。设置 `USER_EMAIL_VERIFICATION=false` 会关闭验证路由及这类验证邮件，密码找回保持可用。

已安装套件在登录和有效 remember Cookie 恢复时立即保存现有密码指纹，Guard 在后续认证读取中拒绝缺失或过时指纹。修改密码保留当前浏览器会话并拒绝其他密码会话；重置还会轮换 remember token。独立认证的默认行为不变。


### 验证身份与模型主键

签名验证 URL 使用模型主键，它可以与认证标识不同。默认 User 在 `MustVerifyEmail` 中声明 `get_email_verification_identifier`，非 Eloquent 实现保留沿用认证标识的默认值。自定义模型按邮箱或其他字段认证时，在现有 `MustVerifyEmail` 实现中加入以下方法，返回真实 Eloquent 主键：

```rust
fn get_email_verification_identifier(
    &self,
) -> openrust::auth::AuthResult<openrust::auth::AuthIdentifier> {
    self.get_key()
        .ok_or(openrust::auth::AuthError::InvalidIdentity)?
        .try_into()
}
```

`VerifyEmail::send` 与 `VerifyEmail::url` 使用这个身份。`EmailVerificationRequest` 核对已认证的具体身份、路由主键及当前邮箱哈希，认证标识相同的其他模型不能互换。原有 `auth::email_verification::set_verification` 辅助方法在模型自身连接上按真实主键、精确当前邮箱和验证状态做条件更新；`EmailVerificationRequest` 只为实际成功转换派发 `Verified`，辅助方法自身不派发保存事件，并发有效请求保持幂等。验证时间也使用应用 Date 时钟，不改变签名算法。

## 凭据生命周期 {#credential-lifecycle}

显式安装套件提供两份迁移模板（发布时按 Laravel 机制生成时间戳文件名）：`0001_01_01_000020_add_credential_version_to_users_table.rs` 和 `0001_01_01_000021_add_credential_stamp_to_personal_access_tokens_table.rs`，跟随所配置的用户及令牌模型表名和连接。升级应用开始服务前先执行正常迁移。启用此策略后，既有会话须重新认证，既有个人令牌须重新签发；不要手动重置凭据版本。

生成的 User 保留 Sanctum 的 `create_token` 入口。在已认证的应用作用域内使用，先完成所需的近期认证及额外因素；此服务不会替调用方执行这些 HTTP 检查：

```rust
use openrust::sanctum::HasApiTokens;

// `user` is the authenticated application User; authenticate before issuing.
let token = user.create_token(
    &sanctum,
    &database,
    "phone",
    &["account:read"],
    None,
).await?;
let plaintext = token.plain_text_token;
```

`user.token_expiration_days` 默认30天。显式传入更短期限会保留，更长期限会被限制；Sanctum 全局期限可以更早拒绝令牌。`user.token_abilities` 是服务端能力白名单，未知或空能力会报错。明文只返回一次，数据库仅保存摘要及隐藏的凭据标记，勿写入日志。过期记录继续用 `sanctum:prune-expired` 清理，认证不等待清理才拒绝过期令牌。

生成的密码 Action 在修改密码的同一事务内递增持久化用户凭据版本，保留当前浏览器登录，使其他会话及个人令牌失效；密码重置使全部旧登录失效。普通密码哈希升级及原生“退出其他浏览器会话”不会撤销个人令牌。自定义 Action 应调用 `openrust_fortify::starter::credentials::change_password`，直接写数据库密码不会自动推进此策略。`credentials::revoke_all` 也支持无本地密码账户，调用方须先取得有效的近期认证。

修改凭据与签发令牌会在提交前锁定并重新核对当前身份／版本，要求用户和令牌使用同一数据库连接身份；独立配置的令牌连接会明确失败，不假装具有原子安全保证。自定义用户模型实现 `Clone` 并保留 `credential_version`；自定义令牌模型保留 `credential_stamp` 并在序列化中隐藏。含瞬态字段的令牌模型可覆写 `PersonalAccessTokenModel::clone_token_model` 返回 `self.clone()`，使属性访问器保留这些状态。

以上为生命周期服务和生成的密码 Action。完整账户中心仍在开发；设备登录及近期确认／令牌管理见下节。仅注册响应适配器不会启用凭据策略。


### 其他 Guard 与多态令牌归属

套件将凭据版本及登录时强制指纹限制在选定会话 Guard、匹配的 Eloquent Provider 配置及实际连接／表。独立后台表不需要套件凭据列，其他身份的原生登录、密码重哈希和 remember Cookie 保持原有行为。撤销套件用户设备不会撤销数值 ID 相同的独立归属。

Sanctum 仍可认证其他已注册令牌模型，它们在自己的路由照常使用；套件账户管理路由对其他用户模型类型返回401。套件追加归属检查，不覆盖应用既有校验器。使用 `Sanctum::add_token_owner_validator` 追加检查，所有检查必须通过，错误不会放行；`validate_token_owner_using` 保留明确替换回调的原有行为。

手动使用框架的应用仍可配置原有全局 `auth.session_credential_version_attribute`；未设置 `auth.session_credential_version_scope` 时保留全局语义。可选作用域包含 `guards` 与 `tables`（每项含 `connection`、`table`），格式错误会失败，不静默回退到无限制。已安装套件按配置装配作用域；手动接入运行时 Provider 或异名模型别名时，须在开始服务前明确核对选中 Guard／连接并相应设置作用域，不能把默认 User 验收当成自定义组合已通过。

## 显式设备令牌登录 {#client-tokens}

网页和第一方 SPA 仍默认使用 Cookie 认证。向 `/login` 请求 JSON 不会自动签发个人令牌。已注册账户可以显式申请设备令牌：

```sh
curl -X POST http://localhost:8000/api/auth/token \
  -H 'Accept: application/json' -H 'Content-Type: application/json' \
  -d '{"email":"developer@example.test","password":"YOUR_PASSWORD","device_name":"phone","abilities":["account:read"]}'
```

```json
{
  "success": true,
  "code": "ok",
  "message": "成功",
  "data": {
    "token_type": "Bearer",
    "token": "TOKEN_SHOWN_ONCE",
    "expires_at": "2030-02-01T03:04:05Z",
    "user": {
      "id": 1,
      "name": "开发者",
      "email": "developer@example.test",
      "email_verified_at": null,
      "avatar_url": null
    }
  }
}
```

签发成功使用 HTTP 201 和 `Cache-Control: no-store`。请安全保存凭据：数据库仅存摘要，后续读取不会再次返回明文。访问 `/api/user` 等受保护入口时发送 `Authorization: Bearer TOKEN_SHOWN_ONCE`。服务端和移动令牌客户端不需要浏览器会话；第一方请求仍须遵守状态中间件和 CSRF 保护。

`device_name` 必填，最多255个字符；可选 `abilities` 是最多100个名称的数组。服务端按 `user.token_abilities` 核对，未知或空能力返回422。启用邮箱验证时，未验证用户只能获得与 `user.unverified_token_abilities` 的交集，默认是账户及令牌管理能力；判断遵循实际 `MustVerifyEmail` 状态。验证成功不扩大已有令牌的能力。业务路由仍需应用授权及邮箱验证中间件。

路径跟随 `API_PREFIX`。设置 `API_PREFIX=api/admin` 后，使用 `/api/admin/auth/token` 和 `/api/admin/auth/token/challenge`；保留现有信息页与当前用户入口。生成的 Provider 显式调用 `openrust_fortify::starter::client::register_routes::<User>(app)`，独立 Fortify 应用不受影响。

账户存在已启用的双因素认证时，正确密码返回 HTTP 200 和短期挑战，不返回访问令牌：

```json
{
  "success": true,
  "code": "auth_challenge_required",
  "message": "请完成额外身份验证。",
  "data": {
    "challenge": "OPAQUE_CHALLENGE",
    "methods": [
      "totp",
      "recovery_code"
    ],
    "expires_at": "2030-01-02T03:09:05Z"
  }
}
```

向 POST `/api/auth/token/challenge` 发送实际挑战和当前 code，也可将 code 换成 `recovery_code`。完成后返回相同的201令牌格式。挑战五分钟过期、仅能消费一次，保留原设备名称和已批准能力，不能用于访问资源。因素无效返回422；挑战过期、已消费或已失效返回401。登录复用已配置的命名限流，因素尝试另按账户限流，不能靠更换挑战标识绕过。

```json
{
  "challenge": "OPAQUE_CHALLENGE",
  "recovery_code": "YOUR_RECOVERY_CODE"
}
```

签发在用户行锁内重新检查最新状态。凭据版本、相关账户／因素状态或应用加密密钥变化会使待完成认证失效。恢复码认证在数据库锁下单次消费，普通公开替换方法保持原生行为。兑换中断或失败时可能需要重新登录，不会遗留可重复使用的已授权挑战。

原生时间戳迁移发布器在 User 连接中新增 `create_user_auth_challenges_table`，不保存原始挑战键或密码。多节点共享数据库、应用密钥、认证策略及缓存；各节点读取自己的应用配置。

```sh
./artisan auth:prune-challenges --hours=0
```

命令默认保留过期记录24小时，`--hours=0` 立即清理已经过期的记录。用现有[调度器](../digging-deeper/scheduling.md)安排执行，并配合 `sanctum:prune-expired`。认证不等待清理才拒绝过期，重复清理安全。

消息遵循应用／请求语言及回退。已发布的 `lang/{locale}/user.json` 优先于套件编译默认值；升级补足英语、简体中文和繁体中文缺键，保留已有自定义翻译。

本节覆盖设备登录和 TOTP／恢复码挑战。其他账户操作见[无会话注册与找回](#client-mail)、[近期确认与个人令牌](#account-tokens)及[浏览器设备](#browser-sessions)。

## 近期确认与个人令牌管理 {#account-tokens}

下列端点属于显式安装的用户套件。API 路径跟随 `API_PREFIX`，示例使用 `/api`。普通 JSON 登录继续使用 Cookie 会话。

| 方法和路径 | 行为 |
| --- | --- |
| `POST /api/auth/confirm` | 验证当前用户密码，或调用应用配置的确认回调。 |
| `GET /api/user/tokens` | 仅列本人令牌的安全元数据；PAT 客户端需要 `tokens:manage`。 |
| `POST /api/user/tokens` | 近期确认后创建设备令牌，成功201。 |
| `DELETE /api/user/tokens/{`token`}` | 近期确认后仅撤销本人令牌，成功204。 |
| `POST /api/auth/logout` | 撤销本次实际认证的 PAT，成功204，不要求近期证明。 |

PAT 客户端先确认密码，使用与随后敏感操作相同的 Bearer Token：

```http
POST /api/auth/confirm
Authorization: Bearer YOUR_TOKEN
Content-Type: application/json
Accept: application/json

{"purpose":"tokens","password":"YOUR_PASSWORD"}
```

```json
{"success":true,"code":"ok","message":"成功","data":{"proof":"OPAQUE_SINGLE_USE_PROOF","expires_at":"2030-01-01T03:00:00Z"}}
```

证明通过 `X-Auth-Confirmation` 请求头或 JSON 的 `proof` 字段提交，只能使用一次。再次创建或撤销前重新确认：

```http
POST /api/user/tokens
Authorization: Bearer YOUR_TOKEN
X-Auth-Confirmation: OPAQUE_SINGLE_USE_PROOF
Content-Type: application/json
Accept: application/json

{"device_name":"tablet","abilities":["account:read"]}
```

201响应与[设备登录](#client-tokens)使用相同的 `token_type`、`token`、`expires_at` 和 `user` 字段。省略 `abilities` 时采用服务端策略与当前父令牌的交集。失败可能消耗证明，重试前重新确认。子令牌不能超过服务端白名单、未验证账户策略和当前父令牌的能力。妥善保管仅返回一次的明文令牌，令牌列表不会再次显示它。

`GET /api/user/tokens` 的 `data` 为数组，每项只包含 `id`、`name`、`abilities`、`created_at`、`last_used_at`、`expires_at` 和 `current`；没有摘要、凭据版本摘要或内部归属属性。撤销其他用户的ID返回404；`POST /api/auth/logout` 不接受客户端指定其他凭据代替本次认证令牌。成功204没有正文，撤销后认证返回401，其他设备仍可使用。

第一方 Cookie／SPA 保留正常 CSRF 流程并发送凭据。`POST /user/confirm-password` 将确认时间和套件身份快照记入当前 Session；随后向 `POST /api/user/tokens` 提交 `device_name` 和 `abilities`，此模式不需要另一份不透明证明。`POST /api/auth/confirm` 也接受有状态 Cookie 请求，成功201且不返回 `data`。其他浏览器会话不会继承确认；缺失或过期返回423及本地化 code `password_confirmation_required`，缺少 CSRF 令牌仍返回419。

PAT 证明在应用数据库中只保存摘要，绑定用户、当前PAT、用途、凭据版本和有效的因素／邮箱状态。`auth.password_timeout` 为正整数秒，默认10800；不等清理任务就会到期。错误设备／用途、篡改或重放返回401。签发／撤销使用共同用户锁和令牌行锁，模型事件取消或提交前证明过期会回滚数据库变更。应用确认回调可支持自己的身份流程，没有本地密码不代表自动确认。无状态 Passkey 重新认证适配仍单独待补。

密码错误422，一分钟内5次失败后429并带 `Retry-After`；缺少令牌能力403。消息跟随请求语言，证明、密码和令牌秘密不会进入旧输入；包含新证明／令牌的响应使用 `no-store`。设备管理见[浏览器设备](#browser-sessions)；账户注销见[对应章节](#account-deletion)。

### 退出全部设备

`POST /api/auth/logout-all` 使套件全部旧会话、remember Cookie 和个人访问令牌失效，包括调用方当前令牌；不会修改密码。PAT 须有 `account:update`，并提交 `purpose: "devices"` 的单次近期证明；Cookie 认证须有原生近期确认及 CSRF。成功204空正文。只撤销当前 PAT 时使用 `/api/auth/logout`。

```sh
curl -i -X POST http://localhost:8000/api/auth/confirm \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: application/json' \
  -d '{"purpose":"devices","password":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/logout-all \
  -H 'Authorization: Bearer <token>' -H 'X-Auth-Confirmation: <proof>' \
  -H 'Content-Type: application/json' -d '{}'
```

持久化凭据版本在与令牌签发共用的用户锁中变更，模型事件结束后、提交前再次核对证明期限。保存被取消或失败时撤销事务回滚；失败可消费证明，重试前重新确认。成功撤销不依赖删除全部会话文件或扫描 Redis 键，各节点后续认证读取会拒绝旧版本。旧令牌记录仍可显式管理或清理，但不能认证。已进入业务执行的请求不被追溯取消；重新登录才能建立新凭据。

此操作与[设备列表](#browser-sessions)是两个能力，撤销可用不代表所有会话驱动都已提供设备枚举。

## 无会话客户端注册与邮件恢复 {#client-mail}

显式安装套件后，以下路由读取 `api.prefix`（默认 `/api`）。它们复用 Fortify Action、验证器、密码代理和通知；独立 Fortify 的原有 Cookie／CSRF 路由不变。

| 方法 | 路径 | 成功行为 |
| --- | --- | --- |
| POST | `/api/auth/register` | 201，返回安全用户资源；不自动登录或签发令牌。 |
| POST | `/api/auth/forgot-password` | 200，本地化重置邮件状态。 |
| POST | `/api/auth/reset-password` | 200，本地化重置状态；重置凭据仅能兑换一次。 |
| POST | `/api/auth/email/verification-notification` | Bearer 或有状态 Cookie 认证；发送成功202，已验证204。 |

注册、密码找回和验证路由分别跟随相应功能开关。敏感凭据放在 JSON 正文，不放查询字符串。客户端认证路由拒绝查询凭据并返回400；成功、认证失败、验证错误及限流响应均带 `Cache-Control: no-store`。有状态请求仍需 CSRF，返回 JSON 不会取消此要求。

```sh
curl -i -X POST http://localhost:8000/api/auth/register \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"name":"开发者","email":"developer@example.test","password":"long-password-123","password_confirmation":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/forgot-password \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"email":"developer@example.test"}'
```

注册后显式完成[令牌登录](#client-tokens)。原生客户端验证邮箱时，向**邮件中的原始签名 URL**发送 `Authorization: Bearer <token>` 和 `Accept: application/json`。不要将签名路由搬到 `/api`、向 URL 追加凭据或重组查询串。签名错误、过期或其他用户的链接均不能通过。成功204，重复有效链接幂等。这个网页落地路由可能产生匿名会话 Cookie，但 Bearer 验证不会登录浏览器；浏览器也可沿用邮件登录落地页。

找回密码时，从原邮件链接取得重置凭据和邮箱，与 `password`、`password_confirmation` 一起提交至 `/api/auth/reset-password`。缺失、过期或已消费的凭据返回422；并发提交只有一个成功。重置使套件既有会话及个人访问令牌失效，须用新密码重新登录。消息跟随应用语言；验证签名和密码重置期限使用应用 Date 时钟。

依赖邮件前先配置实际传输。同步发送失败返回错误，不会报告已发送；修复投递后可显式重发验证通知。这些路由不承诺队列自动补发或真实外部邮箱收信，应用通知回调仍可替换。

## 应用用户资源 {#user-resource}

两种预设都发布 `app/http/resources/user_resource.rs`。其中 `UserResource` 实现原生 `JsonResource`，开发者修改 `to_array` 即可选择公开字段；默认使用身份字段及头像的安全投影，不要整体序列化用户模型。生成的 Provider 为所属应用选择该资源：

```rust
openrust_fortify::starter::UserResource::use_resource::<
    crate::app::models::User,
    crate::app::http::resources::user_resource::UserResource,
>(app)?;
```

JSON 注册／登录、双因素完成、当前用户，以及个人令牌签发中的用户数据共用该资源。资源接收真实请求，先运行原生 `prepare_response`，再调用 `to_array`；返回值必须为 JSON 对象，准备或序列化错误向外传递。这是嵌入认证响应的数据，资源的 `wrap`、`with`、状态和 HTTP 响应钩子不会改变外层认证响应；要替换整个响应，继续绑定现有 Fortify 响应契约。网页账户与邮件页面保留内部安全字段，API 字段改名不会破坏页面。

重复安装保留该文件，`--force` 会替换。已有独立文件模块会复用；内联或条件不匹配的资源模块会在套件写入前报错，提示手动接线。选择保存在所属应用中，不使用进程全局回调。

## 资料与密码 API {#user-updates}

两种预设都提供 `PUT /api/user/profile-information` 和 `PUT /api/user/password`，路径跟随 API 前缀。需要选中的用户模型及 `account:update` 能力。资料更新（包括仅改姓名）使用 `email` 用途的近期确认；修改密码使用 `password` 用途。PAT 证明绑定当前实际令牌、用户和用途，有期限且单次消费，不能放在 URL 中。

```shell
# 先用当前密码获取 email 用途证明：
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"purpose":"email","password":"current-password"}'
# 将 data.proof 保存到 PROOF，再更新资料：
curl -X PUT "$APP_URL/api/user/profile-information" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -d '{"name":"Alice","email":"alice@example.test"}'
```

修改密码时重新获取 `purpose: "password"` 的证明，再向 `/api/user/password` 提交 `password` 和 `password_confirmation`。此 API 使用近期证明，不重复要求 `current_password`。成功200，返回正常本地化的空载荷响应。密码变化后旧 PAT 失效，令牌客户端须用新密码重新登录。浏览器使用原生 Session 近期确认与 CSRF，无需 PAT 证明；当前会话同步并轮换，其他旧会话与 remember Cookie 失效。轮换后刷新 CSRF 令牌，下次敏感操作须重新确认。配置的 Fortify 会话 Guard 必须认证该浏览器身份。

邮箱变化会清空验证状态，向新地址发送原生验证通知，旧邮箱链接失效。保存期间证明到期、模型取消或事务错误都会回滚数据库变更。错误／已用 PAT 证明返回401，缺少能力403，字段错误422，浏览器未近期确认423，CSRF错误419。失败可以消费证明，重试前重新确认。通知发送和密码代理清理在用户事务提交后执行，沿用原生流程：此时失败不会撤回已提交的资料／密码，重试前先核对当前状态。SMTP 投递保证遵循配置的邮件驱动。

应用 Action 继续可替换。API 在取得用户锁前调用 `validate_for_transaction`，再将同一个事务传给 `update_in_transaction`，不会调用可能另取池连接保存的旧 Action。默认资料验证器使用实际模型的表和连接。自定义密码 Action 应使用 `credentials` 中的 `change_password_in_transaction` 保存摘要、递增持久化凭据版本并清空 remember token；所有写入使用传入事务，返回同一用户／模型／连接。套件提交前复核证明期限。安装器给标准 Action 追加方法并保留注释；自定义旧 Action 会停止并提示手动接线，已完整实现的自定义事务方法保持原样。独立 Fortify 保留原入口。

## 密码重置的身份绑定 {#password-token-binding}

生成的用户 Provider 为选中模型调用 `bind_password_tokens`。套件将重置记录绑定到实际模型主键、连接／表范围、重置邮箱、密码摘要及可选凭据版本。邮箱被另一个账号重新注册后，原账号的旧令牌不能重置新账号；旧凭据快照延迟写入的记录，也不能在密码／版本变化后授权重置。没有本地密码摘要的账号同样通过主键区分。密码重哈希也会改变快照，使旧链接失效。共同用户锁在保存前再次核对重置邮箱：即使令牌先领取，邮箱变更已经提交后也不能继续用旧快照改密码。

继续使用现有 database／cache 仓储、邮件中的64位令牌、到期、限流和原子消费，不新增迁移或平行令牌存储。数据库与配置的缓存驱动执行相同绑定。绑定只作用于选中用户模型及具名密码代理；独立 Fortify 和其他用户类型保留原有行为。已缓存代理读取所属应用的同一规则表，套件重复装配保持幂等。

启用后，先前未绑定的旧重置链接失效，需重新请求邮件；所有应用／Worker 节点须使用相同模型和绑定配置。若模型的重置能力返回自定义包装类型，或使用额外动态租户范围，应显式注册匹配规则。高级应用可通过 Password 的 `bind_user_tokens_using` 追加具名规则：不同名称组合，同名替换，回调错误直接失败，不回退签发未绑定令牌。规则名称和值须在各节点稳定，不向客户端返回密码或计算出的绑定；普通业务继续调用既有忘记／重置密码接口。

### 双因素账户管理 {#user-two-factor-management}

安装用户套件并执行迁移后，设置 `USER_TWO_FACTOR=true` 启用入口，默认关闭。完整套件的账户页提供 `/user/two-factor-authentication` 链接，复用原生 Fortify 表单、CSRF、配置的 Session Guard 和应用翻译。依次启用、扫描身份验证器二维码或输入设置密钥、按要求输入 TOTP 确认，并离线保存八个恢复码。页面也支持重新生成恢复码和关闭双因素认证。设置详情和恢复码使用 `no-store`、`no-referrer`，不写入表单闪存，且要求近期密码确认。启用或确认因素后旧确认快照失效，出现提示时须再次确认密码；隐藏设置详情不会关闭认证因素。

仅 API 预设提供相同的管理能力，不发布这些页面。下表路径跟随 API 前缀配置：

| 方法 | `/api` 后的路径 | 返回内容 |
| --- | --- | --- |
| GET | `/user/two-factor-authentication` | 安全状态 `enabled`、`has_secret`、`confirmation_required`，要求 `account:read`。 |
| POST | `/user/two-factor-authentication` | 启用，返回设置 SVG、URL、`secretKey` 和 `recovery_codes`。 |
| POST | `/user/confirmed-two-factor-authentication` | 用 `code` 确认，返回安全状态。 |
| DELETE | `/user/two-factor-authentication` | 关闭，省略空成功数据。 |
| GET | `/user/two-factor-qr-code` | `svg`、`url`。 |
| GET | `/user/two-factor-secret-key` | `secretKey`。 |
| GET | `/user/two-factor-recovery-codes` | `data` 内的 `recovery_codes` 数组。 |
| POST | `/user/two-factor-recovery-codes` | 生成并返回替换后的 `recovery_codes`。 |

除安全状态 GET 外，每次操作均要求 `account:update`，并通过 `POST /api/auth/confirm` 获取用途为 `factors` 的新单次证明。通过 `X-Auth-Confirmation` 请求头发送，有正文的请求也可使用 JSON `proof` 字段。URL 查询参数中的证明返回400，避免进入访问日志、历史或复制链接。下一次操作须获取新证明，错误验证码后也一样；证明不能重放或转交其他令牌。Cookie 客户端使用本浏览器近期确认的 Session 和 CSRF，不使用 PAT 证明。因素管理不会额外签发访问令牌。

```bash
# 用当前 PAT 为本次操作获取证明。
curl -X POST http://localhost:8000/api/auth/confirm \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"factors","password":"your-password"}'

# 将 data.proof 保存到 PROOF，再启用身份验证器。
curl -X POST http://localhost:8000/api/user/two-factor-authentication \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -H 'Content-Type: application/json' -d '{}'
```

写入和敏感读取均使用共同用户锁及同连接事务，在返回前核对当前令牌和凭据快照。错误验证码返回422，缺少或已消费证明返回401，令牌权限不足返回403，功能关闭时路由返回404。因素激活后，登录仍须提交 TOTP 或未使用的恢复码。独立 Fortify 应用保留既有路由响应；额外的 API 封装和更严格的确认要求属于安装后的用户套件。

### 网页资料与密码表单 {#web-user-updates}

账户页提供资料、密码表单和身份确认入口。原生 `PUT /user/profile-information`、`PUT /user/password` 与上述API共用应用Action、用户写锁和同连接事务。姓名更新也要求近期确认；普通浏览器未确认时跳转到 `password.confirm`，确认后返回账户页重新提交，密码不会被保存在旧输入。JSON协商保持423及本地化提示。密码账户的网页密码表单仍校验 `current_password`，使用配置的Fortify Guard，不写死web。

当前Cookie确认绑定确认时的用户、邮箱、凭据及双因素状态和Session时间。其他设备变更邮箱／因素后，旧时间标记不足以继续授权；重新确认后方可操作。密码变更保留当前浏览器，旧会话及PAT失效。更新会轮换Session和CSRF，JSON／SPA客户端须刷新CSRF后继续；普通浏览器跟随重定向读取新表单。独立Fortify应用未装配用户套件时保持原生行为。

网页保留原生闪存状态码，通过 `lang/<locale>/user.json` 翻译显示，例如 `password-updated`；覆盖对应条目即可定制。JSON成功消息仍使用ApiResponse本地化。升级前没有套件快照的既有浏览器确认需要重新确认。

## 认证邮件队列与开发预览 {#auth-mail-queue}

邮箱验证开关只作用于套件的具体用户类型，不会全局拦截应用的所有验证通知；其他模型保留自己的验证流程。高级 Provider 可用 `VerifyEmail::send_when::<User>(app, |user| Ok(true))` 决定该类型是否发送，回调错误直接使发送失败。条件在渲染及入队前执行，并按应用隔离。

默认沿用原生同步邮件。需要请求快速返回、由独立 Worker 发送时，在安装后生成的 `config/user.rs` 配置：

```rust
"mail": {
    "queue": get(env, "USER_MAIL_QUEUE", Value::Null)?,
    "connection": get(env, "USER_MAIL_QUEUE_CONNECTION", Value::Null)?,
},
```

```dotenv
QUEUE_CONNECTION=database
USER_MAIL_QUEUE=auth-mail
# 可选；省略时使用 queue.default。
USER_MAIL_QUEUE_CONNECTION=database
```

执行现有迁移后启动消费进程：

```sh
./artisan migrate
./artisan queue:work database --queue=auth-mail --tries=3 --backoff=10
```

注册／重发验证与密码找回使用同一具名队列；关闭邮箱验证后不会创建验证邮件任务，密码找回继续有效。队列名未设置或为 `null` 时恢复同步；空字符串、包含逗号或首尾空白的名称无效。同步、丢弃、进程内驱动及包含这些驱动或循环的 failover 会被拒绝；可用的持久化连接还须按所选驱动正确注册和配置。Redis 连接可将上面两处 `database` 改为对应的队列连接名称。Worker 的 `--queue` 须匹配配置，重试、回退与超时沿用原生队列选项。

套件对具体用户类型分别配置 `ResetPassword` 和 `VerifyEmail`，独立应用与其他模型保留自己的发送策略。开启时链接载荷强制加密，任务沿用原生通知的事务提交后派发。生产者与 Worker 须共享 `APP_KEY`、应用 URL、用户模型和队列配置；Worker 使用自身邮件配置解析传输和凭据，密码、签名 URL 与重置令牌不可打印到日志。邮件连接名可以保留在任务中，SMTP 密码不会因此进入任务。

如需独立控制两类通知，可在 Provider 中使用 `ResetPassword::queue_for::<User>(app, Some(options))` 与 `VerifyEmail::queue_for::<User>(app, Some(options))`，其中 `options` 为 `openrust::auth::notifications::AuthMailQueueOptions`；传入 `None` 恢复该类通知的同步发送。自定义 URL／邮件回调继续有效。模型通过包装对象提供重置／验证能力时，套件自动注册会明确报错，应为该实际能力类型手动注册；不静默回退同步。旧项目升级不会覆盖已有 `config/user.rs`，请合并上面的配置组并重新部署 Provider／Worker。

请求成功表示已入队，不能据此认定收件成功。供应商暂时拒绝后，原生 Worker 会按重试配置释放并重试；超过次数进入所配置的失败任务存储。SMTP 在供应商受理后、队列确认前崩溃可能重复投递，不能保证恰好一次；重置令牌保持单次消费，验证链接保持幂等。

开发可使用独立 [Mailpit](https://mailpit.axllent.org/docs/install/docker/) 实例，不连接真实收件人：

```sh
docker run --rm --name openrust-auth-mailpit \
  --tmpfs /data \
  -e MP_DATABASE=/data/mailpit.db \
  -p 127.0.0.1:1025:1025 -p 127.0.0.1:8025:8025 \
  axllent/mailpit:v1.34.0
```

```dotenv
MAIL_MAILER=smtp
MAIL_SCHEME=smtp
MAIL_HOST=127.0.0.1
MAIL_PORT=1025
MAIL_USERNAME=null
MAIL_PASSWORD=null
MAIL_FROM_ADDRESS=hello@example.test
```

浏览器访问 `http://127.0.0.1:8025` 预览邮件。以上地址用于应用运行在宿主机的情况；容器内应用应使用同一网络中的邮件服务名。已有 `MAIL_URL` 会覆盖单项设置，开发预览时须移除或设为 `null`。此实例关闭后预览数据丢失，不作为生产邮件服务；真实供应商凭据、生产 TLS、投递与最终收信仍需独立验收。

## 浏览器设备 {#browser-sessions}

完整预设同时发布 `GET /user/sessions` 页面。原生 HTML 表单支持撤销单个或其他设备，保留 CSRF 和近期密码确认，完成后重定向并显示本地化状态。仅 API 预设不发布此账户页面。

升级保留的 `config/user.rs` 时，手动合并以下配置组；旧文件不会自动读取 `USER_DEVICES`。安装器保留应用已有配置。

```rust
"devices": {"enabled": get(env, "USER_DEVICES", true)?},
```

安装的套件默认开启 `user.devices.enabled`。使用原生 `SESSION_DRIVER=database`，并运行发布的 `0001_01_01_000023_create_user_browser_sessions_table.rs` 迁移。独立索引跟随 User 连接，按模型、数据库、表、真实主键和 Guard 隔离，不修改原始 users 迁移。未登记索引的旧会话须重新登录。file、Redis 或自定义 database 处理器须显式设置 `USER_DEVICES=false`，关闭相关路由，不假装提供安全枚举；按凭据版本退出全部设备的能力仍可使用。 安装器按正常包迁移规则，发布时使用当前时间戳命名。

| 接口 | 结果 |
| --- | --- |
| `GET /api/user/sessions` | 200，`data` 为本人的浏览器设备摘要；PAT 需要 `account:read`。 |
| `DELETE /api/user/sessions/{session}` | 204，撤销指定不透明设备 ID；不存在或其他用户的 ID 返回404。 |
| `DELETE /api/user/sessions` | 204，撤销其他浏览器设备，保留当前 Cookie 会话；PAT 没有当前浏览器，因此撤销全部浏览器设备。 |

删除操作要求 PAT 具备 `account:update`，并完成用途为 `devices` 的近期确认。Cookie 客户端通过 `POST /user/confirm-password` 确认，继续遵循 CSRF 机制；令牌客户端通过 `POST /api/auth/confirm` 领取单次证明，放入 `X-Auth-Confirmation`。Cookie 未确认返回423。授权有效时，重复撤销已有记录返回204。浏览器撤销不影响独立 PAT；需要撤销全部凭据时使用[全设备退出](#credential-lifecycle)。

```sh
curl "$APP_URL/api/user/sessions" -H "Authorization: Bearer <token>" -H "Accept: application/json"
curl -X DELETE "$APP_URL/api/user/sessions/<opaque-device-id>"   -H "Authorization: Bearer <token>" -H "X-Auth-Confirmation: <proof>" -H "Accept: application/json"
```

摘要仅有 `id`、`ip_address`、`user_agent`、`last_active`（Unix 秒）及 `is_current_device`。PAT 请求的设备均不标为当前设备。ID 是服务端生成的小写摘要，不是认证凭据；不返回原始会话 ID、加密载荷、CSRF 或登录指纹。活动时间不代表在线状态。归属使用原生会话解码和 Guard 指纹核对，不能依赖只能表示一个 Guard 的 `sessions.user_id` 元数据。

撤销记录与敏感账户操作使用共同用户锁。旧请求写回原会话、或在撤销后轮换 ID，都不能建立新的有效设备；已领取的 Cookie 确认也须在事务内复核，不能再签发 PAT。已执行的业务不会被追溯取消。原生 remember token 属于用户共享：撤销浏览器会清除该用户所有旧的记住登录凭据，但保留其他仍有效的活动会话。设备策略只退出实际 Guard，保留另一个 Guard 及无关会话数据。

套件向现有 `web` 和 `api` 组追加 `TrackBrowserSessions<User>`，保留 Cookie、Session 及 CSRF 装配。自定义会话组可在 `StartSession` 之后加入 `openrust_fortify::starter::devices::TrackBrowserSessions::<User>::new()`。多个应用节点应共用 `APP_KEY`、旧密钥、数据库及会话配置；旧加密会话有效期间保留对应旧密钥。

```sh
./artisan auth:prune-browser-sessions --hours=24
```

命令按会话过期时间加保留期清理索引，包括已撤销记录；`--hours=0` 立即清理已经过期的记录。可重复执行，不删除活动记录、用户、PAT 或外部会话数据。通过原生调度器定期运行；过期及撤销不等待清理才生效。

## API 令牌管理页面 {#token-management-page}

完整预设提供 `GET /user/api-tokens`，本人账户页面含跳转入口。页面使用原生 web Guard、Session 和 CSRF 中间件，浏览器不必先创建 PAT 才能管理 PAT。仅 API 预设不发布此页面，仍提供[令牌接口](#account-tokens)。

先通过页面链接确认密码，再填写令牌名称并显式勾选能力。`POST /user/api-tokens` 复用 API 的凭据锁事务及可配置的30天期限。未勾选的能力不会被暗中授予；未选能力或提交服务端白名单之外的能力时验证失败，未验证邮箱的能力限制同样生效。

创建结果页只展示一次令牌明文，请立即复制并妥善保存。后续 GET 只展示安全摘要；明文不进入闪存、URL 或 Cookie。响应设置 `Cache-Control: no-store`，页面设置 `Referrer-Policy: no-referrer`。不要将含新令牌的截图或页面内容写入日志；数据库仍只保存原生 Sanctum 哈希。

每个令牌含撤销表单，通过标准方法覆盖请求 `DELETE /user/api-tokens/{token}`。撤销同样要求近期确认及 CSRF，在用户锁内核对本人归属后重定向并显示本地化状态，不退出当前浏览器。应用可修改发布的 `resources/views/user/api-tokens.blade.html`，不必替换底层认证契约。

服务端能力白名单为空时，页面关闭新建令牌，仍保留已有令牌的撤销入口。

## 可靠资源清理 {#resource-cleanup}

安装的套件提供 `openrust_fortify::starter::resource_cleanup::ResourceCleanup`，供可信的应用 Action 使用。这是头像替换与账户注销共用的后端；头像消费方式见[用户头像](#profile-photos)，账户注销的应用 Action 与入口见[对应章节](#account-deletion)。

清理账本和原生数据库队列任务在本人事务中一起写入；回滚会保留原有资源。sync、Redis、被替换的 database 处理器或其他数据库连接无法提供该保证，该清理队列会明确拒绝，不修改应用默认队列，其他任务仍可使用 Redis。生成的 `config/user.rs` 默认选择 `USER_CLEANUP_CONNECTION=database`、`USER_CLEANUP_QUEUE=user-resources`。User 数据库须具有标准 jobs 表；自定义 User 连接时，配置专用的同库数据库队列连接。

```sh
./artisan queue:work database --queue=user-resources --tries=3 --backoff=5
./artisan auth:prune-user-cleanups
# 队列配置变化后，可显式指定记录所属连接：
./artisan auth:prune-user-cleanups --database=mysql
```

Worker 命令须使用配置后的连接和队列名称。多个独立应用共用 jobs 表时应使用不同队列名；同一应用的多个节点须共享存储和数据库配置。Worker 从自己的文件系统配置读取凭据，队列载荷不含凭据。升级时保留应用配置，须先合并新 `cleanup` 配置组，再使用对应环境变量覆盖。

Action 先核对本人归属与近期认证，再在提供的用户事务内调用 `ResourceCleanup::enqueue`。仅接受套件 `profile-photos/` 目录中的独占随机文件名，不能把同一路径重新用于另一张图片。返回的收据明文只交付一次，数据库只保存摘要。`ResourceCleanup::status` 只返回 `pending`、`failed` 或 `completed` 等有限状态，不公开用户资料或内部文件路径，也不依赖 User 仍然存在。收据七天后过期。存储删除失败仍可追踪，可用原生 Worker 重试或 `queue:retry` 恢复；重复投递保持幂等，不假装 SQL 和对象存储拥有共同事务。

记录清理命令只删除收据已过期的完成记录，保留待处理和失败任务、用户、令牌及外部文件。命令幂等，可由正常调度器定期执行。账户注销使用同一清理生命周期。

## 用户头像 {#profile-photos}

两种预设提供 `POST /api/user/profile-photo`（multipart 字段 `photo`）及 `DELETE /api/user/profile-photo`，前缀跟随 `api.prefix`。需要选中的用户和 `account:update` 能力。PAT 每次操作先从 `/api/auth/confirm` 获取 `avatar` 用途的证明，通过 `X-Auth-Confirmation` 发送；Cookie 客户端使用近期密码确认及 CSRF。完整预设在 `/home` 增加上传与移除表单；API 预设不增加账户页面。

`user.avatar` 默认 `enabled=true`、磁盘 `public`、2048 KiB、每边4096像素。设置 `USER_AVATAR=false` 可关闭接口及表单。默认处理器要求明确公开的磁盘；私有头像须使用应用自定义处理器与资源。实际内容检测与解码只接受 JPEG／PNG／WebP，并取套件及 Image 上限中较严格者；客户端原文件名不会作为存储路径。

```sh
# TOKEN 为具有 account:update 能力的 PAT；PROOF 为新取得的 avatar 确认证明。
curl -X POST "$APP_URL/api/user/profile-photo" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -F 'photo=@avatar.png'
```

默认用户资源增加可空的 `avatar_url`，不公开路径或磁盘名。首次上传返回200；替换或移除已有图片返回202，`meta.cleanup` 包含 `id`、仅交付一次的 `receipt`、`expires_at`、`status_url` 和 `status=pending`。自定义资源自行决定头像输出。更新头像保留浏览器会话、CSRF 和近期确认。

写入新文件前先提交持久化延迟预留，正式附着与完成预留在同一用户事务内提交。失败或中断的上传在15分钟宽限期后回收，取消的写入还有最终清理；已附着头像不受延迟或重复投递影响。旧头像删除任务与替换引用同事务入队。部署时须启动[资源清理 Worker](#resource-cleanup)，并配置重试。

```sh
# 收据不要放入 URL；有限状态查询不需要登录 Cookie。
curl "$STATUS_URL" -H "X-Operation-Receipt: $RECEIPT"
```

查询只返回 pending／failed／completed 等状态和文件数量。缺失、错误或过期收据返回404；通过 query 传入收据返回400。响应使用 `no-store` 和 `no-referrer`，Telescope 隐藏收据标头及 `meta.cleanup.receipt`。收据应保密。公开本地磁盘 URL 与应用同源时使用原生静态磁盘路由提供文件；多节点与 Worker 须共享磁盘根目录或对象存储，本地共享挂载可设置 `FILESYSTEM_PUBLIC_ROOT`。CDN／对象存储 URL 由对应服务提供，生产存储 ACL／CDN 行为须在部署环境验收。

## 账户注销 {#account-deletion}

两种预设提供 `DELETE /api/user`，遵循 API 前缀配置。PAT 需要 `account:update` 和用途为 `delete` 的新单次证明；Cookie 客户端使用近期密码确认和 CSRF。完整预设在 `/home` 提供原生注销表单，提交到 `DELETE /user`，未确认时跳转密码确认页。

```sh
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"delete","password":"your-current-password"}'
# 将 data.proof 读入 PROOF，随后仅使用一次：
curl -i -X DELETE "$APP_URL/api/user" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF"
```

安装发布 `app/actions/fortify/delete_user.rs`，由应用 UserServiceProvider 选择。修改其中实现 `starter::deletion::DeletesUsers<User>` 的 `DeleteUser`，定义业务依赖及删除策略。可选 `preflight` 在撤销凭据前运行；`delete` 必须使用传入的用户事务。默认转发原生模型删除，不自动删除业务关联行或远端身份供应商账户。软删除策略需要应用实际配置软删除模型及列；只返回成功却未删除会被拒绝。

需要在 Provider 启动时显式替换 Action，可使用：

```rust
openrust_fortify::starter::deletion::delete_users_using::<crate::app::models::User>(
    app,
    std::sync::Arc::new(crate::app::actions::fortify::delete_user::DeleteUser),
)?;
```

用户锁与令牌签发、账户变更串行。同一事务内撤销凭据版本，通过已注册模型事件物理清理全部本人 PAT 及已安装通行密钥，删除套件待用证明、撤销浏览器设备索引、持久化旧头像清理任务，最后删除用户。凭据存储须与用户同连接，不能原子处理的跨连接配置明确失败。模型取消、依赖约束或入队失败会回滚数据库更改，保留旧头像。密码重置链接绑定原身份，注销后不能使用；旧重置记录仍由对应仓储的过期与清理机制管理。

提交后退出当前浏览器中属于同一用户的全部 Guard，并轮换会话及 CSRF；独立管理员身份、购物车数据保留。已经进入执行的业务请求不被追溯取消。应用模型钩子的外部副作用应推迟到提交后。

没有外部文件时 API 返回204，HTML 表单跳转登录；存在头像时返回202。JSON 带 `meta.cleanup`，HTML 提供私密状态页及刷新按钮。此时账户已删除，文件尚待清理。妥善保管收据，使用[已有公开收据查询与 Worker](#resource-cleanup)，无需已删除用户的会话。存储失败保留失败及重试状态，不把已提交的注销冒充回滚；不要将收据放进 URL 或日志。

## 用户套件通行密钥页面 {#user-passkey-pages}

完整用户套件与可选通行密钥扩展一起安装：`./artisan init --starter-kit=full --extensions=api,fortify,passkeys`。应用环境保留 `USER_PASSKEYS=true`，设置公开 `APP_URL`，在 `config/passkeys.rs` 配置依赖方和允许的来源，再执行生成的迁移。生产环境需要 HTTPS，本地开发可用 localhost。此功能复用固定版本的原生通行密钥动作及本地打包客户端，不依赖运行时 CDN 或 npm 安装；请随客户端保留发布的 `public/vendor/user-starter/passkeys.NOTICE`。

账户页提供 `/user/passkeys` 入口。确认身份后填写设备名称并注册通行密钥，登录页和身份确认页也提供通行密钥按钮。启用双因素认证时，通行密钥登录仍需完成 TOTP 或恢复码验证。注册和撤销共用用户事务及敏感操作的近期确认策略；撤销本次确认所用的通行密钥后，下次敏感请求要求重新确认。无本地密码账户必须通过已绑定的通行密钥确认，或通过邮件重置流程安全设置本地密码，页面不会跳过确认。

本地客户端使用原生 `/user/passkeys/options`、`/user/passkeys`、`/passkeys/login/options`、`/passkeys/login`、`/passkeys/confirm/options` 和 `/passkeys/confirm` 路由，发送受 CSRF 保护的会话请求。`options` 和 `redirect` 字段保留客户端协议，这些路由不是 Sanctum Bearer Token 接口；应用业务 JSON API 继续使用用户套件响应封装。多节点需保持同一公开来源并共享配置的会话和原子挑战缓存。应用视图可以修改，重复安装默认保留定制文件，只有显式强制安装才覆盖。
