//! 受信任应用邮件模板的注册、渲染与消息构建。

use std::collections::BTreeMap;
use std::io::{self, Write};

use lettre::{
    Message,
    message::{MultiPart, SinglePart},
};
use minijinja::{AutoEscape, Environment, UndefinedBehavior};
use serde::Serialize;

use crate::translation::Locale;
use crate::translation::MessageArgs;
use crate::translation::Translator;

const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SUBJECT_BYTES: usize = 998;
const TEMPLATE_FUEL: u64 = 100_000;

#[derive(Default)]
struct LimitedWriter {
    output: Vec<u8>,
    exceeded: bool,
}

impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.output.len().saturating_add(bytes.len()) > MAX_OUTPUT_BYTES {
            self.exceeded = true;
            return Err(io::Error::other("template output limit exceeded"));
        }
        self.output.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// 邮件模板注册或渲染失败。
#[derive(Debug, thiserror::Error)]
pub enum EmailTemplateError {
    /// 模板名称不是受支持的相对标识符。
    #[error("邮件模板名称无效")]
    InvalidName,
    /// locale 标签无效。
    #[error("邮件模板 locale 无效")]
    InvalidLocale,
    /// 同一 locale 下已注册该模板。
    #[error("邮件模板重复注册")]
    Duplicate,
    /// 请求的模板不存在。
    #[error("邮件模板不存在")]
    Missing,
    /// 模板语法、变量或执行预算错误。
    #[error("邮件模板渲染失败")]
    Render,
    /// 渲染结果或主题超过限制，或主题包含换行。
    #[error("邮件模板输出超出限制或主题格式无效")]
    InvalidOutput,
}

/// 邮件正文模板的纯文本与可选 HTML 注册名。
#[derive(Debug, Clone, Copy)]
pub struct EmailTemplateBody<'a> {
    /// 纯文本模板注册名。
    pub text: &'a str,
    /// 可选 HTML 模板注册名。
    pub html: Option<&'a str>,
}

/// 仅接收应用编译时嵌入的模板，不读取文件或网络。
#[derive(Debug, Clone)]
pub struct EmailTemplateCatalog {
    fallback_locale: Locale,
    templates: BTreeMap<String, BTreeMap<String, &'static str>>,
}

impl EmailTemplateCatalog {
    /// 创建目录，并指定模板资源缺失时使用的 locale。
    pub fn new(fallback_locale: Locale) -> Self {
        Self {
            fallback_locale,
            templates: BTreeMap::new(),
        }
    }

    /// 注册编译期嵌入的模板源；名称如 `welcome/html.html`，不能包含路径跳转。
    pub fn insert(
        &mut self,
        locale: impl AsRef<str>,
        name: impl Into<String>,
        source: &'static str,
    ) -> Result<(), EmailTemplateError> {
        let locale = Locale::new(locale.as_ref()).map_err(|_| EmailTemplateError::InvalidLocale)?;
        let name = name.into();
        if !valid_name(&name) {
            return Err(EmailTemplateError::InvalidName);
        }
        let templates = self.templates.entry(locale.to_string()).or_default();
        if templates.contains_key(&name) {
            return Err(EmailTemplateError::Duplicate);
        }
        templates.insert(name, source);
        Ok(())
    }

    /// 按显式 locale 渲染主题、纯文本正文及可选 HTML 正文。
    pub fn render_email<T: Serialize>(
        &self,
        i18n: &Translator,
        locale: &Locale,
        subject_key: &str,
        subject_args: &MessageArgs,
        body: EmailTemplateBody<'_>,
        context: &T,
    ) -> Result<RenderedEmail, EmailTemplateError> {
        let subject = i18n.translate(locale, subject_key, subject_args);
        if subject.len() > MAX_SUBJECT_BYTES || subject.contains(['\r', '\n']) {
            return Err(EmailTemplateError::InvalidOutput);
        }
        let text = self.render(locale, body.text, context)?;
        let html = body
            .html
            .map(|name| self.render(locale, name, context))
            .transpose()?;
        Ok(RenderedEmail {
            subject,
            text,
            html,
        })
    }

