# Fortify

## 簡介

Fortify 提供認證路由與應用擴充契約，不包含使用者介面。認證 API 對應 Fortify 1.43.0。

## Fortify 與 Sanctum

Fortify 管理註冊、Session 登入與憑證變更。[Sanctum](sanctum.md) 的 API 認證能力請見對應章節。Fortify 共用現有的認證、工作階段、CSRF、雜湊、驗證器與通知服務。

## 安裝

在應用程式 `Cargo.toml` 的 `[features].default` 清單中加入 `fortify`，保留已有功能，然後使用原始碼入口：

```sh
./artisan fortify:install
```

命令發佈 `config/fortify.rs`、`app/actions/fortify/`、應用 Provider、雙因素欄位與 Passkeys 遷移，註冊 Rust 模組、設定與 Provider，並為預設 User 接入所需能力。已有發佈檔案會保留；無法辨識的原始碼配置會明確要求手動註冊。安裝不會執行遷移。

修改發佈設定：預設 User 須啟用 `email-verification`；設定視圖回呼或將 `views` 設為 `false`。接著執行遷移；原始碼入口會在需要時重新編譯應用程式：

```sh
./artisan migrate --no-interaction
```

自動探索的 `FortifyResourcesServiceProvider` 僅提供安裝資源。明確註冊的 `openrust_fortify::FortifyServiceProvider` 啟用執行階段認證，產生的應用 Provider 連接動作、模型與具名限流。編譯型應用在原始碼變更後需要重新建置。

## 設定與視圖

安裝預設使用 `web` 守衛、`users` 密碼代理、`email` 使用者名稱與郵件欄位、名稱小寫轉換、空前綴與 `/home` 首頁。這些是應用模板預設值，與套件內預設設定不同。透過 `fortify.features` 選擇功能，透過 `fortify-options.two-factor-authentication` 設定 `{"confirm":true,"confirmPassword":true}`；安裝器會載入這個獨立的 Rust 選項群組。

`fortify.views=false` 移除介面路由，保留已啟用的動作端點；Session Cookie 與 CSRF 仍有效。須提供名為 `password.reset` 的前端路由，或設定原生重設通知的 URL 產生方式，才能建立重設郵件。視圖回呼包含 `login_view`、`register_view`、`request_password_reset_link_view`、`reset_password_view`、`verify_email_view`、`confirm_password_view` 與 `two_factor_challenge_view`。

## 認證

`POST /login` 接收設定的使用者名稱、`password` 與選用的 `remember`。一般 JSON 登入成功回傳 `200` 與 `two_factor:false`，需要雙因素挑戰時回傳 `two_factor:true`。認證成功會輪替 Session。`POST /logout` 使其失效並重新產生 CSRF 權杖。瀏覽器變更請求須攜帶工作階段 Cookie 與 CSRF 標頭。JSON 驗證失敗回傳 `422`，具名限流回傳 `429`。

透過 `Fortify::authenticate_using` 自訂身分查詢，`confirm_passwords_using` 自訂密碼確認，`authenticate_through` 替換登入管線。預設管線處理限流、名稱正規化、雙因素導向、認證與工作階段準備。Web 回應可透過 `fortify.redirects` 設定導向。

安裝的 `login` 限流器依轉寫後的小寫名稱與 IP 每分鐘允許五次請求，透過 `RateLimiter::for_async` 讀取真實請求輸入。設定具名限流後會替換 Fortify 內建的失敗計數限流。具名策略可回傳 `Limit`、多個配額、`Unlimited` 或回應。

## 雙因素認證

確認密碼後，`POST /user/two-factor-authentication` 建立加密的 TOTP 金鑰與八個復原碼。透過 `GET /user/two-factor-qr-code` 讀取 QR SVG，透過 `GET /user/two-factor-secret-key` 讀取金鑰；啟用確認時，將 TOTP `code` 提交至 `POST /user/confirmed-two-factor-authentication`。

`GET /user/two-factor-recovery-codes` 列出復原碼，同一路徑的 `POST` 重新產生復原碼。`DELETE /user/two-factor-authentication` 關閉該能力。預設模型在序列化時隱藏加密金鑰與復原碼。

