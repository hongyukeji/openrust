# 郵件

## 產生郵件類型 {#generating-mailables}

使用 Artisan 產生 Rust 郵件類型。用 `--view` 同時產生 Blade 範本，用 `--test` 產生配套功能測試。

```sh
./artisan make:mail OrderShipped
./artisan make:mail Billing/InvoiceReady --view=mail.billing.invoice --test
```

在終端機執行不帶名稱的 `./artisan make:mail`，可輸入類型名稱並選擇 Markdown 視圖、空白 Blade 視圖或不建立視圖。傳入選項時，Artisan 只詢問名稱。使用 `--no-interaction` 或管線輸入時，需明確提供名稱。

產生的類型位於 `app/mail`，檔案和模組目錄使用 snake_case。建置前需在上層 `mod.rs` 宣告產生的模組。預設視圖名稱以 `mail.` 開頭，後接連字號格式的各名稱段落。`--force` 可覆寫 Rust 原始碼，既有視圖會保留。應用程式可在 `stubs/` 覆寫原始碼範本。

## 傳送郵件

實作 `Mailable`，提供信封和內容。透過 `Mail::to` 新增收件人。可用 `Mail::mailer(Some("smtp"))` 選擇命名郵件器，或覆寫 `Mailable::mailer`。

```rust
use openrust::{
    mail::{MailResult, Mailable, mailables::{Content, Envelope}},
    support::facades::Mail,
};

pub struct OrderShipped;

impl Mailable for OrderShipped {
    fn envelope(&self) -> MailResult<Envelope> {
        Ok(Envelope { subject: "Order shipped".into(), ..Envelope::default() })
    }

    fn content(&self) -> MailResult<Content> {
        Ok(Content { text: Some("Your order is on its way.".into()), ..Content::default() })
    }
}

pub async fn send_order() -> MailResult<()> {
    Mail::to("customer@example.test")?.send(&OrderShipped).await?;
    Ok(())
}
```

`Content` 支援文字、HTML 和帶資料的命名視圖。可用 `view` 和 `text_view` 產生兩種 MIME 內文。附件提供位元組、檔名和 MIME 類型，自訂郵件標頭透過 `headers` 提供。密件副本地址保留在投遞信封中，郵件標頭會移除 Bcc。

### 附件 {#attachments}

從 `Mailable::attachment_sources` 回傳附件來源。用 `Attachment::from_path` 讀取本機檔案，`from_storage` 讀取預設磁碟，`from_storage_disk` 讀取指定磁碟，`from_data` 接收產生的位元組。使用 `r#as` 設定顯示名稱，並用 `with_mime` 覆寫 MIME 類型。

```rust
use openrust::mail::{
    MailResult,
    mailables::{Attachment, AttachmentSource},
};

fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
    Ok(vec![
        Attachment::from_path(&self.local_report),
        Attachment::from_storage("reports/monthly.csv"),
        Attachment::from_storage_disk("s3", "reports/annual.pdf")
            .r#as("annual-report.pdf")
            .with_mime("application/pdf"),
        Attachment::from_data(self.generated_report.clone(), "summary.csv")
            .with_mime("text/csv"),
    ])
}
```

檔案及磁碟附件預設使用檔名。S3 優先採用物件的內容類型。本機檔案會先偵測類型，再依副檔名回退。產生的位元組預設為 `application/octet-stream`。傳送時才讀取檔案或磁碟，佇列中的郵件由 Worker 讀取，因此路徑或磁碟必須可由 Worker 存取。產生的位元組會隨佇列承載資料保存。

用 `Attachment::from_uploaded_file(&file)` 附加 HTTP 請求中的上傳檔案。預設使用用戶端提供的檔名和偵測到的 MIME 類型，無法辨識時採用用戶端宣告的類型。上傳位元組會隨佇列郵件保存。用 `Attachment::from_cloud_storage(path)?` 選取 `filesystems.cloud` 設定的磁碟，預設為 `s3`。

用 `Attachment::from_url(url)?` 附加絕對 HTTP 或 HTTPS URL。郵件器在傳送時下載附件，佇列郵件由 Worker 下載。URL 路徑提供預設檔名，回應可提供 MIME 類型。用 `r#as` 重新命名，用 `with_mime` 覆寫類型。

