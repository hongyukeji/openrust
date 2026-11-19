# Passport

## 安裝 {#installation}

預設應用程式可執行：

```sh
./artisan install:api --passport --without-migration-prompt --no-interaction
./artisan passport:install --no-interaction
```

API 安裝器啟用隨專案提供的 `passport` Cargo feature，保留其他預設 feature，為預設 User 加入 `HasApiTokens`、設定 `api` Passport guard，並產生及註冊 `routes/api.rs`。它重新建置應用程式，呼叫 `passport:install`，再重新建置已發布的設定與遷移。Cargo feature 與編譯承擔 Composer 安裝的職責，不提供 `--composer`。執行遷移前設定 `APP_KEY` 與資料庫連線。預設使用者端點使用 `auth:api`，遵守 `API_PREFIX`，支援多級前綴。

`passport:install` 保留既有完整金鑰組，發布設定與五個遷移並掛載設定載入器。真實終端中詢問執行遷移與建立個人用戶端，兩項預設 yes；`--no-interaction` 執行兩項，每次成功重跑可能再次建立個人用戶端。`--without-migration-prompt` 僅發布並明確回報遷移待執行，就緒後執行第二條命令；此明確略過選項是套件安裝器的原生擴充。沒有終端也未傳 `--no-interaction` 時回報待執行工作，不等待輸入。正式環境遷移仍需明確確認，可先執行部署步驟 `artisan migrate --force`，再單獨執行 `artisan passport:client --personal --no-interaction` 建立個人用戶端。

既有路由、設定及遷移檔案會保留。API 的 `--force` 僅重寫 API 路由 stub，保留金鑰；套件命令 `passport:install --force --length=2048` 才明確重新產生金鑰。金鑰組不完整時失敗並提示復原方式。建置、發布、設定、遷移或用戶端建立失敗均回傳非零狀態，修正錯誤後可重試。自訂模型、guard、設定原始碼缺少可辨識插入點時需要手動接線，命令明確回報未完成。已有獨立裝配的應用程式也可使用下列手動步驟。


## 部署 Passport {#deploying-passport}

產生預設 4096 位元 RSA 金鑰組：

```sh
./artisan passport:keys
```

金鑰位於應用程式設定的儲存目錄，檔名為 `oauth-private.key` 與 `oauth-public.key`。Unix 權限分別為 0600、0660；Windows 使用檔案系統存取控制。金鑰已存在時退出碼為 1。`--force` 取代既有金鑰，`--length=2048` 指定長度。取代後重新啟動服務。並行的非強制產生作業最多產生一組相符金鑰。

```sh
./artisan passport:keys --force --length=2048
./artisan vendor:publish --tag=passport-migrations
cargo build
./artisan migrate
```

五張表的遷移（包含 `oauth_device_codes`）依遷移時間戳發布。Rust 在編譯時探索遷移原始碼，因此發布後先重新建置，再執行遷移。Provider 不會自動註冊或執行套件遷移。重複發布會保留既有檔案，除非明確傳入 `--force`。狀態、復原與重建命令見[資料庫遷移](../database/migrations.md)。

## 設定 {#configuration}

套件預設提供 `guard = "web"`、空的中介軟體清單，以及可為空的金鑰和連線設定；應用程式設定覆寫套件預設值。`PASSPORT_PRIVATE_KEY`、`PASSPORT_PUBLIC_KEY` 支援內嵌 RSA PEM、字面 `\n` 換行、本機絕對路徑或 `file://` 路徑（Unix 包含 `file://localhost/`）。相對路徑依固定 PHP 本機檔案包裝器行為拒絕，不會自動繫結至應用程式根目錄。未設定時延遲讀取儲存目錄中的檔案。支援 RSA PKCS#1／PKCS#8 私密金鑰與 RSA／SubjectPublicKeyInfo 公開金鑰，不宣稱涵蓋 League CryptKey 的通用 EC 金鑰 API。Debug 與載入錯誤不包含原始金鑰內容。 線上 RS256 使用 AWS-LC，支援 2048–8192 位 RSA 金鑰。

需要應用程式編譯設定覆寫時先發布：

```sh
./artisan vendor:publish --tag=passport-config
```

在 `config/mod.rs` 增加 `mod passport;`，並在設定項目增加 `("passport".to_owned(), passport::load(env)?),`，然後重新建置。這對應 PHP 載入發布設定的機制。發布的載入器與套件預設值使用同一份實作。`config:cache` 保存解析後的值；載入新環境變數前先清除設定快取。

`PASSPORT_CONNECTION` 為五個遷移定義選擇具名連線；null 使用應用程式預設連線。遷移帳本仍放在遷移命令選定的連線，維持 Laravel 的職責。遷移明確讀取所屬應用程式設定，並行應用程式不會交換連線名稱。

