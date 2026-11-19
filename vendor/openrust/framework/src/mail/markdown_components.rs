use std::fmt::Write as _;
use std::{cell::RefCell, collections::HashMap};

use chrono::Datelike;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Parser, Tag, TagEnd, html};

use crate::{
    support::{Date, escape_html, facades::Lang, markdown::safe_url},
    translation::TranslationError,
    view::{Factory, ViewData, ViewError, ViewResult},
};

use super::{
    MailResult, composition,
    markdown::{Markdown, plain_text},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    Html,
    Text,
}

impl Format {
    pub(super) fn directory(self) -> &'static str {
        match self {
            Self::Html => "html",
            Self::Text => "text",
        }
    }
}

struct Component {
    name: &'static str,
    html: &'static str,
    text: &'static str,
}

macro_rules! component {
    ($name:literal) => {
        Component {
            name: $name,
            html: include_str!(concat!("resources/views/html/", $name, ".blade.html")),
            text: include_str!(concat!("resources/views/text/", $name, ".blade.html")),
        }
    };
}

const HEADER: Component = component!("header");
const FOOTER: Component = component!("footer");
const SUBCOPY: Component = component!("subcopy");
const MESSAGE: Component = component!("message");

const COMPONENTS: &[Component] = &[
    MESSAGE,
    component!("button"),
    component!("panel"),
    component!("table"),
    HEADER,
    FOOTER,
    SUBCOPY,
];

const COPYRIGHT: &str = "All rights reserved.";

pub(super) fn publishable() -> impl Iterator<Item = (String, &'static [u8])> {
    COMPONENTS.iter().flat_map(|component| {
        [
            (Format::Html, component.html),
            (Format::Text, component.text),
        ]
        .into_iter()
        .map(|(format, template)| {
            let directory = format.directory();
            let name = component.name;
            (
                format!("resources/views/vendor/mail/{directory}/{name}.blade.html"),
                template.as_bytes(),
            )
        })
    })
}

pub(super) struct Render<'a> {
    markdown: &'a Markdown,
    views: &'a Factory,
    format: Format,
    nonce: uuid::Uuid,
    fragments: RefCell<HashMap<String, String>>,
    document: RefCell<Option<String>>,
    echo_prefix: String,
    echoes: RefCell<Vec<String>>,
}

impl<'a> Render<'a> {
    pub(super) fn new(markdown: &'a Markdown, views: &'a Factory, format: Format) -> Self {
        let nonce = uuid::Uuid::now_v7();
        Self {
            markdown,
            views,
            format,
            nonce,
            fragments: RefCell::default(),
            document: RefCell::default(),
            echo_prefix: format!("orecho{}z", nonce.simple()),
            echoes: RefCell::default(),
        }
    }

    pub(super) fn component(
        &self,
        name: &str,
        properties: &ViewData,
        slots: &ViewData,
    ) -> Option<ViewResult<String>> {
        let name = name.strip_prefix("mail::")?;
        if name == "layout" {
            return Some(
                self.explicit_layout(properties, slots)
                    .map_err(|error| ViewError::RenderFailed(Box::new(error))),
            );
        }
        let component = COMPONENTS.iter().find(|component| component.name == name)?;
        Some(
            self.render_component(component, properties, slots)
                .map_err(|error| ViewError::RenderFailed(Box::new(error))),
        )
    }

    pub(super) fn finish(&self, source: &str, data: &ViewData) -> MailResult<String> {
        if let Some(document) = self.document.borrow_mut().take() {
            if source.trim() != self.document_marker() {
                return Err(composition("The mail layout must wrap the entire view."));
            }
            return Ok(document);
        }
        let mut data = data.clone();
        let slot = self.markdown(source, false);
        data.insert("slot".into(), slot.as_str().into());
        data.insert("mail_content".into(), slot.into());
        data.shift_remove("mail_head");
        self.layout(data)
    }

    fn document_marker(&self) -> String {
        format!("<!--openrust-mail-{}-layout-->", self.nonce)
    }

