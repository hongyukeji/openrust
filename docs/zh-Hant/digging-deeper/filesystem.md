# 檔案儲存

## 設定

用 `FILESYSTEM_DISK` 設定預設磁碟，在 `config/filesystems.rs` 設定具名磁碟。`local` 與 `public` 都使用本機驅動程式，各有獨立根目錄。磁碟管理器在目前應用程式內解析並快取具名磁碟。

### 本機驅動程式

預設 `local` 磁碟的目錄是 `storage/app/private`。使用 `FILESYSTEM_LOCAL_ROOT` 覆寫此目錄；相對路徑從應用根目錄解析，絕對路徑直接使用。未設定時，預設目錄跟隨 `APP_STORAGE_PATH` 設定的儲存根目錄。操作路徑相對於磁碟根目錄。絕對路徑、穿越目錄及保留的暫存檔名會被拒絕。Unix 驅動程式透過目錄控制代碼開啟檔案，遍歷時拒絕符號連結，也拒絕讀取具有多個硬連結的檔案。 讀寫路徑中間重複的 `/` 會合併，例如 `avatars///photo.png` 與 `avatars/photo.png` 存取同一個檔案或物件鍵。`path`、`url` 和臨時 URL 建構器保留傳入的路徑文字，只按各自職責加入前綴或簽名。

### 公共磁碟

`public` 磁碟設定在 `config/filesystems.rs`。預設本機根目錄為 `storage/app/public`，URL 為應用 URL 加上 `/storage`。使用 `FILESYSTEM_PUBLIC_ROOT` 修改目錄，使用 `FILESYSTEM_PUBLIC_URL` 修改公開 URL。相對檔案路徑從應用根目錄解析。

Artisan 可以建立或移除 `filesystems.links` 設定的連結。預設連結為 `public/storage`，指向公共磁碟目錄。使用 `--force` 重建既有符號連結。既有一般檔案與目錄會保留。

```sh
./artisan storage:link --relative
./artisan storage:unlink
```

#### 提供本機檔案（OpenRust 擴充）

預設 `routes/web.rs` 按以下順序掛載：

```rust
Route::static_disk("web")?;
Route::static_disk("public")?;
```

`web` 磁碟描述網站目錄，根目錄預設是 `public`，URL 預設是 `app.url`。在 `config/filesystems.rs` 中透過 `FILESYSTEM_WEB_ROOT` 和 `FILESYSTEM_WEB_URL` 覆寫預設值。`FILESYSTEM_WEB_ROOT` 優先指定網站磁碟根目錄；未設定時使用 `APP_PUBLIC_PATH`（預設 `public`）。相對路徑基於應用根目錄，絕對路徑直接使用。空字串或全空白值會導致設定載入失敗。應用程式使用 `web.root` 作為 `public_path()`；啟動前呼叫 `use_public_path()` 會覆寫目錄並同步 `web.root`。兩個掛載統一使用 `static_disk`，讀取本機磁碟根目錄，並從 `url` 取得完整掛載路徑。例如，`FILESYSTEM_PUBLIC_URL=/storage/tmp/` 將 `storage/app/public/photo.jpg` 公開在 `/storage/tmp/photo.jpg`。絕對 URL 必須與 `app.url` 同源，協定、主機或有效連接埠不同會導致 HTTP 引導失敗。

兩個磁碟掛載在 Unix 上都要求檔案公開且各級目錄可遍歷，並拒絕符號連結、寫入暫存檔及具有多個硬連結的檔案。如果 `public/storage` 是符號連結，`web` 磁碟會跳過它，由 `public` 磁碟直接從其設定的根目錄提供檔案。

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

`read_stream` 為本機磁碟和 S3 磁碟提供相同的非同步讀取介面。S3 每段寫入 8 MiB，完成分段上傳後發布物件。目前最多上傳 10,000 段，單一物件約 78 GiB。上傳中斷後會盡量取消未完成的上傳。連線失敗後遠端可能已完成寫入，此時 `throw = true` 會傳回 `PublishOutcomeUnknown`。重試前可讀取目標核對狀態。

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

`get` 傳回檔案內容。讀取大型檔案時可透過 `read_stream` 取得非同步讀取器。預設 `throw = false` 時讀取失敗傳回 `None`，寫入失敗傳回 `false`。設定 `throw = true` 可傳回錯誤，設定 `report = true` 可記錄預設策略處理的錯誤。無效路徑一律傳回錯誤。

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

