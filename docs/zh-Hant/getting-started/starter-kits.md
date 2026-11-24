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

已安裝套件在登入與有效 remember Cookie 恢復時立即儲存既有密碼指紋，Guard 在後續認證讀取中拒絕缺少或過時指紋。修改密碼保留目前瀏覽器工作階段並拒絕其他密碼工作階段；重置也會輪換 remember token。獨立認證的預設行為不變。本節瀏覽器工作階段行為不包含仍在開發的個人權杖與無密碼帳戶撤銷鏈路。
