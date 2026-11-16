use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

use crate::{
    config::Value,
    view::{Factory, ViewData},
};

use super::{MailResult, composition};

const DEFAULT_CSS: &str = "body { color: #1f2937; font-family: Arial, sans-serif; } \
    a { color: #1d4ed8; } blockquote { border-left: 4px solid #d1d5db; padding-left: 1em; } \
    td, th { border: 1px solid #d1d5db; padding: .4em .6em; }";

/// Renders a named Blade view as a Markdown email in HTML and plain text.
pub struct Markdown {
    theme: String,
    paths: Vec<PathBuf>,
}

impl Markdown {
    pub(crate) fn from_config(config: &Value) -> Self {
        Self {
            theme: config["theme"].as_str().unwrap_or("default").to_owned(),
            paths: config["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|path| path.as_str().map(PathBuf::from))
                .collect(),
        }
    }

    pub fn render(
        &self,
        view: &str,
        data: &ViewData,
        views: &Factory,
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
        let text = plain_text(&events);
        let mut body =
            String::from("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>");
        html::push_html(&mut body, events.into_iter());
        body.push_str("</body></html>");
        let html = css_inline::CSSInliner::options()
            .load_remote_stylesheets(false)
            .extra_css(Some(css))
            .build()
            .inline(&body)
            .map_err(|_| composition("Could not apply the Markdown mail stylesheet."))?;
        Ok((html, text))
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
            if let Some(css) = load_css(path, &self.theme)? {
                return Ok(Cow::Owned(css));
            }
        }
        if self.theme == "default" {
            return Ok(Cow::Borrowed(DEFAULT_CSS));
        }
        Err(composition("Markdown mail theme was not found."))
    }
}

fn load_css(path: &Path, theme: &str) -> MailResult<Option<String>> {
    let path = path.join("html/themes").join(format!("{theme}.css"));
    match std::fs::read_to_string(path) {
        Ok(css) => Ok(Some(css)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(composition("Could not read the Markdown mail stylesheet.")),
    }
}

fn safe_url(url: pulldown_cmark::CowStr<'_>) -> pulldown_cmark::CowStr<'_> {
    let value = url.trim();
    let prefix = value.split(['/', '?', '#']).next().unwrap_or_default();
    if prefix.split_once(':').is_some_and(|(scheme, _)| {
        !["https", "http", "mailto"]
            .iter()
            .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
    }) {
        return "".into();
    }
    url
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