    fn render<T: Serialize>(
        &self,
        locale: &Locale,
        name: &str,
        context: &T,
    ) -> Result<String, EmailTemplateError> {
        if !valid_name(name) {
            return Err(EmailTemplateError::InvalidName);
        }
        let mut environment = Environment::new();
        environment.set_undefined_behavior(UndefinedBehavior::Strict);
        environment.set_auto_escape_callback(|name| {
            if std::path::Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("html"))
            {
                AutoEscape::Html
            } else {
                AutoEscape::None
            }
        });
        environment.set_fuel(Some(TEMPLATE_FUEL));
        for templates in self.locale_templates(locale) {
            for (template_name, source) in templates {
                environment
                    .add_template(template_name, source)
                    .map_err(|_| EmailTemplateError::Render)?;
            }
        }
        let template = environment
            .get_template(name)
            .map_err(|_| EmailTemplateError::Missing)?;
        let mut writer = LimitedWriter::default();
        let result = template.render_captured_to(context, &mut writer);
        if writer.exceeded {
            return Err(EmailTemplateError::InvalidOutput);
        }
        result.map_err(|_| EmailTemplateError::Render)?;
        String::from_utf8(writer.output).map_err(|_| EmailTemplateError::Render)
    }

    fn locale_templates(&self, locale: &Locale) -> Vec<&BTreeMap<String, &'static str>> {
        let mut locales = Vec::new();
        if let Some(exact) = self.templates.get(locale.as_str()) {
            locales.push(exact);
        }
        let language = locale.as_str().split('-').next().unwrap_or(locale.as_str());
        let language_prefix = format!("{language}-");
        if let Some((tag, templates)) = self
            .templates
            .iter()
            .find(|(tag, _)| tag.as_str() == language || tag.starts_with(&language_prefix))
            && tag != locale.as_str()
        {
            locales.push(templates);
        }
        if let Some(fallback) = self.templates.get(self.fallback_locale.as_str())
            && !locales.contains(&fallback)
        {
            locales.push(fallback);
        }
        locales.reverse();
        locales
    }
}

/// 已渲染邮件，可直接构造 multipart 消息并继续添加附件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEmail {
    subject: String,
    text: String,
    html: Option<String>,
}

impl RenderedEmail {
    /// 返回邮件主题。
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// 返回纯文本正文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 返回可选 HTML 正文。
    pub fn html(&self) -> Option<&str> {
        self.html.as_deref()
    }

    /// 创建正文 multipart；调用方可将其作为附件混合消息的一个 part。
    pub fn multipart(&self) -> MultiPart {
        let text = SinglePart::plain(self.text.clone());
        match &self.html {
            Some(html) => MultiPart::alternative()
                .singlepart(text)
                .singlepart(SinglePart::html(html.clone())),
            None => MultiPart::alternative().singlepart(text),
        }
    }

