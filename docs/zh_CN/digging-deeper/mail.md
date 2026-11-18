# 邮件

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

在测试应用中调用 `Mail::fake()`，可以检查邮件并跳过传输和入队。断言区分即时发送和排队发送。用 `Mailable::kind()` 选择邮件记录，用匹配断言检查信封或队列选项。

```rust
let mail = Mail::fake()?;
Mail::to("ops@example.test")?.sendNow(&receipt).await?;
mail.assertSentMatching(receipt.kind(), |entry| {
    entry.envelope.to.iter().any(|to| to.address == "ops@example.test")
});
mail.assertNothingQueued();
```
