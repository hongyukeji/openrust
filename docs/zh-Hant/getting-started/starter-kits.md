# 入門套件

## 支援狀態 {#support-status}

明確選取的使用者套件安裝原生 Fortify Action、安全資源、Blade 認證及 Cookie／權杖 API。下文說明已提供的操作；已提供帳戶註銷介面，可選因素管理頁面尚未提供。不產生 React、Vue 或 Livewire 前端。

## 可用入口 {#available-entry-points}

```shell
./artisan init --starter-kit=full --extensions=api,fortify
# 僅 API，保留郵件連結所需的最小網頁：
./artisan init --starter-kit=api --extensions=api,fortify
```

先設定應用資料庫與郵件。命令發佈資源、註冊 Provider、重建並處理遷移；自動化可在設定完成後使用 `--no-interaction`。`all` 不會自動安裝使用者套件，兩種預設只能選擇一種。

兩種預設均啟用第一方 stateful API 認證。完整預設增加登入、註冊與本人頁面；API 預設僅保留郵件連結落地頁面。API 路徑遵循 `config/api.rs` 與 `API_PREFIX`：受保護使用者介面預設為 `/api/user`，設定 `API_PREFIX=api/admin` 後為 `/api/admin/user`。原有公開 API 資訊路由保持可用。

Cookie 用戶端須先取得 CSRF Cookie，並傳送最新的 `X-XSRF-TOKEN`，登入導致工作階段輪換後也要更新。請求 JSON 不代表簽發個人權杖。參閱 [Sanctum](../packages/sanctum.md) 與 [Fortify](../packages/fortify.md)。

## 設定與既有應用 {#scope}

產生的 `config/user.rs` 控制 `USER_STARTER_PROFILE`、註冊、信箱驗證與可選因素。註冊與驗證預設啟用，額外因素預設關閉。原有 Guard 與認證前綴保持有效。使用驗證與密碼找回前，應設定可用的郵件傳輸。

重複安裝保留已發佈檔案。`--force` 明確替換所選產生資源，執行前應檢查業務修改。無法安全辨識的自訂 Provider 清單、bootstrap 中介軟體或 API 使用者處理器需要手動接線，不會靜默覆寫。標準使用者處理器之外的業務路由會保留。

## 可選 JSON 回應 {#json-responses}

產生的應用 Provider 自動安裝 `openrust_fortify::starter::StarterKitServiceProvider`。既有 Fortify 應用也可僅註冊回應配接，不安裝頁面：

```rust
Box::new(openrust_fortify::starter::StarterKitServiceProvider),
```

HTML 保留原生重新導向與快閃訊息。JSON 登入、註冊與雙因素登入完成後，透過 `ApiResponse` 回傳目前使用者，註冊使用201。更新省略空資料；登出與信箱驗證保留204空本文。訊息依應用語言顯示，認證成功回應使用 `Cache-Control: no-store`。

`UserResource` 只讀取 `id`、`name`、`email`、`email_verified_at` 及可為空的 `avatar_url`，保留字串主鍵，不列舉屬性或讀取憑證。自訂資源可替換原有 Fortify 回應綁定。單獨使用回應配接不會簽發權杖或變更 Guard。

## 郵件連結與密碼工作階段 {#mail-and-sessions}

套件使用 Fortify 原生的 `POST /forgot-password`、`POST /reset-password`、`PUT /user/profile-information` 與 `PUT /user/password`。JSON 用戶端傳送 `Accept: application/json`；Cookie 認證須同時攜帶目前 CSRF Cookie。成功更新回傳200，欄位或重置憑證無效回傳422。驗證連結要求本人已認證、目前信箱雜湊相符、簽章有效且未過期。JSON 驗證成功使用204空本文，重複驗證保持冪等。更換信箱會清除驗證狀態並向新地址傳送連結。

API 預設也提供最小登入、密碼重置與驗證網頁。未登入瀏覽器開啟驗證郵件後，登入仍回到原始簽章 URL。驗證收據讀取資料庫狀態，加入 `?verified=1` 不能讓帳戶變成已驗證。依賴郵件投遞前須設定傳輸。設定 `USER_EMAIL_VERIFICATION=false` 會關閉驗證路由與這類驗證郵件，密碼找回保持可用。

已安裝套件在登入與有效 remember Cookie 恢復時立即儲存既有密碼指紋，Guard 在後續認證讀取中拒絕缺少或過時指紋。修改密碼保留目前瀏覽器工作階段並拒絕其他密碼工作階段；重置也會輪換 remember token。獨立認證的預設行為不變。


### 驗證身分與模型主鍵

簽章驗證 URL 使用模型主鍵，它可以與認證識別不同。預設 User 在 `MustVerifyEmail` 中宣告 `get_email_verification_identifier`，非 Eloquent 實作保留沿用認證識別的預設值。自訂模型以信箱或其他欄位認證時，在現有 `MustVerifyEmail` 實作中加入以下方法，傳回真實 Eloquent 主鍵：

```rust
fn get_email_verification_identifier(
    &self,
) -> openrust::auth::AuthResult<openrust::auth::AuthIdentifier> {
    self.get_key()
        .ok_or(openrust::auth::AuthError::InvalidIdentity)?
        .try_into()
}
```

`VerifyEmail::send` 與 `VerifyEmail::url` 使用這個身分。`EmailVerificationRequest` 核對已認證的具體身分、路由主鍵及目前信箱雜湊，認證識別相同的其他模型不能互換。原有 `auth::email_verification::set_verification` 輔助方法在模型自身連線上按真實主鍵、精確目前信箱和驗證狀態做條件更新；`EmailVerificationRequest` 只為實際成功轉換派發 `Verified`，輔助方法自身不派發儲存事件，並行有效請求保持冪等。驗證時間亦使用應用 Date 時鐘，不改變簽章演算法。

## 憑據生命週期 {#credential-lifecycle}

明確安裝套件提供兩份遷移範本（發佈時依 Laravel 機制產生時間戳記檔名）：`0001_01_01_000020_add_credential_version_to_users_table.rs` 和 `0001_01_01_000021_add_credential_stamp_to_personal_access_tokens_table.rs`，跟隨所設定的使用者及權杖模型資料表和連線。升級應用開始服務前先執行正常遷移。啟用此策略後，既有工作階段須重新驗證，既有個人權杖須重新簽發；請勿手動重設憑據版本。

