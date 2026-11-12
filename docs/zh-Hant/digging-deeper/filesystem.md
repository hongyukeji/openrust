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

`staticPublic` 讀取網站目錄 `app.public_path`，預設為 `public`，可用 `APP_PUBLIC_PATH` 覆蓋。`staticDisk` 讀取本機磁碟根目錄，從磁碟的 `url` 提取完整掛載路徑，沒有單獨的 `url_prefix` 設定。例如，`PUBLIC_STORAGE_URL=/storage/tmp/` 將 `storage/app/public/photo.jpg` 暴露在 `/storage/tmp/photo.jpg`。絕對 URL 必須與 `app.url` 同源；協定、主機或有效連接埠不同會在 HTTP 引導時被拒絕。本機檔案服務只掛載本機磁碟。

OpenRust HTTP 服務也可以直接提供公共磁碟檔案。在 Unix 上，磁碟掛載只傳回公開檔案，並要求各級目錄允許遍歷。符號連結、寫入暫存檔及具有多個硬連結的檔案會被拒絕。

應用路由優先於靜態檔案。靜態掛載按宣告順序尋找，檔案不存在時繼續尋找後續掛載。預設順序下，同為 `/storage/photo.jpg` 時，`public/storage/photo.jpg` 優先於 `storage/app/public/photo.jpg`。服務支援 GET、HEAD、MIME 類型、`Last-Modified` 和 `If-Modified-Since`。不列出目錄，拒絕路徑穿越及逃逸掛載根目錄的符號連結。缺失檔案使用應用的正常 404 策略。

參閱[路由](../the-basics/routing.md#掛載靜態目錄)和[公共檔案設定](../getting-started/configuration.md#公開檔案)。

### 相容 Amazon S3 的檔案系統

為 `s3` 磁碟設定 `AWS_BUCKET` 和 `AWS_DEFAULT_REGION`。如需明確的憑證，同時設定 `AWS_ACCESS_KEY_ID` 和 `AWS_SECRET_ACCESS_KEY`。暫時憑證另需 `AWS_SESSION_TOKEN`。省略兩項金鑰時，由 AWS SDK 的憑證鏈解析。相容服務可設定 `AWS_ENDPOINT`，服務使用路徑式儲存貯體 URL 時啟用 `AWS_USE_PATH_STYLE_ENDPOINT`。

在應用程式請求或命令中依名稱選取磁碟。設定 `FILESYSTEM_DISK=s3` 可作為預設磁碟。磁碟設定中的選用 `root` 用作物件鍵前綴，操作路徑相對於磁碟。

```rust
use openrust::{filesystem::FilesystemResult, support::facades::Storage};

pub async fn store_report() -> FilesystemResult<Option<Vec<u8>>> {
    let disk = Storage::disk(Some("s3"))?;
    if !disk.put("reports/summary.txt", b"Ready").await? {
        return Ok(None);
    }
    disk.get("reports/summary.txt").await
}
```

`readStream` 為本機磁碟和 S3 磁碟提供相同的非同步讀取介面。S3 每段寫入 8 MiB，完成分段上傳後發布物件。目前最多上傳 10,000 段，單一物件約 78 GiB。上傳中斷後會盡量取消未完成的上傳。連線失敗後遠端可能已完成寫入，此時 `throw = true` 會傳回 `PublishOutcomeUnknown`。重試前可讀取目標核對狀態。

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

`get` 傳回檔案內容。讀取大型檔案時可透過 `readStream` 取得非同步讀取器。預設 `throw = false` 時讀取失敗傳回 `None`，寫入失敗傳回 `false`。設定 `throw = true` 可傳回錯誤，設定 `report = true` 可記錄預設策略處理的錯誤。無效路徑一律傳回錯誤。

### 下載檔案

使用 `download` 產生附件回應，或用 `response` 在瀏覽器內顯示。兩者均以串流方式讀取檔案，可傳入下載名稱及回應標頭。介面卡會補充內容類型、已知長度及內容處置標頭。下載私有檔案前，由應用程式授權。處理器回傳 `HttpError` 時，檔案不存在會轉為 HTTP 404。

```rust
use openrust::{
    http::{HeaderMap, HttpError, Response},
    support::facades::Storage,
};

pub async fn download_report() -> Result<Response, HttpError> {
    Ok(Storage::disk(Some("public"))?
        .download("reports/summary.txt", Some("summary.txt"), HeaderMap::new())
        .await?)
}
```

### 檔案 URL

`Storage::url` 使用預設磁碟。具名磁碟優先使用設定的 `url`，本機磁碟未設定時使用 `/storage`。本機 URL 請使用適合 URL 的檔名。S3 可透過 `AWS_URL` 設定公開基底網址。未設定時由 AWS SDK 解析儲存貯體端點，介面卡附加編碼後的物件鍵與設定前綴。檔案是否可存取由磁碟掛載或儲存貯體控制。

## 儲存檔案

`writeStream` 從非同步讀取器複製資料。本機驅動程式先寫入同目錄暫存檔，串流完成後重新命名並發布。來源失敗或發布前取消會保留原檔。重新命名操作已經發出後，即使呼叫端取消，完整寫入仍可能發布。

### 檔案上傳

透過 `Request::file` 取得 multipart 上傳。可呼叫上傳物件的 `store` 或 `storeAs`，也可使用磁碟的 `putFile` 或 `putFileAs`。`storePublicly` 和 `storePubliclyAs` 會要求公開可見性。自動檔名使用 40 個隨機字元，辨識出類型時附加由內容推斷的副檔名。用戶端檔名與媒體類型屬於中繼資料，儲存前應執行應用程式的上傳驗證。

```rust
use openrust::{
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Storage,
};

pub async fn store_attachment(request: &mut Request) -> Result<Response, HttpError> {
    let attachment = request.file("attachment").await?.ok_or(HttpError::InvalidRequest)?;
    let disk = Storage::disk(Some("public"))?;
    let path = attachment.storePublicly("attachments", Some("public")).await?
        .ok_or_else(|| HttpError::Internal {
            source: std::io::Error::other("Could not store attachment").into(),
        })?;
    Response::json(StatusCode::CREATED, disk.url(&path)?)
}
```

### 檔案可見性

Unix 上公開和私有檔案權限分別是 `0644` 和 `0600`。新子目錄使用磁碟的目錄可見性設定。可用 `WriteOptions` 覆寫單次寫入的可見性，透過 `getVisibility` 和 `setVisibility` 管理既有檔案。Windows 寫入使用檔案系統 ACL，明確的可見性變更目前支援 Unix。

S3 可見性使用物件 ACL。未指定可見性時，寫入沿用儲存貯體的存取控制。明確設定可見性需要儲存貯體支援 ACL，確認設定結果還需要讀取 ACL 的權限。實際匿名存取取決於儲存貯體政策及服務是否執行 ACL。

## 暫存檔案 URL

使用 `disk.temporaryUrl(path, expiration).await?` 產生下載連結，或使用 `disk.temporaryUploadUrl(path, expiration).await?` 產生上傳連結。上傳時向傳回的 `url` 發出請求，並附上傳回的 `headers`。本機磁碟須在磁碟設定中啟用 `serve`，路由使用該磁碟 `url` 的路徑，省略時使用 `/storage`。本機上傳連結接收 PUT。S3 配接器依設定的儲存貯體端點簽名，用戶端必須能存取該端點。