### 內嵌附件 {#inline-attachments}

從 `Mailable::inline_attachments` 回傳 `InlineAttachment`，用 `cid()` 取得圖片網址。視圖和附件使用相同的 Content-ID。

```rust
let logo = InlineAttachment::new("logo@example.test", Attachment {
    name: "logo.png".into(),
    mime: "image/png".into(),
    bytes: logo_bytes,
});
let image_src = logo.cid();
```

將 `image_src` 傳入 Blade 的 `<img src="{{ $image_src }}">`，也可以用它作為 Markdown 圖片的網址。Worker 隨工作收到圖片位元組。SMTP 等原始 MIME 傳輸會將圖片和 HTML 一起傳送，瀏覽器預覽則把相符的圖片網址轉成資料 URL。Postmark、Resend 和 Cloudflare 透過 JSON 附件欄位傳遞原始檔名和 Content-ID。Mailgun 表單 API 以 Content-ID 作為內嵌分段的檔名，使其與 HTML 引用相符。Mailgun 原始 MIME 模式保留原始檔名。

### 可附加物件 {#attachable-objects}

領域物件負責選擇附件時，實作 `Attachable`。在郵件類型回傳附件來源時，用 `to_mail_attachment()` 轉換物件。

```rust
use openrust::{
    contracts::mail::Attachable,
    mail::{MailResult, mailables::{Attachment, AttachmentSource}},
};

struct Report(String);

impl Attachable for Report {
    #[allow(non_snake_case)]
    fn to_mail_attachment(&self) -> MailResult<AttachmentSource> {
        Ok(Attachment::from_url(&self.0)?.r#as("report.pdf"))
    }
}
```

### 標籤和中繼資料 {#tags-and-metadata}

設定 `Envelope::tags` 和 `Envelope::metadata` 以分組及追蹤郵件。兩個欄位都會隨佇列郵件保存。

```rust
fn envelope(&self) -> MailResult<Envelope> {
    Ok(Envelope {
        subject: "Order shipped".into(),
        tags: vec!["shipment".into()],
        metadata: [("order-id".into(), "R123".into())].into(),
        ..Envelope::default()
    })
}
```

測試中可用 `assert_has_tag` 和 `assert_has_metadata` 檢查宣告。SMTP 和原始 MIME 使用 `X-Tag` 和 `X-Metadata-*` 標頭。Mailgun 的結構化 API 將標籤對應至 `o:tag`，中繼資料對應至 `v:*`。Postmark 接受一個標籤和一組中繼資料。SES 和 SES v2 將信封中繼資料附加至設定的供應商標籤。Resend 將標籤以名稱與值傳送，中繼資料保留在郵件標頭。Cloudflare 在郵件標頭保存中繼資料和最後一個標籤。

### 自訂郵件訊息 {#customizing-the-message}

在服務提供者啟動時向應用程式的 `MailManager` 註冊郵件自訂回呼，並在 `Envelope::using` 依執行順序列出。回呼會收到 `MessageDraft`，其中包含信封、郵件標頭及已轉譯的文字和 HTML。回呼會在讀取宣告的附件及觸發投遞事件之前執行，也可以在草稿中加入附件。

```rust
manager.register_message_customizer("shipment.tracking", |draft, data| {
    if let Some(code) = data["code"].as_str() {
        draft.headers.text.insert("X-Tracking-Code".into(), code.into());
    }
    Ok(())
})?;
```

```rust
fn envelope(&self) -> MailResult<Envelope> {
    Ok(Envelope {
        subject: "Order shipped".into(),
        using: vec![
            MessageCustomization::new("shipment.tracking", serde_json::json!({"code":"R123"})),
        ],
        ..Envelope::default()
    })
}
```

佇列郵件會保存回呼名稱和 JSON 資料。Worker 應用程式註冊同名回呼，並在每次投遞嘗試時執行。找不到回呼或回呼回傳錯誤時，本次嘗試停止。郵件預覽使用原始內容。

## 設定

