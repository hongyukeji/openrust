# 內容審核（擴充）

啟用 `content-moderation` 功能，或在依賴 `openrust-content-moderation` 的應用程式中註冊 `ModerationServiceProvider`。執行 `./artisan vendor:publish --tag=moderation-config` 發布設定範本。在應用程式的 Rust 設定中將 `moderation.drivers.comments.driver` 設為 `review-text`，並透過 `MODERATION_DRIVER=comments` 選擇預設設定。

目前套件提供自訂驅動契約、阿里雲文字和已儲存圖片審核。其他供應商操作及持久化影片編排正在實作。

## 阿里雲文字 {#aliyun-text}

在 `moderation.drivers.aliyun_comments` 設定 `driver: "aliyun"`、`region: "cn-shanghai"`、`text_service: "comment_detection"`、`access_key_id_env` 和 `access_key_secret_env`。最後兩個欄位填寫環境變數名稱。使用 STS 時還須設定 `security_token_env`。用 `Moderation::driver("aliyun_comments")` 選擇設定後呼叫 `text`，也可透過 `MODERATION_DRIVER=aliyun_comments` 設為預設。

介接器呼叫 `Green/2022-03-02` 的 `TextModeration`，最多接受 600 個 Unicode 字元。高風險回傳 `Block`，低風險和中風險回傳 `Review`。明確的空標籤且無風險時，文字涵蓋可得到 `Allow`。未識別的標籤保留在結果中。 [阿里雲文字審核增強版](https://help.aliyun.com/zh/document_detail/434034.html)

如果傳入語言提示，在 `text_languages` 列出所選服務支援的語言標籤。設定以外的提示會在送出請求前拒絕。未提供提示時，原文直接交由所選服務審核。

## 阿里雲圖片 {#aliyun-image}

審核已儲存圖片時，設定 `image_service`（例如 `baselineCheck`），以 `media_disk` 指定私有 S3 磁碟，並將 `media_origin` 設為該磁碟的 HTTPS 下載來源。向具名驅動傳入 `ImageInput::stored(StoredMedia::new(disk, key, version_id)?)`，其中 `version_id` 是不可變的 S3 物件版本。驅動向阿里雲送出繫結該版本、有效期五分鐘的簽章網址。來源或磁碟不相符、物件未版本化時，會在提交前拒絕。

此操作提供視覺涵蓋。擷取畫格取樣或語義不明確的 `nonLabel` 只計部分涵蓋。若回應含有置信分，透過 `Assessment::confidence` 讀取原值與 0–100 量程。業務還要求 OCR 時應檢查涵蓋欄位。 [阿里雲 ImageModeration](https://help.aliyun.com/zh/document_detail/2528742.html)

## 自訂驅動 {#custom-drivers}

以下範例會將文字送人工複核。在套件 Provider 之後註冊範例 Provider，HTTP、Artisan 和 Worker 都可解析相同的具名設定。

```rust
use openrust::{
    contracts::{container::Container as _, foundation::Application as _},
    foundation::Application,
    http::HttpFuture,
    support::{ProviderResult, ServiceProvider},
};
use openrust_content_moderation::{
    Assessment, Coverage, Decision, Modality, ModerationDriver, ModerationFuture,
    ModerationManager, Operation, TextInput,
};
use std::{collections::BTreeMap, sync::Arc};

struct ReviewText;

impl ModerationDriver for ReviewText {
    fn supports(&self, operation: Operation) -> bool {
        operation == Operation::Text
    }

    fn text<'a>(&'a self, _: &'a TextInput) -> ModerationFuture<'a, Assessment> {
        Box::pin(async {
            Ok(Assessment::new(
                Decision::Review,
                BTreeMap::from([(Modality::Text, Coverage::Complete)]),
                vec![],
            ))
        })
    }
}

pub struct ReviewTextProvider;

impl ServiceProvider for ReviewTextProvider {
    fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
        Box::pin(async move {
            app.container().make::<ModerationManager>()?
                .extend("review-text", |_| Ok(Arc::new(ReviewText)));
            Ok(())
        })
    }
}
```

```rust
use openrust_content_moderation::{Modality, Moderation, TextInput};

let input = TextInput::new("Sample text")?.with_language("en")?;
let result = Moderation::text(&input).await?;
let recommendation = result.decision(&[Modality::Text]);
```

使用 `Moderation::driver("comments")` 選擇具名設定。`ModerationManager::forget_drivers` 使後續解析使用替換後的工廠和設定，已取得的控制代碼繼續使用原快照。設定快取儲存設定值，各行程自行註冊工廠。

## 媒體與非同步結果 {#media}

`ImageInput::inline` 接受最多 10 MiB 的圖片位元組。驅動須核對實際格式，並可進一步縮小上限。私有物件使用 `StoredMedia`，影片使用 `VideoInput`。驅動應透過設定的儲存核對物件版本。

提交後依原具名驅動和設定修訂查詢 `TaskId`。可序列化的任務 ID 僅包含遠端識別碼。持久化時由應用程式保留設定、憑證修訂和物件版本。待處理、失敗或必檢涵蓋不完整時均無法核准發布。

透過 `Assessment::decision` 檢查所需模態。結果不完整時仍可保留人工複核或封鎖所需的風險證據。若業務要求語音審核，部分語音涵蓋的成功結果無法產生 `Allow`。
