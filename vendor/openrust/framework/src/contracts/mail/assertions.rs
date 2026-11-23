use std::{borrow::Cow, sync::LazyLock};

use regex::{Captures, Regex};

use crate::{
    mail::{Mail, Mailable},
    support::escape_html,
};

macro_rules! address_assertions {
    ($has:ident, $assert:ident, $field:ident) => {
        fn $has(&self, address: &str, name: Option<&str>) -> MailResult<bool> {
            Ok(self.envelope()?.$field.iter().any(|actual| {
                !address.is_empty()
                    && actual.address == address
                    && name.is_none_or(|name| actual.name.as_deref() == Some(name))
            }))
        }

        #[track_caller]
        fn $assert(&self, address: &str, name: Option<&str>) -> &Self
        where
            Self: Sized,
        {
            assert!(
                self.$has(address, name)
                    .expect("could not inspect mailable addresses"),
                "Did not find [{address}] in the mail {} addresses.",
                stringify!($field)
            );
            self
        }
    };
}

pub(super) use address_assertions;

pub(super) enum Rendered {
    Html { escape: bool },
    Text,
}

pub(super) enum Rule<'a> {
    Contains(&'a str),
    DoesNotContain(&'a str),
    InOrder(&'a [&'a str]),
}

pub(super) async fn assert_body<'a, M: Mailable>(
    mail: &'a M,
    part: Rendered,
    rule: Rule<'a>,
) -> &'a M {
    let (html, text) = Mail::render_for_assertions(mail)
        .await
        .expect("could not render mailable for assertions");
    let (body, label, escape) = match part {
        Rendered::Html { escape } => (html, "HTML", escape),
        Rendered::Text => (text, "text", false),
    };
    let expected = |value: &'a str| -> Cow<'a, str> {
        if escape {
            let mut escaped = String::new();
            escape_html(&mut escaped, value);
            Cow::Owned(escaped)
        } else {
            Cow::Borrowed(value)
        }
    };
    match rule {
        Rule::Contains(value) => assert!(
            body.contains(expected(value).as_ref()),
            "Did not see [{value}] in the {label} email body."
        ),
        Rule::DoesNotContain(value) => assert!(
            !body.contains(expected(value).as_ref()),
            "Saw unexpected [{value}] in the {label} email body."
        ),
        Rule::InOrder(values) => {
            let decoded = decode_entities(&body);
            let mut remaining = decoded.as_ref();
            for value in values
                .iter()
                .copied()
                .filter(|value| !matches!(*value, "" | "0"))
            {
                let expected = expected(value);
                let needle = decode_entities(&expected);
                let offset = remaining.find(needle.as_ref()).unwrap_or_else(|| {
                    panic!("Did not see [{value}] in order in the {label} email body.")
                });
                remaining = &remaining[offset + needle.len()..];
            }
        }
    }
    mail
}

fn decode_entities(value: &str) -> Cow<'_, str> {
    static ENTITIES: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"&(?:#[xX][0-9A-Fa-f]+|#[0-9]+|[A-Za-z][A-Za-z0-9]+);")
            .expect("valid entity pattern")
    });
    ENTITIES.replace_all(value, |capture: &Captures<'_>| {
        let entity = &capture[0];
        if let Some(number) = entity.strip_prefix("&#") {
            let number = &number[..number.len() - 1];
            let code = if let Some(hex) = number.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                number.parse().ok()
            };
            return code
                .and_then(char::from_u32)
                .map_or_else(|| entity.to_owned(), |c| c.to_string());
        }
        web_atoms::NAMED_ENTITIES.get(&entity[1..]).map_or_else(
            || entity.to_owned(),
            |&(first, second)| {
                [first, second]
                    .into_iter()
                    .filter(|&code| code != 0)
                    .filter_map(char::from_u32)
                    .collect()
            },
        )
    })
}
