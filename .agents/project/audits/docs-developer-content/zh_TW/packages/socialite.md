# 开发者文档边界整理：docs/zh_TW/packages/socialite.md

2026-11-20 从开发者文档迁出的内部过程说明及正文调整。以下为历史原文对照，不作为当前功能状态或新验收结果。

````diff
--- docs/zh_TW/packages/socialite.md (迁移前)
+++ docs/zh_TW/packages/socialite.md (开发者正文)
@@ -1,6 +1,6 @@
 # Socialite

-Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。已驗收範圍現包括固定 Socialite 5.33.0 對應的 GitHub、GitLab、Bitbucket、LinkedIn（舊式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2、Google 及 Facebook。Twitter OAuth1 與下方本機帳戶登入範例同樣可用。完整 OIDC RP 與 SSO 由 API-O01／02 獨立交付。
+Socialite 將第三方登入接入應用程式的 HTTP Client 與 Session。供應商 API 對應 Socialite 5.33.0，包括 GitHub、GitLab、Bitbucket、LinkedIn（舊式／OpenID）、Slack（identity／OpenID）、X／Twitter OAuth2、Google 和 Facebook，也支援 Twitter OAuth1 和下方本機帳戶登入範例。完整 OIDC RP 與 SSO 使用獨立的 [OIDC 套件](oidc.md)。

 ## 安裝 {#installation}

@@ -12,7 +12,7 @@

 ## 設定 {#configuration}

-透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。真實供應商驗收需要已註冊的 GitHub OAuth 應用程式和對應回呼。
+透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。供應商登入需要已註冊的 GitHub OAuth 應用程式和對應回呼。


 以下驅動同樣設定 client id、client secret 和 redirect；環境變數前綴後接 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`：
@@ -26,7 +26,7 @@
 | `slack` | `SLACK` | `identity.basic,identity.email,identity.team,identity.avatar` |
 | `slack-openid` | `SLACK_OPENID` | `openid email profile` |

-`services.gitlab.host`／`GITLAB_HOST` 預設 `https://gitlab.com`。`GitlabProvider::setHost` 保留自託管路徑前綴並移除結尾斜線。固定上游資料路徑為 `/api/v3/user`，本批保留此契約。Slack 登入設定與既有 `slack.notifications` 共處同一 services 物件；真實供應商仍需分別註冊回呼並取得權限。
+`services.gitlab.host`／`GITLAB_HOST` 預設 `https://gitlab.com`。`GitlabProvider::setHost` 保留自託管路徑前綴並移除結尾斜線。資料端點為 `/api/v3/user`。Slack 登入設定與既有 `slack.notifications` 共處同一 services 物件；真實供應商仍需分別註冊回呼並取得權限。

 `x` 使用 `X_CLIENT_ID`、`X_CLIENT_SECRET`、`X_REDIRECT_URI`。`twitter` 使用對應的 `TWITTER_` 變數，並設定 `TWITTER_OAUTH=2` 選擇 OAuth2；預設值 1 選擇 Twitter OAuth1；只有整數 2 選擇 OAuth2，字串 `"2"` 仍選擇 OAuth1。明確的 `twitter-oauth-2` 驅動先讀取 `services.twitter`，僅在物件缺失或為 null 時回退 `services.twitter-oauth-2`。`x` 同樣從 `services.x` 回退 `services.x-oauth-2`；已存在但不完整的設定會失敗，不靜默改用另一組憑證。

@@ -101,11 +101,11 @@
 使用者提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 數字／字串型別。`userFromToken(token)` 使用既有 token 讀取身分，`refreshToken(token)` 交換更新權杖。憑據顯式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字串使用 `Zeroizing<String>`，不會自動序列化或由 Debug 輸出。遠端 access token 不是 Passport／Sanctum 憑據。


