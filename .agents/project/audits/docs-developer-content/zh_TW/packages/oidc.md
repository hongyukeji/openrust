# 开发者文档边界整理：docs/zh_TW/packages/oidc.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/packages/oidc.md (迁移前)
+++ docs/zh_TW/packages/oidc.md (开发者正文)
@@ -117,7 +117,7 @@

 ## 瀏覽器 SSO 與登出 {#sso-logout}

-API-O02 驗證兩個獨立 RP 應用連接同一受控 OP：用戶端 ID、應用金鑰、資料庫和本地使用者各自隔離。首次在 OP 登入後，第二個 RP 可藉助既有 OP 瀏覽器 Session 授權；每個 RP 仍擁有獨立本地 Session，並分別解析 `(issuer, subject)`。`prompt("login")` 請求 OP 重新認證，`maxAge` 另外檢查傳回的 `auth_time`。
+為每個 RP 分別設定用戶端 ID、應用金鑰、資料庫和本地使用者綁定。首次在 OP 登入後，第二個 RP 可藉助既有 OP 瀏覽器 Session 授權；每個 RP 仍擁有獨立本地 Session，並分別解析 `(issuer, subject)`。`prompt("login")` 請求 OP 重新認證，`maxAge` 另外檢查傳回的 `auth_time`。

 RP 發起登出遵循 OpenID Connect RP-Initiated Logout 1.0（2022-09-12）。將 `OIDC_POST_LOGOUT_REDIRECT_URI`（`post_logout_redirect`）設為 OP 已登記的固定回呼。在 `user()` 及本地帳戶綁定成功後，呼叫 `Oidc::provider("work", request)?.rememberLogout(&authentication)?`，明確將已驗證的 ID Token 儲存到伺服器端 Session。`LogoutHint` 是不透明型別，修改公開 token 欄位無法替換擷取的已驗證提示。Session 儲存和送出的登出 URL 均視為敏感資料。

@@ -169,9 +169,9 @@

 固定傳回處理器呼叫 `Oidc::provider("work", request)?.completeLogout().await?`，再傳回本地已登出頁面。完成時校驗 Session 綁定、設定、目標、期限與共用原子重放宣告；失敗同樣消耗本次嘗試，且不會登入任何使用者。此結果確認傳回狀態，不證明全部遠端憑證均已撤銷。

-OP 瀏覽器 Session 結束後，另一個 RP 的既有本地 Session 仍有效。既有 access／refresh token 遵循 OP 獨立的撤銷策略。本實作不提供 front-channel／back-channel 登出通知及全域單點登出。瀏覽器驗收涵蓋並行授權、強制重新認證、issuer／綁定拒絕、CSRF、本地／OP 登出、並行／過期／重放傳回狀態、OP 不支援及 bearer token 邊界。夾具為受控測試 OP，不是正式身份提供方。
+OP 瀏覽器 Session 結束後，另一個 RP 的既有本地 Session 仍有效。既有 access／refresh token 遵循 OP 獨立的撤銷策略。本實作不提供 front-channel／back-channel 登出通知及全域單點登出。


 ## 測試與部署 {#testing}

-受控供應商測試使用真實 TCP HTTP、RSA 簽章、同 kid 輪換、Session 中介層和 MySQL 8.4。兩個 RP 副本在載入相同回呼 Session 後，透過共用資料庫快取原子認領 state，只有一個請求呼叫 token 端點。另涵蓋 PKCE／用戶端認證、宣告／Discovery／UserInfo 失敗、到期或替換嘗試、跨應用 Cookie、本機綁定和同信箱不合併。真實第三方授權需要供應商 issuer、已登記測試應用、用戶端憑證、獲准回呼與測試帳戶。API-O02 瀏覽器結果與登出邊界見上文。
+在身分提供方註冊應用程式和回呼，設定 issuer 與用戶端憑證，並使用獲該應用程式授權的帳戶。多個 RP 副本須共用 Session 儲存及支援原子操作的資料庫或 Redis 快取，防止回呼重播。
````
