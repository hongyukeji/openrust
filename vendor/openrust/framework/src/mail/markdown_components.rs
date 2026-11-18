use std::{cell::RefCell, collections::HashMap, time::SystemTime};

use chrono::{DateTime, Datelike, Utc};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html};

use crate::{
    support::markdown::safe_url,
    view::{Factory, ViewData, ViewError, ViewResult},
};

use super::{
    MailResult, composition,
    markdown::{Markdown, plain_text},
};

#[derive(Clone, Copy)]
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

const COMPONENTS: &[Component] = &[
    component!("message"),
    component!("button"),
    component!("panel"),
    component!("table"),
];

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
}

impl<'a> Render<'a> {
    pub(super) fn new(markdown: &'a Markdown, views: &'a Factory, format: Format) -> Self {
        Self {
            markdown,
            views,
            format,
            nonce: uuid::Uuid::now_v7(),
            fragments: RefCell::default(),
        }
    }

    pub(super) fn component(
        &self,
        name: &str,
        properties: &ViewData,
        slots: &ViewData,
    ) -> Option<ViewResult<String>> {
        let name = name.strip_prefix("mail::")?;
        let component = COMPONENTS.iter().find(|component| component.name == name)?;
        Some(
            self.render_component(component, properties, slots)
                .map_err(|error| ViewError::RenderFailed(Box::new(error))),
        )
    }

    fn render_component(
        &self,
        component: &Component,
        properties: &ViewData,
        slots: &ViewData,
    ) -> MailResult<String> {
        let slot = slots
            .get("slot")
            .and_then(|slot| slot.as_str())
            .unwrap_or_default();
        let mut data = properties.clone();
        data.insert(
            "slot".into(),
            self.markdown(slot, component.name == "button").into(),
        );
        match component.name {
            "button" => {
                let url = properties
                    .get("url")
                    .and_then(|url| url.as_str())
                    .ok_or_else(|| composition("The mail button requires a URL."))?;
                data.insert("url".into(), safe_url(url.into()).to_string().into());
                for (key, default) in [("color", "primary"), ("align", "center")] {
                    data.entry(key).or_insert_with(|| default.into());
                }
            }
            "message" => {
                let year = DateTime::<Utc>::from(SystemTime::now()).year();
                let subcopy = slots
                    .get("subcopy")
                    .and_then(|slot| slot.as_str())
                    .unwrap_or_default();
                data.insert("subcopy".into(), self.markdown(subcopy, false).into());
                data.insert("app_name".into(), self.markdown.app_name.as_str().into());
                data.insert(
                    "app_url".into(),
                    safe_url(self.markdown.app_url.as_str().into())
                        .to_string()
                        .into(),
                );
                data.insert("year".into(), year.into());
            }
            _ => {}
        }
        let default = match self.format {
            Format::Html => component.html,
            Format::Text => component.text,
        };
        let rendered = self.markdown.render_template(
            self.format,
            component.name,
            default,
            self.views,
            &data,
        )?;
        let mut fragments = self.fragments.borrow_mut();
        let marker = format!("<!--openrust-mail-{}-{}-->", self.nonce, fragments.len());
        fragments.insert(marker.clone(), rendered);
        Ok(format!("\n\n{marker}\n\n"))
    }

    pub(super) fn markdown(&self, source: &str, inline: bool) -> String {
        let fragments = self.fragments.borrow();
        let events = Parser::new_ext(source, Options::ENABLE_TABLES).filter_map(|event| {
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
                        None => Event::Text(value),
                    }
                }
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