產生的 User 保留 Sanctum 的 `create_token` 入口。在已驗證的應用作用域內使用，先完成所需的近期驗證及額外因素；此服務不會替呼叫方執行這些 HTTP 檢查：

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

`user.token_expiration_days` 預設30天。明確傳入較短期限會保留，較長期限會受到限制；Sanctum 全域期限可以更早拒絕權杖。`user.token_abilities` 是伺服器端能力白名單，未知或空能力會報錯。明文只回傳一次，資料庫僅儲存摘要及隱藏的憑據標記，請勿寫入日誌。過期紀錄繼續用 `sanctum:prune-expired` 清理，驗證不等待清理才拒絕過期權杖。

產生的密碼 Action 在修改密碼的同一交易內遞增持久化使用者憑據版本，保留目前瀏覽器登入，使其他工作階段及個人權杖失效；密碼重設使全部舊登入失效。一般密碼雜湊升級及原生「登出其他瀏覽器工作階段」不會撤銷個人權杖。自訂 Action 應呼叫 `openrust_fortify::starter::credentials::change_password`，直接寫入資料庫密碼不會自動推進此策略。`credentials::revoke_all` 也支援沒有本機密碼的帳戶，呼叫方須先取得有效的近期驗證。

修改憑據與簽發權杖會在提交前鎖定並重新核對目前身分／版本，要求使用者和權杖使用同一資料庫連線身分；獨立設定的權杖連線會明確失敗，不假裝具有原子安全保證。自訂使用者模型實作 `Clone` 並保留 `credential_version`；自訂權杖模型保留 `credential_stamp` 並在序列化中隱藏。含暫態欄位的權杖模型可覆寫 `PersonalAccessTokenModel::clone_token_model` 回傳 `self.clone()`，使屬性存取器保留這些狀態。

以上為生命週期服務和產生的密碼 Action。完整帳戶中心仍在開發；裝置登入及近期確認／權杖管理見下節。僅註冊回應適配器不會啟用憑據策略。


### 其他 Guard 與多態權杖歸屬

套件將憑據版本及登入時強制指紋限制在選定會話 Guard、匹配的 Eloquent Provider 設定及實際連線／資料表。獨立後台資料表不需要套件憑據欄位，其他身分的原生登入、密碼重新雜湊和 remember Cookie 保持原有行為。撤銷套件使用者裝置不會撤銷數值 ID 相同的獨立歸屬。

Sanctum 仍可認證其他已註冊權杖模型，它們在自己的路由照常使用；套件帳戶管理路由對其他使用者模型類型回傳401。套件附加歸屬檢查，不覆蓋應用既有驗證器。使用 `Sanctum::add_token_owner_validator` 附加檢查，所有檢查必須通過，錯誤不會放行；`validate_token_owner_using` 保留明確取代回呼的原有行為。

手動使用框架的應用仍可設定原有全域 `auth.session_credential_version_attribute`；未設定 `auth.session_credential_version_scope` 時保留全域語義。選用作用域包含 `guards` 與 `tables`（每項含 `connection`、`table`），格式錯誤會失敗，不靜默退回無限制。已安裝套件按設定裝配作用域；手動接入執行期 Provider 或異名模型別名時，須在開始服務前明確核對選中 Guard／連線並對應設定作用域，不能將預設 User 驗收當成自訂組合已通過。

## 明確選擇裝置權杖登入 {#client-tokens}

