# 內容審核（擴充）

啟用 `content-moderation` 功能，或在依賴 `openrust-content-moderation` 的應用程式中註冊 `ModerationServiceProvider`。執行 `./artisan vendor:publish --tag=moderation-config` 發布設定範本。在應用程式的 Rust 設定中將 `moderation.drivers.comments.driver` 設為 `review-text`，並透過 `MODERATION_DRIVER=comments` 選擇預設設定。

本套件提供自訂驅動契約、阿里雲、騰訊雲、Sightengine 和 Hive 的具名設定，以及持久化影片審核流程。各供應商小節說明支援的輸入和審核涵蓋範圍。模型、語言與模態的實際涵蓋範圍取決於具名設定關聯的供應商專案及已開通服務。

## 供應商憑證 {#credentials}

正式部署可將 `moderation.credential_directory`（預設讀取 `MODERATION_CREDENTIAL_DIRECTORY`）設為受控唯讀掛載的絕對路徑。在具名設定中填寫 `credential_ref` 和 `credential_revision`。例如參照 `sightengine` 和修訂 `r1` 會讀取 `sightengine.r1.json`，其中的 JSON 物件填寫 `api_user` 和 `api_secret`。設定快取保存目錄、參照和修訂，HTTP、Artisan 和 Worker 都需要讀取同一掛載。

下文的 `*_env` 鍵供明確的環境變數注入使用。使用掛載檔案時，移除這些鍵，並在檔案中使用去掉 `_env` 後綴的欄位名稱。每個檔案最多 16 KiB。Unix 上可用 `0400` 或 `0440`，目錄和檔案由部署方控制。輪替時新增修訂檔案，保留舊任務仍參照的版本，撤銷時移除舊檔案並重新啟動持有舊驅動實例的程序。應用程式可在容器中綁定 `dyn ModerationCredentialProvider`，從既有密鑰服務讀取相同參照和修訂。

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

