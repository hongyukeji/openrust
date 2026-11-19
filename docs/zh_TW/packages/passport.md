# Passport

## 安裝 {#installation}

已提供 Passport 的設定、金鑰生命週期與五張 OAuth 表的遷移發布。OAuth 端點與權杖 Guard 按後續 Passport 單元交付，目前尚無 `install:api --passport`。既有 Sanctum 安裝維持獨立。

目前應用程式預留了可選的 `passport` Cargo feature。將它加入 Cargo 預設 features，讓 `./artisan` 與一般建置持續啟用套件，並保留原有其他預設項目：

```toml
[features]
default = ["passport"]
```

套件探索會註冊 `openrust_passport::PassportServiceProvider`。沒有套件探索的應用程式可用 `withProviders` 明確註冊。啟用 Provider 不會建立資料表、產生金鑰或開放占位 OAuth 路由。

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

套件預設提供 `guard = "web"`、空的中介軟體清單，以及可為空的金鑰和連線設定；應用程式設定覆寫套件預設值。`PASSPORT_PRIVATE_KEY`、`PASSPORT_PUBLIC_KEY` 支援內嵌 RSA PEM、字面 `\n` 換行、本機絕對路徑或 `file://` 路徑（Unix 包含 `file://localhost/`）。相對路徑依固定 PHP 本機檔案包裝器行為拒絕，不會自動繫結至應用程式根目錄。未設定時延遲讀取儲存目錄中的檔案。支援 RSA PKCS#1／PKCS#8 私密金鑰與 RSA／SubjectPublicKeyInfo 公開金鑰，不宣稱涵蓋 League CryptKey 的通用 EC 金鑰 API。Debug 與載入錯誤不包含原始金鑰內容。

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

用戶端儲存庫支援查詢、建立、更新、秘密輪替、擁有權篩選和撤銷，操作真實 Eloquent 模型。`passport:client` 可建立六類用戶端。OAuth 端點和權杖驗證仍待完成。

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

`createPersonalAccessGrantClient`、`createPasswordGrantClient`、`createClientCredentialsGrantClient`、`createImplicitGrantClient` 和 `createDeviceAuthorizationGrantClient` 選擇對應授權類型。建立記錄不等於啟用 Password、Implicit 或裝置協定端點。`update` 修改名稱和重新導向位址，`regenerateSecret` 產生新秘密；兩者經 Eloquent 儲存並回傳其布林結果。`findForUser` 和 `forUser` 排除已撤銷用戶端，後者按名稱排序。`personalAccessClient(provider)` 選擇對應使用者提供者的最新有效個人用戶端，僅當提供者等於 `auth.guards.api.provider` 時也接受空提供者，找不到時回傳錯誤。

機密用戶端重用應用既有的 Eloquent 非同步雜湊服務。建立或輪替的 `Client` 在 `plainSecret` 暫時保留 setter 輸入，重新讀庫不會還原明文。預設 JSON 隱藏 `secret`，不附加明文。只在交付擁有者時複製新產生的秘密，不要記錄日誌。`record.model::<M>()` 回傳的 `Arc<tokio::sync::Mutex<M>>` 指向實際選定模型；`sameInstance` 檢查共用物件身分，不代表兩行模型的主鍵相等。

`client.append(&["plain_secret"])` 顯式附加該實例的暫時明文；重新讀庫的模型附加後得到 null。附加仍遵守 `makeHidden` 與 `setVisible`。

`ClientModel::hasScope` 與 `ClientRecord::hasScope` 在未設定 `scopes` 或其原始值為 null 時允許所有 scope；已設定時按陣列內容檢查，成員判斷沿用 PHP 非嚴格比較。預設不開啟父 scope 繼承。若在目前應用上呼叫 `passport.withInheritedScopes.store(true, std::sync::atomic::Ordering::Relaxed)`，`orders` 也允許 `orders:read` 等後代。`*` 在這裡是一般 scope 名稱。開關儲存在各應用自己的實例中，呼叫須位於所屬應用的請求或 Artisan 生命週期。

在應用裝配時、解析儲存庫前呼叫 `passport.useClientModel::<CustomClient>()`。編譯期類型實作 `ClientModel` 和 Eloquent `Model`，保留唯一識別校驗、全域作用域、事件與轉換。格式錯誤的 UUID／ULID 在解析連線前直接未命中。儲存庫查詢與未綁定實例的 `Client::query()`／`save()` 優先使用模型顯式連線，接著是 `passport.connection`，最後是資料庫預設連線。建立和讀出的模型保留實際連線；隨後更改設定也不會把該模型的儲存轉到其他連線。`Client::queryOn(&connection)` 顯式選擇連線。交易寫入檢查模型的有效連線，拒絕不同連線的交易。Rust 的 `resolveConnectionName()` 回傳可擁有的名稱，以讀取應用設定；`getConnection()` 回傳實際連線。完整 Client 模型介面、PHP JSON 編碼選項與浮點轉字串的精確規則仍待驗收。