在 Provider boot 期間解析 `Passport`，於首次解析金鑰服務前設定目錄：

```rust
use openrust::foundation::Application;
use openrust_passport::Passport;

pub async fn configure_keys(app: &Application) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let passport = app.make::<Passport>().await?;
    passport.loadKeysFrom(passport.keyPath("keys"));
    Ok(())
}
```

產生前目錄須已存在且可寫。Rust 使用應用程式持有的 `Passport` 實例，取代 PHP 的行程全域可變設定，保留 `keyPath`、`loadKeysFrom` 名稱。兩個金鑰各自延遲載入；僅設定成功不代表兩者已配對。Unix 檔案權限不在 0400、0440、0600、0640、0660 時驗證失敗。應用程式可明確呼叫 `passport.validateKeyPermissions.store(false, std::sync::atomic::Ordering::Relaxed)` 關閉檢查，不影響其他應用程式。內嵌金鑰與 Windows 不檢查 Unix 權限位元。

預設及標準長度使用既有原生 AWS-LC 產生，其餘支援長度使用獨立產生器。RustCrypto 僅用於產生與 PEM 解析，不用於線上 RSA 解密或簽章；不需安裝 PHP 或 OpenSSL 命令列工具。

## 管理用戶端 {#managing-clients}

用戶端儲存庫支援查詢、建立、更新、秘密輪替、擁有權篩選和撤銷，操作真實 Eloquent 模型。`passport:client` 可建立六類用戶端。授權碼、裝置端點及資源權杖驗證已提供。

在請求或 Artisan 生命週期內解析 `ClientRepository`，容器為每個生命週期提供新儲存庫。同一參數的重複 `find` 共用實際模型，並快取未命中；資料庫錯誤向上傳遞，允許重試。`findActive` 還檢查目前模型的 `revoked` 屬性。不要把此作用域服務保存在應用單例中。顯式管理的操作可透過 `Passport::clientRepository()` 建立獨立擁有快取的儲存庫。

發布並執行用戶端遷移後，在上述生命週期內呼叫以下輔助函式：

```rust
use openrust::foundation::Application;
use openrust_passport::{ClientOwner, ClientRecord, ClientRepository};

pub async fn create_client(
    app: &Application,
    owner: Option<&ClientOwner>,
) -> Result<ClientRecord, Box<dyn std::error::Error + Send + Sync>> {
    let clients = app.make::<ClientRepository>().await?;
    Ok(clients
        .createAuthorizationCodeGrantClient(
            "My application",
            &["https://example.com/callback".to_owned()],
            true,
            owner,
            false,
        )
        .await?)
}
```

參數順序保留 Laravel 原意。Rust 顯式傳遞預設參數：`true` 建立機密用戶端，`owner` 可空，最後的 `false` 不額外啟用裝置授權。使用 `ClientOwner::forUser(&user)` 從編譯註冊的 `Model + Authenticatable` 建立擁有者，取模型主鍵與多型類型，不取可能不同的驗證識別。公開用戶端將機密參數設為 `false`。

`createPersonalAccessGrantClient`、`createPasswordGrantClient`、`createClientCredentialsGrantClient`、`createImplicitGrantClient` 和 `createDeviceAuthorizationGrantClient` 選擇對應授權類型。建立記錄不會啟用可選的 Password 或 Implicit 協定；裝置端點遵循應用裝置開關。`update` 修改名稱和重新導向位址，`regenerateSecret` 產生新秘密；兩者經 Eloquent 儲存並回傳其布林結果。`findForUser` 和 `forUser` 排除已撤銷用戶端，後者按名稱排序。`personalAccessClient(provider)` 選擇對應使用者提供者的最新有效個人用戶端，僅當提供者等於 `auth.guards.api.provider` 時也接受空提供者，找不到時回傳錯誤。

機密用戶端重用應用既有的 Eloquent 非同步雜湊服務。建立或輪替的 `Client` 在 `plainSecret` 暫時保留 setter 輸入，重新讀庫不會還原明文。預設 JSON 隱藏 `secret`，不附加明文。只在交付擁有者時複製新產生的秘密，不要記錄日誌。`record.model::<M>()` 回傳的 `Arc<tokio::sync::Mutex<M>>` 指向實際選定模型；`sameInstance` 檢查共用物件身分，不代表兩行模型的主鍵相等。

`client.append(&["plain_secret"])` 顯式附加該實例的暫時明文；重新讀庫的模型附加後得到 null。附加仍遵守 `makeHidden` 與 `setVisible`。

