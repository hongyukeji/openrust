use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

use pulldown_cmark::{Event, Options, Tag, TagEnd};

use crate::{
    contracts::config::Repository,
    view::{Factory, ViewData},
};

use super::{
    MailResult, composition,
    markdown_components::{Format, Render},
};

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
    pub(super) options: Options,
    pub(super) secured_encoding: bool,
    pub(super) app_name: String,
    pub(super) app_url: String,
    pub(super) locale: String,
    pub(super) timezone: chrono_tz::Tz,
}

impl Markdown {
    pub(crate) fn from_config(settings: &dyn Repository, base: &Path) -> MailResult<Self> {
        let config = settings.get("mail.markdown").unwrap_or_default();
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
        let mut options = Options::ENABLE_TABLES;
        if let Some(extensions) = config.get("extensions") {
            let extensions = extensions.as_array().ok_or_else(|| {
                composition("mail.markdown.extensions must be an array of supported names.")
            })?;
            for extension in extensions {
                options |= match extension.as_str() {
                    Some("strikethrough") => Options::ENABLE_STRIKETHROUGH,
                    Some("task_lists") => Options::ENABLE_TASKLISTS,
                    Some("footnotes") => Options::ENABLE_FOOTNOTES,
                    Some("smart_punctuation") => Options::ENABLE_SMART_PUNCTUATION,
                    _ => return Err(composition("Unsupported mail.markdown.extensions entry.")),
                };
            }
        }
        let secured_encoding = config.get("secured_encoding").map_or(Ok(false), |value| {
            value
                .as_bool()
                .ok_or_else(|| composition("mail.markdown.secured_encoding must be a boolean."))
        })?;
        Ok(Self {
            theme: config["theme"].as_str().unwrap_or("default").to_owned(),
            paths,
            options,
            secured_encoding,
            app_name: settings
                .get("app.name")
                .and_then(|name| name.as_str().map(str::to_owned))
                .unwrap_or_else(|| "OpenRust".to_owned()),
            app_url: settings
                .get("app.url")
                .and_then(|url| url.as_str().map(str::to_owned))
                .unwrap_or_else(|| "/".to_owned()),
            locale: settings
                .get("app.locale")
                .and_then(|locale| locale.as_str().map(str::to_owned))
                .unwrap_or_else(|| "en".to_owned()),
            timezone: settings
                .get("app.timezone")
                .and_then(|zone| zone.as_str()?.parse().ok())
                .unwrap_or(chrono_tz::UTC),
        })
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
        let html = self.render_format(view, data, views, Format::Html)?;
        let css = self.css(views, data)?;
        let html = css_inline::CSSInliner::options()
            .load_remote_stylesheets(false)
            .keep_at_rules(true)
            .extra_css(Some(css))
            .build()
            .inline(&html)
            .map_err(|_| composition("Could not apply the Markdown mail stylesheet."))?;
        let text = match text {
            Some(text) => text,
            None => self
                .render_format(view, data, views, Format::Text)?
                .trim()
                .to_owned(),
        };
        Ok((html, text))
    }

    fn render_format(
        &self,
        view: &str,
        data: &ViewData,
        views: &Factory,
        format: Format,
    ) -> MailResult<String> {
        let render = Render::new(self, views, format);
        let escaped_echo = |value: &str, output: &mut String| render.echo(value, output);
        let source = views
            .render_with_components(
                view,
                data,
                &|name, properties, slots| render.component(name, properties, slots),
                self.secured_encoding
                    .then_some(&escaped_echo as &crate::view::EscapedEcho<'_>),
            )
            .map_err(composition)?;
        render.finish(&source, data)
    }

    pub(super) fn render_layout(
        &self,
        format: Format,
        views: &Factory,
        data: &ViewData,
    ) -> MailResult<String> {
        let default = match format {
            Format::Html => HTML_LAYOUT,
            Format::Text => TEXT_LAYOUT,
        };
        self.render_template(format, "layout", default, views, data)
    }

    pub(super) fn render_template(
        &self,
        format: Format,
        name: &str,
        default: &str,
        views: &Factory,
        data: &ViewData,
    ) -> MailResult<String> {
        for path in &self.paths {
            let path = path
                .join(format.directory())
                .join(format!("{name}.blade.html"));
            if let Some(source) =
                load_optional(&path, "Could not read the Markdown mail template.")?
            {
                return views.renderInline(&source, data).map_err(composition);
            }
        }
        views.renderInline(default, data).map_err(composition)
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

pub(super) fn plain_text(events: &[Event<'_>]) -> String {
    let mut text = String::new();
    let mut link = None;
    for event in events {
        match event {
            Event::Text(value) | Event::Code(value) => text.push_str(value),
            Event::SoftBreak => text.push(' '),
            Event::HardBreak | Event::End(TagEnd::Item | TagEnd::TableRow) => text.push('\n'),
            Event::Start(Tag::Item) => text.push_str("- "),
            Event::TaskListMarker(done) => text.push_str(if *done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(label) => {
                text.push_str("[^");
                text.push_str(label);
                text.push(']');
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                text.push_str("[^");
                text.push_str(label);
                text.push_str("]: ");
            }
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
