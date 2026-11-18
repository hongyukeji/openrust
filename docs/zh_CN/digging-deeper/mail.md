# 邮件

## 生成邮件类型 {#generating-mailables}

用 Artisan 生成 Rust 邮件类型。用 `--view` 同时生成 Blade 模板，用 `--test` 生成配套功能测试。

```sh
./artisan make:mail OrderShipped
./artisan make:mail Billing/InvoiceReady --view=mail.billing.invoice --test
```

在终端运行不带名称的 `./artisan make:mail`，可输入类型名称并选择 Markdown 视图、空白 Blade 视图或不创建视图。传入选项时，Artisan 只询问名称。使用 `--no-interaction` 或管道输入时，需显式提供名称。

生成的类型位于 `app/mail`，文件和模块目录使用 snake_case。构建前需在上级 `mod.rs` 声明生成的模块。默认视图名以 `mail.` 开头，后接连字符格式的名称各段。`--force` 可覆盖 Rust 源码，已有视图会保留。应用可在 `stubs/` 覆盖源码模板。

## 发送邮件

实现 `Mailable`，提供信封和内容。通过 `Mail::to` 添加收件人。可用 `Mail::mailer(Some("smtp"))` 选择命名邮件器，或覆盖 `Mailable::mailer`。

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

`Content` 支持文本、HTML 和带数据的命名视图。可用 `view` 和 `text_view` 生成两种 MIME 正文。附件提供字节、文件名和 MIME 类型，自定义邮件头通过 `headers` 提供。密送地址保留在投递信封中，邮件头会移除 Bcc。

### 附件 {#attachments}

从 `Mailable::attachment_sources` 返回附件来源。用 `Attachment::fromPath` 读取本地文件，`fromStorage` 读取默认磁盘，`fromStorageDisk` 读取指定磁盘，`fromData` 接收生成的字节。用 `r#as` 设置显示名称，用 `withMime` 覆盖 MIME 类型。

```rust
use openrust::mail::{
    MailResult,
    mailables::{Attachment, AttachmentSource},
};

fn attachment_sources(&self) -> MailResult<Vec<AttachmentSource>> {
    Ok(vec![
        Attachment::fromPath(&self.local_report),
        Attachment::fromStorage("reports/monthly.csv"),
        Attachment::fromStorageDisk("s3", "reports/annual.pdf")
            .r#as("annual-report.pdf")
            .withMime("application/pdf"),
        Attachment::fromData(self.generated_report.clone(), "summary.csv")
            .withMime("text/csv"),
    ])
}
```

文件和磁盘附件默认使用文件名。S3 优先使用对象的内容类型。本地文件先检测类型，再按扩展名回退。生成字节的默认类型是 `application/octet-stream`。发送时才读取文件或磁盘，队列中的邮件由 Worker 读取，因此路径或磁盘必须对 Worker 可用。生成的字节随队列载荷保存。

用 `Attachment::fromUploadedFile(&file)` 附加 HTTP 请求中的上传文件。默认使用客户端提供的文件名和检测到的 MIME 类型，无法识别时采用客户端声明的类型。上传字节随队列邮件保存。用 `Attachment::fromCloudStorage(path)?` 选择 `filesystems.cloud` 配置的磁盘，默认是 `s3`。

用 `Attachment::fromUrl(url)?` 附加绝对 HTTP 或 HTTPS URL。邮件器在发送时下载附件，队列邮件由 Worker 下载。URL 路径提供默认文件名，响应可提供 MIME 类型。用 `r#as` 重命名，用 `withMime` 覆盖类型。

### 内嵌附件 {#inline-attachments}

从 `Mailable::inline_attachments` 返回 `InlineAttachment`，用 `cid()` 取得图片地址。视图和附件使用相同的 Content-ID。

```rust
let logo = InlineAttachment::new("logo@example.test", Attachment {
    name: "logo.png".into(),
    mime: "image/png".into(),
    bytes: logo_bytes,
});
let image_src = logo.cid();
```