`config/mail.rs` 定義預設郵件器、命名郵件器和寄件人。骨架預設使用 `log` 驅動，在 debug 層級寫入 MIME 郵件。`array` 驅動可透過 `Mailer::sent` 檢查目前應用程式的郵件。

使用 SMTP 時設定 `MAIL_MAILER=smtp`，再設定 `MAIL_HOST`、`MAIL_PORT` 或 `MAIL_URL`。URL 覆寫其提供的連線欄位。`smtps` 使用隱含 TLS。`smtp` 在伺服器支援時使用 STARTTLS，設定憑證時必須成功升級 TLS。

SMTP 預設使用系統信任庫驗證憑證鏈和伺服器主機名稱。私有 CA 可透過行程環境變數 `SSL_CERT_FILE` 或 `SSL_CERT_DIR` 提供受信任憑證；寄信行程和 Worker 應使用相同的信任設定。

SMTP 信封位址保留帶引號的本機部分。顯示名稱包含 CR 或 LF 時回傳郵件組裝錯誤。無效或過長的伺服器回覆，以及 STARTTLS 回覆後夾帶的明文會使投遞失敗；通訊協定錯誤不回顯伺服器回覆或憑證資訊。原始 SMTP 資料會跳脫 CR、LF 與 CRLF 後的行首點，防止提早結束郵件本文。

選擇 `sendmail` 可執行 `MAIL_SENDMAIL_PATH` 設定的程式，支援透過標準輸入輸出進行 SMTP 的 `-bs` 模式，以及從標準輸入投遞郵件的 `-t` 模式。

選擇 `ses`、`ses-v2`、`postmark`、`resend`、`mailgun` 或 `cloudflare` 透過供應商投遞。先在 `config/services.rs` 設定對應服務，再用 `MAIL_MAILER` 選擇郵件器。

- SES 和 SES v2 使用 AWS 憑證或 AWS SDK 憑證鏈。SES v2 支援 `ConfigurationSetName`、`EmailTags` 和 `ListManagementOptions` 等郵件器選項。
- Postmark 使用 `POSTMARK_TOKEN`，Resend 使用 `RESEND_KEY`。
- Mailgun 使用 `MAILGUN_DOMAIN` 和 `MAILGUN_SECRET`。使用 EU API 時設定 `MAILGUN_ENDPOINT=api.eu.mailgun.net`。預設 `https` 方案會提交已組裝的 MIME 郵件。若要提交 multipart 欄位，將 `scheme` 設為 `api`。
- Cloudflare 使用 `CLOUDFLARE_ACCOUNT_ID` 和 `CLOUDFLARE_KEY`。

在 `config/mail.rs` 中，`failover` 在目前郵件器傳送失敗時切換，`roundrobin` 輪流使用郵件器。兩者皆透過 `mailers` 指定郵件器清單，並在 `retry_after` 秒內跳過失敗的傳輸。

## Markdown 郵件

將 `Content::markdown` 設為包含 CommonMark 的具名 Blade 視圖。郵件器產生 HTML 和純文字兩種版本，並將主題樣式內嵌到 HTML。框架提供預設主題。若要使用應用程式樣式，設定 `MAIL_MARKDOWN_THEME=receipt`，再將 `receipt.css` 放到 `resources/views/vendor/mail/html/themes/`。可透過 `text_view` 提供自己的純文字版本。

```rust
let content = Content {
    markdown: Some("markdown-receipt".into()),
    with: [("name".into(), serde_json::json!("Sam"))].into_iter().collect(),
    ..Content::default()
};
```

### 產生 Markdown 郵件 {#generating-markdown-mailables}

用 `--markdown` 產生使用郵件元件的 Blade 範本。宣告模組並重新建置後即可使用新類型。兩個視圖選項同時出現時選擇 Markdown 範本。

```sh
./artisan make:mail Orders/ShipmentNotice --markdown=mail.orders.shipment
```

### 撰寫 Markdown 郵件 {#writing-markdown-messages}

以 `<x-mail::message>` 包住 Markdown 郵件視圖。元件使用應用程式名稱和 URL 產生頁首，並加入頁尾。可用名為 `subcopy` 的插槽加入補充說明。按鈕支援 `url`、`color` 和 `align`。面板內文會依 Markdown 轉譯。表格元件在 HTML 中轉譯 Markdown 表格，產生純文字時會省略該表格。

