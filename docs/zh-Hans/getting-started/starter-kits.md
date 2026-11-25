# 入门套件

## 支持状态 {#support-status}

显式用户套件预设可安装原生 Fortify Action、安全用户响应和基础 Blade 认证页面。完整账户中心、个人令牌登录协议和可选因素 UI 仍在开发，不能据此视为已完成。不会生成 React、Vue 或 Livewire 前端。

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

`UserResource` 只读取 `id`、`name`、`email`、`email_verified_at`，保留字符串主键，不枚举属性或读取凭据。自定义资源可替换原有 Fortify 响应绑定。单独使用响应适配不会签发令牌或改变 Guard。

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
      "email_verified_at": null
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

本节覆盖设备登录和 TOTP／恢复码挑战。纯令牌客户端注册、邮件验证／找回及完整账户中心仍在接线，近期确认与令牌管理见下节，单独跟踪剩余工作。

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

第一方 Cookie／SPA 保留正常 CSRF 流程并发送凭据。`POST /user/confirm-password` 将确认状态记入当前 Session，原生 Passkey 确认也写入相同状态；随后向 `POST /api/user/tokens` 提交 `device_name` 和 `abilities`，此模式不需要另一份不透明证明。`POST /api/auth/confirm` 也接受有状态 Cookie 请求，成功201且不返回 `data`。其他浏览器会话不会继承确认；缺失或过期返回423及本地化 code `password_confirmation_required`，缺少 CSRF 令牌仍返回419。

PAT 证明在应用数据库中只保存摘要，绑定用户、当前PAT、用途、凭据版本和有效的因素／邮箱状态。`auth.password_timeout` 为正整数秒，默认10800；不等清理任务就会到期。错误设备／用途、篡改或重放返回401。签发／撤销使用共同用户锁和令牌行锁，模型事件取消或提交前证明过期会回滚数据库变更。应用确认回调可支持自己的身份流程，没有本地密码不代表自动确认。无状态 Passkey 重新认证适配仍单独待补。

密码错误422，一分钟内5次失败后429并带 `Retry-After`；缺少令牌能力403。消息跟随请求语言，证明、密码和令牌秘密不会进入旧输入；包含新证明／令牌的响应使用 `no-store`。完整账户中心、头像、设备列表和账户注销仍在开发。

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

此操作与[设备列表](#scope)是两个能力，撤销可用不代表所有会话驱动都已提供设备枚举。

## 无会话客户端注册与邮件恢复 {#client-mail}

显式安装套件后，以下路由读取 `api.prefix`（默认 `/api`）。它们复用 Fortify Action、验证器、密码代理和通知；独立 Fortify 的原有 Cookie／CSRF 路由不变。

| 方法 | 路径 | 成功行为 |
| --- | --- | --- |
| POST | `/api/auth/register` | 201，返回四字段用户资源；不自动登录或签发令牌。 |
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

两种预设都发布 `app/http/resources/user_resource.rs`。其中 `UserResource` 实现原生 `JsonResource`，开发者修改 `to_array` 即可选择公开字段；默认使用四字段安全投影，不要整体序列化用户模型。生成的 Provider 为所属应用选择该资源：

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