-舊式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按偏好 locale 和上游嚴格的 100／800 像素選擇圖片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始圖片；Slack 兩種驅動暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定套件使用 access token 存取固定 UserInfo 介面，不驗證 ID Token、不執行 Discovery，也不是完整 OIDC RP；這些屬於 API-O01。
-
-Google 按固定驅動將長度大於 100 位元組且恰有兩個句點的 token 識別為 JWT。它讀取固定 Google JWKS 端點，透過採用 AWS-LC 的 `jsonwebtoken` 11.1.0 校驗金鑰演算法和簽章，要求 issuer 嚴格等於 `https://accounts.google.com`，audience 為與 client ID 相同的單一字串。存在的 `exp`／`nbf` 與 `iat` 回退按固定 PHP NumericDate 行為驗證。JWT 驗證失敗不會退回 Bearer UserInfo。此 Socialite 路徑按上游接受缺少 `exp`、數字日期字串與 null 日期，不執行 Discovery、不要求 nonce，也不是完整 OIDC RP；API-O01 有獨立協定範圍。Google raw 資料還補入 `id`、`verified_email`、`link`，並映射 `avatar_original`。
-
-Facebook Limited Login 從未驗證的 token header 偵測非 null 的 `kid`，隨後要求所選 Facebook JWKS 金鑰的 RS256 簽章、嚴格的 `https://www.facebook.com` issuer、單一 client audience 與匹配 nonce。在 `userFromToken(token).await?` 或回呼 `user()` 前呼叫 `provider.withNonce(expected)`；未明確指定時，從 `with` 參數的純量 `nonce` 讀取期待值。明確的 `withNonce` 優先，nonce 必須來自應用保存的授權嘗試。`withNonce` 只設定期待值，不自動產生、儲存或加入授權 nonce。簽章／宣告／nonce 失敗不會退回 Graph。請求選項與 `lastToken()` 留在目前 provider，保留的 token 使用 zeroize 清理。此行為對應固定 Socialite Limited Login（日期可選），不替代 API-O01 的完整 RP 協定。
+舊式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按偏好 locale 和上游嚴格的 100／800 像素選擇圖片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始圖片；Slack 兩種驅動暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定套件使用 access token 存取固定 UserInfo 介面，不驗證 ID Token、不執行 Discovery，也不是完整 OIDC RP；這些能力使用 [OIDC 套件](oidc.md)。
+
+Google 按固定驅動將長度大於 100 位元組且恰有兩個句點的 token 識別為 JWT。它讀取固定 Google JWKS 端點，透過採用 AWS-LC 的 `jsonwebtoken` 11.1.0 校驗金鑰演算法和簽章，要求 issuer 嚴格等於 `https://accounts.google.com`，audience 為與 client ID 相同的單一字串。存在的 `exp`／`nbf` 與 `iat` 回退按固定 PHP NumericDate 行為驗證。JWT 驗證失敗不會退回 Bearer UserInfo。此 Socialite 路徑按上游接受缺少 `exp`、數字日期字串與 null 日期，不執行 Discovery、不要求 nonce，也不是完整 OIDC RP；完整 RP 校驗使用 [OIDC 套件](oidc.md)。Google raw 資料還補入 `id`、`verified_email`、`link`，並映射 `avatar_original`。
+
+Facebook Limited Login 從未驗證的 token header 偵測非 null 的 `kid`，隨後要求所選 Facebook JWKS 金鑰的 RS256 簽章、嚴格的 `https://www.facebook.com` issuer、單一 client audience 與匹配 nonce。在 `userFromToken(token).await?` 或回呼 `user()` 前呼叫 `provider.withNonce(expected)`；未明確指定時，從 `with` 參數的純量 `nonce` 讀取期待值。明確的 `withNonce` 優先，nonce 必須來自應用保存的授權嘗試。`withNonce` 只設定期待值，不自動產生、儲存或加入授權 nonce。簽章／宣告／nonce 失敗不會退回 Graph。請求選項與 `lastToken()` 留在目前 provider，保留的 token 使用 zeroize 清理。此行為對應固定 Socialite Limited Login（日期可選），不替代 完整的 [OIDC RP 協定](oidc.md)。

 ### 登入已綁定的本機帳戶

@@ -184,21 +184,21 @@

 接入多個供應商時，在跳轉時儲存所選 namespace，回呼前消費並核對；為各供應商註冊固定路由，讓應用程式控制回呼 namespace。保留正常 state 校驗，不接受回呼傳入本機使用者 ID。本機登出使用受 CSRF 保護的 POST 路由，執行 `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`，結束目前應用 Session；供應商 token 撤銷與聯邦登出遵循各自協定。

-MySQL 8.4 驗收應用使用上述動作、真實 Eloquent 使用者、資料庫 Session、HTTP token／profile 端點和兩個隔離 schema。涵蓋同信箱不合併、供應商／subject 大小寫及數值區別、並行登入、唯一綁定、固定 Session 拒絕、恢復、CSRF／登出輪換、跨應用 Cookie、缺少／刪除帳戶、錯誤／重播 state、遠端撤銷及資料庫失敗。此為受控供應商應用整合驗收，真實供應商帳戶授權仍需要憑證與登記回呼。
+

 ## 測試 {#testing}