## 用戶端工廠

`Client::factory()?` 建立 `database::factories::ClientFactory<Client>`。可呼叫 `asPublic`、`asPasswordClient`、`asPersonalAccessTokenClient`、`asImplicitClient`、`asClientCredentials` 或 `asDeviceCodeClient` 套用對應授權狀態。後續狀態覆寫先前屬性。預設產生機密授權碼用戶端，包含隨機 40 字元秘密和一個產生的重新導向 URI；擁有者為空，revoked 為 false。

`make().await?` 回傳未儲存模型，`create().await?` 持久化模型。賦值執行模型的非同步修改器和轉換：新 Client 保留 `plainSecret`，重新讀取資料庫只包含雜湊。`state` 新增應用程式屬性；`intoFactory()` 回傳現有 Eloquent 工廠 Builder，以使用數量、關係與回呼。

Rust 在編譯期選擇工廠模型：自訂 `ClientModel` 使用 `ClientFactory::<CustomClient>::new()?`，不動態讀取 `useClientModel`。建構時解析並保存模型的有效連線，包括 `passport.connection`，設定錯誤透過 Result 回傳；修改設定後應新建工廠。協定端點與其餘 Client 關係仍待完成。

## 用戶端命令與撤銷

執行遷移後，用 `./artisan passport:client` 建立用戶端。`--personal`、`--password`、`--client`、`--implicit`、`--device` 選擇授權類型；未傳這些選項時建立授權碼用戶端。多個選項同時存在時依上述順序優先。`--name`、`--provider`、`--redirect_uri` 提供互動值，多個重新導向位址以逗號分隔。`--public` 為密碼、裝置和授權碼用戶端省略秘密。隱含用戶端始終公開；個人和用戶端憑證用戶端始終保密。

使用 `--no-interaction` 時，名稱預設取 `app.name`，回呼預設取應用 URL 加 `/auth/callback`，附加裝置流程預設關閉。互動建立授權碼用戶端時，若 `deviceCodeGrantEnabled` 為 true（預設值），會詢問是否啟用裝置流程。命令輸出用戶端 ID；保密用戶端還輸出暫存秘密和保存提醒。個人用戶端建立只輸出成功訊息。

`clients.delete(&client).await?` 按每批 1000 個載入用戶端的存取權杖，逐一先撤銷重新整理權杖、再撤銷存取權杖，最後撤銷用戶端。儲存經過 Eloquent 轉換和事件；錯誤向上傳遞，先前儲存保留。事件取消儲存回傳 false，不中止撤銷鏈，與 Passport 一致。授權碼不變。`TokenModel::revoke`、`RefreshTokenModel::revoke` 也可個別撤銷實例。

## 模型與關聯

`Token`、`RefreshToken`、`AuthCode` 使用非自增字串主鍵、布林 `revoked`、日期時間 `expires_at` 轉換，以及 Passport 連線回退。只有 `Token` 使用時間戳記，其 scopes 為陣列。`Token::can` 拒絕空 scope 清單，支援 PHP 非嚴格比較的 `*` 萬用字元，並讀取目前應用的繼承設定；`cant` 回傳相反結果。

`ClientModel` 提供 `tokens`、`authCodes`、`owner` 和已棄用的 `user::<U>()`；`Token` 提供 `client`、`refreshToken` 和已棄用的 `user::<U>("驗證識別欄")`；`RefreshToken` 提供 `accessToken`；`AuthCode` 提供 `client`。Token 的使用者鍵須與編譯 provider 模型的 `Authenticatable` 識別欄一致，Client 的使用者關聯使用主鍵。多型擁有者透過既有 `owner().target::<U>(別名, 主鍵)` Builder 綁定。

Rust 在編譯時綁定關聯回傳類型。預設輔助方法回傳預設具體模型；自訂模型使用 `tokensFor::<T>`、`clientFor::<C>`、`refreshTokenFor::<R>`、`accessTokenFor::<T>`。`passport.useTokenModel::<T>()` 和 `useRefreshTokenModel::<R>()` 選擇儲存庫撤銷時使用的實際模型，保留其連線和事件；共用註冊也作用於已解析的儲存庫。使用者 provider 由呼叫端選擇編譯模型，替代 PHP 的執行期類別名稱。

`clientUuids` 預設 true。已有數字自增用戶端表時，在建構模型前呼叫 `passport.clientUuids.store(false, std::sync::atomic::Ordering::Relaxed)`。每個實例保留建構時選定的策略。開關不會修改既有資料表結構；發布的用戶端遷移仍建立 UUID 主鍵。