将 `image_src` 传入 Blade 的 `<img src="{{ $image_src }}">`，也可用它作为 Markdown 图片的地址。Worker 随任务收到图片字节。SMTP 等原始 MIME 传输会将图片与 HTML 一起发送，浏览器预览把匹配的图片地址转换成数据 URL。Postmark、Resend 和 Cloudflare 通过 JSON 附件字段传递原文件名和 Content-ID。Mailgun 表单 API 用 Content-ID 作为内嵌分段的文件名，使其与 HTML 引用匹配。Mailgun 原始 MIME 模式保留原文件名。

### 可附加对象 {#attachable-objects}

领域对象负责选择附件时，实现 `Attachable`。在邮件类型返回附件来源时，用 `toMailAttachment()` 转换对象。

```rust
use openrust::{
    contracts::mail::Attachable,
    mail::{MailResult, mailables::{Attachment, AttachmentSource}},
};

struct Report(String);

impl Attachable for Report {
    #[allow(non_snake_case)]
    fn toMailAttachment(&self) -> MailResult<AttachmentSource> {
        Ok(Attachment::fromUrl(&self.0)?.r#as("report.pdf"))
    }
}
```

### 标签和元数据 {#tags-and-metadata}

设置 `Envelope::tags` 和 `Envelope::metadata` 来分组和追踪邮件。两个字段都会随队列邮件保存。

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

测试中可用 `assertHasTag` 和 `assertHasMetadata` 核对声明。SMTP 和原始 MIME 使用 `X-Tag` 和 `X-Metadata-*` 头。Mailgun 的结构化 API 把标签映射为 `o:tag`，元数据映射为 `v:*`。Postmark 接受一个标签和一组元数据。SES 和 SES v2 将信封元数据追加到配置的供应商标签。Resend 将标签作为名称与值发送，元数据保留在邮件头中。Cloudflare 在邮件头中保存元数据和最后一个标签。

### 定制邮件消息 {#customizing-the-message}

在服务提供者启动时向应用的 `MailManager` 注册邮件定制回调，并在 `Envelope::using` 按执行顺序列出。回调收到 `MessageDraft`，其中包含信封、邮件头及已渲染的文本和 HTML。回调在读取声明的附件和触发投递事件之前执行，也可以向草稿添加附件。