內嵌圖片經 multipart 上傳。已儲存圖片依 `media_disk`、`media_origin` 產生固定私人 S3 物件版本的短時 HTTPS 連結。介接器保留取樣結果中每個已設定類別的最高分。供應商未證明整個影像序列已處理時，視覺維持部分涵蓋。OCR 可透過單獨的 Hive 專案設定。 [Hive 視覺審核](https://docs.thehive.ai/docs/visual-content-moderation)

## Hive OCR {#hive-ocr}

在具名 `hive` 設定填寫 `ocr_project`、`ocr_token_env`、`ocr_classes` 和 `ocr_review_severity`。OCR 專案使用 Hive 的 `OCR_MODERATION` 模型。對該設定呼叫 `image()`，判斷結果時指定 `Modality::Ocr`。如果業務同時需要視覺與 OCR，呼叫兩個具名專案並檢查各自的結果。

回應確認靜態圖僅一個影格、審核時間戳為零，且每個辨識區塊都具備必查類別的結果時，可算完整涵蓋。動態圖與證據不完整時仍為部分涵蓋。介接器保留類別層級的證據和原始嚴重度，省略 OCR 原文、命中值與座標。 [Hive OCR 審核](https://docs.thehive.ai/reference/ocr-moderation)

## Hive 隨選影片提交 {#hive-video}

在具名 `hive` 設定中，將 `video_project` 設為 Hive V2 Enterprise 影片專案的數字 ID。將 `video_callback_url` 設為應用程式的 HTTPS 回呼路由，並用 `media_disk` 和 `media_origin` 指定私有物件來源。明確使用環境變數時設定 `video_token_env`，使用憑證檔案時填寫 `video_token`。

`submit_video()` 將指定版本的物件交給 V2 非同步介面。Hive 回應通過專案和實際回傳的回呼位址核對後，回傳供應商任務 ID。回執不明確時先核對原始任務，再決定是否重新提交。將回傳 ID 用於追蹤任務，發布前應透過專案接入的管道確認審核結果。 [Hive 建立任務](https://docs.thehive.ai/reference/submitting-via-task-api)

如果 Hive 已為該專案啟用 V1 Task Summary 介面，可設定 `video_query_api: "v1-task-summary"`，在 `video_classes` 列出類別，並將 `video_review_score` 設為 0 至 1。該設定的任務可呼叫 `query_video()`。每次最多讀取四頁，每頁 64 幀，並保留類別的最高分和對應時間。畫面取樣或分頁受限時，涵蓋範圍維持部分。 [Hive 查詢介面](https://docs.thehive.ai/reference/get-task-summary)

各項影片檢查使用獨立專案與憑證。畫面文字將 `video_model_type` 設為 `OCR_MODERATION`，語音設為 `TRANSCRIPTION`，兩者都用 `video_review_severity` 設定 1、2 或 3 級複核門檻。OCR 證據保存對應幀時間，語音證據保存分類嚴重度。正規化結果僅提供類別證據，原始轉錄和比對文字留在供應商回應。 [Hive OCR](https://docs.thehive.ai/reference/ocr-moderation) · [Hive 語音審核](https://docs.thehive.ai/reference/speech-moderation)

Hive 回呼可以透過 Webhook Client 接收查詢提示。將 `HiveLookupHint` 綁定到固定的 Hive 設定修訂，在路由設定 `moderation-hive-hint` 驗章器，將該實例分別註冊為具名 `unverified_hint_receiver` 的策略及 `QueuedWebhookHint` 的處理器。提示佇列和收件資料庫使用同一資料庫，多實例部署使用共用快取執行入口限流。

僅在 Hive 專案已開通 `video_query_api = "v1-task-summary"` 查詢時啟用此入口。已登記工作須與固定專案和設定修訂相符，最多提前一次排程查詢。回呼本文不寫入收件資料庫。Worker 用原專案憑據向 Hive 查詢並套用原有涵蓋規則。 [Hive 工作回呼](https://docs.thehive.ai/reference/submit-a-task-asynchronously)

## 阿里雲隨選影片 {#aliyun-video}

在具名 `aliyun` 設定指定 `video_service`。上海、北京、杭州支援 `videoDetection`，上海和北京支援 `videoDetectionByVL`。私人版本化 S3 物件使用 `media_disk`、`media_origin`，驅動會為指定版本產生五分鐘有效的 HTTPS 簽名下載連結。

`submit_video()` 傳回供應商工作 ID。保留原具名設定與 ID，再透過 `query_video()` 查詢。阿里雲建議提交至少 30 秒後首次查詢，結果保留 24 小時。提交回應遺失時先核對 `SubmissionUnknown`，再決定是否建立新工作。阿里雲的遠端取消介面適用於直播串流。

完成的工作仍可能只有部分畫面、語音或 OCR 涵蓋。`Assessment::evidence_span()` 使用原影片起點後的毫秒表示取樣影格或語音證據。辨識原文及證據暫時連結均不出現在結果中。 [阿里雲隨選影片 API](https://help.aliyun.com/zh/document_detail/2505810.html)

接收帶簽章的影片通知時，將 `video_callback_url` 設為固定 HTTPS 入口，並在憑據掛載提供 `video_callback_seed`。使用環境變數憑據時，透過 `video_callback_seed_env` 指定變數名稱。Seed 使用 16–64 個 ASCII 字母、數字或底線。在 Webhook Client 對應路由設定 `moderation-aliyun`、`moderation_callback_account_uid` 中的阿里雲主帳戶 UID，以及 `moderation_callback_credential` 中的憑據修訂。將 `AliyunCallbackProcessor` 綁定到同一帳戶別名和專用收件範圍。

驅動選擇 SHA-256 回呼校驗。驗章器比對解碼後原始 `content` 字串和 `checksum`。終態通知可提前觸發一次帶認證的原工作查詢，提前到達時可延期關聯。表單本文適用收件保存策略。 [阿里雲影片回呼](https://www.alibabacloud.com/help/en/content-moderation/latest/video-moderation-enhanced-api)

## 騰訊雲隨選影片 {#tencent-video}

為具名 `tencent` 設定指定標準隨選策略 `video_biz_type`，並設定私人版本化 S3 素材的 `media_disk` 和 `media_origin`。驅動呼叫 VM `2021-09-22` 介面並使用 `Type=VIDEO`。

透過 `submit_video()` 送出，再用同一具名設定查詢傳回的工作 ID。`cancel_video()` 用於等待中的工作。取消前核對工作類型和策略，已經開始的工作可能由騰訊雲拒絕取消。提交回傳 `SubmissionUnknown` 時，先核對之前的工作再考慮重新提交。

驅動將回應中的畫面、語音和 OCR 證據依原件時間軸轉成毫秒。缺少完整涵蓋證明時仍保留部分涵蓋。 [提交](https://cloud.tencent.com/document/api/1265/80017) · [查詢](https://cloud.tencent.com/document/product/1265/80016) · [取消](https://cloud.tencent.com/document/api/1265/80018)

具名隨選設定可用 `video_callback_url` 向騰訊雲傳入固定 HTTPS 通知入口。在帳戶憑據掛載中提供 `video_callback_seed`，使用環境變數憑據時用 `video_callback_seed_env` 指定變數名稱。Seed 長度為 16–128 位元組，使用不含空白的可列印 ASCII。在 Webhook Client 對應路由設定 `moderation-tencent` 驗章器、憑據掛載引用 `moderation_callback_credential` 和 `TencentCallbackProcessor`。限制收件儲存的存取，並設定保存期限。

驗章器按騰訊雲校驗和規則驗證原始本文的 `X-Signature`。處理器核對路由、帳戶、策略和工作類型。若終態通知早於原工作 ID 寫入，則延期處理；關聯後提前執行一次帶認證的查詢。審核決定採用查詢結果，並繼續要求完整涵蓋。 [騰訊雲回呼設定](https://cloud.tencent.com/document/product/1265/104001)

## Sightengine 隨選影片 {#sightengine-video}

在具名 `sightengine` 設定填寫 `video_models` 和 `video_review_score`，啟用 OCR 時另填 `video_text_categories`。`video_max_duration` 預設 1,800 秒，方案允許時可設至 3,600 秒。使用 `media_disk` 與 `media_origin` 指定版本化私人素材來源。

`submit_video()` 傳回媒體 ID，用原具名設定查詢或停止工作。完成狀態可能來自時長限制或部分影格取樣。非同步入口只報告已取得的畫面和 OCR 證據，語音維持涵蓋缺失。HTTP 用戶端的診斷事件從查詢字串及停止請求正文中省略憑證。 [非同步影片](https://sightengine.com/docs/moderate-stored-video-asynchronously) · [工作操作](https://sightengine.com/docs/interact-with-video-moderation-jobs)

### 短影片語音 {#sightengine-short-speech}

為語音建立具名 `sightengine` 設定，在 `video_speech_categories` 填寫類別，例如 `["insult", "phone_number"]`，並用 `media_disk` 和 `media_origin` 指定私人素材。此設定透過 `video/check-sync.json` 呼叫 `audio-moderation`，請求逾時預設 75 秒，可用 `timeout_ms` 設定。使用獨立設定填寫 `video_models`。

呼叫 `StoredMedia::with_video_duration_ms`，將應用程式已核實的時長與確切素材版本一併傳入。Sightengine 的短影片介面只接受不足 60 秒的素材。需要持久化時將該設定固定為 `required_modalities = ["speech"]`，依一般流程登記。送審 Worker 一次寫入終態與結果。證據保留類別、嚴重程度標籤和時間範圍，記錄與診斷不保存辨識原文。缺少音軌結果時語音涵蓋為無法使用。同一素材還有其他必查模態時，在審核群組中設定獨立檢查。 [語音審核](https://sightengine.com/docs/audio-moderation-model) · [短影片介面](https://sightengine.com/docs/moderate-stored-video)

### 長影片語音分段 {#sightengine-speech-segments}

應用程式從同一原素材版本產生並核實 2–64 個片段。呼叫 `VideoReviewSegment::new` 傳入各片段在原素材時間軸上的起點。用 `StoredMedia::with_video_duration_ms` 為原素材和每個片段記錄已核實的時長。每個片段不足 60 秒，使用不同素材版本，依序涵蓋整條時間軸。相鄰片段可以重疊以保留語境。

將原素材版本、固定語音設定和片段清單傳給 `VideoReviewGroupRequest::for_speech_segments`，沿用審核群組和 Worker 的登記與執行方式。用 `segment_window_ms(index)` 定位原素材區間，顯示證據時將片段內的時間戳加上片段起點。發布時使用審核群組和原內容修訂，每個片段都須具備完整語音涵蓋。

### 帳戶層級影片回呼 {#sightengine-callbacks}

同時啟用 `content-moderation` 和 `webhook-client`。在 Sightengine 帳戶中將回呼 URL 指向透過 Webhook Client 註冊的路由。各供應商帳戶使用自己的路由設定，將 `signature_validator` 設為 `moderation-sightengine`。`moderation_callback_credential` 填寫 `{"name":"sightengine-callback","revision":"r2"}` 這樣的物件，將 `signing_secret` 寫入掛載的 `sightengine-callback.r2.json`。選擇應用程式的 Webhook Client 儲存和處理 Job。

輪替期間，`moderation_previous_callback_credentials` 可保留至多三個早期參照。撤銷時從路由設定移除已停用的參照。驗簽使用原始請求本文，並核對五分鐘時間窗。Sightengine 的逐請求回呼 URL 會傳送未簽章通知，簽章路由應使用帳戶層級 URL。 [Sightengine 回呼簽章](https://sightengine.com/docs/callback-signature)

持久審核可透過 `QueuedWebhookJob::register_with_database_deferred_processing` 註冊 `SightengineCallbackProcessor`。建立時傳入審核儲存、固定設定使用的帳戶別名，以及 Webhook Client 的作用域、設定名稱和修訂。提前送達的回呼由 Webhook Client 延期處理，等候可關聯的供應商任務 ID。如果應用程式已知接收記錄 ID，可呼叫 `QueuedWebhookJob::wake_deferred` 提前喚醒。

符合的終態通知會關聯到原審核，並讓原供應商任務到期查詢。`moderation:recover` 派送帶認證的查詢，審核持續使用原有查詢預算、截止時間和必檢涵蓋要求。

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

私有 S3 磁碟透過 TLS 反向代理提供下載時，將審核設定的 `media_origin` 與磁碟的 `temporary_url` 設為同一個 HTTPS 來源。預設 S3 磁碟使用 `AWS_TEMPORARY_URL`。代理轉送至儲存端點時保留簽章中的原始 Host、路徑與查詢字串。保持儲存貯體私有，代理存取日誌應省略簽章查詢字串。[S3 暫時連結](../digging-deeper/filesystem.md#s3-temporary-url-origin)

啟用儲存素材審核前，確認供應商能透過該 HTTPS 入口下載簽章對應的物件版本，且未簽章請求會被拒絕。

在反向代理終止回呼 TLS 時，保留原始請求本文、內容型別和供應商簽章標頭。代理應避免解析後重新編碼 JSON 或表單，也應避免將回呼本文和簽章寫入存取日誌。

需要延後查詢時，在 `moderation.profile_revisions.<名稱>.<修訂>` 下宣告完整設定，加入 `account`、`policy_revision`、`credential_ref`、`credential_revision` 和非空的 `required_modalities`。可用的模態為 `text`、`visual`、`speech` 和 `ocr`。`account` 填寫部署時與該憑證配對的帳號或專案別名。位址、模型、策略或憑證修訂變更時建立新設定修訂，並保留未完成任務參照的舊設定。

提交前用 `Moderation::pin_profile(name, revision)` 固定設定，用 `Moderation::driver_for_revision(&profile)` 解析驅動。應用程式將傳回的設定定位符、遠端 `TaskId` 和原始物件版本一起持久化，查詢時依該定位符重新解析。原設定遭直接修改、移除或對應憑證無法讀取時，復原失敗。待處理、失敗或必檢涵蓋不完整時均無法核准發布。

透過 `Assessment::decision` 檢查所需模態。結果不完整時仍可保留人工複核或封鎖所需的風險證據。若業務要求語音審核，部分語音涵蓋的成功結果無法產生 `Allow`。

## 持久化影片送審意圖 {#video-review-storage}

啟用擴充後執行 `./artisan migrate`。需要將兩份遷移原始碼發布到應用程式時，執行 `./artisan vendor:publish --tag=moderation-migrations`。

使用資料庫連線和穩定的應用程式、租戶識別碼建立 `ModerationStore`，HTTP 和 Worker 使用相同識別碼。用冪等鍵、業務內容 ID 與修訂、原始 `StoredMedia`、已固定的設定和逾時秒數建立 `VideoReviewRequest`。呼叫 `stage_video` 在同一交易寫入待審核記錄和隨選影片子檢查。重試相同請求時可取得原記錄。同一範圍內用相同的鍵提交不同內容會產生衝突。透過 `find` 按審核 ID 從同一範圍讀取。

落單後用儲存介面及目前應用程式的 `ModerationManager` 建立 `VideoReviewService`，透過 Worker 或應用程式呼叫 `submit(review_id)`。服務在資料庫領取子檢查，解析原憑證修訂，並提交原始物件版本。並行呼叫可能讀到 `Submitting`。供應商傳回可確認的回應時保存其任務 ID。回應不明確或領取後 Worker 中斷時，後續呼叫可在租約到期後將狀態記為 `Unknown`。先向供應商核對原任務，再決定是否建立新審核。達到截止時間的審核保持過期。

呼叫 `VideoReviewService::query(review_id)` 推進已確認的任務。可在固定設定中設定 `query_interval_seconds` 和 `query_max_attempts`，預設 30 秒、120 次，兩者各可設 1–300。服務在資料庫中記錄下次查詢時間、租約和嘗試次數，重新啟動後仍使用相同預算。遠端結果正常待定時使用設定間隔，暫時故障採用有上限的退避。HTTP 429 或 503 帶有有效的 `Retry-After` 時，可以延後下次查詢，支援等待秒數和 HTTP 日期。審核截止時間和嘗試次數仍有效。等待期間可記錄已驗簽回呼或未驗簽查詢提示，下一次查詢仍須等到供應商指定的時間。

透過 `VideoReview::assessment()` 讀取正規化證據，透過 `VideoReview::decision()` 讀取審核決定。所有必檢模態涵蓋完整後，審核才有決定。供應商完成但涵蓋不足時，審核保留證據並回報涵蓋失敗。

持久化隨選送審與查詢分別依應用程式、供應商驅動和帳號共享速率額度，涵蓋該應用程式的不同租戶及固定設定修訂。預設每 60 秒允許 60 次送審和 300 次查詢。可在 `moderation.account_rate_limits.<driver>.<account>` 設定 `submit`、`query` 和 `window_seconds`，次數支援 1–10,000，視窗支援 1–300 秒。同一應用程式的副本使用相同應用程式 ID、設定和資料庫。等待額度時，原審核截止時間和查詢次數預算仍有效。直接呼叫驅動的應用程式自行管理呼叫頻率。

同一帳號預設最多保留 32 個遠端執行中的隨選檢查。可在 `moderation.account_in_flight_limits.<driver>.<account>` 設定 1–1,024 的整數。送審已受理或結果未知時繼續佔用名額。遠端確認結束、停止或拒絕受理後釋放名額。遠端狀態仍不確定時，租約持續到審核截止時間。本機租約到期時遠端工作仍可能存在。升級已有資料庫時先暫停新送審，讓舊 Worker 推進已有檢查。仍可能執行的審核須完成、確認取消或達到原截止時間，再停止舊 Worker 並套用遷移。

### 多項必查的審核 {#video-review-groups}

透過 `VideoReviewGroupRequest::new` 和 `ModerationStore::stage_video_group` 為同一內容修訂和媒體版本保存 2–8 個相異的固定設定修訂，明確選擇各必查模態對應的設定。同一意圖及相同順序的檢查會傳回原審核群組。同一意圖下變更順序、設定、內容或媒體版本時會傳回衝突。

每個子檢查獨立提交和查詢，可由原有 Worker 與復原指令推進。用 `find_video_group` 讀取彙總結果。每個必查項成功且涵蓋完整後才產生決定，並採用最嚴格的建議，優先順序依次為 Block、Review、Allow。應用程式仍應根據目前內容修訂執行發布授權。

### 人工裁決與發布 {#video-review-decision}

應用程式驗證操作者身分並完成授權後，使用內容 ID 和修訂、預期決定修訂、裁決、操作者 ID 與理由建立 `ReviewDecisionRequest`。依業務審核策略選擇 `VideoReviewTarget::Single` 或 `VideoReviewTarget::Group` 呼叫 `VideoReviewService::record_decision`。自動結論的修訂為 0。每次成功的人工裁決追加新修訂並保留原始評估；使用過期修訂的並行寫入會遭拒絕。全部必查項成功且涵蓋完整後才可記錄人工裁決。

用同一審核對象和目前內容 ID、修訂呼叫 `effective_decision`。僅在有效決定為 `Allow` 且決定修訂與應用程式接受的修訂一致時繼續發布。多項必查策略一律使用審核群組。應用程式在自身發布流程內確認內容仍存在、仍可發布且修訂一致，並在提交發布前重新讀取有效決定。操作者 ID 和理由僅在獲授權的稽核入口顯示。

撤回或刪除內容修訂時，應用程式授權後用確切的內容 ID 和修訂呼叫 `ModerationStore::withdraw_video_revision`。重複呼叫會傳回首次撤回時間。同一應用程式和租戶內，該修訂的所有審核立即失去有效發布決定，原始評估仍可供授權稽核。新內容修訂依其審核結果發布。撤回時如需停止執行中的檢查，可一併呼叫取消介面。

需要清除已撤回修訂的審核資料時，先取消檢查或等待終態，再呼叫 `ModerationStore::purge_withdrawn_video`，傳入審核對象和 60–604,800 秒的保留時長。清除子檢查時選擇整個審核群組。清除會刪除評估、人工裁決和診斷。已知供應商工作的摘要會暫時保留，用於確認遲到回呼。可排程 `./artisan moderation:prune-tombstones app tenant --limit=100` 分批刪除過期摘要。Webhook Client 接收記錄繼續依自己的保留規則運作。修訂仍處於撤回狀態。

### 取消已保存的隨選檢查 {#video-review-cancellation}

呼叫 `VideoReviewService::cancel(id)`，凍結待處理或進行中的檢查。取消使用資料庫時鐘和記錄版本，已超過截止時間的檢查會標記為過期。晚到的 Worker 無法改寫取消結果。

已有確認的工作 ID 且供應商支援停止時，服務會用原設定嘗試遠端取消。透過 `provider_cancellation()` 分別查看供應商確認、拒絕、不支援或結果未確認等狀態。重複請求會傳回已儲存的狀態。工作編號未知或停止結果未確認時，應先向供應商核對再繼續處理。

呼叫 `VideoReviewService::cancel_group(id)`，在同一交易凍結待處理或執行中群組內的全部未結束檢查。已結束檢查保留原有證據。本機交易提交後，服務再嘗試停止已確認的遠端工作，並在對應檢查記錄供應商結果。

用 `ModerationStore::late_video_notices(id)` 檢視目前應用程式和租戶內的遲到結果。送審、認證查詢和驗簽回呼各保留首次觀察，記錄當時的審核狀態和資料庫時間。回呼診斷可參照 Webhook Client 接收記錄。發布繼續使用 `effective_decision`。

## 定時復原影片審核 {#review-recovery}

對每個需要管理的應用程式和租戶，將 `moderation:recover` 加入每分鐘排程。排程程序和 Worker 共用資料庫與佇列。命令領取最多 `--limit` 筆到期審核，預設 100，範圍 1–500，只將本機審核 ID 和版本等定位資訊加入佇列。

```sh
./artisan moderation:recover app tenant --connection=database --queue=moderation
./artisan queue:work database --queue=moderation
```

透過 `--database` 選擇具名審核資料庫。將 Worker 佇列的保留或可見性逾時設為長於復原 Job 的 125 秒逾時。資料庫保存下次復原時間，訊息遺失後可在 60 秒派送租約到期時重新加入佇列。收到舊審核版本的訊息時保留目前狀態。
