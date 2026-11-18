# Sanctum

## 安裝 API 路由 {#installation}

執行 `./artisan install:api`，建立並註冊 `routes/api.rs`，同時把 Sanctum 加入應用程式的 Cargo 預設功能。互動終端會詢問是否執行待處理的遷移，確認後會先重新建置再遷移。CI 可使用 `--without-migration-prompt`，再單獨執行 `./artisan migrate`。使用 `--force` 覆寫既有 API 路由。 安裝器無法修改自訂啟動檔時，會保留產生的路由並提示手動註冊。產生的路由會回傳已註冊驗證模型的可見屬性。選用的遷移失敗時，安裝器會顯示錯誤並保留鷹架。自動化需要遷移結束代碼時，請單獨執行 `./artisan migrate`。

預設應用程式骨架不預置 Sanctum 專屬設定或使用者模型註冊。啟用 Cargo 功能後，由套件提供預設設定。新應用程式執行 `install:api` 會發佈權杖資料表遷移，並接入發佈的設定和預設使用者模型。使用自訂使用者模型或 Provider 時，請實作 `HasApiTokens` 並透過 `AuthManager::registerSanctumModel` 註冊支援權杖的模型。手動啟用功能時，可用 `vendor:publish --tag=sanctum-migrations` 發佈遷移，用 `vendor:publish --tag=sanctum-config` 發佈可編輯設定。手動發佈後，在 `config/mod.rs` 宣告模組並將載入結果註冊到 `sanctum` 設定群組。

在終端機執行 `vendor:publish` 可互動選擇服務提供者或標籤。第三方套件在服務提供者的 `boot` 中呼叫 `publishes` 或 `publishesMigrations` 註冊編譯進程式的資源。啟用 `database.migrations.update_date_on_publish` 時，新遷移會使用新的時間戳記。關閉設定，或在服務提供者中呼叫 `VendorPublishCommand::dontUpdateMigrationDates`，可保留來源檔案日期。重複發佈沿用既有檔名，維持遷移帳本穩定。遷移發佈到 `database/migrations/` 後，重新建置即可將其納入編譯清單。

## 權杖資料表遷移 {#token-table-migration}

發佈後的遷移屬於應用程式。重新建置會將它納入遷移清單，即使關閉 Cargo 的 `sanctum` 功能也能執行或復原。

```sh
./artisan install:api --without-migration-prompt
./artisan migrate
./artisan migrate:status
```

遷移建立多型歸屬欄位、唯一的 64 字元權杖雜湊欄位、可為空值的能力和時間欄位，以及到期時間索引。此功能同時註冊 Sanctum 守衛與 CSRF Cookie 路由。

發佈的遷移使用 `table.morphs("tokenable")`，預設建立整數型 `tokenable_id`。歸屬模型使用 UUID 或 ULID 時，請在首次遷移前，從 `openrust::database::schema` 匯入 `Schema`，並在應用程式服務提供者的 `boot` 中呼叫 `Schema::morphUsingUuids()?` 或 `Schema::morphUsingUlids()?`。此設定適用於目前應用程式後續的 `morphs` 呼叫。權杖資料表本身的主鍵仍為整數。

## 簽發個人存取權杖 {#issuing-api-tokens}

使用 `Sanctum::personalAccessTokens` 解析出的權杖儲存庫建立權杖，傳入歸屬型別、歸屬 ID、名稱、能力和選用的到期時間。簽發時將傳回的 `plain_text_token` 交給用戶端。資料庫保存 SHA-256 摘要。`findToken` 查找憑證，`revokeToken` 依歸屬刪除權杖。`can` 支援精確能力及萬用字元 `*`。HTTP 守衛在驗證請求前檢查到期時間。 簽發、驗證時寫回及單筆撤銷都會走 Eloquent 生命週期。預設權杖回應包含 `updated_at`。

```rust
use openrust::{auth::AuthResult, database::DatabaseManager, sanctum::{NewAccessToken, Sanctum}};

async fn issue(sanctum: &Sanctum, database: &DatabaseManager, user_id: u64) -> AuthResult<NewAccessToken> {
    sanctum.personalAccessTokens(database, "")?
        .createToken("app::models::User", user_id, "browser", &["reports:read"], None)
        .await
}
```