-透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。驗收執行真實 HTTP、兩個獨立 Session 應用、PKCE、state 消費、scope／參數映射、已驗證信箱、更新和失敗路徑，固定 PHP 原始碼 SHA 保護對照夾具。
-
-Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。受控驗收不冒稱真實 GitHub 登入，後者仍需要 OAuth 應用、憑據、回呼網域和帳戶。
-
-
-`ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。各驅動均以真實受控 HTTP 驗收 token 表單、認證標頭、資料查詢參數、可選信箱、Slack user／bot 憑據、refresh 與失效／撤銷憑據拒絕。六個驅動的映射、scope 及 Slack 專屬欄位與真實固定 PHP 方法對照。受控結果不冒稱已登入真實 GitLab、Bitbucket、LinkedIn 或 Slack 帳戶。
-
-X／Twitter 測試使用真實 HTTP 與加密 Session Cookie，涵蓋預設 PKCE、跨應用拒絕、state 重放、RFC3986 參數、Basic 交換／更新、錯誤回應及遠端撤銷。映射、欄位和 HTTP 選項以固定 PHP 方法提供對照。真實 X／Twitter 授權仍需供應商應用憑證、登記回呼和帳戶。
-
-`GoogleProvider::withEndpoints(endpoints, jwks)` 用於受控 UserInfo 與 JWKS 服務。真實 HTTP 測試使用原 PHP Socialite／Firebase JWT 方法對照 25 組簽章 JWT，包括錯誤簽章、issuer／audience、缺失或錯誤宣告、未來／過期時間、演算法混淆、金鑰輪換和跨 client 隔離。PEM 測試金鑰僅屬於這些夾具。真實 Google 帳戶授權仍需測試應用及回呼。
-
-`FacebookProvider::withEndpoints(endpoints, jwks)` 為受控測試替換三個 Graph／授權端點與 JWKS；包含 `/v23.0/` 的路徑隨 `usingGraphVersion` 變化。21 組原 PHP Limited Login 對照涵蓋 nonce 優先級、issuer／audience、簽章／過期、缺失金鑰及輪換；實際 Graph HTTP 驗證 HMAC proof、欄位、過期轉換、請求隔離及去識別觀察。真實 Facebook 授權仍需應用、憑證、登記回呼及帳戶。
-
-Twitter OAuth1 傳送 HMAC-SHA1 簽章請求，存取憑證另含明確的 `user.credentials.tokenSecret`。`userFromTokenAndSecret(token, secret).await?` 直接呼叫 profile。回呼映射包含 URL 欄位、location、description 與 `avatar_original`；憑證直取僅保留 League extra 欄位，與上游一致。成功 profile 按憑證對快取於目前請求內，任一憑證變化就重新取得。`one::TwitterProvider::withEndpoints` 與 `manager.extendOAuth1` 提供受控端點。固定 League 1.12.0／Socialite PHP 對照及真實 HTTP／Session 測試涵蓋簽章、空 verifier、scope、憑證快取、跨應用拒絕、遠端重播拒絕和失敗去敏；真實 Twitter 授權仍需要應用程式、憑證、登記回呼及帳戶。
+透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。
+
+Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。
+
+
+`ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。
+
+使用供應商授權前，先註冊應用程式和回呼並設定憑證。測試登入流程時使用專用帳戶。
+
+`GoogleProvider::withEndpoints(endpoints, jwks)` 用於受控 UserInfo 與 JWKS 服務。
+
+`FacebookProvider::withEndpoints(endpoints, jwks)` 為受控測試替換三個 Graph／授權端點與 JWKS；包含 `/v23.0/` 的路徑隨 `usingGraphVersion` 變化。
+
+Twitter OAuth1 傳送 HMAC-SHA1 簽章請求，存取憑證另含明確的 `user.credentials.tokenSecret`。`userFromTokenAndSecret(token, secret).await?` 直接呼叫 profile。回呼映射包含 URL 欄位、location、description 與 `avatar_original`；憑證直取僅保留 League extra 欄位，與上游一致。成功 profile 按憑證對快取於目前請求內，任一憑證變化就重新取得。`one::TwitterProvider::withEndpoints` 與 `manager.extendOAuth1` 提供受控端點。
````

````diff
--- docs/zh_TW/packages/socialite.md (迁移前)
+++ docs/zh_TW/packages/socialite.md (开发者正文)
@@ -13,7 +13,6 @@
 ## 設定 {#configuration}

 透過 `GITHUB_CLIENT_ID`、`GITHUB_CLIENT_SECRET` 和 `GITHUB_REDIRECT_URI` 設定 `services.github.client_id`、`client_secret`、`redirect`。以 `/` 開頭的回呼拼接到合法絕對 HTTP(S) `app.url`，絕對回呼保留原值。`services.github.scopes` 向驅動預設值追加 scope。缺少或無效設定明確失敗。供應商登入需要已註冊的 GitHub OAuth 應用程式和對應回呼。