```rust
manager.registerMessageCustomizer("shipment.tracking", |draft, data| {
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

队列邮件保存回调名称和 JSON 数据。Worker 应用注册同名回调，并在每次投递尝试时执行。回调缺失或返回错误时，本次尝试停止。邮件预览使用原始内容。

## 配置

`config/mail.rs` 定义默认邮件器、命名邮件器和发件人。骨架默认使用 `log` 驱动，在 debug 级别写入 MIME 邮件。`array` 驱动可通过 `Mailer::sent` 检查当前应用的邮件。

使用 SMTP 时设置 `MAIL_MAILER=smtp`，再配置 `MAIL_HOST`、`MAIL_PORT` 或 `MAIL_URL`。URL 覆盖其提供的连接字段。`smtps` 使用隐式 TLS。`smtp` 在服务器支持时使用 STARTTLS，配置凭据时必须成功升级 TLS。

选择 `sendmail` 可运行 `MAIL_SENDMAIL_PATH` 配置的程序，支持通过标准输入输出进行 SMTP 的 `-bs` 模式，以及从标准输入投递邮件的 `-t` 模式。

选择 `ses`、`ses-v2`、`postmark`、`resend`、`mailgun` 或 `cloudflare` 通过供应商投递。先在 `config/services.rs` 配置对应服务，再用 `MAIL_MAILER` 选择邮件器。

- SES 和 SES v2 使用 AWS 凭据或 AWS SDK 凭据链。SES v2 支持 `ConfigurationSetName`、`EmailTags` 和 `ListManagementOptions` 等邮件器选项。
- Postmark 使用 `POSTMARK_TOKEN`，Resend 使用 `RESEND_KEY`。
- Mailgun 使用 `MAILGUN_DOMAIN` 和 `MAILGUN_SECRET`。使用 EU API 时设置 `MAILGUN_ENDPOINT=api.eu.mailgun.net`。默认 `https` 方案提交已组装的 MIME 邮件。若要提交 multipart 字段，将 `scheme` 设为 `api`。
- Cloudflare 使用 `CLOUDFLARE_ACCOUNT_ID` 和 `CLOUDFLARE_KEY`。

在 `config/mail.rs` 中，`failover` 在当前邮件器发送失败时切换，`roundrobin` 轮流使用邮件器。两者均通过 `mailers` 指定邮件器列表，并在 `retry_after` 秒内跳过失败的传输。

## Markdown 邮件

将 `Content::markdown` 设为包含 CommonMark 的命名 Blade 视图。邮件器生成 HTML 和纯文本两种版本，并将主题样式内联到 HTML。框架自带默认主题。若要使用应用样式，设置 `MAIL_MARKDOWN_THEME=receipt`，再将 `receipt.css` 放到 `resources/views/vendor/mail/html/themes/`。可通过 `text_view` 提供自己的纯文本版本。

```rust
let content = Content {
    markdown: Some("markdown-receipt".into()),
    with: [("name".into(), serde_json::json!("Sam"))].into_iter().collect(),
    ..Content::default()
};
```

### 生成 Markdown 邮件 {#generating-markdown-mailables}

用 `--markdown` 生成使用邮件组件的 Blade 模板。声明模块并重新构建后即可使用新类型。两个视图选项同时出现时选择 Markdown 模板。

```sh
./artisan make:mail Orders/ShipmentNotice --markdown=mail.orders.shipment
```

### 编写 Markdown 邮件 {#writing-markdown-messages}

用 `<x-mail::message>` 包裹 Markdown 邮件视图。组件使用应用名称和 URL 生成页眉，同时添加页脚。用名为 `subcopy` 的插槽添加补充说明。按钮支持 `url`、`color` 和 `align`。面板正文按 Markdown 渲染。表格组件在 HTML 中渲染 Markdown 表格，生成纯文本时省略该表格。

可用 `<x-mail::header url="https://example.com">`、`<x-mail::footer>` 和 `<x-mail::subcopy>` 分别添加独立的页眉、页脚和补充说明。默认页脚按应用时区显示年份，并按邮件语言查找 `All rights reserved.`。可在对应语言的 JSON 翻译文件中添加这个键。

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

用 `<x-mail::layout>` 包裹整个视图，即可自行组合邮件区块。命名插槽为 `head`、`header`、`subcopy` 和 `footer`。`head` 接收写入 HTML 文档头部的内容，纯文本邮件会省略该插槽。

```blade
<x-mail::layout>
<x-slot:header>
<x-mail::header :url="$url">订单</x-mail::header>
</x-slot:header>

# 订单已就绪

<x-slot:footer>
<x-mail::footer>感谢您的订购。</x-mail::footer>
</x-slot:footer>
</x-mail::layout>
```

### 自定义 Markdown 布局 {#customizing-the-components}

将 HTML、纯文本布局、组件模板和默认样式表发布到 `resources/views/vendor/mail`。

修改 `html/` 或 `text/` 下的 `header.blade.html`、`footer.blade.html` 和 `subcopy.blade.html` 即可定制这些区块。内置 `message` 模板也使用这三份模板。

发布后的默认布局用 `$mail_content` 放置组装好的内容，邮件表格默认宽 570 px 并适应窄屏。使用显式布局时，自定义模板可用 `$slot` 放置正文，再单独安排 `$header`、`$subcopy` 和 `$footer`。

#### 自定义单封邮件的 CSS {#customizing-the-css}

将 `receipt.css` 放到 `resources/views/vendor/mail/html/themes/`，并在邮件的 `Content` 中设定 `theme: Some("receipt".into())`。

```rust
use openrust::mail::mailables::Content;