網頁和第一方 SPA 仍預設使用 Cookie 驗證。向 `/login` 請求 JSON 不會自動簽發個人權杖。已註冊帳戶可以明確申請裝置權杖：

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
      "name": "開發者",
      "email": "developer@example.test",
      "email_verified_at": null,
      "avatar_url": null
    }
  }
}
```

簽發成功使用 HTTP 201 和 `Cache-Control: no-store`。請安全儲存憑據：資料庫僅存摘要，後續讀取不會再次回傳明文。存取 `/api/user` 等受保護入口時傳送 `Authorization: Bearer TOKEN_SHOWN_ONCE`。伺服器端和行動權杖客戶端不需要瀏覽器工作階段；第一方請求仍須遵守狀態中介軟體和 CSRF 保護。

`device_name` 必填，最多255個字元；可選 `abilities` 是最多100個名稱的陣列。伺服器端依 `user.token_abilities` 核對，未知或空能力回傳422。啟用信箱驗證時，未驗證使用者只能取得與 `user.unverified_token_abilities` 的交集，預設是帳戶及權杖管理能力；判斷遵循實際 `MustVerifyEmail` 狀態。驗證成功不擴大既有權杖的能力。業務路由仍需應用授權及信箱驗證中介軟體。

路徑跟隨 `API_PREFIX`。設定 `API_PREFIX=api/admin` 後，使用 `/api/admin/auth/token` 和 `/api/admin/auth/token/challenge`；保留現有資訊頁與目前使用者入口。產生的 Provider 明確呼叫 `openrust_fortify::starter::client::register_routes::<User>(app)`，獨立 Fortify 應用不受影響。

帳戶存在已啟用的雙因素驗證時，正確密碼回傳 HTTP 200 和短期挑戰，不回傳存取權杖：

```json
{
  "success": true,
  "code": "auth_challenge_required",
  "message": "請完成額外身分驗證。",
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

向 POST `/api/auth/token/challenge` 傳送實際挑戰和目前 code，也可將 code 換成 `recovery_code`。完成後回傳相同的201權杖格式。挑戰五分鐘過期、僅能消費一次，保留原裝置名稱和已批准能力，不能用於存取資源。因素無效回傳422；挑戰過期、已消費或已失效回傳401。登入重用已設定的具名限流，因素嘗試另依帳戶限流，不能靠更換挑戰識別碼繞過。

```json
{
  "challenge": "OPAQUE_CHALLENGE",
  "recovery_code": "YOUR_RECOVERY_CODE"
}
```

簽發在使用者列鎖內重新檢查最新狀態。憑據版本、相關帳戶／因素狀態或應用加密金鑰變化會使待完成驗證失效。恢復碼驗證在資料庫鎖下單次消費，一般公開替換方法維持原生行為。兌換中斷或失敗時可能需要重新登入，不會遺留可重複使用的已授權挑戰。

原生時間戳記遷移發佈器在 User 連線中新增 `create_user_auth_challenges_table`，不儲存原始挑戰鍵或密碼。多節點共享資料庫、應用金鑰、驗證策略及快取；各節點讀取自己的應用設定。

```sh
./artisan auth:prune-challenges --hours=0
```

命令預設保留過期紀錄24小時，`--hours=0` 立即清理已經過期的紀錄。用現有[排程器](../digging-deeper/scheduling.md)安排執行，並配合 `sanctum:prune-expired`。驗證不等待清理才拒絕過期，重複清理安全。

訊息遵循應用／請求語言及回退。已發佈的 `lang/{locale}/user.json` 優先於套件編譯預設值；升級補足英語、簡體中文和繁體中文缺鍵，保留既有自訂翻譯。

本節涵蓋裝置登入和 TOTP／恢復碼挑戰。其他帳戶操作見[無工作階段註冊與找回](#client-mail)、[近期確認與個人權杖](#account-tokens)及[瀏覽器裝置](#browser-sessions)。

## 近期確認與個人權杖管理 {#account-tokens}

下列端點屬於明確安裝的使用者套件。API 路徑跟隨 `API_PREFIX`，範例使用 `/api`。一般 JSON 登入繼續使用 Cookie 工作階段。

| 方法與路徑 | 行為 |
| --- | --- |
| `POST /api/auth/confirm` | 驗證目前使用者密碼，或呼叫應用設定的確認回呼。 |
| `GET /api/user/tokens` | 僅列本人的安全權杖中繼資料；PAT 用戶端需要 `tokens:manage`。 |
| `POST /api/user/tokens` | 近期確認後建立裝置權杖，成功201。 |
| `DELETE /api/user/tokens/{`token`}` | 近期確認後僅撤銷本人權杖，成功204。 |
| `POST /api/auth/logout` | 撤銷本次實際驗證的 PAT，成功204，不要求近期證明。 |

PAT 用戶端先確認密碼，使用與後續敏感操作相同的 Bearer Token：

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

證明透過 `X-Auth-Confirmation` 標頭或 JSON 的 `proof` 欄位提交，只能使用一次。再次建立或撤銷前重新確認：

```http
POST /api/user/tokens
Authorization: Bearer YOUR_TOKEN
X-Auth-Confirmation: OPAQUE_SINGLE_USE_PROOF
Content-Type: application/json
Accept: application/json

{"device_name":"tablet","abilities":["account:read"]}
```

201回應與[裝置登入](#client-tokens)使用相同的 `token_type`、`token`、`expires_at` 和 `user` 欄位。省略 `abilities` 時採用伺服器策略與目前父權杖的交集。失敗可能消耗證明，重試前重新確認。子權杖不能超過伺服器白名單、未驗證帳戶策略及目前父權杖的能力。妥善保管僅回傳一次的明文權杖，權杖清單不會再次顯示它。

`GET /api/user/tokens` 的 `data` 為陣列，每項僅包含 `id`、`name`、`abilities`、`created_at`、`last_used_at`、`expires_at` 和 `current`，沒有雜湊、憑據版本摘要或內部歸屬屬性。撤銷其他使用者ID回傳404；`POST /api/auth/logout` 不接受用戶端指定其他憑據代替本次驗證權杖。成功204沒有本文，撤銷後驗證回傳401，其他裝置仍可使用。

第一方 Cookie／SPA 保留正常 CSRF 流程並傳送憑據。`POST /user/confirm-password` 將確認時間與套件身分快照記入目前 Session；隨後向 `POST /api/user/tokens` 提交 `device_name` 和 `abilities`，此模式不需要另一份不透明證明。`POST /api/auth/confirm` 也接受有狀態 Cookie 請求，成功201且不回傳 `data`。其他瀏覽器工作階段不會繼承確認；缺少或過期回傳423及本地化 code `password_confirmation_required`，缺少 CSRF 權杖仍回傳419。

PAT 證明在應用資料庫中僅儲存雜湊，綁定使用者、目前PAT、用途、憑據版本及有效的因素／郵箱狀態。`auth.password_timeout` 為正整數秒，預設10800；不等清理工作就會到期。錯誤裝置／用途、竄改或重放回傳401。簽發／撤銷使用共同使用者鎖及權杖列鎖，模型事件取消或提交前證明過期會回滾資料庫變更。應用確認回呼可支援自己的身分流程，沒有本機密碼不代表自動確認。無狀態 Passkey 重新驗證適配仍另列待補。

密碼錯誤422，一分鐘內5次失敗後429並帶 `Retry-After`；缺少權杖能力403。訊息跟隨請求語言，證明、密碼與權杖秘密不會進入舊輸入；包含新證明／權杖的回應使用 `no-store`。裝置管理見[瀏覽器裝置](#browser-sessions)；帳戶註銷見[對應章節](#account-deletion)。

### 登出全部裝置

`POST /api/auth/logout-all` 使套件全部舊會話、remember Cookie 和個人存取權杖失效，包括呼叫方目前權杖；不會修改密碼。PAT 須有 `account:update`，並提交 `purpose: "devices"` 的單次近期證明；Cookie 認證須有原生近期確認與 CSRF。成功204空本文。只撤銷目前 PAT 時使用 `/api/auth/logout`。

```sh
curl -i -X POST http://localhost:8000/api/auth/confirm \
  -H 'Authorization: Bearer <token>' -H 'Content-Type: application/json' \
  -d '{"purpose":"devices","password":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/logout-all \
  -H 'Authorization: Bearer <token>' -H 'X-Auth-Confirmation: <proof>' \
  -H 'Content-Type: application/json' -d '{}'
```

持久化憑據版本在與權杖簽發共用的使用者鎖中變更，模型事件結束後、提交前再次核對證明期限。儲存被取消或失敗時撤銷交易回復；失敗可消費證明，重試前重新確認。成功撤銷不依賴刪除全部會話檔案或掃描 Redis 鍵，各節點後續認證讀取會拒絕舊版本。舊權杖記錄仍可明確管理或清理，但不能認證。已進入業務執行的請求不被追溯取消；重新登入才能建立新憑據。

此操作與[裝置列表](#scope)是兩種能力，撤銷可用不代表所有會話驅動都已提供裝置列舉。

## 無會話用戶端註冊與郵件恢復 {#client-mail}

明確安裝套件後，以下路由讀取 `api.prefix`（預設 `/api`）。它們重用 Fortify Action、驗證器、密碼代理和通知；獨立 Fortify 原有的 Cookie／CSRF 路由不變。

| 方法 | 路徑 | 成功行為 |
| --- | --- | --- |
| POST | `/api/auth/register` | 201，傳回安全使用者資源；不自動登入或簽發權杖。 |
| POST | `/api/auth/forgot-password` | 200，本地化重設郵件狀態。 |
| POST | `/api/auth/reset-password` | 200，本地化重設狀態；重設憑據只能兌換一次。 |
| POST | `/api/auth/email/verification-notification` | Bearer 或有狀態 Cookie 認證；寄送成功202，已驗證204。 |

註冊、密碼找回和驗證路由分別依循對應功能開關。敏感憑據放在 JSON 本文，不放查詢字串。用戶端認證路由拒絕查詢憑據並回傳400；成功、認證失敗、驗證錯誤與限流回應均帶 `Cache-Control: no-store`。有狀態請求仍需 CSRF，回傳 JSON 不會取消此要求。

```sh
curl -i -X POST http://localhost:8000/api/auth/register \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"name":"開發者","email":"developer@example.test","password":"long-password-123","password_confirmation":"long-password-123"}'

curl -i -X POST http://localhost:8000/api/auth/forgot-password \
  -H 'Content-Type: application/json' -H 'Accept: application/json' \
  -d '{"email":"developer@example.test"}'
```

註冊後明確完成[權杖登入](#client-tokens)。原生用戶端驗證信箱時，向**郵件中的原始簽章 URL**送出 `Authorization: Bearer <token>` 和 `Accept: application/json`。不要將簽章路由搬到 `/api`、向 URL 附加憑據或重組查詢字串。簽章錯誤、過期或其他使用者的連結均不能通過。成功204，重複有效連結冪等。這個網頁落地路由可能產生匿名會話 Cookie，但 Bearer 驗證不會登入瀏覽器；瀏覽器亦可使用郵件登入落地頁。

找回密碼時，從原郵件連結取得重設憑據和信箱，與 `password`、`password_confirmation` 一起提交至 `/api/auth/reset-password`。缺失、過期或已消費的憑據回傳422；並行提交只有一個成功。重設使套件既有會話與個人存取權杖失效，須以新密碼重新登入。訊息依循應用語言；驗證簽章與密碼重設期限使用應用 Date 時鐘。

依賴郵件前先設定實際傳輸。同步寄送失敗回傳錯誤，不會報告已寄送；修復投遞後可明確重寄驗證通知。這些路由不承諾佇列自動補寄或真實外部信箱收信，應用通知回呼仍可替換。

## 應用使用者資源 {#user-resource}

兩種預設都發佈 `app/http/resources/user_resource.rs`。其中 `UserResource` 實作原生 `JsonResource`，開發者修改 `to_array` 即可選擇公開欄位；預設使用身分欄位及頭像的安全投影，不要整體序列化使用者模型。產生的 Provider 為所屬應用選擇該資源：

```rust
openrust_fortify::starter::UserResource::use_resource::<
    crate::app::models::User,
    crate::app::http::resources::user_resource::UserResource,
>(app)?;
```

JSON 註冊／登入、雙因素完成、目前使用者，以及個人權杖簽發中的使用者資料共用該資源。資源接收真實請求，先執行原生 `prepare_response`，再呼叫 `to_array`；回傳值必須為 JSON 物件，準備或序列化錯誤向外傳遞。這是嵌入認證回應的資料，資源的 `wrap`、`with`、狀態和 HTTP 回應掛鉤不會改變外層認證回應；要替換整個回應，繼續繫結現有 Fortify 回應契約。網頁帳戶與郵件頁面保留內部安全欄位，API 欄位改名不會破壞頁面。

重複安裝保留該檔案，`--force` 會替換。已有獨立檔案模組會重用；行內或條件不相符的資源模組會在套件寫入前報錯，提示手動接線。選擇儲存在所屬應用中，不使用程序全域回呼。

## 資料與密碼 API {#user-updates}

兩種預設都提供 `PUT /api/user/profile-information` 和 `PUT /api/user/password`，路徑跟隨 API 前綴。需要選中的使用者模型及 `account:update` 能力。資料更新（包括僅改姓名）使用 `email` 用途的近期確認；修改密碼使用 `password` 用途。PAT 證明繫結目前實際權杖、使用者和用途，有期限且單次消費，不能放在 URL 中。

```shell
# 先用目前密碼取得 email 用途證明：
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"purpose":"email","password":"current-password"}'
# 將 data.proof 儲存到 PROOF，再更新資料：
curl -X PUT "$APP_URL/api/user/profile-information" \
  -H "Accept: application/json" -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -d '{"name":"Alice","email":"alice@example.test"}'
```

修改密碼時重新取得 `purpose: "password"` 的證明，再向 `/api/user/password` 提交 `password` 和 `password_confirmation`。此 API 使用近期證明，不重複要求 `current_password`。成功200，回傳正常本地化的空載荷回應。密碼變化後舊 PAT 失效，權杖用戶端須用新密碼重新登入。瀏覽器使用原生 Session 近期確認與 CSRF，無需 PAT 證明；目前工作階段同步並輪換，其他舊工作階段與 remember Cookie 失效。輪換後重新取得 CSRF 權杖，下次敏感操作須重新確認。設定的 Fortify 工作階段 Guard 必須認證該瀏覽器身分。

信箱變化會清空驗證狀態，向新地址傳送原生驗證通知，舊信箱連結失效。儲存期間證明到期、模型取消或交易錯誤都會回滾資料庫變更。錯誤／已用 PAT 證明回傳401，缺少能力403，欄位錯誤422，瀏覽器未近期確認423，CSRF錯誤419。失敗可以消費證明，重試前重新確認。通知傳送和密碼代理清理在使用者交易提交後執行，沿用原生流程：此時失敗不會撤回已提交的資料／密碼，重試前先核對目前狀態。SMTP 投遞保證遵循設定的郵件驅動。

應用 Action 繼續可替換。API 在取得使用者鎖前呼叫 `validate_for_transaction`，再將同一個交易傳給 `update_in_transaction`，不會呼叫可能另取連線池連線儲存的舊 Action。預設資料驗證器使用實際模型的表和連線。自訂密碼 Action 應使用 `credentials` 中的 `change_password_in_transaction` 儲存摘要、遞增持久化憑據版本並清空 remember token；所有寫入使用傳入交易，回傳同一使用者／模型／連線。套件提交前複核證明期限。安裝器給標準 Action 追加方法並保留註解；自訂舊 Action 會停止並提示手動接線，已完整實作的自訂交易方法保持原樣。獨立 Fortify 保留原入口。

## 密碼重置的身分繫結 {#password-token-binding}

產生的使用者 Provider 為選中模型呼叫 `bind_password_tokens`。套件將重置記錄繫結到實際模型主鍵、連線／資料表範圍、重置信箱、密碼摘要及可選憑據版本。信箱被另一個帳號重新註冊後，原帳號的舊權杖不能重置新帳號；舊憑據快照延遲寫入的記錄，也不能在密碼／版本變化後授權重置。沒有本地密碼摘要的帳號同樣透過主鍵區分。密碼重新雜湊也會改變快照，使舊連結失效。共同使用者鎖在儲存前再次核對重置信箱：即使權杖先領取，信箱變更已經提交後也不能繼續用舊快照改密碼。

繼續使用現有 database／cache 倉儲、郵件中的64位權杖、到期、限流和原子消費，不新增遷移或平行權杖儲存。資料庫與設定的快取驅動執行相同繫結。繫結只作用於選中使用者模型及具名密碼代理；獨立 Fortify 和其他使用者類型保留原有行為。已快取代理讀取所屬應用的同一規則表，套件重複裝配保持冪等。

啟用後，先前未繫結的舊重置連結失效，需重新請求郵件；所有應用／Worker 節點須使用相同模型和繫結設定。若模型的重置能力回傳自訂包裝類型，或使用額外動態租戶範圍，應明確註冊相符規則。進階應用可透過 Password 的 `bind_user_tokens_using` 追加具名規則：不同名稱組合，同名替換，回呼錯誤直接失敗，不退回簽發未繫結權杖。規則名稱和值須在各節點穩定，不向用戶端回傳密碼或計算出的繫結；一般業務繼續呼叫既有忘記／重置密碼介面。

### 雙因素帳戶管理 {#user-two-factor-management}

安裝使用者套件並執行遷移後，設定 `USER_TWO_FACTOR=true` 啟用入口，預設關閉。完整套件的帳戶頁提供 `/user/two-factor-authentication` 連結，沿用原生 Fortify 表單、CSRF、設定的 Session Guard 與應用翻譯。依序啟用、掃描身分驗證器二維碼或輸入設定金鑰、按要求輸入 TOTP 確認，並離線保存八個復原碼。頁面也支援重新產生復原碼及關閉雙因素認證。設定詳情與復原碼使用 `no-store`、`no-referrer`，不寫入表單快閃資料，且要求近期密碼確認。啟用或確認因素後舊確認快照失效，出現提示時須再次確認密碼；隱藏設定詳情不會關閉認證因素。

僅 API 預設提供相同的管理能力，不發佈這些頁面。下表路徑依 API 前綴設定：

| 方法 | `/api` 後的路徑 | 回傳內容 |
| --- | --- | --- |
| GET | `/user/two-factor-authentication` | 安全狀態 `enabled`、`has_secret`、`confirmation_required`，要求 `account:read`。 |
| POST | `/user/two-factor-authentication` | 啟用，回傳設定 SVG、URL、`secretKey` 及 `recovery_codes`。 |
| POST | `/user/confirmed-two-factor-authentication` | 用 `code` 確認，回傳安全狀態。 |
| DELETE | `/user/two-factor-authentication` | 關閉，省略空成功資料。 |
| GET | `/user/two-factor-qr-code` | `svg`、`url`。 |
| GET | `/user/two-factor-secret-key` | `secretKey`。 |
| GET | `/user/two-factor-recovery-codes` | `data` 內的 `recovery_codes` 陣列。 |
| POST | `/user/two-factor-recovery-codes` | 產生並回傳替換後的 `recovery_codes`。 |

除安全狀態 GET 外，每次操作均要求 `account:update`，並透過 `POST /api/auth/confirm` 取得用途為 `factors` 的新單次證明。透過 `X-Auth-Confirmation` 請求標頭傳送，有本文的請求亦可使用 JSON `proof` 欄位。URL 查詢參數中的證明回傳400，避免進入存取日誌、歷史或複製連結。下一次操作須取得新證明，錯誤驗證碼後也一樣；證明不能重放或轉交其他權杖。Cookie 客戶端使用本瀏覽器近期確認的 Session 與 CSRF，不使用 PAT 證明。因素管理不會額外簽發存取權杖。

```bash
# 用目前 PAT 為這次操作取得證明。
curl -X POST http://localhost:8000/api/auth/confirm \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"factors","password":"your-password"}'

# 將 data.proof 存入 PROOF，再啟用身分驗證器。
curl -X POST http://localhost:8000/api/user/two-factor-authentication \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -H 'Content-Type: application/json' -d '{}'
```

寫入與敏感讀取均使用共同使用者鎖及同連線交易，在回傳前核對目前權杖及憑證快照。錯誤驗證碼回傳422，缺少或已消費證明回傳401，權杖權限不足回傳403，功能關閉時路由回傳404。因素啟用後，登入仍須提交 TOTP 或未使用的復原碼。獨立 Fortify 應用保留既有路由回應；額外的 API 封裝與更嚴格的確認要求屬於安裝後的使用者套件。

### 網頁資料與密碼表單 {#web-user-updates}

帳戶頁提供資料、密碼表單與身分確認入口。原生 `PUT /user/profile-information`、`PUT /user/password` 與上述API共用應用Action、使用者寫入鎖及同連線交易。姓名更新也需要近期確認；一般瀏覽器尚未確認時導向 `password.confirm`，確認後返回帳戶頁重新提交，密碼不會保存在舊輸入。JSON協商維持423與本地化提示。密碼帳戶的網頁密碼表單仍驗證 `current_password`，使用已設定的Fortify Guard。

Cookie確認綁定確認時的使用者、信箱、憑證、雙因素狀態及Session時間。其他裝置變更信箱／因素後，舊時間標記不足以繼續授權；重新確認後才可操作。密碼變更保留目前瀏覽器，舊會話及PAT失效。更新會輪換Session與CSRF，JSON／SPA客戶端須重新讀取CSRF；一般瀏覽器跟隨重新導向讀取新表單。未裝配使用者套件的獨立Fortify應用維持原生行為。

網頁保留原生快閃狀態碼，透過 `lang/<locale>/user.json` 翻譯顯示，例如 `password-updated`；覆寫對應項目即可自訂。JSON成功訊息仍使用ApiResponse本地化。升級前沒有套件快照的既有瀏覽器確認需要重新確認。

## 認證郵件佇列與開發預覽 {#auth-mail-queue}

信箱驗證開關只套用於套件的具體使用者類型，不會全域攔截應用的所有驗證通知；其他模型保留自己的驗證流程。進階 Provider 可用 `VerifyEmail::send_when::<User>(app, |user| Ok(true))` 決定該類型是否傳送，回呼錯誤直接使傳送失敗。條件在渲染及入列前執行，並依應用隔離。

預設沿用原生同步郵件。需要由獨立 Worker 傳送時，在安裝後產生的 `config/user.rs` 設定：

```rust
"mail": {
    "queue": get(env, "USER_MAIL_QUEUE", Value::Null)?,
    "connection": get(env, "USER_MAIL_QUEUE_CONNECTION", Value::Null)?,
},
```

```dotenv
QUEUE_CONNECTION=database
USER_MAIL_QUEUE=auth-mail
# 選用；省略時使用 queue.default。
USER_MAIL_QUEUE_CONNECTION=database
```

執行既有遷移後啟動消費程序：

```sh
./artisan migrate
./artisan queue:work database --queue=auth-mail --tries=3 --backoff=10
```

註冊／重寄驗證與密碼找回使用同一具名佇列；關閉信箱驗證後不會建立驗證郵件任務，密碼找回仍可使用。未設定或為 `null` 時恢復同步；空字串、逗號或首尾空白名稱無效。同步、丟棄、程序內驅動，以及包含這些驅動或循環的 failover 均被拒絕；持久化連線亦須依驅動正確註冊及設定。Redis 可將上面兩處 `database` 改為設定的佇列連線名稱。Worker 的 `--queue` 須相符；重試、退避及逾時沿用原生選項。

套件針對具體使用者類型分別設定 `ResetPassword` 與 `VerifyEmail`，其他應用與模型保留自己的策略。啟用後連結載荷強制加密，沿用原生通知的交易提交後派發。生產者及 Worker 須共用 `APP_KEY`、應用 URL、模型及佇列設定；Worker 從自身設定解析郵件傳輸與憑證。不得將密碼、簽名 URL 或重設權杖印到日誌。任務可保存郵件連線名稱，不因此序列化 SMTP 密碼。

需要獨立策略時，可在 Provider 呼叫 `ResetPassword::queue_for::<User>(app, Some(options))` 或 `VerifyEmail::queue_for::<User>(app, Some(options))`，其中 `options` 為 `openrust::auth::notifications::AuthMailQueueOptions`；傳入 `None` 恢復該通知的同步傳送。自訂 URL／郵件回呼仍有效。以包裝物件提供重設／驗證能力的模型會被自動註冊明確拒絕，應手動註冊實際能力類型，不靜默回退。升級保留既有 `config/user.rs`，請合併上述設定組並重新部署 Provider／Worker。

請求成功表示已入列，不能認定已收信。供應商暫時拒絕後，原生 Worker 依設定釋放並重試；耗盡次數後進入設定的失敗任務儲存。SMTP 在供應商受理後、佇列確認前崩潰可能重複投遞，不保證恰好一次；重設權杖維持單次消費，驗證連結維持冪等。

開發可使用獨立 [Mailpit](https://mailpit.axllent.org/docs/install/docker/) 預覽，不連接真實收件人：

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

瀏覽器開啟 `http://127.0.0.1:8025`。以上假設應用在宿主機執行；容器內應用使用共用網路中的郵件服務名。既有 `MAIL_URL` 優先於單項設定，預覽時須移除或設為 `null`。此實例停止後預覽資料消失，不作正式郵件服務；真實供應商憑證、生產 TLS、投遞與最終收信須另行驗收。

## 瀏覽器裝置 {#browser-sessions}

完整預設同時發布 `GET /user/sessions` 頁面。原生 HTML 表單支援撤銷單個或其他裝置，保留 CSRF 和近期密碼確認，完成後重新導向並顯示本地化狀態。僅 API 預設不發布此帳戶頁面。

升級保留的 `config/user.rs` 時，手動合併以下設定群組；舊檔案不會自動讀取 `USER_DEVICES`。安裝器保留應用已有設定。

```rust
"devices": {"enabled": get(env, "USER_DEVICES", true)?},
```

安裝的套件預設開啟 `user.devices.enabled`。使用原生 `SESSION_DRIVER=database`，並執行發布的 `0001_01_01_000023_create_user_browser_sessions_table.rs` 遷移。獨立索引跟隨 User 連線，依模型、資料庫、資料表、真實主鍵及 Guard 隔離，不修改原始 users 遷移。未登記索引的舊工作階段須重新登入。file、Redis 或自訂 database 處理器須明確設定 `USER_DEVICES=false`，關閉相關路由，不假裝提供安全列舉；依憑據版本登出全部裝置的能力仍可使用。 安裝器按正常套件遷移規則，發布時使用目前時間戳命名。

| 介面 | 結果 |
| --- | --- |
| `GET /api/user/sessions` | 200，`data` 為本人的瀏覽器裝置摘要；PAT 需要 `account:read`。 |
| `DELETE /api/user/sessions/{session}` | 204，撤銷指定不透明裝置 ID；不存在或其他使用者的 ID 回傳404。 |
| `DELETE /api/user/sessions` | 204，撤銷其他瀏覽器裝置，保留目前 Cookie 工作階段；PAT 沒有目前瀏覽器，因此撤銷全部瀏覽器裝置。 |

刪除操作要求 PAT 具備 `account:update`，並完成用途為 `devices` 的近期確認。Cookie 用戶端透過 `POST /user/confirm-password` 確認，繼續遵循 CSRF 機制；權杖用戶端透過 `POST /api/auth/confirm` 領取單次證明，放入 `X-Auth-Confirmation`。Cookie 未確認回傳423。授權有效時，重複撤銷已有紀錄回傳204。瀏覽器撤銷不影響獨立 PAT；需要撤銷全部憑據時使用[全裝置登出](#credential-lifecycle)。

```sh
curl "$APP_URL/api/user/sessions" -H "Authorization: Bearer <token>" -H "Accept: application/json"
curl -X DELETE "$APP_URL/api/user/sessions/<opaque-device-id>"   -H "Authorization: Bearer <token>" -H "X-Auth-Confirmation: <proof>" -H "Accept: application/json"
```

摘要僅有 `id`、`ip_address`、`user_agent`、`last_active`（Unix 秒）及 `is_current_device`。PAT 請求的裝置均不標為目前裝置。ID 是伺服器產生的小寫摘要，不是認證憑據；不回傳原始工作階段 ID、加密載荷、CSRF 或登入指紋。活動時間不代表在線狀態。歸屬使用原生工作階段解碼與 Guard 指紋核對，不能依賴只能表示一個 Guard 的 `sessions.user_id` 中繼資料。

撤銷紀錄與敏感帳戶操作使用共同使用者鎖。舊請求寫回原工作階段、或在撤銷後輪換 ID，都不能建立新的有效裝置；已領取的 Cookie 確認也須在交易內複核，不能再簽發 PAT。已執行的業務不會被追溯取消。原生 remember token 屬於使用者共用：撤銷瀏覽器會清除該使用者所有舊的記住登入憑據，但保留其他仍有效的活動工作階段。裝置策略只登出實際 Guard，保留另一個 Guard 及無關工作階段資料。

套件向現有 `web` 與 `api` 群組追加 `TrackBrowserSessions<User>`，保留 Cookie、Session 及 CSRF 裝配。自訂工作階段群組可在 `StartSession` 之後加入 `openrust_fortify::starter::devices::TrackBrowserSessions::<User>::new()`。多個應用節點應共用 `APP_KEY`、舊金鑰、資料庫及工作階段設定；舊加密工作階段有效期間保留對應舊金鑰。

```sh
./artisan auth:prune-browser-sessions --hours=24
```

命令依工作階段過期時間加保留期清理索引，包括已撤銷紀錄；`--hours=0` 立即清理已經過期的紀錄。可重複執行，不刪除活動紀錄、使用者、PAT 或外部工作階段資料。透過原生排程器定期執行；過期與撤銷不等待清理才生效。

## API 權杖管理頁面 {#token-management-page}

完整預設提供 `GET /user/api-tokens`，本人帳戶頁面含跳轉入口。頁面使用原生 web Guard、Session 和 CSRF 中介軟體，瀏覽器不必先建立 PAT 才能管理 PAT。僅 API 預設不發布此頁面，仍提供[權杖介面](#account-tokens)。

先透過頁面連結確認密碼，再填寫權杖名稱並明確勾選能力。`POST /user/api-tokens` 複用 API 的憑據鎖交易及可設定的30天期限。未勾選的能力不會被暗中授予；未選能力或提交伺服器白名單之外的能力時驗證失敗，未驗證信箱的能力限制同樣生效。

建立結果頁只展示一次權杖明文，請立即複製並妥善保存。後續 GET 只展示安全摘要；明文不進入閃存、URL 或 Cookie。回應設定 `Cache-Control: no-store`，頁面設定 `Referrer-Policy: no-referrer`。不要將含新權杖的截圖或頁面內容寫入日誌；資料庫仍只保存原生 Sanctum 雜湊。

每個權杖含撤銷表單，透過標準方法覆寫請求 `DELETE /user/api-tokens/{token}`。撤銷同樣要求近期確認及 CSRF，在使用者鎖內核對本人歸屬後重新導向並顯示本地化狀態，不登出目前瀏覽器。應用可修改發布的 `resources/views/user/api-tokens.blade.html`，不必替換底層認證契約。

伺服器能力白名單為空時，頁面關閉新建權杖，仍保留已有權杖的撤銷入口。

## 可靠資源清理 {#resource-cleanup}

安裝的套件提供 `openrust_fortify::starter::resource_cleanup::ResourceCleanup`，供可信的應用 Action 使用。這是頭像替換與帳戶註銷共用的後端；頭像消費方式見[使用者頭像](#profile-photos)，帳戶註銷的應用 Action 與入口見[對應章節](#account-deletion)。

清理帳本和原生資料庫佇列任務在本人交易中一起寫入；回滾會保留原有資源。sync、Redis、被替換的 database 處理器或其他資料庫連線無法提供該保證，該清理佇列會明確拒絕，不修改應用預設佇列，其他任務仍可使用 Redis。產生的 `config/user.rs` 預設選擇 `USER_CLEANUP_CONNECTION=database`、`USER_CLEANUP_QUEUE=user-resources`。User 資料庫須具有標準 jobs 資料表；自訂 User 連線時，設定專用的同庫資料庫佇列連線。

```sh
./artisan queue:work database --queue=user-resources --tries=3 --backoff=5
./artisan auth:prune-user-cleanups
# 佇列設定變化後，可明確指定紀錄所屬連線：
./artisan auth:prune-user-cleanups --database=mysql
```

Worker 命令須使用設定後的連線和佇列名稱。多個獨立應用共用 jobs 資料表時應使用不同佇列名稱；同一應用的多個節點須共用儲存和資料庫設定。Worker 從自己的檔案系統設定讀取憑據，佇列載荷不含憑據。升級時保留應用設定，須先合併新 `cleanup` 設定群組，再使用對應環境變數覆寫。

Action 先核對本人歸屬與近期認證，再在提供的使用者交易內呼叫 `ResourceCleanup::enqueue`。僅接受套件 `profile-photos/` 目錄中的獨占隨機檔名，不能把同一路徑重新用於另一張圖片。回傳的收據明文只交付一次，資料庫只保存摘要。`ResourceCleanup::status` 只回傳 `pending`、`failed` 或 `completed` 等有限狀態，不公開使用者資料或內部檔案路徑，也不依賴 User 仍然存在。收據七天後過期。儲存刪除失敗仍可追蹤，可用原生 Worker 重試或 `queue:retry` 恢復；重複投遞保持冪等，不假裝 SQL 和物件儲存擁有共同交易。

紀錄清理命令只刪除收據已過期的完成紀錄，保留待處理和失敗任務、使用者、權杖及外部檔案。命令冪等，可由正常排程器定期執行。帳戶註銷使用同一清理生命週期。

## 使用者頭像 {#profile-photos}

兩種預設提供 `POST /api/user/profile-photo`（multipart 欄位 `photo`）及 `DELETE /api/user/profile-photo`，前綴跟隨 `api.prefix`。需要選中的使用者和 `account:update` 能力。PAT 每次操作先從 `/api/auth/confirm` 取得 `avatar` 用途的證明，透過 `X-Auth-Confirmation` 傳送；Cookie 用戶端使用近期密碼確認及 CSRF。完整預設在 `/home` 增加上傳與移除表單；API 預設不增加帳戶頁面。

`user.avatar` 預設 `enabled=true`、磁碟 `public`、2048 KiB、每邊4096像素。設定 `USER_AVATAR=false` 可關閉介面及表單。預設處理器要求明確公開的磁碟；私有頭像須使用應用自訂處理器與資源。實際內容偵測與解碼只接受 JPEG／PNG／WebP，並取套件及 Image 上限中較嚴格者；用戶端原檔名不會作為儲存路徑。

```sh
# TOKEN 為具有 account:update 能力的 PAT；PROOF 為新取得的 avatar 確認證明。
curl -X POST "$APP_URL/api/user/profile-photo" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF" \
  -F 'photo=@avatar.png'
```

預設使用者資源增加可為空的 `avatar_url`，不公開路徑或磁碟名稱。首次上傳回傳200；替換或移除既有圖片回傳202，`meta.cleanup` 包含 `id`、僅交付一次的 `receipt`、`expires_at`、`status_url` 和 `status=pending`。自訂資源自行決定頭像輸出。更新頭像保留瀏覽器工作階段、CSRF 和近期確認。

寫入新檔案前先提交持久化延遲預留，正式附著與完成預留在同一使用者交易內提交。失敗或中斷的上傳在15分鐘寬限期後回收，取消的寫入還有最終清理；已附著頭像不受延遲或重複投遞影響。舊頭像刪除工作與替換引用同交易入列。部署時須啟動[資源清理 Worker](#resource-cleanup)，並設定重試。

```sh
# 收據不要放入 URL；有限狀態查詢不需要登入 Cookie。
curl "$STATUS_URL" -H "X-Operation-Receipt: $RECEIPT"
```

查詢只回傳 pending／failed／completed 等狀態和檔案數量。缺少、錯誤或過期收據回傳404；透過 query 傳入收據回傳400。回應使用 `no-store` 和 `no-referrer`，Telescope 隱藏收據標頭及 `meta.cleanup.receipt`。收據應保密。公開本地磁碟 URL 與應用同源時使用原生靜態磁碟路由提供檔案；多節點與 Worker 須共用磁碟根目錄或物件儲存，本地共用掛載可設定 `FILESYSTEM_PUBLIC_ROOT`。CDN／物件儲存 URL 由對應服務提供，正式儲存 ACL／CDN 行為須在部署環境驗收。

## 帳戶註銷 {#account-deletion}

兩種預設提供 `DELETE /api/user`，遵循 API 前綴設定。PAT 需要 `account:update` 與用途為 `delete` 的新單次證明；Cookie 用戶端使用近期密碼確認與 CSRF。完整預設在 `/home` 提供原生註銷表單，提交至 `DELETE /user`，未確認時導向密碼確認頁。

```sh
curl -X POST "$APP_URL/api/auth/confirm" \
  -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"purpose":"delete","password":"your-current-password"}'
# 將 data.proof 讀入 PROOF，隨後僅使用一次：
curl -i -X DELETE "$APP_URL/api/user" \
  -H "Authorization: Bearer $TOKEN" -H "X-Auth-Confirmation: $PROOF"
```

安裝發布 `app/actions/fortify/delete_user.rs`，由應用 UserServiceProvider 選擇。修改其中實作 `starter::deletion::DeletesUsers<User>` 的 `DeleteUser`，定義業務依賴與刪除策略。選用 `preflight` 在撤銷憑證前執行；`delete` 必須使用傳入的使用者交易。預設轉發原生模型刪除，不自動刪除業務關聯資料或遠端身分供應商帳戶。軟刪除策略需要應用實際設定軟刪除模型與欄位；僅回報成功卻未刪除會被拒絕。

需要在 Provider 啟動時明確替換 Action，可使用：

```rust
openrust_fortify::starter::deletion::delete_users_using::<crate::app::models::User>(
    app,
    std::sync::Arc::new(crate::app::actions::fortify::delete_user::DeleteUser),
)?;
```

使用者鎖與權杖簽發、帳戶變更串行。同一交易內撤銷憑證版本，透過已註冊模型事件實體清理全部本人 PAT 與已安裝通行密鑰，刪除套件待用證明、撤銷瀏覽器裝置索引、持久化舊頭像清理任務，最後刪除使用者。憑證儲存須與使用者同連線，無法原子處理的跨連線設定明確失敗。模型取消、依賴約束或入列失敗會回復資料庫變更，保留舊頭像。密碼重設連結綁定原身分，註銷後無法使用；舊重設記錄仍由對應倉儲的到期與清理機制管理。

提交後登出目前瀏覽器中屬於同一使用者的全部 Guard，並輪換工作階段與 CSRF；獨立管理員身分、購物車資料保留。已進入執行的業務請求不會追溯取消。應用模型掛鉤的外部副作用應延後至提交後。

沒有外部檔案時 API 回傳204，HTML 表單導向登入；存在頭像時回傳202。JSON 帶有 `meta.cleanup`，HTML 提供私密狀態頁與重新整理按鈕。此時帳戶已刪除，檔案尚待清理。妥善保管收據，使用[既有公開收據查詢與 Worker](#resource-cleanup)，無需已刪除使用者的工作階段。儲存失敗保留失敗與重試狀態，不將已提交的註銷冒充回復；不要把收據放進 URL 或記錄。
