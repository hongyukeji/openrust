//! 验证 server 应用可通过公开 facade 渲染并组装本地化邮件。

#![allow(clippy::expect_used, reason = "测试夹具的固定配置失败时应立即终止")]

use std::collections::BTreeMap;

use openrust::mail::{EmailTemplateBody, EmailTemplateCatalog};
use openrust::translation::Locale;
use openrust::translation::MessageArgs;
use openrust::translation::MessageCatalog;
use openrust::translation::TranslationConfig;
use openrust::translation::Translator;

const TEXT: &str = "Hello, {{ user }}";
const HTML: &str = "<p>Hello, {{ user }}</p>";

#[test]
fn server_application_can_render_and_build_localized_multipart_mail() {
    let mut messages = MessageCatalog::new();
    messages
        .insert("en", "mail.welcome.subject", "Welcome")
        .expect("insert English subject");
    let i18n = Translator::new(TranslationConfig::default(), messages).expect("i18n service");
    let locale = Locale::new("en").expect("locale");
    let mut templates = EmailTemplateCatalog::new(locale.clone());
    templates
        .insert("en", "welcome/text.txt", TEXT)
        .expect("text template");
    templates
        .insert("en", "welcome/html.html", HTML)
        .expect("HTML template");
    let context = BTreeMap::from([(String::from("user"), String::from("Application"))]);
    let rendered = templates
        .render_email(
            &i18n,
            &locale,
            "mail.welcome.subject",
            &MessageArgs::new(),
            EmailTemplateBody {
                text: "welcome/text.txt",
                html: Some("welcome/html.html"),
            },
            &context,
        )
        .expect("render mail templates");
    let message = rendered
        .message("sender@example.test", "person@example.test")
        .expect("compose message");
    let formatted = String::from_utf8(message.formatted()).expect("format message");
    assert!(formatted.contains("Welcome"));
    assert!(formatted.contains("Hello, Application"));
    assert!(formatted.contains("text/html"));
}