    fn explicit_layout(&self, properties: &ViewData, slots: &ViewData) -> MailResult<String> {
        if self.document.borrow().is_some() {
            return Err(composition("A Markdown mail view accepts one layout."));
        }
        let mut data = properties.clone();
        for name in ["header", "slot", "subcopy", "footer"] {
            data.insert(name.into(), self.markdown(slot(slots, name), false).into());
        }
        data.insert("rendered_subcopy".into(), data["subcopy"].clone());
        if self.format == Format::Html {
            // Blade has already evaluated the head slot with its normal echo escaping.
            let head = self.restore(slot(slots, "head").into(), true);
            data.insert("head".into(), head.as_ref().into());
            data.insert("mail_head".into(), head.as_ref().into());
        }
        let content = self
            .views
            .renderInline(Self::builtin(&MESSAGE, self.format), &data)
            .map_err(composition)?;
        data.insert("mail_content".into(), content.into());
        self.document.replace(Some(self.layout(data)?));
        Ok(format!("\n\n{}\n\n", self.document_marker()))
    }

    fn layout(&self, mut data: ViewData) -> MailResult<String> {
        let locale = Lang::getLocale().unwrap_or_else(|_| self.markdown.locale.clone());
        data.insert("mail_locale".into(), locale.replace('_', "-").into());
        data.insert("mail_title".into(), self.markdown.app_name.as_str().into());
        self.markdown.render_layout(self.format, self.views, &data)
    }

    fn render_component(
        &self,
        component: &Component,
        properties: &ViewData,
        slots: &ViewData,
    ) -> MailResult<String> {
        let slot = slot(slots, "slot");
        let mut data = properties.clone();
        data.insert(
            "slot".into(),
            self.markdown(slot, matches!(component.name, "button" | "header"))
                .into(),
        );
        match component.name {
            "button" | "header" => {
                let url = properties
                    .get("url")
                    .and_then(|url| url.as_str())
                    .ok_or_else(|| {
                        composition(format!("The mail {} requires a URL.", component.name))
                    })?;
                data.insert("url".into(), safe_url(url.into()).to_string().into());
                if component.name == "button" {
                    for (key, default) in [("color", "primary"), ("align", "center")] {
                        data.entry(key.to_owned()).or_insert_with(|| default.into());
                    }
                }
            }
            "message" => self.message_slots(&mut data, slots)?,
            _ => {}
        }
        let rendered = self.template(component, &data)?;
        let mut fragments = self.fragments.borrow_mut();
        let marker = format!("<!--openrust-mail-{}-{}-->", self.nonce, fragments.len());
        fragments.insert(marker.clone(), rendered);
        Ok(format!("\n\n{marker}\n\n"))
    }

    fn message_slots(&self, data: &mut ViewData, slots: &ViewData) -> MailResult<()> {
        let app_name = self.markdown.app_name.as_str();
        let app_url = safe_url(self.markdown.app_url.as_str().into()).to_string();
        let year = Date::now(Some(self.markdown.timezone))
            .map_err(|error| composition(error.to_string()))?
            .asDateTime()
            .year();
        let copyright = match Lang::getWith(COPYRIGHT, &[], None) {
            Ok(line) => line,
            Err(TranslationError::Unavailable(_)) => COPYRIGHT.into(),
            Err(error) => return Err(composition(error)),
        };
        let footer = format!("© {year} {app_name}. {copyright}");
        let mut header_data = self.literal_slot(app_name);
        header_data.insert("url".into(), app_url.as_str().into());
        let header = self.template(&HEADER, &header_data)?;
        let footer = self.template(&FOOTER, &self.literal_slot(&footer))?;
        let subcopy = slots
            .get("subcopy")
            .and_then(|slot| slot.as_str())
            .map(|slot| self.markdown(slot, false))
            .unwrap_or_default();
        let rendered_subcopy = if subcopy.is_empty() {
            String::new()
        } else {
            self.template(
                &SUBCOPY,
                &std::iter::once(("slot".into(), subcopy.as_str().into())).collect(),
            )?
        };
        data.extend([
            ("app_name".into(), app_name.into()),
            ("app_url".into(), app_url.into()),
            ("year".into(), year.into()),
            ("header".into(), header.into()),
            ("footer".into(), footer.into()),
            ("subcopy".into(), subcopy.into()),
            ("rendered_subcopy".into(), rendered_subcopy.into()),
        ]);
        Ok(())
    }

