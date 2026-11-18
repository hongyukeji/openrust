use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

use crate::{
    config::Value,
    support::markdown::safe_url,
    view::{Factory, ViewData},
};

use super::{MailResult, composition};

const DEFAULT_CSS: &str = include_str!("resources/views/html/themes/default.css");
const HTML_LAYOUT: &str = include_str!("resources/views/html/layout.blade.html");
const TEXT_LAYOUT: &str = include_str!("resources/views/text/layout.blade.html");
pub(super) const PUBLISHABLE_VIEWS: &[(&str, &[u8])] = &[
    (
        "resources/views/vendor/mail/html/layout.blade.html",
        HTML_LAYOUT.as_bytes(),
    ),
    (
        "resources/views/vendor/mail/html/themes/default.css",
        DEFAULT_CSS.as_bytes(),
    ),
    (
        "resources/views/vendor/mail/text/layout.blade.html",
        TEXT_LAYOUT.as_bytes(),
    ),
];

/// Renders a named Blade view as a Markdown email in HTML and plain text.
pub struct Markdown {
    theme: String,
    paths: Vec<PathBuf>,
}

impl Markdown {
    pub(crate) fn from_config(config: &Value, base: &Path) -> Self {
        let default_path = base.join("resources/views/vendor/mail");
        let mut paths: Vec<_> = config["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|path| path.as_str().map(|path| base.join(path)))
            .collect();
        if !paths.contains(&default_path) {
            paths.push(default_path);
        }
        Self {
            theme: config["theme"].as_str().unwrap_or("default").to_owned(),
            paths,
        }
    }

    pub fn render(
        &self,
        view: &str,
        data: &ViewData,
        views: &Factory,
    ) -> MailResult<(String, String)> {
        self.render_with_text(view, data, views, None)
    }

    pub(super) fn render_with_text(
        &self,
        view: &str,
        data: &ViewData,
        views: &Factory,
        text: Option<String>,
    ) -> MailResult<(String, String)> {
        let source = views.render(view, data).map_err(composition)?;
        let css = self.css(views, data)?;
        let events: Vec<_> = Parser::new_ext(&source, Options::ENABLE_TABLES)
            .map(|event| match event {
                Event::Html(value) | Event::InlineHtml(value) => Event::Text(value),
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => Event::Start(Tag::Link {
                    link_type,
                    dest_url: safe_url(dest_url),
                    title,
                    id,
                }),
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => Event::Start(Tag::Image {
                    link_type,
                    dest_url: safe_url(dest_url),
                    title,
                    id,
                }),
                event => event,
            })
            .collect();
        let text = match text {
            Some(text) => text,
            None => self
                .render_layout("text", TEXT_LAYOUT, plain_text(&events), views, data)?
                .trim()
                .to_owned(),
        };
        let mut fragment = String::new();
        html::push_html(&mut fragment, events.into_iter());
        let body = self.render_layout("html", HTML_LAYOUT, fragment, views, data)?;
        let html = css_inline::CSSInliner::options()
            .load_remote_stylesheets(false)
            .extra_css(Some(css))
            .build()
            .inline(&body)
            .map_err(|_| composition("Could not apply the Markdown mail stylesheet."))?;
        Ok((html, text))
    }

    fn render_layout(
        &self,
        format: &str,
        default: &str,
        slot: String,
        views: &Factory,
        data: &ViewData,
    ) -> MailResult<String> {
        let mut data = data.clone();
        data.insert("slot".into(), slot.into());
        for path in &self.paths {
            let path = path.join(format).join("layout.blade.html");
            if let Some(source) = load_optional(&path, "Could not read the Markdown mail layout.")?
            {
                return views.renderInline(&source, &data).map_err(composition);
            }
        }
        views.renderInline(default, &data).map_err(composition)
    }

    fn css(&self, views: &Factory, data: &ViewData) -> MailResult<Cow<'static, str>> {
        let view = format!("mail.{}", self.theme);
        if views.exists(&view) {
            return views
                .render(&view, data)
                .map(Cow::Owned)
                .map_err(composition);
        }
        if self.theme.contains("::") {
            return views
                .render(&self.theme, data)
                .map(Cow::Owned)
                .map_err(composition);
        }
        if self.theme.is_empty()
            || !self
                .theme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(composition("Invalid Markdown mail theme name."));
        }
        for path in &self.paths {
            let file = path.join("html/themes").join(format!("{}.css", self.theme));
            if let Some(css) = load_optional(&file, "Could not read the Markdown mail stylesheet.")?
            {
                return Ok(Cow::Owned(css));
            }
        }
        if self.theme == "default" {
            return Ok(Cow::Borrowed(DEFAULT_CSS));
        }
        Err(composition("Markdown mail theme was not found."))
    }
}

fn load_optional(path: &Path, message: &'static str) -> MailResult<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(css) => Ok(Some(css)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(composition(message)),
    }
}

fn plain_text(events: &[Event<'_>]) -> String {
    let mut text = String::new();
    let mut link = None;
    for event in events {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(value),
            Event::SoftBreak => text.push(' '),
            Event::HardBreak | Event::End(TagEnd::Item | TagEnd::TableRow) => text.push('\n'),
            Event::Start(Tag::Item) => text.push_str("- "),
            Event::Start(Tag::Link { dest_url, .. }) => link = Some(dest_url),
            Event::End(TagEnd::Link) => {
                if let Some(url) = link.take().filter(|url| !url.is_empty()) {
                    text.push_str(" (");
                    text.push_str(url);
                    text.push(')');
                }
            }
            Event::End(TagEnd::TableCell) => text.push_str("  "),
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::List(_),
            ) => {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                if !text.ends_with("\n\n") {
                    text.push('\n');
                }
            }
            _ => {}
        }
    }
    text.trim().to_owned()
}
