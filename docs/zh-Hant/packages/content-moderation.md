# 內容審核（擴充）

啟用 `content-moderation` 功能，或在依賴 `openrust-content-moderation` 的應用程式中註冊 `ModerationServiceProvider`。執行 `./artisan vendor:publish --tag=moderation-config` 發布設定範本。在應用程式的 Rust 設定中將 `moderation.drivers.comments.driver` 設為 `review-text`，並透過 `MODERATION_DRIVER=comments` 選擇預設設定。

目前套件提供自訂驅動契約、阿里雲文字及已儲存圖片審核，騰訊雲文字及圖片審核，Sightengine 文字及圖片審核，以及 Hive 文字和視覺審核。其他供應商操作及持久化影片編排正在實作。

## 阿里雲文字 {#aliyun-text}

在 `moderation.drivers.aliyun_comments` 設定 `driver: "aliyun"`、`region: "cn-shanghai"`、`text_service: "comment_detection"`、`access_key_id_env` 和 `access_key_secret_env`。最後兩個欄位填寫環境變數名稱。使用 STS 時還須設定 `security_token_env`。用 `Moderation::driver("aliyun_comments")` 選擇設定後呼叫 `text`，也可透過 `MODERATION_DRIVER=aliyun_comments` 設為預設。

介接器呼叫 `Green/2022-03-02` 的 `TextModeration`，最多接受 600 個 Unicode 字元。高風險回傳 `Block`，低風險和中風險回傳 `Review`。明確的空標籤且無風險時，文字涵蓋可得到 `Allow`。未識別的標籤保留在結果中。 [阿里雲文字審核增強版](https://help.aliyun.com/zh/document_detail/434034.html)

如果傳入語言提示，在 `text_languages` 列出所選服務支援的語言標籤。設定以外的提示會在送出請求前拒絕。未提供提示時，原文直接交由所選服務審核。

## 阿里雲圖片 {#aliyun-image}

審核已儲存圖片時，設定 `image_service`（例如 `baselineCheck`），以 `media_disk` 指定私有 S3 磁碟，並將 `media_origin` 設為該磁碟的 HTTPS 下載來源。向具名驅動傳入 `ImageInput::stored(StoredMedia::new(disk, key, version_id)?)`，其中 `version_id` 是不可變的 S3 物件版本。驅動向阿里雲送出繫結該版本、有效期五分鐘的簽章網址。來源或磁碟不相符、物件未版本化時，會在提交前拒絕。

此操作提供視覺涵蓋。擷取畫格取樣或語義不明確的 `nonLabel` 只計部分涵蓋。若回應含有置信分，透過 `Assessment::confidence` 讀取原值與 0–100 量程。業務還要求 OCR 時應檢查涵蓋欄位。 [阿里雲 ImageModeration](https://help.aliyun.com/zh/document_detail/2528742.html)

## 騰訊雲文字 {#tencent-text}

在具名設定指定 `driver: "tencent"`、騰訊雲開通的 `region` 和 `biz_type`，並填寫環境變數名稱 `secret_id_env`、`secret_key_env`。臨時憑證另設 `security_token_env`。介接器使用 UTF-8 Base64 呼叫標準 `TextModeration`，最多接受 10,000 個 Unicode 字元。

中文提示對應 `zh`。騰訊雲確認帳號支援英文選項後設定 `english_text: true`，純英文文字的提示才會轉為 `en`。介接器核對回應策略與設定的 `biz_type`。明細可提高總建議的風險級別，未識別標籤無法得到 `Allow`。 [騰訊雲 TextModeration](https://cloud.tencent.com.cn/document/api/1124/51860)

## 騰訊雲圖片 {#tencent-image}

用 `image_biz_type` 指定已開通的標準圖片策略。具名設定可單獨啟用圖片，也可和文字共用。標準 `ImageModeration` 接收內嵌圖片的 Base64（原始內容至少 16 位元組，編碼後小於 10 MB），也可使用五分鐘有效、固定私人 S3 物件版本的簽名連結。已儲存圖片需要設定 `media_disk` 和已授權的 HTTPS `media_origin`。

介接器保留畫面和 OCR 的風險結果與原始分數。騰訊雲可能擷取部分影格或切分長圖，同步回應無法證明完整涵蓋，因此供應商傳回 `Pass` 時仍無法得到放行結論。辨識原文、命中詞及暫時連結均不出現在對外結果。 [騰訊雲 ImageModeration](https://cloud.tencent.com/document/product/1125/53273)

## Sightengine 文字 {#sightengine-text}

在具名設定指定 `driver: "sightengine"`、`api_user_env` 和 `api_secret_env`，在 `text_languages` 列出啟用的語言代碼。`text_mode: "rules"` 使用 `text_categories`，例如 `["profanity", "personal"]`。`text_mode: "ml"` 使用 `text_models` 和 0 至 1 的 `text_review_score`。`self-harm` 模型需要 `text_languages: ["en"]`。

介接器保留規則或模型標籤及 ML 命中的原始分數。分數達到設定門檻時進入複審，缺少已設定類別的結果時涵蓋為部分。最多接受 10,000 個 Unicode 字元，命中的原文和 API 憑證均不出現在結果或日誌。 [規則](https://sightengine.com/docs/text-moderation-rule-based) · [ML 模型](https://sightengine.com/docs/text-moderation-ml-models)

## Sightengine 圖片 {#sightengine-image}

在 `image_models` 選擇具名設定使用的模型，並將 `image_review_score` 設為 0 至 1 的視覺複審門檻。支援 `nudity-2.1`、`violence`、`gore-2.0`、`text-content-2.0`。啟用 OCR 時用 `image_text_categories` 指定查核類別，例如 `["email", "insult"]`。

內嵌圖片經 multipart 上傳。已儲存的圖片使用 `media_disk` 和 `media_origin` 產生固定私人 S3 物件版本的短時 HTTPS 連結。介接器保留傳回影格中每個視覺標籤的最高分及 OCR 類別，省略命中原文。無法確認完整涵蓋時標為部分，結果無法放行。 [Sightengine 圖片 API](https://sightengine.com/docs/reference) · [OCR](https://sightengine.com/docs/ocr-text-moderation-in-images-2.0)

## Hive 文字 {#hive-text}

在具名設定指定 `driver: "hive"`、Hive V2 Enterprise 文字專案的數字 ID `text_project`，以及引用該專案權杖的環境變數名稱 `token_env`。用 `text_classes` 設定必查類別，`text_review_severity` 取 1、2 或 3。如需傳入明確的語言提示，在 `text_languages` 列出該專案已開通的語言。未傳提示時由 Hive 自動偵測。

輸入上限為 1,024 個 UTF-16 碼元。介接器核對回應專案、模型類型及工作 ID。類別分數為 -1 或缺少必查類別時，文字只算部分涵蓋。多段回應採用每個類別的最高嚴重等級。樣式比對證據保留來源類別，省略命中原詞及個人資料。 [Hive V2 文字 API](https://docs.thehive.ai/docs/classification-text)

## Hive 圖片 {#hive-image}

將 `image_project` 設為 V2 Enterprise 視覺專案的數字 ID，`image_token_env` 設為保存該專案權杖的環境變數名稱。`image_model_type` 使用專案傳回的模型類型，`image_classes` 列出需要檢查的風險類別，`image_review_score` 是 0 至 1 的複審門檻。同一具名設定可啟用文字和視覺，各自使用獨立的專案 ID 與權杖。

內嵌圖片經 multipart 上傳。已儲存圖片依 `media_disk`、`media_origin` 產生固定私人 S3 物件版本的短時 HTTPS 連結。介接器保留取樣結果中每個已設定類別的最高分。供應商未證明整個影像序列已處理時，視覺維持部分涵蓋。視覺專案提供視覺證據，OCR 對應 Hive 的獨立模型。 [Hive 視覺審核](https://docs.thehive.ai/docs/visual-content-moderation)

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