`ClientModel::hasScope` 與 `ClientRecord::hasScope` 在未設定 `scopes` 或其原始值為 null 時允許所有 scope；已設定時按陣列內容檢查，成員判斷沿用 PHP 非嚴格比較。預設不開啟父 scope 繼承。若在目前應用上呼叫 `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)`，`orders` 也允許 `orders:read` 等後代。`*` 在這裡是一般 scope 名稱。開關儲存在各應用自己的實例中，呼叫須位於所屬應用的請求或 Artisan 生命週期。

在應用裝配時、解析儲存庫前呼叫 `passport.useClientModel::<CustomClient>()`。編譯期類型實作 `ClientModel` 和 Eloquent `Model`，保留唯一識別校驗、全域作用域、事件與轉換。格式錯誤的 UUID／ULID 在解析連線前直接未命中。儲存庫查詢與未綁定實例的 `Client::query()`／`save()` 優先使用模型顯式連線，接著是 `passport.connection`，最後是資料庫預設連線。建立和讀出的模型保留實際連線；隨後更改設定也不會把該模型的儲存轉到其他連線。`Client::queryOn(&connection)` 顯式選擇連線。交易寫入檢查模型的有效連線，拒絕不同連線的交易。Rust 的 `resolveConnectionName()` 回傳可擁有的名稱，以讀取應用設定；`getConnection()` 回傳實際連線。秘密賦值使用 PHP 純量字串轉換。雜湊失敗保留既有雜湊，並在 `plainSecret` 中保留本次 setter 輸入。批次賦值沿用 Rust 既有的屬性鍵順序；後續 setter 失敗時保留先前成功的賦值。

## 用戶端工廠

`Client::factory()?` 建立 `database::factories::ClientFactory<Client>`。可呼叫 `asPublic`、`asPasswordClient`、`asPersonalAccessTokenClient`、`asImplicitClient`、`asClientCredentials` 或 `asDeviceCodeClient` 套用對應授權狀態。後續狀態覆寫先前屬性。預設產生機密授權碼用戶端，包含隨機 40 字元秘密和一個產生的重新導向 URI；擁有者為空，revoked 為 false。

`make().await?` 回傳未儲存模型，`create().await?` 持久化模型。賦值執行模型的非同步修改器和轉換：新 Client 保留 `plainSecret`，重新讀取資料庫只包含雜湊。`state` 新增應用程式屬性；`intoFactory()` 回傳現有 Eloquent 工廠 Builder，以使用數量、關係與回呼。

Rust 在編譯期選擇工廠模型：自訂 `ClientModel` 使用 `ClientFactory::<CustomClient>::new()?`，不動態讀取 `useClientModel`。建構時解析並保存模型的有效連線，包括 `passport.connection`，設定錯誤透過 Result 回傳；修改設定後應新建工廠。目前提供授權碼、重新整理與裝置協定。

## 用戶端命令與撤銷

執行遷移後，用 `./artisan passport:client` 建立用戶端。`--personal`、`--password`、`--client`、`--implicit`、`--device` 選擇授權類型；未傳這些選項時建立授權碼用戶端。多個選項同時存在時依上述順序優先。`--name`、`--provider`、`--redirect_uri` 提供互動值，多個重新導向位址以逗號分隔。`--public` 為密碼、裝置和授權碼用戶端省略秘密。隱含用戶端始終公開；個人和用戶端憑證用戶端始終保密。

使用 `--no-interaction` 時，名稱預設取 `app.name`，回呼預設取應用 URL 加 `/auth/callback`，附加裝置流程預設關閉。互動建立授權碼用戶端時，若 `deviceCodeGrantEnabled` 為 true（預設值），會詢問是否啟用裝置流程。命令輸出用戶端 ID；保密用戶端還輸出暫存秘密和保存提醒。個人用戶端建立只輸出成功訊息。

`clients.delete(&client).await?` 按每批 1000 個載入用戶端的存取權杖，逐一先撤銷重新整理權杖、再撤銷存取權杖，最後撤銷用戶端。儲存經過 Eloquent 轉換和事件；錯誤向上傳遞，先前儲存保留。事件取消儲存回傳 false，不中止撤銷鏈，與 Passport 一致。授權碼不變。`TokenModel::revoke`、`RefreshTokenModel::revoke` 也可個別撤銷實例。

## 模型與關聯

`Token`、`RefreshToken`、`AuthCode` 使用非自增字串主鍵、布林 `revoked`、日期時間 `expires_at` 轉換，以及 Passport 連線回退。只有 `Token` 使用時間戳記，其 scopes 為陣列。`Token::can` 拒絕空 scope 清單，支援 PHP 非嚴格比較的 `*` 萬用字元，並讀取目前應用的繼承設定；`cant` 回傳相反結果。

