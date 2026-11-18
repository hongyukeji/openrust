use std::{
    collections::{HashMap, HashSet},
    sync::LazyLock,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use lettre::message::header::ContentType;
use regex::Regex;

use super::{MailResult, composition, mailables::InlineAttachment};

static TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<!--.*?(?:-->|$)|<(/?)([a-z][a-z0-9:-]*)([\x09-\x0d /](?:[^>"']|"[^"]*"|'[^']*')*)?>"#)
        .expect("HTML mail tag expression")
});

static ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=`<>]+)))?"#)
        .expect("HTML mail attribute expression")
});

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@' | b'+')
        })
}

pub(super) fn cid(url: &str) -> Option<&str> {
    url.get(..4)
        .filter(|scheme| scheme.eq_ignore_ascii_case("cid:"))?;
    let id = &url[4..];
    valid_id(id).then_some(id)
}

pub(super) fn types(attachments: &[InlineAttachment]) -> MailResult<Vec<ContentType>> {
    let mut seen = HashSet::new();
    attachments
        .iter()
        .map(|part| {
            if !valid_id(&part.content_id) || !seen.insert(part.content_id.as_str()) {
                return Err(composition(
                    "Invalid or duplicate inline attachment Content-ID.",
                ));
            }
            ContentType::parse(&part.attachment.mime)
                .map_err(|_| composition("Invalid inline attachment media type."))
        })
        .collect()
}

pub(super) fn preview(html: String, attachments: &[InlineAttachment]) -> MailResult<String> {
    if attachments.is_empty() {
        return Ok(html);
    }
    types(attachments)?;
    let images: HashMap<_, _> = attachments
        .iter()
        .filter_map(|part| {
            let mime: mime_guess::Mime = part.attachment.mime.parse().ok()?;
            (mime.type_().as_str() == "image").then(|| {
                (
                    part.content_id.as_str(),
                    format!(
                        "data:{};base64,{}",
                        mime.essence_str(),
                        STANDARD.encode(&part.attachment.bytes)
                    ),
                )
            })
        })
        .collect();

    let mut output = String::new();
    let mut copied = 0;
    let mut raw_text = None::<&str>;
    for tag in TAG.captures_iter(&html) {
        let Some(name) = tag.get(2).map(|m| m.as_str()) else {
            continue;
        };
        let closing = &tag[1] == "/";
        if let Some(raw) = raw_text {
            if closing && name.eq_ignore_ascii_case(raw) {
                raw_text = None;
            }
            continue;
        }
        if closing {
            continue;
        }
        if [
            "script",
            "style",
            "textarea",
            "title",
            "xmp",
            "iframe",
            "noembed",
            "noframes",
            "plaintext",
        ]
        .iter()
        .any(|raw| name.eq_ignore_ascii_case(raw))
        {
            raw_text = Some(name);
            continue;
        }
        if !name.eq_ignore_ascii_case("img") {
            continue;
        }
        let Some(attributes) = tag.get(3) else {
            continue;
        };
        let Some(src) = ATTRIBUTE
            .captures_iter(attributes.as_str())
            .find(|attribute| attribute[1].eq_ignore_ascii_case("src"))
        else {
            continue;
        };
        let Some(value) = src.get(2).or_else(|| src.get(3)).or_else(|| src.get(4)) else {
            continue;
        };
        let Some(image) = cid(value.as_str()).and_then(|id| images.get(id)) else {
            continue;
        };

        let start = attributes.start() + value.start();
        output.push_str(&html[copied..start]);
        if src.get(4).is_some() {
            output.push('"');
            output.push_str(image);
            output.push('"');
        } else {
            output.push_str(image);
        }
        copied = attributes.start() + value.end();
    }
    if copied == 0 {
        Ok(html)
    } else {
        output.push_str(&html[copied..]);
        Ok(output)
    }
}