在服務提供者中透過應用程式的 `Sanctum` 服務呼叫 `usePersonalAccessTokenModel::<M>()`，選擇 Eloquent 模型的資料表和連線。應用程式遷移負責建立自訂資料表，使用標準 Sanctum 屬性以及模型宣告的主鍵。簽發時套用模型的屬性修改器和轉換，並觸發 Eloquent 建立事件。Bearer 驗證透過模型的存取器和轉換讀取屬性，再透過同一模型實例儲存最後使用時間。`revokeToken` 依歸屬刪除模型實例，遵循模型刪除事件。Artisan 清理套用模型全域作用域並批次刪除，不逐筆觸發模型事件。

使用 UUID 或 ULID 主鍵時，在遷移中宣告對應的欄位，並在模型中用 `ModelMetadata::uniqueId(UniqueId::Uuid)` 或 `UniqueId::Ulid` 設定產生策略。若主鍵欄位名稱不是 `id`，用 `primaryKey` 宣告。憑證格式仍為 `<主鍵>|<密鑰>`。權杖與歸屬識別碼使用 `AuthIdentifier` 保留整數或字串型別。歸屬主鍵是字串時，遷移須使用相符的 `tokenable_id` 欄位型別，簽發和撤銷時傳入該歸屬識別碼。

可用 `SANCTUM_TOKEN_PREFIX` 為新簽發的權杖密鑰設定應用程式專用前綴。

自訂模型須實作 `PersonalAccessTokenModel`。預設 `findToken` 會驗證 SHA-256 摘要。覆寫方法會收到請求憑證和已設定的 Eloquent 查詢，可在附加條件或解碼憑證後呼叫 `findTokenByDigest`。覆寫方法須驗證其接受的憑證。

應用程式服務透過 `personalAccessTokenModel()` 傳回已註冊的模型型別名稱。使用 `currentApplicationUrlWithPort()` 取得應用程式 URL 的主機和明確連接埠。傳回值帶有前導逗號，可附加至有狀態網域設定。

## 模型權杖關聯 {#model-token-relationship}

在 Eloquent 歸屬模型上實作 `HasApiTokens`。使用 `user.tokens(&sanctum, &database)?` 查詢或撤銷該模型的權杖，使用 `user.createToken(&sanctum, &database, name, abilities, expires_at)` 簽發。關聯使用已設定的權杖模型及歸屬模型的多型別名和主鍵。`deleteToken` 和 `delete` 依關聯範圍批次刪除。權杖可透過 `tokenable` 解析歸屬模型。歸屬模型可透過 `currentAccessToken`、`tokenCan` 和 `tokenCant` 檢查目前請求的權杖及能力。

使用 `user.tokensQuery::<DefaultPersonalAccessToken>(&sanctum, &database)?` 串接查詢。用 `user.tokensRelation::<DefaultPersonalAccessToken>(&sanctum)?` 取得關聯，再使用 Eloquent 的 `with("tokens", relation)` 預先載入。使用自訂模型時，請替換成已註冊的模型型別。簽發回應的內嵌權杖採用模型的屬性轉換、存取器、附加欄位及可見性，並一律排除摘要。直接序列化自訂模型時，應在模型中隱藏 `token` 欄位。

## 保護路由與檢查能力 {#protecting-routes}

在受保護的路由加上 `auth:sanctum`，以 `Authorization: Bearer <token>` 傳送憑證。憑證無效、到期或已撤銷時回傳 HTTP 401。`abilities:reports:read,reports:write` 要求具備兩項能力，`ability:reports:read,reports:write` 要求具備任一項。權杖已通過驗證但缺少能力時回傳 HTTP 403。

```rust
use openrust::{http::ApiResponse, routing::RoutingError, support::facades::Route};

fn register() -> Result<(), RoutingError> {
    Route::getWith("/reports", || ApiResponse::success(["quarterly"]), |route| {
        route.middleware_name("auth:sanctum").middleware_name("abilities:reports:read");
        Ok(())
    })
}
```