`ClientModel` 提供 `tokens`、`authCodes`、`owner` 和已棄用的 `user::<U>()`；`Token` 提供 `client`、`refreshToken` 和已棄用的 `user::<U>("驗證識別欄")`；`RefreshToken` 提供 `accessToken`；`AuthCode` 提供 `client`。Token 的使用者鍵須與編譯 provider 模型的 `Authenticatable` 識別欄一致，Client 的使用者關聯使用主鍵。多型擁有者透過既有 `owner().target::<U>(別名, 主鍵)` Builder 綁定。

Rust 在編譯時綁定關聯回傳類型。預設輔助方法回傳預設具體模型；自訂模型使用 `tokensFor::<T>`、`clientFor::<C>`、`refreshTokenFor::<R>`、`accessTokenFor::<T>`。`passport.useTokenModel::<T>()` 和 `useRefreshTokenModel::<R>()` 選擇儲存庫撤銷時使用的實際模型，保留其連線和事件；共用註冊也作用於已解析的儲存庫。使用者 provider 由呼叫端選擇編譯模型，替代 PHP 的執行期類別名稱。

`clientUuids` 預設 true。已有數字自增用戶端表時，在建構模型前呼叫 `passport.clientUuids.store(false, std::sync::atomic::Ordering::Relaxed)`。每個實例保留建構時選定的策略。開關不會修改既有資料表結構；發布的用戶端遷移仍建立 UUID 主鍵。

## Scope JSON

`Scope::toJson()` 保留 `id`、`description` 欄位順序，以及 PHP 預設的 Unicode 和斜線跳脫。`toJsonWithOptions(ScopeJsonOptions)` 支援 `JSON_HEX_TAG`、`JSON_HEX_AMP`、`JSON_HEX_APOS`、`JSON_HEX_QUOT`、`JSON_NUMERIC_CHECK`、`JSON_UNESCAPED_SLASHES`、`JSON_PRETTY_PRINT`、`JSON_UNESCAPED_UNICODE`、`JSON_PRESERVE_ZERO_FRACTION`、`JSON_UNESCAPED_LINE_TERMINATORS`。以 `|` 組合常數，`fromBits` 可傳入 PHP 選項位元。Rust 字串始終是有效 UTF-8，因此無效 UTF-8 相關旗標不會改變結果。

```rust
use openrust_passport::{Scope, ScopeJsonOptions};
let scope = Scope::new("orders/read", "讀取訂單");
let json = scope.toJsonWithOptions(
    ScopeJsonOptions::JSON_UNESCAPED_UNICODE | ScopeJsonOptions::JSON_UNESCAPED_SLASHES,
)?;
```

## 授權碼授權 {#authorization-code-grant}

Provider 註冊 GET、POST、DELETE `/oauth/authorize`，使用 `web` 中介層及 Session 鎖；POST `/oauth/token` 使用限流。`passport.path` 修改預設 `oauth` 前綴，`passport.middleware` 為這些路由新增中介層。在套件啟動前呼叫 `passport.ignoreRoutes()` 可自行註冊路由。瀏覽器驗證使用 `passport.guard`，預設為 `web`；設定應用本身的登入路由與 SessionGuard 使用者提供者，並先發布執行遷移、設定 `APP_KEY`、產生 RSA 金鑰。

在應用裝配時解析 `Passport`，呼叫 `passport.tokensCan(BTreeMap::from([("read".to_owned(), "Read data".to_owned())]))` 註冊 scope，依需要呼叫 `passport.setDefaultScope(vec!["read".to_owned()])`，並設定 `passport.authorizationView(consent)`。這些設定屬於目前應用。以下回呼為應用介面回傳同意資料；介面負責顯示用戶端、權限與批准／拒絕控制項：

```rust
use openrust::http::{HttpError, HttpFuture, Request, Response, StatusCode};
use openrust::contracts::session::Session as _;
use openrust_passport::http::AuthorizationViewParameters;

pub fn consent(request: &mut Request, params: AuthorizationViewParameters)
    -> HttpFuture<'_, Result<Response, HttpError>>
{
    Box::pin(async move {
        Response::json(StatusCode::OK, serde_json::json!({
            "auth_token": params.auth_token,
            "csrf": request.session()?.token(),
            "user": params.user.get_auth_identifier(),
            "scopes": params.scopes,
        }))
    })
}
```

批准時向 POST `/oauth/authorize` 提交 `auth_token` 和 Session CSRF `_token`；拒絕使用 DELETE。瀏覽器必須保留 Session Cookie。伺服器一次性取出已儲存的請求與隨機權杖，並核對目前驗證主體；表單中的用戶端、回呼和 scope 不能替換它們。批准前重新核對目前用戶端和註冊位址；拒絕會向已驗證位址回傳 `error=access_denied` 與原始 `state`。