登入進入挑戰後，使用相同 Session 將 `code` 或 `recovery_code` 提交至 `POST /two-factor-challenge`。兌換復原碼會替換該碼。挑戰狀態、重放檢查、具名限流與原生認證守衛均保留在請求流程。應用可替換 `TwoFactorAuthenticationProvider` 契約。

`TwoFactorAuthenticatable::replace_recovery_code` 仍是可覆寫的模型入口。預設實作使用 Fortify 設定的恢復碼產生器與加密器，儲存加密後的模型屬性，再以原恢復碼與模型上下文派送 `RecoveryCodeReplaced`。資料庫錯誤透過既有結果向上傳遞，自訂實作仍由模型方法控制。

## 註冊與帳戶更新

`POST /register` 呼叫 `CreatesNewUsers`。安裝的 `CreateNewUser` 驗證姓名與郵件地址，透過 User 模型檢查郵件唯一性，驗證預設密碼規則與確認欄位，雜湊密碼並儲存帳戶。JSON 成功回傳 `201`。

`PUT /user/profile-information` 呼叫 `UpdatesUserProfileInformation`；郵件地址變更會清除驗證狀態並寄送新的原生通知。`PUT /user/password` 呼叫 `UpdatesUserPasswords`，驗證 `current_password:web`、雜湊新密碼、刪除重設權杖並發出更新事件。安裝的 `update_from_request` 獨立讀取真實 `web` 守衛，與待更新使用者區分。直接呼叫預設動作而未提供請求時會明確失敗；既有自訂動作仍可實作原本的 `update` 方法。

可在應用的 Fortify 執行個體上透過 `create_users_using`、`update_user_profile_information_using`、`update_user_passwords_using` 與 `reset_user_passwords_using` 替換動作。安裝的密碼規則為 `required`、`string`、`Password::default()` 與 `confirmed`。資料與密碼的 Web 驗證使用具名錯誤包。

## 密碼重設

`POST /forgot-password` 接收設定的郵件欄位，透過選定的密碼代理寄送原生重設通知。將 `token`、郵件地址、`password` 與 `password_confirmation` 提交至 `POST /reset-password`。安裝的 `ResetsUserPasswords` 動作驗證、雜湊並儲存代理提供的實際使用者。完成後輪替 remember 權杖並發出 `PasswordReset`，不會自動登入。動作輸入不合法時可以重試；成功消耗的權杖不能再次使用。

自訂 `ResetsUserPasswords` 動作負責在實際 broker 使用者上驗證並儲存密碼。隨後 Fortify 持久化 remember token 輪換並派送 `PasswordReset`；自訂動作與此完成步驟保持分工，避免重複執行這些副作用。

JSON 重設結果包含翻譯後的訊息，失敗使用驗證錯誤；Web 結果使用導向與 Session 狀態。可實作密碼回應契約替換這些回應，其 Rust `to_response` 方法明確接收代理狀態。

## 郵件驗證

為實作 `MustVerifyEmail` 的使用者啟用 `email-verification`。註冊與資料郵件變更使用原生驗證通知，`POST /email/verification-notification` 重新寄送。帶簽章的 `GET /email/verify/{id}/{hash}` 連結驗證目前使用者與目前郵件地址，更新驗證狀態並發出驗證事件。可透過既有的 `verified` 中介軟體保護應用路由。實際投遞須設定真實郵件傳輸；預設 `log` 郵件器僅記錄訊息。

`MustVerifyEmail` 實作應回傳持久化驗證狀態是否實際變更。Fortify 為該狀態轉移派送 `Verified`，然後更新 Session 使用者；重複驗證不會再次派送事件。實作模型能力時，將事件派送留在驗證流程中。

## 密碼確認

`POST /user/confirm-password` 驗證 `password` 並記錄確認時間。在有效期內，`GET /user/confirmed-password-status` 回傳 `{"confirmed":true}`。設定要求時，憑證管理使用 `password.confirm`。應用可透過既有容器替換 `LoginResponse`、`RegisterResponse`、`PasswordUpdateResponse` 與雙因素回應等契約；各應用獨立持有自己的替換實作。