-

 以下驅動同樣設定 client id、client secret 和 redirect；環境變數前綴後接 `_CLIENT_ID`、`_CLIENT_SECRET`、`_REDIRECT_URI`：

@@ -80,13 +79,11 @@

 跳轉或交換 code 前呼叫 `provider.with(serde_json::json!({"prompt":"select_account","login_hint":"alice@example.test"}).as_object().unwrap().clone())`。參數按固定上游順序覆蓋普通欄位，支援字串、布林和數字純量。`redirectUrl` 修改目前 provider 的回呼。

-
 Slack identity 預設使用 `user_scope`，同時將 `scope` 設為空。在跳轉和回呼的 provider 都執行 `provider.asBotUser()` 後，使用 `scope` 及 token 回應頂層憑據；普通使用者憑據取自 `authed_user`。預設 user-scope 欄位在 `with` 參數後套用，與上游一致。`BitbucketProvider` 另有顯式 `provider.getAccessToken(code).await?` 路徑，會追加 HTTP Basic 客戶端認證；普通 `user()` 使用繼承的 token 回應路徑。

 ## 存取範圍 {#access-scopes}

 GitHub 預設 `user:email`。`scopes(["read:user".into()])` 去重合併，`setScopes(vec!["read:user".into()])` 替換；`getScopes()` 讀取清單。GitHub 使用逗號分隔。包含 `user:email` 時查詢 `/user/emails` 並選擇 primary 且 verified 的信箱，失敗或無匹配傳回 `None`；移除該 scope 則保留 profile 的信箱。
-

 GitLab、Bitbucket、LinkedIn 兩個驅動以空格分隔 scopes；Slack identity 用逗號，Slack OpenID 用空格。Bitbucket 選擇型別為 `email` 且 primary、confirmed 的信箱，信箱介面失敗傳回 `None`。舊式 LinkedIn 分別載入本地化資料和信箱，含 `r_liteprofile` 時請求 `vanityName`，信箱請求失敗正常傳播。

@@ -99,7 +96,6 @@
 ## 讀取使用者詳情 {#retrieving-user-details}

 使用者提供 `getId`、`getNickname`、`getName`、`getEmail`、`getAvatar`、`getRaw` 和 GitHub `nodeId` 等映射 `attributes`；ID 保留 JSON 數字／字串型別。`userFromToken(token)` 使用既有 token 讀取身分，`refreshToken(token)` 交換更新權杖。憑據顯式放在 `user.credentials` 的 `token`、`refreshToken`、`expiresIn`、`approvedScopes`；字串使用 `Zeroizing<String>`，不會自動序列化或由 Debug 輸出。遠端 access token 不是 Passport／Sanctum 憑據。
-

 舊式 LinkedIn 增加 `first_name`、`last_name`、`avatar_original`，按偏好 locale 和上游嚴格的 100／800 像素選擇圖片。LinkedIn OpenID 增加 given／family name、`email_verified` 和原始圖片；Slack 兩種驅動暴露 `organization_id`。`linkedin-openid`、`slack-openid` 按固定套件使用 access token 存取固定 UserInfo 介面，不驗證 ID Token、不執行 Discovery，也不是完整 OIDC RP；這些能力使用 [OIDC 套件](oidc.md)。

@@ -185,13 +181,11 @@
 接入多個供應商時，在跳轉時儲存所選 namespace，回呼前消費並核對；為各供應商註冊固定路由，讓應用程式控制回呼 namespace。保留正常 state 校驗，不接受回呼傳入本機使用者 ID。本機登出使用受 CSRF 保護的 POST 路由，執行 `Auth::sessionGuard(request, Some("web"))?.logoutAndInvalidate().await?`，結束目前應用 Session；供應商 token 撤銷與聯邦登出遵循各自協定。


-
 ## 測試 {#testing}

 透過 `GithubProvider::withEndpoints` 配合應用內 `manager.extend("github", Arc::new(provider))` 指向受控 HTTP 供應商，替換授權／token／profile／email URL 並保留驅動行為。`ProviderDefinition` 支援編譯型協定客製，`facades::Socialite::driver` / `contracts::Factory`、`Provider`、`User` 暴露公開契約。

 Socialite 使用共用客戶端顯式 `redactPayload()`：觀察者與錄製保留狀態及路徑，省略 query、authorization、請求內容、回應內容和回應標頭；實際網路與傳回回應完整。供應商狀態／無效回應錯誤不列印回應憑據。
-

 `ProviderEndpoints::new(authorize, token, user).email(email)` 可傳給新增驅動的 `withEndpoints` 建構器，顯式替換端點。

````