### 檔案中繼資料 {#file-metadata}

用 `size` 取得本機磁碟檔案的位元組數，用 `last_modified` 取得 `SystemTime`。兩者在中繼資料無法取得時傳回錯誤。透過 `Storage` 呼叫時使用預設磁碟。

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn report_metadata(disk: &FilesystemAdapter) -> FilesystemResult<(u64, Option<String>)> {
    let path = "reports/today.txt";
    Ok((disk.size(path).await?, disk.mime_type(path).await?))
}
```

用 `mime_type` 檢查內容簽章，並回退到副檔名推斷或 `application/octet-stream`。用 `checksum` 串流計算 MD5，或用 `checksum_with::<D>` 選用 Rust 摘要演算法。這兩項操作在停用 `throw` 時將磁碟錯誤回退為 `None`。無效路徑一律傳回錯誤。

## 儲存檔案

`write_stream` 從非同步讀取器複製資料。本機驅動程式先寫入同目錄暫存檔，串流完成後重新命名並發布。來源失敗或發布前取消會保留原檔。重新命名操作已經發出後，即使呼叫端取消，完整寫入仍可能發布。

### 檔案上傳

透過 `Request::file` 取得 multipart 上傳。可呼叫上傳物件的 `store` 或 `store_as`，也可使用磁碟的 `put_file` 或 `put_file_as`。`store_publicly` 和 `store_publicly_as` 會要求公開可見性。自動檔名使用 40 個隨機字元，辨識出類型時附加由內容推斷的副檔名。用戶端檔名與媒體類型屬於中繼資料，儲存前應執行應用程式的上傳驗證。

```rust
use openrust::{
    http::{HttpError, Request, Response, StatusCode},
    support::facades::Storage,
};

pub async fn store_attachment(request: &mut Request) -> Result<Response, HttpError> {
    let attachment = request.file("attachment").await?.ok_or(HttpError::InvalidRequest)?;
    let disk = Storage::disk(Some("public"))?;
    let path = attachment.store_publicly("attachments", Some("public")).await?
        .ok_or_else(|| HttpError::Internal {
            source: std::io::Error::other("Could not store attachment").into(),
        })?;
    Response::json(StatusCode::CREATED, disk.url(&path)?)
}
```

### 檔案可見性

Unix 上公開和私有檔案權限分別是 `0644` 和 `0600`。新子目錄使用磁碟的目錄可見性設定。可用 `WriteOptions` 覆寫單次寫入的可見性，透過 `get_visibility` 和 `set_visibility` 管理既有檔案。Windows 寫入使用檔案系統 ACL，明確的可見性變更目前支援 Unix。

S3 可見性使用物件 ACL。未指定可見性時，寫入沿用儲存貯體的存取控制。明確設定可見性需要儲存貯體支援 ACL，確認設定結果還需要讀取 ACL 的權限。實際匿名存取取決於儲存貯體政策及服務是否執行 ACL。

#### 自訂 Unix 權限 {#local-permissions}

具名本機磁碟可覆寫每種可見性對應的 Unix 權限。可設定 `permissions.file.public`、`permissions.file.private`、`permissions.dir.public` 或 `permissions.dir.private`。未設定的項目沿用預設值。在 Rust 設定中使用 `0o664` 等八進位數值。

```rust
use openrust::filesystem::{FilesystemResult, LocalPermissions};

