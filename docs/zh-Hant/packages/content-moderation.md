# 內容審核（擴充）

啟用 `content-moderation` 功能，或在依賴 `openrust-content-moderation` 的應用程式中註冊 `ModerationServiceProvider`。執行 `./artisan vendor:publish --tag=moderation-config` 發布設定範本。在應用程式的 Rust 設定中將 `moderation.drivers.comments.driver` 設為 `review-text`，並透過 `MODERATION_DRIVER=comments` 選擇預設設定。

目前套件提供驅動契約和應用程式裝配。內建雲端服務介接器及持久化影片編排正在實作。

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