用戶端必須產生不可預測的新 `state`，回呼時先核對它，再接受授權碼或錯誤。機密用戶端兌換時使用 HTTP Basic，或在本文提交 `client_id`、`client_secret`；本文憑證優先，查詢字串不提供兌換參數。成功回應包含 `token_type`、`expires_in`、`access_token`，允許重新整理的用戶端也取得 `refresh_token`，並帶 `Cache-Control: no-store`、`Pragma: no-cache`。存取權杖使用 RS256。授權碼十分鐘後到期，消費與權杖儲存屬於同一交易，並行兌換只有一個成功。存取／重新整理期限分別預設為一個日曆年，可按下文自訂。

`prompt=none` 在需要互動時回傳帶 `login_required` 或 `consent_required` 的重新導向。已有未撤銷、未到期授權可略過已授予權限的同意頁；`prompt=consent` 重新請求同意，`prompt=login` 請求重新登入。無效用戶端和未註冊回呼在本機失敗，不回傳不可信 Location。OAuth 錯誤保留協定欄位與狀態碼。

## 使用 PKCE 的授權碼授權 {#code-grant-pkce}

用 `passport:client --public` 建立公開用戶端，必須提交 challenge；機密用戶端也可使用 PKCE。每次產生由字母、數字與 `-._~` 組成的 43–128 字元 verifier，計算 SHA-256 後作無填補 base64url 編碼，使用 `S256`：

```text
GET /oauth/authorize?response_type=code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&scope=read&state=RANDOM_STATE&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256
```

批准後，用原 verifier 和授權時提供的完全相同的回呼位址兌換：

```text
POST /oauth/token
Content-Type: application/x-www-form-urlencoded

grant_type=authorization_code&client_id=CLIENT_ID&redirect_uri=https%3A%2F%2Fclient.test%2Fcb&code=AUTHORIZATION_CODE&code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk
```

這些是固定範例值，實際授權必須每次重新產生。缺失／錯誤 verifier、竄改回呼、到期與重播均拒絕簽發。授權階段允許已註冊 HTTP loopback 回呼改變連接埠；兌換階段綁定原來提供的精確字串。

Rust 在 Session 儲存已驗證的原生請求，透過編譯回呼提供檢視，不使用 PHP 物件序列化。授權碼與重新整理權杖使用應用現有認證加密並綁定應用上下文，內部密文格式不與 PHP 互通。用戶端／授權碼鎖與單個資料庫交易保證消費及存取／重新整理儲存原子完成。自訂權杖模型沿用實際連線、Eloquent cast 與事件；儲存被取消或資料庫失敗都會回滾簽發。其他 grant 與統一 API 安裝器在後續單元交付。

## 裝置授權 {#device-authorization-grant}

用 `./artisan passport:client --device --public` 建立公開裝置用戶端，機密裝置用戶端省略 `--public`。裝置授權預設開啟。若要關閉端點，在 Passport 裝配路由之前的 Provider boot 階段設定 `passport.deviceCodeGrantEnabled.store(false, std::sync::atomic::Ordering::Relaxed)`；應用啟動後修改開關不會移除已註冊路由。

向 POST `/oauth/device/code` 提交 `client_id` 和可選的空格分隔 `scope`。回應包含 `device_code`、`user_code`、`verification_uri`、`verification_uri_complete`、`expires_in`（600 秒）和 `interval`（5 秒）。建立步驟只識別用戶端，不驗證秘密。裝置保存 `device_code`，向使用者顯示驗證 URI 和使用者碼，或在瀏覽器開啟完整 URI。回應帶 `no-store` 和 `no-cache`。

在應用裝配時設定 `passport.deviceUserCodeView(code_view)` 與 `passport.deviceAuthorizationView(consent)`。`code_view` 簽名為 `fn(&mut Request) -> HttpFuture<'_, Result<Response, HttpError>>`，同意回呼沿用上文 `authorizationView` 的 `AuthorizationViewParameters` 簽名。應用負責渲染 HTML，並跳脫用戶端與 scope 顯示值。GET `/oauth/device` 顯示輸入頁，GET 表單將 `user_code` 提交回該位址。輸入碼僅移除連字號，保留大小寫。`verification_uri_complete` 跳過輸入，重新導向 GET `/oauth/device/authorize`；該端點要求設定的 Session 使用者登入並顯示同意頁。

同意表單攜帶保存的 `auth_token` 與 Session CSRF `_token`，提交到 POST `/oauth/device/authorize` 批准，或 DELETE 拒絕。瀏覽器保留 Session Cookie。伺服器將請求綁定至實際登入使用者，僅消費一次 nonce，已決定的裝置碼不能被競爭瀏覽器重新綁定。完成後重新導向 `/oauth/device`，Session `status` 為 `authorization-approved` 或 `authorization-denied`。錯誤使用者碼返回輸入頁，帶舊 `user_code` 輸入及驗證錯誤；缺少檢視回呼會明確失敗。

