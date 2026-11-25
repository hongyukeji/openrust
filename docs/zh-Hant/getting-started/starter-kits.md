# 入門套件

## 支援狀態 {#support-status}

明確選取的使用者套件預設可安裝原生 Fortify Action、安全使用者回應與基礎 Blade 認證頁面。完整帳戶中心、個人權杖登入協定與可選因素 UI 仍在開發，不能據此視為完成。不會產生 React、Vue 或 Livewire 前端。

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

`UserResource` 只讀取 `id`、`name`、`email`、`email_verified_at`，保留字串主鍵，不列舉屬性或讀取憑證。自訂資源可替換原有 Fortify 回應綁定。單獨使用回應配接不會簽發權杖或變更 Guard。

## 郵件連結與密碼工作階段 {#mail-and-sessions}

套件使用 Fortify 原生的 `POST /forgot-password`、`POST /reset-password`、`PUT /user/profile-information` 與 `PUT /user/password`。JSON 用戶端傳送 `Accept: application/json`；Cookie 認證須同時攜帶目前 CSRF Cookie。成功更新回傳200，欄位或重置憑證無效回傳422。驗證連結要求本人已認證、目前信箱雜湊相符、簽章有效且未過期。JSON 驗證成功使用204空本文，重複驗證保持冪等。更換信箱會清除驗證狀態並向新地址傳送連結。

API 預設也提供最小登入、密碼重置與驗證網頁。未登入瀏覽器開啟驗證郵件後，登入仍回到原始簽章 URL。驗證收據讀取資料庫狀態，加入 `?verified=1` 不能讓帳戶變成已驗證。依賴郵件投遞前須設定傳輸。設定 `USER_EMAIL_VERIFICATION=false` 會關閉驗證路由與這類驗證郵件，密碼找回保持可用。

已安裝套件在登入與有效 remember Cookie 恢復時立即儲存既有密碼指紋，Guard 在後續認證讀取中拒絕缺少或過時指紋。修改密碼保留目前瀏覽器工作階段並拒絕其他密碼工作階段；重置也會輪換 remember token。獨立認證的預設行為不變。

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
      "email_verified_at": null
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

本節涵蓋裝置登入和 TOTP／恢復碼挑戰。純權杖客戶端註冊、郵件驗證／找回及完整帳戶中心仍在接線，近期確認與權杖管理見下節，另行追蹤剩餘工作。

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

第一方 Cookie／SPA 保留正常 CSRF 流程並傳送憑據。`POST /user/confirm-password` 將確認狀態記入目前 Session，原生 Passkey 確認也寫入相同狀態；隨後向 `POST /api/user/tokens` 提交 `device_name` 和 `abilities`，此模式不需要另一份不透明證明。`POST /api/auth/confirm` 也接受有狀態 Cookie 請求，成功201且不回傳 `data`。其他瀏覽器工作階段不會繼承確認；缺少或過期回傳423及本地化 code `password_confirmation_required`，缺少 CSRF 權杖仍回傳419。

PAT 證明在應用資料庫中僅儲存雜湊，綁定使用者、目前PAT、用途、憑據版本及有效的因素／郵箱狀態。`auth.password_timeout` 為正整數秒，預設10800；不等清理工作就會到期。錯誤裝置／用途、竄改或重放回傳401。簽發／撤銷使用共同使用者鎖及權杖列鎖，模型事件取消或提交前證明過期會回滾資料庫變更。應用確認回呼可支援自己的身分流程，沒有本機密碼不代表自動確認。無狀態 Passkey 重新驗證適配仍另列待補。

密碼錯誤422，一分鐘內5次失敗後429並帶 `Retry-After`；缺少權杖能力403。訊息跟隨請求語言，證明、密碼與權杖秘密不會進入舊輸入；包含新證明／權杖的回應使用 `no-store`。完整帳戶中心、頭像、裝置清單與帳戶註銷仍在開發。