## Passkeys

Passkeys API 搭配 `@laravel/passkeys` 0.4.1 用戶端使用。`fortify:install` 發佈 `passkeys` 資料表遷移，為預設 User 加入 `PasskeyAuthenticatable`，並在產生的 Provider 註冊 `Passkeys::use_user_model::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新建置並執行遷移。自訂 User 實作 `openrust_fortify::passkeys::PasskeyAuthenticatable`，並在應用 Provider 明確註冊。

設定 `fortify.passkeys.relying_party_id`、`allowed_origins`、`user_handle_secret` 與 `timeout`。預設由 `APP_URL` 推導 RP ID 與精確 Origin，密鑰讀取 `PASSKEYS_USER_HANDLE_SECRET` 並退回 `APP_KEY`，逾時為 60000 毫秒。正式環境使用 HTTPS；開發支援 HTTP localhost。Origin 包含連接埠，不包含尾端斜線或路徑。輪替應用密鑰時保持使用者控制代碼密鑰穩定：控制代碼為模型資料表名稱與主鍵衍生的 64 位元組十六進位 HMAC-SHA256；更換密鑰會改變新註冊的控制代碼。

| 方法 | 路徑 | 用途 |
| --- | --- | --- |
| GET | `/user/passkeys/options` | 目前登入使用者的註冊選項 |
| POST | `/user/passkeys` | 驗證並儲存 `name` 與 `credential` |
| DELETE | `/user/passkeys/{passkey}` | 刪除自己的憑證 |
| GET | `/passkeys/login/options` | 可探索憑證登入選項 |
| POST | `/passkeys/login` | 驗證 `credential`、選用 `remember` 並登入 |
| GET | `/passkeys/confirm/options` | 限定目前帳戶的確認選項 |
| POST | `/passkeys/confirm` | 驗證並記錄密碼確認時間 |

路由沿用 Fortify 的前綴、網域、中介軟體與 Guard。註冊與刪除預設要求近期密碼確認；`fortify-options.passkeys.confirm_password=false` 可關閉此管理門檻。產生的 `passkeys` 具名限流對選項按 Session、對提交按憑證 ID 每分鐘允許十次。套件未提供 GET 清單路由：在應用已認證的控制器使用原生關係 `user.passkeys().get_results(&user).await?`，只回傳介面所需欄位。

固定官方用戶端執行真實 WebAuthn 操作：

```js
import { Passkeys } from '@laravel/passkeys';
await Passkeys.register({ name: 'My device' });
await Passkeys.verify({ remember: true });
await Passkeys.verify({ routes: {
  options: '/passkeys/confirm/options', submit: '/passkeys/confirm'
} });
```

頁面須提供 `csrf-token` meta 標籤並攜帶 Session Cookie；設定前綴時覆寫用戶端路徑。註冊 JSON 包含 `status`、`id`、`name`，驗證 JSON 包含 `redirect`。Web 請求重新導向並快閃儲存註冊／刪除狀態。透過原生容器替換 `passkeys::contracts` 的四種回應契約。`Passkeys::authorize_login_using` 提供應用內非同步授權回呼，在密碼學驗證後執行。原生 `PasskeyRegistered`、`PasskeyVerified`、`PasskeyDeleted` 事件攜帶使用者與已持久化模型的上下文。

註冊要求常駐憑證與使用者驗證，支援 ES256／RS256，證明類型為 none。伺服器端 Session 將挑戰綁定到操作與帳戶，並強制執行截止時間；原子快取宣告拒絕並行重用，MySQL 資料列鎖保護獨立挑戰間的計數器更新與回退檢查。多程序部署必須共用 Session 儲存與支援原子 `add` 的快取；array 驅動涵蓋單一程序。可探索憑證登入也會檢查已儲存的使用者控制代碼。伺服器端狀態遺失後須重新發起操作。

在應用 Provider 中呼叫 `Passkeys::use_passkey_model::<CustomPasskey>()` 選擇實際 Eloquent 模型。為實作 `Model + Clone` 且具備憑證轉換及必要欄位的型別實作 `PasskeyModel`；查詢、插入、資料列鎖驗證及刪除均使用其資料表、主鍵、存取器和觀察者。`Passkeys::passkey_model()` 與 `user_model()` 傳回所選型別名稱。Rust 使用 `PasskeyRelation` 提供 `get_results`、`exists` 與 `create`，傳回 `PasskeyRecord`。記錄的 `get_key`、`get_attribute_value` 和 `to_array` 為非同步方法；`record.model::<CustomPasskey>()` 傳回非同步互斥鎖中的實際共用模型。呼叫其他記錄方法前先釋放該鎖。事件的 `passkey::<CustomPasskey>(context)` 讀取型別化快照，事件的 `passkey` 記錄保留實際共用模型。

憑證驗證繼續透過既有 Passkeys 動作流程完成。`PasskeyRecord` 保留所選模型的實際實例，驗證與憑證更新仍受同一模型鎖和資料庫交易保護。自訂模型的轉換與觀察器繼續生效，應用程式無須將憑證驗證與儲存拆成兩次獨立操作。

預設模型附加 `authenticator`，依註冊 AAGUID 查詢名稱。`Aaguids::label_for`、`icon_for(id, "light")`、`all` 與 `flush` 使用固定上游資源。需要圖示時，在模型 appends 中明確加入 `authenticator_icon_light` 與 `authenticator_icon_dark`。未知或缺少 AAGUID 傳回 null，舊儲存憑證缺少此欄位仍可使用。none 證明模式下，名稱和圖示僅展示所回報的 AAGUID，不認證製造商身分。

獨立發佈資源時，在 Provider 清單中明確註冊 `openrust_passkeys` 的 `PasskeysResourcesServiceProvider` 並重新建置，然後執行：

```sh
cargo run -- artisan vendor:publish --tag=passkeys-config
cargo run -- artisan vendor:publish --tag=passkeys-migrations
```

發佈會保留自訂檔案並避免重複遷移。將產生的 Rust 設定與遷移模組接入應用載入器及遷移清單，再重新建置並遷移。資源 Provider 只登記可發佈檔案；獨立認證還需設定 `PasskeysServiceProvider`、使用者模型、Guard 和中介軟體。Fortify 安裝繼續使用產生的 Fortify 設定。

PHP 憑證 JSON 無法直接作為 Rust 憑證儲存格式。

安裝器支援預設模型使用 `User::PASSWORD`、`User::REMEMBER_TOKEN` 的隱藏欄位清單，也相容等價的字串寫法。未知自訂結構仍保留原檔並要求手動設定。

## 交易內通行密鑰清理

```rust
passkeys.purge_user_in_transaction(transaction, user).await?;
```

先持有使用者資料列鎖，使用所屬連線的同一交易。此方法逐模型實體刪除本人憑證，包含 Scope 隱藏與軟刪除記錄，觸發模型實體刪除事件；任何錯誤須回復整個交易。已註冊使用者型別與已持久化、未改寫的真實主鍵必須相符。尚未安裝的選用資料表回傳零；已存在但位於其他連線的資料表明確拒絕。這是 Rust 交易擴充，一般密鑰管理介面保留原有行為。

## 雙因素 Action 的交易入口 {#transactional-two-factor-actions}

```rust
openrust_fortify::actions::EnableTwoFactorAuthentication::new(provider, fortify)
    .call_in_transaction(user, transaction, false)
    .await?;
```

EnableTwoFactorAuthentication、ConfirmTwoFactorAuthentication、DisableTwoFactorAuthentication 與 GenerateNewRecoveryCodes 均提供 `call_in_transaction`。重用已持有的使用者寫入鎖，傳入使用者連線的交易；下例假定已取得此鎖。發生錯誤時呼叫方須回復整個操作，回復後捨棄或重新載入記憶體模型。模型取消儲存視為失敗，即使沒有狀態變更也拒絕錯誤連線。一般 `call` 保留原有行為；產生、加密、驗證與事件重用同一實作。事件仍在儲存後、呼叫方提交前派發，具有外部副作用的監聽器應使用既有提交後機制。提供者可能已消費驗證碼，後續資料庫失敗時應使用新驗證碼重試。