裝置向 POST `/oauth/token` 輪詢，提交 `grant_type=urn:ietf:params:oauth:grant-type:device_code`、`device_code`、`client_id`。機密用戶端還須透過 Basic 或本文秘密認證。至少等待回應指定的間隔：HTTP 400 `authorization_pending` 表示等待使用者，HTTP 400 `slow_down` 要求降低輪詢頻率，兩者都會保存最近輪詢時間。拒絕回傳 HTTP 401 `access_denied`，過期回傳 HTTP 400 `expired_token`。成功兌換在一個交易內保存存取／重新整理權杖並撤銷裝置碼，重放失敗。資源守衛、scope 與期限沿用普通存取／重新整理流程。`passport.path` 同樣作用於所有裝置端點。`DeviceCode` 是真實 Eloquent 模型，含 scope、布林與日期轉換，並回退至 Passport 連線。

## 重新整理權杖 {#refreshing-tokens}

向 `/oauth/token` POST `grant_type=refresh_token`、`refresh_token` 和用戶端憑證。公開用戶端提交 `client_id`；機密用戶端還須驗證 secret。省略 `scope` 保留原 scope，也可請求其子集；按 OAuth 參數規則，空參數使用原 scope。新增 scope 會被拒絕。

成功重新整理會撤銷舊存取／重新整理權杖並回傳新的一對。輪換和儲存處於同一資料庫交易；並行請求只有一個成功，儲存失敗或被事件取消時保留舊憑證。存取權杖過期或被單獨撤銷後，重新整理權杖仍可兌換；單獨撤銷重新整理權杖不會撤銷對應存取權杖。

存取／重新整理期限分別預設為一個日曆年。在簽發前設定本應用的 `Passport` 實例；`None` 讀取期限，`Some` 更新期限。`GrantLifetime::Interval(DateInterval)` 透過框架 Date 支援有符號日曆分量，保留應用時區、夏令時間和 PHP 月份溢位規則。

```rust
use openrust_passport::GrantLifetime;
passport.tokensExpireIn(Some(GrantLifetime::Seconds(3600)));
passport.refreshTokensExpireIn(Some(GrantLifetime::CalendarYears(1)));
let current = passport.tokensExpireIn(None);
```

## 撤銷權杖 {#revoking-tokens}

對 Eloquent 實例呼叫 `TokenModel::revoke` 或 `RefreshTokenModel::revoke`。對目前認證請求，克隆 `TokenGuard::token(request, "api")` 後呼叫 `token.revoke().await?`，更新選定的存取權杖儲存。儲存庫撤銷用戶端仍按前述 refresh → access → client 順序執行。

## 清理權杖 {#purging-tokens}

執行 `artisan passport:purge`，刪除已撤銷和過期超過 168 小時的記錄。`--revoked` 或 `--expired` 僅選擇一個條件，同時傳入時按 OR 合併；`--hours=24` 修改過期保留期限。若存取權杖仍關聯未撤銷且期限為空或晚於目前時間的重新整理權杖，則保留該存取記錄。授權碼和重新整理權杖同時清理；裝置授權啟用時也清理裝置碼。自訂存取／重新整理模型的實際表、連線及查詢範圍參與執行。

## 保護路由 {#protecting-routes}

在 `config/auth.rs` 將守衛設定為 `{"driver":"passport","provider":"users"}`，為受保護路由加入 `auth:api`。Provider 須解析應用編譯後的使用者模型。每個請求驗證 RS256 簽章、可選 JWT 時間宣告和選定存取儲存的撤銷狀態。用戶端失效、provider 不相符或使用者不存在時視為未認證；使用者查詢例外也視為未認證，權威權杖儲存故障則令請求失敗。

透過 `Auth::user(request).await?` 讀取真實使用者。Rust 將輕量存取權杖按守衛名附加至請求，保留原編譯使用者物件；透過 `openrust_passport::guards::TokenGuard::token(request, "api")` 讀取，使用 `can`、`cant`、`transient` 檢查 scope。`ResourceServer::validateAuthenticatedRequest` 還插入型別化 `AccessToken` 請求屬性。守衛解析僅在本請求內快取，按守衛及應用隔離。Cookie 認證和 scope 中介軟體見下文。

## 用戶端憑證授權 {#client-credentials-grant}

使用 `artisan passport:client --client` 建立機密機器用戶端，向 `POST /oauth/token` 傳送 `grant_type=client_credentials`、`client_id`、`client_secret` 及可選的空格分隔 `scope`，也支援 HTTP Basic 憑證。回應包含 `access_token`、`token_type`、`expires_in` 與 `Cache-Control: no-store`，沒有重新整理權杖。公開用戶端會遭拒絕。JWT subject 為用戶端 ID，資料庫 user ID 為空。