簽發時以已註冊的 Provider 模型名稱作為權杖歸屬型別。預設應用程式使用 `app::models::User`。可在 `auth.guards` 為自訂 Sanctum 守衛設定 `provider`，省略時透過已註冊的 Eloquent 模型和 `MorphMap` 解析歸屬。以 `AccessToken::currentAccessToken(request)` 檢視目前請求的憑證。`sanctum.expiration` 以分鐘限制權杖最長有效期，與個別權杖的到期時間同時生效。Bearer 驗證成功後派發 `TokenAuthenticated` 事件並更新 `last_used_at`，可設定 `sanctum.last_used_at=false` 關閉。

使用 `getAccessTokenFromRequestUsing` 取代 Bearer 擷取邏輯。使用 `authenticateAccessTokensUsing` 決定已查到的權杖是否有效，回呼接收權杖及內建到期和 Provider 檢查結果。權杖歸屬仍須成功解析。

在驗證管理器中以 `AuthManager::registerSanctumModel` 註冊支援權杖的模型。簽發時可明確呼叫 `NewAccessToken::toJson()` 或 `toArray()` 傳回明文憑證。內嵌權杖不含資料庫摘要，驗證事件資料可從 `TokenAuthenticated::token` 取得。

## SPA 驗證 {#spa-authentication}

在 `bootstrap/app.rs` 現有的 `withMiddleware` 閉包呼叫 `middleware.statefulApi();`。以 `SANCTUM_STATEFUL_DOMAINS` 設定以逗號分隔的第一方前端主機，非預設連接埠須一併填寫。使用預設路由設定時，先請求 `GET /sanctum/csrf-cookie`，再經由應用程式的 Web 工作階段登入入口登入。後續 API 請求攜帶 Cookie。需要 CSRF 檢查時，將 URL 解碼後的 `XSRF-TOKEN` Cookie 值放入 `X-XSRF-TOKEN` 請求標頭。

跨來源第一方前端須為指定來源啟用攜帶憑證的 CORS。符合設定的 Referer 或 Origin 會啟用 Session、Cookie 和 CSRF 中介軟體。Sanctum 優先檢查已驗證的工作階段，再檢查 Bearer 權杖。第一方工作階段會通過權杖能力檢查，使用者操作權限由應用程式授權策略決定。

第一方請求成功記錄密碼指紋後，變更密碼會使該工作階段在下次請求時失效。若要替換前端中介軟體，先透過應用程式的 `alias` 方法註冊，再將別名填入 `sanctum.middleware.encrypt_cookies`、`sanctum.middleware.validate_csrf_token` 或 `sanctum.middleware.authenticate_session`。

在 `SANCTUM_STATEFUL_DOMAINS` 中加入 `__SANCTUM_CURRENT_REQUEST_HOST__`，或在逗號分隔的網域字串末尾附加 `Sanctum::currentRequestHost()`，即可比對目前請求的主機和連接埠。

## 清理過期權杖 {#pruning-expired-tokens}

啟用 Sanctum 功能後執行以下命令。預設保留已過期權杖 24 小時。清理會檢查權杖的明確到期時間，也會檢查設定 `sanctum.expiration` 時依建立時間計算的到期時間。應用程式支援透過 `SANCTUM_EXPIRATION` 設定有效期限，單位為分鐘。

```sh
./artisan sanctum:prune-expired --hours=24
```

## 測試 {#testing}

在測試中傳入擁有所有權的 Eloquent 使用者，用 `Sanctum::actingAs(&mut app, user, &["reports:read"], None)` 為 `TestApp` 後續請求設定身分和能力。預設選擇 Sanctum 守衛。可傳入 `Some("guard-name")` 指定守衛，或傳入 `["*"]` 授予全部能力。測試夾具直接提供身分和能力，無須簽發資料庫權杖。

傳回的共享使用者已清除 `wasRecentlyCreated` 狀態。
