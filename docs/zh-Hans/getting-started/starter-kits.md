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