    /// 构建含纯文本及可选 HTML alternative 的邮件。
    pub fn message(&self, from: &str, to: &str) -> Result<Message, EmailTemplateError> {
        let from = from
            .parse()
            .map_err(|_| EmailTemplateError::InvalidOutput)?;
        let to = to.parse().map_err(|_| EmailTemplateError::InvalidOutput)?;
        Message::builder()
            .from(from)
            .to(to)
            .subject(&self.subject)
            .multipart(self.multipart())
            .map_err(|_| EmailTemplateError::InvalidOutput)
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('/')
        && !name.contains('\\')
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::items_after_statements,
        reason = "固定模板和 locale 测试夹具必须在设置错误时立即失败"
    )]

    use serde::Serialize;

    use super::*;
    use crate::translation::MessageCatalog;
    use crate::translation::TranslationConfig;

    const TEXT: &str = "您好，{{ user }}";
    const HTML: &str = "<p>您好，{{ user }}</p>";
    const EN_TEXT: &str = "Hello, {{ user }}";
    const EN_HTML: &str = "<p>Hello, {{ user }}</p>";
    const LAYOUT: &str = "<main>{% block body %}{% endblock %}</main>";
    const EXTENDS: &str =
        "{% extends \"welcome/layout.html\" %}{% block body %}<p>{{ user }}</p>{% endblock %}";
    const UNDEFINED: &str = "{{ absent }}";

    #[derive(Serialize)]
    struct Context<'a> {
        user: &'a str,
    }

    struct Built(lettre::Message);

    impl super::super::Email for Built {
        fn message(&self) -> crate::core::Result<lettre::Message> {
            Ok(self.0.clone())
        }
    }

    fn i18n() -> Translator {
        let mut catalog = MessageCatalog::new();
        catalog
            .insert("zh-CN", "subject", "问候")
            .expect("catalog entry");
        catalog
            .insert("en", "subject", "Greeting")
            .expect("catalog entry");
        Translator::new(TranslationConfig::default(), catalog).expect("i18n")
    }

    fn catalog() -> EmailTemplateCatalog {
        let mut catalog = EmailTemplateCatalog::new(Locale::new("en").expect("locale"));
        catalog
            .insert("en", "welcome/text.txt", EN_TEXT)
            .expect("English text");
        catalog
            .insert("en", "welcome/html.html", EN_HTML)
            .expect("English HTML");
        catalog
            .insert("zh-CN", "welcome/text.txt", TEXT)
            .expect("Chinese text");
        catalog
            .insert("zh-CN", "welcome/html.html", HTML)
            .expect("Chinese HTML");
        catalog
    }

    #[test]
    fn renders_locale_html_escaped_and_rejects_missing_variables() {
        let rendered = catalog()
            .render_email(
                &i18n(),
                &Locale::new("zh-CN").expect("locale"),
                "subject",
                &MessageArgs::new(),
                EmailTemplateBody {
                    text: "welcome/text.txt",
                    html: Some("welcome/html.html"),
                },
                &Context { user: "<script>" },
            )
            .expect("render");
        assert!(rendered.text().contains("<script>"));
        assert!(rendered.html().expect("HTML").contains("&lt;script&gt;"));

        let mut with_layout = catalog();
        with_layout
            .insert("en", "welcome/layout.html", LAYOUT)
            .expect("layout");
        with_layout
            .insert("en", "welcome/extended.html", EXTENDS)
            .expect("child template");
        assert!(
            with_layout
                .render(
                    &Locale::new("en").expect("locale"),
                    "welcome/extended.html",
                    &Context { user: "A" }
                )
                .expect("inheritance")
                .contains("<main>")
        );
        with_layout
            .insert("en", "welcome/undefined.txt", UNDEFINED)
            .expect("undefined template");
        assert!(matches!(
            with_layout.render(
                &Locale::new("en").expect("locale"),
                "welcome/undefined.txt",
                &Context { user: "A" }
            ),
            Err(EmailTemplateError::Render)
        ));

        let oversized = "x".repeat(MAX_OUTPUT_BYTES + 1);
        assert!(matches!(
            catalog().render(
                &Locale::new("en").expect("locale"),
                "welcome/text.txt",
                &Context { user: &oversized },
            ),
            Err(EmailTemplateError::InvalidOutput)
        ));

        assert!(
            catalog()
                .render_email(
                    &i18n(),
                    &Locale::new("en").expect("locale"),
                    "subject",
                    &MessageArgs::new(),
                    EmailTemplateBody {
                        text: "missing.txt",
                        html: None
                    },
                    &Context { user: "x" },
                )
                .is_err()
        );
    }

    #[test]
    fn validates_closed_template_names_and_duplicate_registration() {
        let mut catalog = catalog();
        assert!(matches!(
            catalog.insert("en", "../secret", "x"),
            Err(EmailTemplateError::InvalidName)
        ));
        assert!(matches!(
            catalog.insert("en", "welcome/text.txt", "x"),
            Err(EmailTemplateError::Duplicate)
        ));
        assert!(
            catalog
                .render_email(
                    &i18n(),
                    &Locale::new("fr").expect("locale"),
                    "subject",
                    &MessageArgs::new(),
                    EmailTemplateBody {
                        text: "welcome/text.txt",
                        html: None
                    },
                    &Context { user: "x" },
                )
                .is_ok()
        );
    }

    #[test]
    fn message_uses_alternative_mime_and_keeps_attachment_composition_available() {
        let rendered = catalog()
            .render_email(
                &i18n(),
                &Locale::new("en").expect("locale"),
                "subject",
                &MessageArgs::new(),
                EmailTemplateBody {
                    text: "welcome/text.txt",
                    html: Some("welcome/html.html"),
                },
                &Context { user: "x" },
            )
            .expect("render");
        let message = rendered
            .message("sender@example.com", "person@example.com")
            .expect("message");
        assert_eq!(message.envelope().to().len(), 1);
        let encoded = String::from_utf8(message.formatted()).expect("formatted message");
        assert!(encoded.contains("text/html"));
        let with_attachment = Message::builder()
            .from("sender@example.com".parse().expect("sender"))
            .to("person@example.com".parse().expect("recipient"))
            .subject(rendered.subject())
            .multipart(
                MultiPart::mixed()
                    .multipart(rendered.multipart())
                    .singlepart(SinglePart::builder().body(String::from("attachment"))),
            )
            .expect("multipart attachment message");
        let encoded = String::from_utf8(with_attachment.formatted()).expect("formatted multipart");
        assert!(encoded.contains("attachment"));
    }

    #[cfg(feature = "mail-smtp")]
    #[tokio::test]
    #[ignore = "需要已启动的 Mailpit；设置 OPENRUST_TEST_MAILPIT_SMTP_PORT 后显式运行"]
    async fn smtp_submits_rendered_template_to_mailpit() -> crate::core::Result<()> {
        let config = super::super::MailConfig {
            driver: "smtp".into(),
            smtp_host: Some("127.0.0.1".into()),
            smtp_port: std::env::var("OPENRUST_TEST_MAILPIT_SMTP_PORT")
                .ok()
                .and_then(|port| port.parse().ok())
                .unwrap_or(1025),
            smtp_starttls: false,
            ..super::super::MailConfig::default()
        };
        let catalog = catalog();
        let email = catalog
            .render_email(
                &i18n(),
                &Locale::new("en").expect("locale"),
                "subject",
                &MessageArgs::new(),
                EmailTemplateBody {
                    text: "welcome/text.txt",
                    html: Some("welcome/html.html"),
                },
                &Context { user: "Mailpit" },
            )
            .map_err(crate::Error::internal)?;
        super::super::configured_mailer(&config)?
            .send(&Built(
                email
                    .message("sender@example.com", "recipient@example.com")
                    .map_err(crate::Error::internal)?,
            ))
            .await
    }

    #[cfg(feature = "test-util")]
    #[tokio::test]
    async fn rendered_template_is_accepted_by_the_public_fake_mailer() -> crate::core::Result<()> {
        let rendered = catalog()
            .render_email(
                &i18n(),
                &Locale::new("en").expect("locale"),
                "subject",
                &MessageArgs::new(),
                EmailTemplateBody {
                    text: "welcome/text.txt",
                    html: Some("welcome/html.html"),
                },
                &Context { user: "Fake" },
            )
            .map_err(crate::Error::internal)?;
        let email = Built(
            rendered
                .message("sender@example.com", "recipient@example.com")
                .map_err(crate::Error::internal)?,
        );
        let (mailer, fake) = super::super::FakeMailer::new();
        mailer.send(&email).await?;
        assert_eq!(fake.sent_count(), 1);
        fake.assert_sent_to("recipient@example.com")?;
        Ok(())
    }
}
