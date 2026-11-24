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

## 使用 File 操作本機檔案 {#local-file-facade}

`openrust::support::facades::File` 直接操作本機路徑。它解析應用級 `files` 單例（`openrust::filesystem::Filesystem`），與 `Storage` 的具名磁碟互相獨立。相對路徑從程序目前目錄解析；執行目錄不固定時應傳入應用的絕對路徑。此服務使用同步檔案操作，大型檔案任務應放入阻塞任務，避免占用非同步請求執行緒。

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_report(directory: &Path) -> FileResult<()> {
    File::ensure_directory_exists(directory)?;
    let path = directory.join("report.txt");
    File::put_with_lock(&path, b"ready\n", true)?;
    File::append_with_lock(&path, b"complete\n", true)?;
    let bytes = File::shared_get(&path)?;
    File::replace(directory.join("copy.txt"), bytes)?;
    for file in File::all_files(directory)? {
        println!("{}", file.relative_pathname().display());
    }
    Ok(())
}
```

`get` 回傳原始位元組；`get_text` 要求有效 UTF-8。`json` 遇到無效 JSON 回傳 null，`json_strict` 則回傳解碼錯誤；兩者都傳遞檔案存取錯誤。`lines` 回傳惰性位元組集合，每次遍歷重新開啟檔案；結尾換行會產生最後一個空項。`hash` 預設使用 MD5，`hash_with_algorithm` 支援 MD5、SHA-1、SHA-256/384/512 與 xxh128。`has_same_hash` 使用 xxh128，任一檔案無法讀取時回傳 false。

`files` 與 `directories` 按相對路徑排序，只列直接子項。`all_files` 與 `all_directories` 遞迴列舉，但不進入目錄符號連結。預設排除隱藏項，即使啟用隱藏檔案，也仍排除版本控制目錄。用 `files_with_options(path, hidden, DirectoryDepth::parse([">= 1", "< 3"])?)` 或 `directories_with_depth` 設定深度條件，直接子項深度為零。`glob` 支援 Shell 萬用字元，除非明確比對，否則排除點檔案。

`replace` 在同目錄建立暫存檔案後原子重新命名。Unix 下跟隨既有目標符號連結，預設權限為經 umask 過濾的 `0777`；用 `replace_with_mode` 指定權限。`chmod(path, None)` 或 `chmod(path, Some(0))` 回傳包含八進位權限的 `FileMode::Current`，非零模式則修改權限。`delete_many` 嘗試所有路徑，任一失敗便回傳 false。`clean_directory` 保留根目錄，遞迴刪除只移除子項符號連結，不刪除連結目標。目錄複製拒絕目標位於來源目錄內及符號連結循環。

帶型別的 `r#macro`、`mixin`、`macro_call`、`has_macro` 與 `flush_macros` 共用目前應用的本機服務，不與其他應用共用。`File` 需要有效的應用作用域，包括 HTTP、Artisan 與 Worker。HTTP 處理器透過 `?` 傳遞的讀取錯誤接入正常例外報告；本機檔案缺失不會自動成為 HTTP 404。

MIME 偵測重用 OpenRust 內容識別器，最多讀取 8 KiB，不等同於完整的 libmagic 資料庫。此批 File API 不執行動態 PHP／Rust 原始碼，也未涵蓋全部 PHP glob／JSON 旗標與摘要演算法。Windows 目錄連結目前依賴符號連結能力；原生 junction 等價行為及 Windows ACL 尚未包含在已驗證的 Unix 行為範圍內。

同步宏的參數及回傳值也可使用 `Rc<Cell<_>>` 等非 `Send` 值，持久註冊的回呼仍須為 `Send + Sync`，回傳值歸本次呼叫所有。編譯後的 `get_require`、`require_once` 回呼也遵循此規則；`require_once` 只保留路徑包含狀態。

### 編譯後的檔案模組 {#compiled-file-modules}

在應用裝配時透過 `File::register_module(path, callback)` 註冊實際存在的原始碼路徑。回呼是編譯進應用的 Rust 程式碼，不在執行時解譯或編譯檔案；具型別參數代替動態擷取區域變數，無參數時傳 `()`。直接重用應用函式，不新增執行時指令碼引擎。修改 Rust 程式碼後需要重新建置應用。

