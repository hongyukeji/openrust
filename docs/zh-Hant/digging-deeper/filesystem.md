# 檔案儲存

## 公共磁碟

`public` 磁碟設定在 `config/filesystems.rs`。預設本機根目錄為 `storage/app/public`，URL 為應用 URL 加上 `/storage`。使用 `PUBLIC_STORAGE_ROOT` 修改目錄，使用 `PUBLIC_STORAGE_URL` 修改公開 URL。相對檔案路徑從應用根目錄解析。

### 提供本機檔案（OpenRust 擴充）

預設 `routes/web.rs` 按以下順序掛載：

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` 讀取網站目錄 `app.public_path`，預設為 `public`，可用 `APP_PUBLIC_PATH` 覆蓋。`staticDisk` 讀取本機磁碟根目錄，從磁碟的 `url` 提取完整掛載路徑，沒有單獨的 `url_prefix` 設定。例如，`PUBLIC_STORAGE_URL=/storage/tmp/` 將 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。絕對 URL 必須與 `app.url` 同源；協定、主機或有效連接埠不同會在 HTTP 引導時被拒絕。S3 等遠端磁碟不能透過這個本機檔案服務掛載。

Laravel 通常透過 `storage:link` 建立符號連結來公開公共磁碟。OpenRust 的掛載直接提供本機檔案，無需該連結。`storage:link` 命令及通用檔案系統 API 不屬於本次已交付範圍。

應用路由優先於靜態檔案。靜態掛載按宣告順序尋找，檔案不存在時繼續尋找後續掛載。預設順序下，同為 `/storage/photo.jpg` 時，`public/storage/photo.jpg` 優先於 `storage/app/public/photo.jpg`。服務支援 GET、HEAD、MIME 類型、`Last-Modified` 和 `If-Modified-Since`。不列出目錄，拒絕路徑穿越及逃逸掛載根目錄的符號連結。缺失檔案使用應用的正常 404 策略。

參閱[路由](../the-basics/routing.md#掛載靜態目錄)和[公共檔案設定](../getting-started/configuration.md#公開檔案)。