可用 `<x-mail::header url="https://example.com">`、`<x-mail::footer>` 和 `<x-mail::subcopy>` 分別加入獨立的頁首、頁尾和補充說明。預設頁尾依應用程式的時區顯示年份，並依郵件語言查找 `All rights reserved.`。可在對應語言的 JSON 翻譯檔中加入這個鍵。

```blade
<x-mail::message>
# Order ready

<x-mail::button :url="$url" color="success">
Review order
</x-mail::button>

<x-mail::panel>Keep this receipt for your records.</x-mail::panel>

<x-mail::table>
| Item | Qty |
| --- | ---: |
| Book | 1 |
</x-mail::table>

<x-slot:subcopy>Contact support if you have questions.</x-slot:subcopy>
</x-mail::message>
```

用 `<x-mail::layout>` 包住整個視圖，即可自行組合郵件區塊。命名插槽為 `head`、`header`、`subcopy` 和 `footer`。`head` 接收寫入 HTML 文件頭部的內容，純文字郵件會省略這個插槽。

```blade
<x-mail::layout>
<x-slot:header>
<x-mail::header :url="$url">訂單</x-mail::header>
</x-slot:header>

# 訂單已就緒

<x-slot:footer>
<x-mail::footer>感謝您的訂購。</x-mail::footer>
</x-slot:footer>
</x-mail::layout>
```

### 自訂 Markdown 版面 {#customizing-the-components}

將 HTML、純文字版面、元件範本與預設樣式表發布到 `resources/views/vendor/mail`。

編輯 `html/` 或 `text/` 下的 `header.blade.html`、`footer.blade.html` 和 `subcopy.blade.html` 即可自訂這些區塊。內建 `message` 範本也使用這三份範本。

發布後的預設版面用 `$mail_content` 放置組合好的內容，郵件表格寬 570 px 並適應窄螢幕。使用顯式版面時，自訂範本可用 `$slot` 放置正文，再分別安排 `$header`、`$subcopy` 和 `$footer`。

#### 自訂單封郵件的 CSS {#customizing-the-css}

將 `receipt.css` 放到 `resources/views/vendor/mail/html/themes/`，並在郵件的 `Content` 設定 `theme: Some("receipt".into())`。

```rust
use openrust::mail::mailables::Content;

let content = Content {
    markdown: Some("mail.receipt".into()),
    theme: Some("receipt".into()),
    ..Content::default()
};
```

主題套用於該 Markdown 郵件的 HTML 版本，佇列投遞也會採用相同設定。未指定主題或提供空字串時，郵件器採用應用程式的 Markdown 主題。Worker 從應用程式的視圖和主題目錄讀取樣式表。

### Markdown 擴充與安全編碼 {#markdown-extensions-and-encoding}

CommonMark 和表格一直可用。在 `config/mail.rs` 的 `mail.markdown.extensions` 填寫要啟用的擴充名稱，預設為空。支援 `strikethrough`、`task_lists`、`footnotes` 和 `smart_punctuation`。未知的擴充名稱會讓郵件器回傳設定錯誤。

將 `mail.markdown.secured_encoding` 設為 `true` 後，Markdown 正文中透過 `{{ $value }}` 插入的字串會以原文顯示。例如值為 `[View](https://example.com)` 時，郵件會原樣顯示該文字。範本可用 `[View]({{ $url }})` 動態指定連結目的地。HTML 跳脫和 URL 過濾仍然生效。此選項預設關閉，依應用程式生效，涵蓋自動產生的純文字郵件。

```sh
./artisan vendor:publish --tag=laravel-mail
```

編輯 `html/layout.blade.html` 及 `text/layout.blade.html` 可加入頁首、頁尾或外層內容。保留 `{!! $slot !!}` 作為渲染後內文的位置。修改 `html/themes/default.css` 可更新預設主題。`mail.markdown.paths` 中的相對路徑依應用程式根目錄解析，按宣告順序尋找，再使用標準發布目錄。只有傳入 `--force` 才會覆寫現有檔案。

## 郵件加入佇列