```rust
use openrust::{filesystem::{FileResult, RequiredFile}, support::facades::File};
use std::path::Path;

fn register_and_load(path: &Path) -> FileResult<()> {
    File::register_module(path, |files, name: String| {
        // 中文参数说明：回调是编译进应用的 Rust 函数。
        Ok(format!("Hello {name}: {}", files.exists("Cargo.toml")))
    })?;
    let value: String = File::get_require(path, "OpenRust".to_owned())?;
    let once: RequiredFile<String> = File::require_once(path, "ignored".to_owned())?;
    assert_eq!(once, RequiredFile::AlreadyIncluded);
    assert!(value.starts_with("Hello OpenRust"));
    Ok(())
}
```

`get_require` 每次執行回呼並回傳具型別結果。`require_once` 首次回傳 `RequiredFile::Value(value)`，之後回傳 `AlreadyIncluded`，與模組回傳 `true` 區分。任何 `get_require` 也會記入包含集合。執行前標記，因此遞迴以及失敗或 panic 後的再次呼叫都會跳過；替換註冊不清空包含狀態，但明確呼叫 `get_require` 仍執行替換後的回呼。

路徑正規化後依實際路徑判斷，符號連結別名共用包含狀態。缺少、已刪除或非普通檔案路徑回傳檔案錯誤，即使之前已包含；已存在但未註冊的檔案回傳 `UnregisteredModule`，參數或結果型別不符明確報錯，不偽造模組值。註冊及包含集合屬於應用的 `files` 單例，在同應用 HTTP／Artisan／Worker 呼叫間共用，與其他應用隔離，不採用解譯器的行程全域包含表。並行 `require_once` 只認領一次執行；其他呼叫可能在首個回呼尚未完成時看到 `AlreadyIncluded`。執行回呼和銷毀被替換的註冊時不持註冊表鎖。

### 條件檔案操作 {#conditional-file-calls}

透過 `File::when(value, callback)` 和 `unless` 執行條件呼叫。`ConditionValue` 保留原始參數型別，支援布林、數字、字串（`""` 和 `"0"` 為假）、可選值和集合；回呼接收實際本機檔案服務與原始值。業務物件可自行實作 `ConditionValue`。JSON 物件即使為空也為真，JSON 陣列依集合是否為空判斷。

```rust
use openrust::{filesystem::FileResult, support::facades::File};
use std::path::Path;

fn write_if_missing(path: &Path) -> FileResult<()> {
    File::unless_test(
        |files| files.exists(path),
        |files, _| files.put(path, b"created").map(Some),
    )?;
    let bytes = File::when_condition(true)?
        .call(|files| files.get(path))?
        .into_value();
    assert!(bytes.is_some());
    Ok(())
}
```

選中的回呼回傳 `FileResult<Option<T>>`。`Some(value)` 回傳 `ConditionalResult::Value(value)`，包括 `false`、`0` 和空字串；`None` 回傳 `Target`，保留原服務實例，沒有其他型別線索時使用 `None::<()>`。未執行分支也回傳 `Target`。以 `into_value`／`into_target` 取出兩種結果，錯誤直接傳播。`when_else`／`unless_else` 增加後備回呼，只執行一個分支。

條件來自檔案服務查詢時使用 `when_test`／`unless_test`，解析器只執行一次，並把原結果交給選中的回呼。`when_condition(value)`／`unless_condition(value)` 回傳具型別高階代理，`.call(|files| ...)` 僅在條件符合時呼叫真實公開操作。先擷取查詢時使用 `when_capture()?.capture(|files| ...)` 或 `unless_capture`；`try_capture` 接受會回傳錯誤的查詢。未擷取條件時回傳 `MissingCondition`。代理方法回傳真實操作值，因此操作回傳 `None` 時仍為 `Value(None)`，不套用回呼式的空值後備。

門面代理持有原應用服務，移動到另一應用作用域不會重新解析為另一份服務。直接呼叫 `Filesystem` 提供相同方法，後備目標為原借用。Rust 以明確後綴與 `ConditionalResult` 表達不同參數數量、動態方法代理和目標／純量聯合回傳，條件、副作用選擇及錯誤傳播仍可直接驗證。