機器資源使用 `EnsureClientIsResourceOwner::shared(["read"])` 驗證權杖與 scope，不要求使用者 provider；使用者守衛仍要求真實使用者。透過 `passport.clientCredentialsTokensExpireIn(Some(GrantLifetime::Seconds(300)))` 設定機器期限，未單獨設定時沿用 `tokensExpireIn`。

## 個人存取權杖 {#personal-access-tokens}

使用 `artisan passport:client --personal --provider=users` 建立個人用戶端。在應用程式上下文呼叫 `PersonalAccessTokenFactory::make(user_id, "Automation", &["read".into()], "users").await?`。個人授權使用專用伺服器，公開權杖端點不接受它。依 provider 選擇用戶端；無 provider 的用戶端僅能作為已設定 API provider 的回退。

編譯使用者模型可實作 `HasApiTokens`，在 `getProviderName` 明確回傳設定的 provider，再呼叫 `user.createToken("Automation", &["read".into()]).await?`。Rust 要求明確宣告，不推斷 PHP 模型類別名稱。權杖保存使用者、名稱與 scope，沒有重新整理權杖。`personalAccessTokensExpireIn` 預設獨立為一個日曆年。

`PersonalAccessTokenResult` 的序列化、`toArray`、`toJson` 提供 `accessTokenId`、`accessToken`、`tokenType`、`expiresIn`。`getToken().await?` 載入並快取真實預設 `Token`，回傳 `Option<PersonalToken<Token>>`，即 `Arc<tokio::sync::Mutex<Token>>`；重複讀取共用同一模型。編譯自訂模型使用 `getTokenFor::<CustomToken>()`。鎖定模型後透過 `TokenModel` 撤銷。Debug 輸出隱藏 Bearer。

## 權杖作用域 {#token-scopes}

使用 `tokensCan(BTreeMap::from([("read".into(), "讀取資料".into())]))` 註冊 scope 描述；`setDefaultScope(vec!["read".into()])` 設定省略 `scope` 時的預設值。明確未知 scope 會遭拒絕，簽名前套用用戶端 scope 限制；機器權杖可以保留 `*`。`defaultScopes(None)` 讀取，`defaultScopes(Some(scopes))` 更新，協定層正規化空格分隔字串。

`scopeIds`、`hasScope`、`validScopes` 及 `scopesFor` 查詢設定。`validScopes` 依輸入順序過濾並去重，`scopesFor` 保留重複並過濾未知 ID。`hasScope("*")` 為真，但 `validScopes` 僅在明確設定時保留 `*`。

```rust
use openrust_passport::http::middleware::{CheckToken, CheckTokenForAnyScope};
route.middleware(CheckToken::shared(["read", "write"]));
route.middleware(CheckTokenForAnyScope::shared(["read", "write"]));
```

原生中間件值取代 PHP 中間件字串。`CheckToken` 要求全部 scope，`CheckTokenForAnyScope` 要求至少一個；空的全部清單通過，空的任一清單拒絕。未認證回傳 401，scope 不足回傳 403、`Invalid scope(s) provided.`。原生錯誤類型位於 `exceptions::MissingScopeException`。`TokenGuard::currentToken(request)` 讀取選定守衛的權杖，第一方暫態權杖允許全部 scope。

## SPA 認證 {#spa-authentication}

將共用的 `CreateFreshApiToken::using(Some("web"))` 放入 web 堆疊，位於 Session 與 Cookie 中間件之後。已認證 GET 回應附加加密 HttpOnly `openrust_token` Cookie，已有同名回應 Cookie 時保留原值。內部簽名 JWT 包含真實使用者識別、CSRF 與期限，期限及 Cookie 屬性沿用 Session 設定。`passport.cookie(Some("custom_name"))` 修改名稱。

API 資源使用 Passport 守衛。API 堆疊通常不含 web `EncryptCookies`，由 Passport 自行解密；若應用程式已提前解密，應明確呼叫 `withoutCookieEncryption`。`withCookieEncryption`、`withCookieSerialization`、`withoutCookieSerialization`、`encryptTokensUsing` 及 `tokenEncryptionKey` 設定產生與消費一致的格式和金鑰。設定歸屬應用程式實例；相同金鑰與格式的應用程式可能互讀 Cookie。

透過 `X-CSRF-TOKEN` 傳送 Session CSRF，或用 `X-XSRF-TOKEN` 傳送加密 XSRF Cookie，即使 Cookie API 請求為 GET 也要求匹配。錯誤簽名、過期、CSRF 或使用者不存在均未認證。明確 Bearer 優先，失敗時不回退 Cookie。`ignoreCsrfToken(true)` 為明確略過，預設關閉。

