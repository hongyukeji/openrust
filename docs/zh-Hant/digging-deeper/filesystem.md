# 檔案儲存

## 設定

用 `FILESYSTEM_DISK` 設定預設磁碟，在 `config/filesystems.rs` 設定具名磁碟。`local` 與 `public` 都使用本機驅動程式，各有獨立根目錄。磁碟管理器在目前應用程式內解析並快取具名磁碟。

### 本機驅動程式

預設 `local` 磁碟的目錄是 `storage/app/private`。操作路徑相對於磁碟根目錄。絕對路徑、穿越目錄及保留的暫存檔名會被拒絕。Unix 驅動程式透過目錄控制代碼開啟檔案，遍歷時拒絕符號連結，也拒絕讀取具有多個硬連結的檔案。

### 公共磁碟

`public` 磁碟設定在 `config/filesystems.rs`。預設本機根目錄為 `storage/app/public`，URL 為應用 URL 加上 `/storage`。使用 `PUBLIC_STORAGE_ROOT` 修改目錄，使用 `PUBLIC_STORAGE_URL` 修改公開 URL。相對檔案路徑從應用根目錄解析。

Artisan 可以建立或移除 `filesystems.links` 設定的連結。預設連結為 `public/storage`，指向公共磁碟目錄。使用 `--force` 重建既有符號連結。既有一般檔案與目錄會保留。

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### 提供本機檔案（OpenRust 擴充）

預設 `routes/web.rs` 按以下順序掛載：

```rust
Route::staticPublic()?;
Route::staticDisk("public")?;
```

`staticPublic` 讀取網站目錄 `app.public_path`，預設為 `public`，可用 `PUBLIC_PATH` 覆蓋。`staticDisk` 讀取本機磁碟根目錄，從磁碟的 `url` 提取完整掛載路徑，沒有單獨的 `url_prefix` 設定。例如，`PUBLIC_STORAGE_URL=/storage/tmp/` 將 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。絕對 URL 必須與 `app.url` 同源；協定、主機或有效連接埠不同會在 HTTP 引導時被拒絕。本機檔案服務只掛載本機磁碟。

OpenRust HTTP 服務也可以直接提供公共磁碟檔案。在 Unix 上，磁碟掛載只傳回公開檔案，並要求各級目錄允許遍歷。符號連結、寫入暫存檔及具有多個硬連結的檔案會被拒絕。

應用路由優先於靜態檔案。靜態掛載按宣告順序尋找，檔案不存在時繼續尋找後續掛載。預設順序下，同為 `/storage/photo.jpg` 時，`public/storage/photo.jpg` 優先於 `storage/app/public/photo.jpg`。服務支援 GET、HEAD、MIME 類型、`Last-Modified` 和 `If-Modified-Since`。不列出目錄，拒絕路徑穿越及逃逸掛載根目錄的符號連結。缺失檔案使用應用的正常 404 策略。

參閱[路由](../the-basics/routing.md#掛載靜態目錄)和[公共檔案設定](../getting-started/configuration.md#公開檔案)。

## 取得磁碟實例

呼叫 `Storage::disk(Some("public"))` 從目前應用程式選擇具名磁碟。`Storage::disk(None)`、`Storage::put` 和 `Storage::get` 使用預設磁碟。下例在請求或命令作用域內執行。

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_note() -> FilesystemResult<Option<Vec<u8>>> {
    if !Storage::put("notes/example.txt", b"Hello").await? {
        return Ok(None);
    }
    let disk = Storage::disk(Some("local"))?;
    disk.get("notes/example.txt").await
}
```

使用 `Storage::build(LocalDiskConfig)` 按需建立本機磁碟。具名磁碟快取屬於單一應用程式。

## 讀取檔案

`get` 傳回檔案內容。讀取大型檔案時可透過 `readStream` 取得 Tokio 檔案控制代碼。預設 `throw = false` 時讀取失敗傳回 `None`，寫入失敗傳回 `false`。設定 `throw = true` 可傳回錯誤，設定 `report = true` 可記錄預設策略處理的錯誤。無效路徑一律傳回錯誤。

## 儲存檔案

`writeStream` 從非同步讀取器複製資料。本機驅動程式先寫入同目錄暫存檔，串流完成後重新命名並發布。來源失敗或發布前取消會保留原檔。重新命名操作已經發出後，即使呼叫端取消，完整寫入仍可能發布。

### 檔案可見性

Unix 上公開和私有檔案權限分別是 `0644` 和 `0600`。新子目錄使用磁碟的目錄可見性設定。可用 `WriteOptions` 覆寫單次寫入的可見性，透過 `getVisibility` 和 `setVisibility` 管理既有檔案。Windows 寫入使用檔案系統 ACL，明確的可見性變更目前支援 Unix。