let content = Content {
    markdown: Some("mail.receipt".into()),
    theme: Some("receipt".into()),
    ..Content::default()
};
```

主题应用于该 Markdown 邮件的 HTML 版本，队列投递也会采用同一设置。未指定主题或提供空字符串时，邮件器采用应用的 Markdown 主题。Worker 从应用的视图和主题目录读取样式表。

### Markdown 扩展与安全编码 {#markdown-extensions-and-encoding}

CommonMark 和表格始终可用。在 `config/mail.rs` 的 `mail.markdown.extensions` 中填写需要启用的扩展名，默认为空。支持 `strikethrough`、`task_lists`、`footnotes` 和 `smart_punctuation`。未知扩展名会导致邮件器返回配置错误。

将 `mail.markdown.secured_encoding` 设为 `true` 后，Markdown 正文中通过 `{{ $value }}` 插入的字符串按字面显示。比如值为 `[View](https://example.com)` 时，邮件会原样显示该文本。模板可以写 `[View]({{ $url }})` 来动态指定链接目的地。HTML 转义和 URL 过滤仍然生效。该选项默认关闭，按应用生效，涵盖自动生成的纯文本邮件。

```sh
./artisan vendor:publish --tag=laravel-mail
```

编辑 `html/layout.blade.html` 和 `text/layout.blade.html` 可添加页眉、页脚或外层内容。保留 `{!! $slot !!}` 作为渲染后正文的位置。修改 `html/themes/default.css` 可更新默认主题。`mail.markdown.paths` 中的相对路径按应用根目录解析，按声明顺序查找，随后使用标准发布目录。只有传入 `--force` 才会覆盖已有文件。

## 邮件入队

让 `Mailable::queue_options` 返回队列选项后，`send` 会将邮件入队。`sendNow` 立即发送。`PendingMail::queue` 接收连接、队列、延迟和可选事务。`after_commit` 将分派延后至事务提交。Worker 用队列中的邮件定义渲染命名视图并发送。用 `./artisan queue:work --stop-when-empty` 消费邮件队列。

## 渲染邮件 {#rendering-mailables}

调用 `mailable.render().await?` 可直接取得正文，不发送或入队。渲染采用该邮件的语言和 Markdown 主题，优先返回 HTML，纯文本邮件返回文本正文。

### 在浏览器预览邮件 {#previewing-mailables-in-the-browser}

在应用路由中调用 `toResponse()`，即可用对应的媒体类型显示 HTML 或纯文本正文。

```rust
use openrust::{
    http::{HttpError, HttpFuture, Request, Response},
    mail::Mailable,
    support::facades::Route,
};

fn preview_order(_: &mut Request) -> HttpFuture<'_, Result<Response, HttpError>> {
    Box::pin(async { OrderShipped.toResponse().await })
}

Route::get("/mail-preview/order", preview_order)?;
```

## 扩展邮件传输和投递事件

在应用服务提供者中用 `MailManager::extend` 注册 Rust 传输工厂，通过邮件器的 `transport` 配置选择它。派发器在调用传输前发出 `MessageSending`，在传输接受邮件后发出 `MessageSent`。监听器返回 `false` 可以取消投递。取消时，`send` 返回 `None`，`sendNow` 返回 `MailError::Cancelled`。

## 测试邮件

可在邮件类型上断言附件声明，无需读取文件或请求存储服务。用 `assertHasAttachmentFromStorageDisk` 核对指定磁盘，用 `assertHasNoAttachments` 核对没有声明普通附件。

```rust
receipt
    .assertHasAttachmentFromStorage("reports/monthly.csv", None, None)
    .assertHasAttachedData(b"total,42", "summary.csv", Some("text/csv"));
```

在测试应用中调用 `Mail::fake()`，可以检查邮件并跳过传输和入队。断言区分即时发送和排队发送。用 `Mailable::kind()` 选择邮件记录，用匹配断言检查信封或队列选项。

```rust
let mail = Mail::fake()?;
Mail::to("ops@example.test")?.sendNow(&receipt).await?;
mail.assertSentMatching(receipt.kind(), |entry| {
    entry.envelope.to.iter().any(|to| to.address == "ops@example.test")
});
mail.assertNothingQueued();
```