`POST /oauth/token/refresh` 經過 web Session 與 CSRF，回傳 200 `Refreshed.` 並重新整理 Cookie。Cookie 認證產生請求內暫態權杖，不保存資料庫存取權杖。web 登出路由也須安裝 `CreateFreshApiToken`，透過前後身分變化清除此 Cookie；這是取代 PHP 全域請求登出監聽器的原生調適，未安裝的路由不會自動清除。

## 事件 {#events}

透過框架分發器監聽型別化 `events::AccessTokenCreated`、`RefreshTokenCreated` 與 `AccessTokenRevoked`。載荷含權杖、用戶端與使用者 ID，不含 Bearer 或用戶端秘密。先保存存取模型並分發建立事件，再保存重新整理模型並分發。同步監聽器失敗會回滾簽發；`Delivery::AfterCommit` 讀取已提交記錄。重新整理輪換在交易內分發撤銷事件，直接呼叫存取權杖 `revoke` 僅修改儲存，不分發該輪換事件。

## 測試 {#testing}

在應用程式上下文用 `passport.actingAs(&mut request, Box::new(user), &["read".into()], "api", None).await?` 設定使用者，或用 `passport.actingAsClient(&mut request, &client, &["read".into()], "api").await?` 設定機器。這些輔助方法選定守衛並將帶 scope 的權杖附到明確請求，不簽發、不寫入資料庫，也不影響後續請求。`actingAsClient` 要求請求沒有快取使用者，從使用者身分切換時建立新請求。真實協定測試仍須取得並提交簽名權杖。

## 密碼授權 {#password-grant}

Password 與 Implicit 預設關閉，建立對應用戶端記錄不會啟用協定。明確需要 Password 的整合，在解析授權伺服器前呼叫應用程式實例的 `passport.enablePasswordGrant()`；`passwordGrantEnabled` 屬於應用程式實例，設為 false 後拒絕後續請求。瀏覽器常規流程仍使用授權碼及 PKCE。

透過 `artisan passport:client --password --provider=users` 建立用戶端（`--public` 建立公開用戶端）。向 `POST /oauth/token` 傳送 `grant_type=password`、`client_id`、`username`、`password` 與可選 `scope`；機密用戶端還提供 `client_secret` 或 HTTP Basic。成功回傳簽名存取權杖，用戶端支援時也簽發重新整理權杖，沿用普通存取／重新整理期限。錯誤使用者憑證回傳 400 `invalid_grant`、`The user credentials were incorrect.`；缺少欄位、錯誤用戶端和未知 scope 保留 OAuth 錯誤回應。

`bridge::UserRepository::getUserEntityByUserCredentials` 選擇用戶端 provider，空值回退到 `auth.guards.api.provider`。預設編譯 Eloquent provider 查詢 `email` 並驗證真實密碼雜湊。框架自訂 `UserProvider` 透過 `retrieve_by_credentials`、`validate_credentials` 提供模型 `findForPassport` 和 `validateForPassportPasswordGrant` 的原生等價職責，不啟動 Session、不重新雜湊密碼。

組合模型鉤子 `findAndValidateForPassport` 使用 `passport.findAndValidateForPassportUsing("users", resolver)` 註冊。原生 `bridge::PasswordGrantResolver` 是接收借用使用者名稱、密碼及 `ClientRecord` 的編譯函數，回傳 `AuthFuture<AuthResult<Option<AuthIdentifier>>>`；它優先處理該 provider，須執行應用程式真實查詢及驗證。明確註冊取代 PHP 執行階段方法探索。例外回傳隱藏敏感資料的 500，不冒充密碼錯誤。

## 隱式授權 {#implicit-grant}

僅在整合明確要求時呼叫 `passport.enableImplicitGrant()`，再透過 `artisan passport:client --implicit --redirect_uri=https://client.example/callback` 建立公開用戶端。向 `GET /oauth/authorize` 傳送 `response_type=token`、`client_id` 及可選 `redirect_uri`、`scope`、`state`、`prompt`，沿用 web Session、CSRF、授權檢視、使用者綁定和一次性同意 nonce。此可選舊流程不取代上文 PKCE 範例。

批准後 302 回到已校驗的回呼，fragment 包含 `access_token`、`token_type`、`expires_in` 與已提供的 `state`，不建立授權碼或重新整理權杖。未知 scope、需要登入／同意及拒絕也使用 fragment，錯誤用戶端或不可信回呼在本機失敗，保留既有回呼查詢。Implicit 與裝置授權的 scope 最終處理移除 `*`；Password、PAT 與用戶端憑證保留它，用戶端 scope 限制仍參與。

`implicitGrantEnabled` 屬於應用程式實例，設為 false 也拒絕尚未完成的 Implicit 同意請求。原生實作提交前複查有效用戶端及註冊位址，模型／監聽器失敗回滾交易。同意 POST 不能修改已保存的用戶端、使用者、scope 或回應類型。權杖端點始終不接受 `grant_type=implicit`。
