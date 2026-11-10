# 路由

在 `routes/web.rs` 宣告 Web 路由。應用程式啟動時透過 `openrust::routing::Router` 註冊這些路由及 `/up` 健康檢查。需要讀取請求的處理器接收可變的 `Request`，並傳回非同步 HTTP 結果。框架呼叫 Web 路由模組的 `register()` 時繫結目前應用程式的 Router。匯入 `openrust::support::facades::Route` 後，可以用 `Route::get("/", || view("welcome"))` 宣告具名檢視。

API 路由的預設 URL 前綴為 `/api`。用 `Routing::default().api(routes::api::register).apiPrefix("v1")` 可掛載到 `/v1`，傳入空字串則省略前綴。`api` 中介軟體群組仍會套用。應用程式的例外轉譯策略由 `withExceptions` 另行設定。

`{id}` 表示必填參數。在末尾參數加上 `?` 即可省略，例如 `/profiles/{id?}/{tab?}` 或 `/report/{name}.{format?}`。使用 `request.route("id")` 讀取符合的值。省略參數時傳回 `None`，也可以用路由的 `defaults` 提供字串預設值。參數後面還有必填參數或固定文字時，仍須提供該參數。

用 `Route::r#where` 指定正規表示式，也可使用 `where_number`、`where_alpha`、`where_alpha_numeric`、`where_uuid`、`where_ulid` 或 `where_in`。在註冊路由前呼叫 `Router::pattern`，可替後續路由的同名參數設定限制。單一路由上的限制優先。`where_in` 將值接成正規表示式的替代分支，特殊字元保留原義。OpenRust 使用 Rust 的 `regex` 語法，PCRE 的環視與反向參照會在註冊時被拒絕。

比對時以 UTF-8 對路徑執行百分號解碼，並忽略查詢字串。解碼結果無效時傳回 HTTP 400。比對路由與檢查可用方法時都會套用限制。例如 `/profiles/{id?}` 是限制數字 ID 的 GET 路由時，`POST /profiles/7` 傳回 405 和 `Allow`。如果沒有其他路由符合該路徑，`POST /profiles/alice` 傳回 404。

`Router::get` 同時註冊 GET 和 HEAD。HEAD 保留回應狀態和標頭，傳送空的回應主體。未知路徑傳回 404。路徑存在但方法不受支援時傳回 405，並附帶 `Allow` 標頭。路徑存在且未明確註冊 OPTIONS 時，路由器傳回 200 並列出允許的方法。同一方法有多條路由符合時，使用最先註冊的符合項目。

## 掛載靜態目錄

應用程式骨架先掛載網站公開目錄，再掛載公開儲存磁碟。目錄及 URL 前綴請見[公開檔案設定](../getting-started/configuration.md#public-files)。

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

相對目錄從應用程式根目錄解析。例如，`public/css/app.css` 對應 `/css/app.css`。如需關閉公開磁碟掛載，移除 `routes/web.rs` 中的 `Route::staticDisk("public")?` 呼叫。網站目錄預設掛載到 `/`，公開磁碟預設掛載到 `/storage`。兩個掛載都能回應同一個 URL 時，框架依宣告順序傳回第一個符合的檔案。依骨架的順序，請求 `/storage/avatar.png` 時，`public/storage/avatar.png` 優先於 `storage/app/public/avatar.png`。公開磁碟無需建立 `public/storage` 符號連結。應用程式路由優先，只有未比對到任何應用程式路由的 GET 和 HEAD 請求才會查找檔案。檔案不存在或請求指向目錄時傳回 404，符號連結無法存取掛載目錄外的檔案。

靜態檔案有可用的修改時間時，回應會帶上 `Last-Modified`。GET 或 HEAD 請求可將該值放入 `If-Modified-Since` 重新驗證檔案。檔案未修改時傳回 `304 Not Modified`，回應本文為空。

## 健康檢查

`bootstrap/app.rs` 的 `health("/up")` 註冊健康端點。普通請求回傳包含「Application up」的 HTML 頁面，期望 JSON 的請求回傳 `{"status":"up"}`。兩者均回傳 200，HEAD 不傳送本文。HTML 標題使用經過跳脫的應用名稱。

```sh
curl -i http://localhost:8000/up
curl -i -H 'Accept: application/json' http://localhost:8000/up
```

這表示 HTTP 引導已成功。診斷事件監聽器及其失敗回應等待事件分發器實作。健康頁面使用本機 HTML，未使用 Laravel 的 Blade 視圖和外部樣式資源。
