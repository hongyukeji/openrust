# 开发者文档边界整理：docs/zh_TW/packages/fortify.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/packages/fortify.md (迁移前)
+++ docs/zh_TW/packages/fortify.md (开发者正文)
@@ -2,7 +2,7 @@

 ## 簡介

-Fortify 提供認證路由與應用擴充契約，不包含使用者介面。本頁介紹依 Fortify 1.43.0 驗收的 API-F01 認證與 API-F02 Passkeys 生命週期。
+Fortify 提供認證路由與應用擴充契約，不包含使用者介面。認證 API 對應 Fortify 1.43.0。

 ## Fortify 與 Sanctum

@@ -74,7 +74,7 @@

 ## Passkeys

-API-F02 生命週期依 Laravel Passkeys 伺服器端 0.3.0 實作，並串接已驗收的 `@laravel/passkeys` 0.4.1 用戶端。`fortify:install` 發佈 `passkeys` 資料表遷移，為預設 User 加入 `PasskeyAuthenticatable`，並在產生的 Provider 註冊 `Passkeys::useUserModel::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新建置並執行遷移。自訂 User 實作 `openrust_fortify::passkeys::PasskeyAuthenticatable`，並在應用 Provider 明確註冊。
+Passkeys API 對應 Laravel Passkeys 伺服器端 0.3.0，搭配 `@laravel/passkeys` 0.4.1 用戶端使用。`fortify:install` 發佈 `passkeys` 資料表遷移，為預設 User 加入 `PasskeyAuthenticatable`，並在產生的 Provider 註冊 `Passkeys::useUserModel::<User>()`。保留 `fortify.features` 中的 `passkeys`，重新建置並執行遷移。自訂 User 實作 `openrust_fortify::passkeys::PasskeyAuthenticatable`，並在應用 Provider 明確註冊。

 設定 `fortify.passkeys.relying_party_id`、`allowed_origins`、`user_handle_secret` 與 `timeout`。預設由 `APP_URL` 推導 RP ID 與精確 Origin，密鑰讀取 `PASSKEYS_USER_HANDLE_SECRET` 並退回 `APP_KEY`，逾時為 60000 毫秒。正式環境使用 HTTPS；開發支援 HTTP localhost。Origin 包含連接埠，不包含尾端斜線或路徑。輪替應用密鑰時保持使用者控制代碼密鑰穩定：控制代碼為模型資料表名稱與主鍵衍生的 64 位元組十六進位 HMAC-SHA256；更換密鑰會改變新註冊的控制代碼。

````