fn permissions() -> FilesystemResult<LocalPermissions> {
    LocalPermissions::new(0o664, 0o640, 0o775, 0o750)
}
```

按需磁碟可用 `Storage::build_with_permissions` 同時傳入 `LocalDiskConfig` 和權限表。新建檔案和目錄使用設定的權限，`set_visibility` 會為檔案套用對應模式。公開磁碟掛載也要求 Unix 的公開讀取和目錄走訪權限，並遵守映射為 `private` 的模式。自訂 Unix 模式適用於 Unix。

## 暫存檔案 URL

使用 `disk.temporary_url(path, expiration).await?` 產生下載連結，或使用 `disk.temporary_upload_url(path, expiration).await?` 產生上傳連結。上傳時向傳回的 `url` 發出請求，並附上傳回的 `headers`。本機磁碟須在磁碟設定中啟用 `serve`，路由使用該磁碟 `url` 的路徑，省略時使用 `/storage`。本機上傳連結接收 PUT。S3 配接器依設定的儲存貯體端點簽名，用戶端必須能存取該端點。

### S3 臨時 URL 基底位址 {#s3-temporary-url-origin}

在 S3 磁碟設定 `temporary_url`，或用 `AWS_TEMPORARY_URL` 設定預設 S3 磁碟。它替換 GET 和 PUT 連結的協定、主機和連接埠，保留已簽章的路徑與查詢字串。代理轉送至 S3 端點時須使用簽章中的原始 Host、路徑與查詢字串。

### S3 請求選項 {#s3-request-parameters}

用 `temporary_url_with` 將下載回應標頭或版本 ID 納入簽章。用 `temporary_upload_url_with` 簽入上傳內容標頭和物件中繼資料，並在 PUT 請求中附上傳回的請求標頭。儲存貯體和物件路徑由磁碟提供。

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryUrlOptions},
    support::facades::Storage,
};

async fn report_url() -> FilesystemResult<String> {
    Storage::disk(Some("s3"))?
        .temporary_url_with(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            TemporaryUrlOptions {
                response_content_type: Some("text/csv".into()),
                response_content_disposition: Some("attachment; filename=report.csv".into()),
                ..Default::default()
            },
        )
        .await
}
```

如需更多 S3 選項，在指定磁碟上呼叫 `temporary_download_request` 或 `temporary_upload_request`，透過閉包設定 AWS SDK 請求建構器。兩種方法都傳回 URL 與簽章請求標頭，用戶端送出請求時須附上這些標頭。磁碟在套用選項後設定儲存貯體與帶前綴的物件鍵。

```rust
use std::time::{Duration, SystemTime};

use openrust::{
    filesystem::{FilesystemResult, TemporaryDownloadUrl},
    support::facades::Storage,
};

async fn report_preview() -> FilesystemResult<TemporaryDownloadUrl> {
    Storage::disk(Some("s3"))?
        .temporary_download_request(
            "reports/current.csv",
            SystemTime::now() + Duration::from_secs(300),
            |request| request.range("bytes=0-1023"),
        )
        .await
}
```

### 自訂本機磁碟簽章回呼 {#customizing-temporary-urls}

在服務提供者啟動時，對本機磁碟註冊 `build_temporary_urls_using` 或 `build_temporary_upload_urls_using`。回呼接收已驗證的路徑、到期時間和具型別的選項。上傳回呼傳回包含 URL 和必要請求標頭的 `TemporaryUploadUrl`。

## 目錄 {#directories}

對本機磁碟使用 `files` 和 `directories` 列出直接子項，使用 `all_files` 和 `all_directories` 納入後代。傳入 `None` 表示磁碟根目錄。結果是以 `/` 分隔的磁碟相對路徑。檔案列表依路徑排序，目錄缺失時傳回空列表。

```rust
use openrust::filesystem::{FilesystemAdapter, FilesystemResult};

async fn daily_reports(disk: &FilesystemAdapter) -> FilesystemResult<Vec<String>> {
    disk.make_directory("reports/daily").await?;
    disk.all_files(Some("reports")).await
}
```

用 `make_directory` 依磁碟設定的目錄可見性建立缺失的父目錄，用 `delete_directory` 刪除目錄及內容。列表略過符號連結和暫存寫入項目。建立與刪除時須傳入具名相對路徑。也可透過 `Storage` 在預設磁碟呼叫這些方法。

## 契約與常用操作

容器可解析 `dyn contracts::filesystem::Factory`、預設 `Filesystem` 與設定的 `Cloud`，重用既有 Manager 和 Adapter。契約串流介面為 `read_stream` / `write_stream`；Adapter 與 Storage Facade 使用 `read_stream` / `write_stream`。`copy`、`move`（Rust 為 `r#move`）、`append`、`prepend`、`json` 重用目前磁碟；遺失檔案與無效 JSON 回傳空結果，越界路徑仍拒絕。`Cloud` 解析依賴 `filesystems.cloud`，不代表本機磁碟具備 S3 簽章上傳能力。

Rust 中 `append(path, text, separator)` 與 `prepend(path, text, separator)` 需要明確傳入分隔符號。換行可傳 `"\n"`，需要 Windows 行尾時傳 `"\r\n"`。`move` 是 Rust 關鍵字，因此使用原始識別字 `r#move`。
