use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};

/// Treatment of raw HTML in Markdown input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MarkdownHtml {
    #[default]
    Escape,
    Strip,
    Allow,
}

/// Options for rendering Markdown. The default escapes HTML and filters link schemes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MarkdownOptions {
    pub html: MarkdownHtml,
    pub allow_unsafe_links: bool,
}

pub(super) fn render(source: &str, options: MarkdownOptions, inline: bool) -> String {
    let syntax = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM;
    let events = Parser::new_ext(source, syntax).filter_map(|event| {
        if inline
            && matches!(
                event,
                Event::Start(
                    Tag::Paragraph
                        | Tag::Heading { .. }
                        | Tag::BlockQuote(_)
                        | Tag::List(_)
                        | Tag::Item
                ) | Event::End(
                    TagEnd::Paragraph
                        | TagEnd::Heading(_)
                        | TagEnd::BlockQuote(_)
                        | TagEnd::List(_)
                        | TagEnd::Item
                )
            )
        {
            return None;
        }
        match event {
            Event::Html(value) | Event::InlineHtml(value)
                if options.html == MarkdownHtml::Escape =>
            {
                Some(Event::Text(value))
            }
            Event::Html(_) | Event::InlineHtml(_) if options.html == MarkdownHtml::Strip => None,
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) if !options.allow_unsafe_links => Some(Event::Start(Tag::Link {
                link_type,
                dest_url: safe_url(dest_url),
                title,
                id,
            })),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) if !options.allow_unsafe_links => Some(Event::Start(Tag::Image {
                link_type,
                dest_url: safe_url(dest_url),
                title,
                id,
            })),
            event => Some(event),
        }
    });
    let mut output = String::new();
    html::push_html(&mut output, events);
    output
}

pub(crate) fn safe_url(url: CowStr<'_>) -> CowStr<'_> {
    let value = url.trim();
    let prefix = value.split(['/', '?', '#']).next().unwrap_or_default();
    if value.bytes().any(|byte| byte.is_ascii_control())
        || value.contains('\\')
        || prefix.split_once(':').is_some_and(|(scheme, _)| {
            !["https", "http", "mailto"]
                .iter()
                .any(|allowed| scheme.eq_ignore_ascii_case(allowed))
        })
    {
        return "".into();
    }
    url
}