    fn literal_slot(&self, content: &str) -> ViewData {
        let mut slot = String::new();
        if self.format == Format::Html {
            escape_html(&mut slot, content);
        } else {
            slot.push_str(content);
        }
        std::iter::once(("slot".into(), slot.into())).collect()
    }

    fn template(&self, component: &Component, data: &ViewData) -> MailResult<String> {
        self.markdown.render_template(
            self.format,
            component.name,
            Self::builtin(component, self.format),
            self.views,
            data,
        )
    }

    fn builtin(component: &Component, format: Format) -> &'static str {
        match format {
            Format::Html => component.html,
            Format::Text => component.text,
        }
    }

    pub(super) fn echo(&self, value: &str, output: &mut String) {
        if !value.is_empty() {
            let mut echoes = self.echoes.borrow_mut();
            write!(output, "{}{}z", self.echo_prefix, echoes.len()).unwrap();
            echoes.push(value.to_owned());
        }
    }

    // CommonMark parses a placeholder as ordinary text. Replacing only in its
    // output events keeps values from introducing syntax or bypassing URL checks.
    fn restore<'source>(&self, value: CowStr<'source>, escape: bool) -> CowStr<'source> {
        if !value.contains(&self.echo_prefix) {
            return value;
        }
        let echoes = self.echoes.borrow();
        let mut result = String::with_capacity(value.len());
        let mut fields = value.split(&self.echo_prefix);
        result.push_str(fields.next().unwrap_or_default());
        for field in fields {
            let echo = field.split_once('z').and_then(|(index, rest)| {
                let index: usize = index.parse().ok()?;
                Some((echoes.get(index)?, rest))
            });
            if let Some((text, rest)) = echo {
                if escape {
                    escape_html(&mut result, text);
                } else {
                    result.push_str(text);
                }
                result.push_str(rest);
            } else {
                result.push_str(&self.echo_prefix);
                result.push_str(field);
            }
        }
        result.into()
    }

    pub(super) fn markdown(&self, source: &str, inline: bool) -> String {
        let fragments = self.fragments.borrow();
        let events = Parser::new_ext(source, self.markdown.options).filter_map(|event| {
            if inline
                && matches!(
                    event,
                    Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph)
                )
            {
                return None;
            }
            Some(match event {
                // Only markers issued by this render tree can introduce HTML.
                Event::Html(value) | Event::InlineHtml(value) => {
                    match fragments.get(value.trim()) {
                        Some(fragment) => match self.format {
                            Format::Html => Event::Html(fragment.as_str().into()),
                            Format::Text => Event::Text(format!("{}\n\n", fragment.trim()).into()),
                        },
                        None => Event::Text(self.restore(value, false)),
                    }
                }
                Event::Text(value) => Event::Text(self.restore(value, false)),
                Event::Code(value) => Event::Code(self.restore(value, false)),
                Event::FootnoteReference(value) => {
                    Event::FootnoteReference(self.restore(value, false))
                }
                Event::Start(Tag::FootnoteDefinition(value)) => {
                    Event::Start(Tag::FootnoteDefinition(self.restore(value, false)))
                }
                Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => Event::Start(
                    Tag::CodeBlock(CodeBlockKind::Fenced(self.restore(info, false))),
                ),
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => Event::Start(Tag::Link {
                    link_type,
                    dest_url: safe_url(self.restore(dest_url, false)),
                    title: self.restore(title, false),
                    id,
                }),
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => {
                    let dest_url = self.restore(dest_url, false);
                    Event::Start(Tag::Image {
                        link_type,
                        dest_url: if super::inline_attachment::cid(&dest_url).is_some() {
                            dest_url
                        } else {
                            safe_url(dest_url)
                        },
                        title: self.restore(title, false),
                        id,
                    })
                }
                event => event,
            })
        });
        match self.format {
            Format::Html => {
                let mut html = String::new();
                html::push_html(&mut html, events);
                html
            }
            Format::Text => plain_text(&events.collect::<Vec<_>>()),
        }
    }
}

fn slot<'a>(slots: &'a ViewData, name: &str) -> &'a str {
    slots
        .get(name)
        .and_then(|value| value.as_str())
        .unwrap_or_default()
}