讓 `Mailable::queue_options` 回傳佇列選項後，`send` 會將郵件加入佇列。`send_now` 立即傳送。`PendingMail::queue` 接收連線、佇列、延遲和選用交易。`after_commit` 將派送延後到交易提交。Worker 用佇列中的郵件定義呈現命名視圖並傳送。用 `./artisan queue:work --stop-when-empty` 消費郵件佇列。

## 呈現郵件 {#rendering-mailables}

呼叫 `mailable.render().await?` 可直接取得內文，免於傳送或加入佇列。呈現時採用該封郵件的語系和 Markdown 主題，優先回傳 HTML，純文字郵件回傳文字內文。

### 在瀏覽器預覽郵件 {#previewing-mailables-in-the-browser}

在應用程式路由中呼叫 `to_response()`，即可用相應的媒體類型顯示 HTML 或純文字內文。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    mail::Mailable,
    support::facades::Route,
};

fn preview_order(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { OrderShipped.to_response().await })
}

Route::get("/mail-preview/order", preview_order)?;
```

## 擴充郵件傳輸和投遞事件

在應用程式服務提供者中用 `MailManager::extend` 註冊 Rust 傳輸工廠，透過郵件器的 `transport` 設定選擇它。派發器在呼叫傳輸前發出 `MessageSending`，在傳輸接受郵件後發出 `MessageSent`。監聽器回傳 `false` 可以取消投遞。取消時，`send` 與 `send_now` 都回傳 `Ok(None)`。

## 測試郵件

### 測試郵件內容 {#testing-mailable-content}

直接在郵件類型上檢查宣告的主旨及地址，可一併傳入顯示名稱以精確比對。在測試應用程式的作用域內，HTML 和純文字斷言會使用郵件的語系渲染正文，無須傳送郵件。HTML 斷言傳入 `true` 會跳脫預期內容，傳入 `false` 可比對標記。順序斷言會依序比對不重疊的內容，並辨識 HTML 實體。

```rust
receipt
    .assert_has_subject("Receipt for Sam")
    .assertTo("sam@example.test", None)
    .assert_see_in_html("A & <Sam>", true)
    .await
    .assert_see_in_order_in_text(&["Plain", "receipt"])
    .await;
```

可在郵件類型上斷言附件宣告，無須讀取檔案或請求儲存服務。用 `assert_has_attachment_from_storage_disk` 檢查指定磁碟，用 `assert_has_no_attachments` 檢查是否沒有宣告一般附件。

```rust
receipt
    .assert_has_attachment_from_storage("reports/monthly.csv", None, None)
    .assert_has_attached_data(b"total,42", "summary.csv", Some("text/csv"));
```

### 測試郵件傳送 {#testing-mailable-sending}

在測試應用程式中呼叫 `Mail::fake()`，即可檢查郵件並跳過傳輸和加入佇列。斷言區分即時傳送與排隊傳送。用 `Mailable::kind()` 選擇郵件記錄，用比對斷言檢查信封或佇列選項。

```rust
let mail = Mail::fake()?;
Mail::to("ops@example.test")?.send_now(&receipt).await?;
mail.assert_sent_matching(receipt.kind(), |entry| {
    entry.envelope.to.iter().any(|to| to.address == "ops@example.test")
});
mail.assert_nothing_queued();
```

## 預設地址與取消

`Mailer::always_to`、`always_from`、`always_reply_to`、`always_return_path` 設定目前郵件器。啟動時設定的地址作為應用程式預設值；請求與佇列作用域內的覆寫僅影響目前執行。`always_to` 在郵件自訂完成後取代收件人並清空副本、密件副本。設定支援 `mail.to`、`mail.from`、`mail.reply_to`、`mail.return_path`。取消時 `send`、`send_now` 與 `PendingMail` 都回傳 `Ok(None)`，不呼叫傳輸、不派發 `MessageSent`；舊呼叫端應明確處理 `Option<SentMessage>`。


## 清除已解析的郵件實例

`MailManager::forget_mailers()` 清除全部已解析的郵件實例並傳回管理器。下次呼叫 `mailer(...)` 時，按原有設定和工廠重新建立實例。呼叫端已持有的 `Arc` 控制代碼仍可使用，直到持有者釋放。
