use std::collections::BTreeMap;

use ::http::header::{ACCEPT, CONTENT_TYPE};

use super::{HeaderMap, Request};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Request {
    /// Returns the Accept entries in descending quality order.
    pub fn getAcceptableContentTypes(&self) -> &[String] {
        self.acceptable_content_types
            .get_or_init(|| parse_accept(self.headers()))
    }

    /// Tests whether any supplied media type is acceptable.
    pub fn accepts(&self, content_types: &[&str]) -> bool {
        let acceptable = self.getAcceptableContentTypes();
        acceptable.is_empty()
            || acceptable.iter().any(|range| {
                let range = strip_params(range).to_ascii_lowercase();
                matches!(range.as_str(), "*" | "*/*")
                    || content_types.iter().any(|content_type| {
                        let content_type = content_type.to_ascii_lowercase();
                        matches_type(&range, &content_type) || type_wildcard(&range, &content_type)
                    })
            })
    }

    /// Returns the preferred supplied media type or format name.
    pub fn prefers<'a>(&self, content_types: &[&'a str]) -> Option<&'a str> {
        for range in self.getAcceptableContentTypes() {
            let range = strip_params(range).to_ascii_lowercase();
            if matches!(range.as_str(), "*" | "*/*") {
                return content_types.first().copied();
            }
            if let Some(content_type) = content_types.iter().copied().find(|content_type| {
                let mime = mime_for_format(content_type)
                    .unwrap_or(content_type)
                    .to_ascii_lowercase();
                matches_type(&mime, &range) || type_wildcard(&range, &mime)
            }) {
                return Some(content_type);
            }
        }
        None
    }

    /// Tests whether the first Accept entry allows every media type.
    pub fn acceptsAnyContentType(&self) -> bool {
        self.getAcceptableContentTypes()
            .first()
            .is_none_or(|entry| matches!(entry.as_str(), "*" | "*/*"))
    }

    pub fn acceptsJson(&self) -> bool {
        self.accepts(&["application/json"])
    }

    pub fn acceptsHtml(&self) -> bool {
        self.accepts(&["text/html"])
    }

    pub fn acceptsMarkdown(&self) -> bool {
        self.accepts(&["text/markdown"])
    }

    /// Tests whether the preferred Accept entry indicates JSON.
    pub fn wantsJson(&self) -> bool {
        self.getAcceptableContentTypes()
            .first()
            .is_some_and(|entry| contains_json(&entry.to_ascii_lowercase()))
    }

    /// Tests whether the preferred Accept entry indicates Markdown.
    pub fn wantsMarkdown(&self) -> bool {
        self.getAcceptableContentTypes()
            .first()
            .is_some_and(|entry| entry.to_ascii_lowercase().starts_with("text/markdown"))
    }

    /// Determines whether to use JSON for a negotiated response.
    pub fn expectsJson(&self) -> bool {
        let ajax = self
            .headers()
            .get("x-requested-with")
            .is_some_and(|value| value == "XMLHttpRequest");
        let pjax = self
            .headers()
            .get("x-pjax")
            .is_some_and(|value| !value.is_empty() && value != "0");
        self.wantsJson() || (ajax && !pjax && self.acceptsAnyContentType())
    }

    /// Tests whether the request declares a JSON body.
    pub fn isJson(&self) -> bool {
        self.headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(contains_json)
    }

    /// Compares a media type with an exact or structured-suffix type.
    pub fn matchesType(actual: &str, content_type: &str) -> bool {
        matches_type(actual, content_type)
    }

    /// Returns the first recognized response format, or the supplied fallback.
    pub fn format<'a>(&self, default: &'a str) -> &'a str {
        self.getAcceptableContentTypes()
            .iter()
            .find_map(|entry| format_for_mime(entry))
            .unwrap_or(default)
    }
}

fn contains_json(value: &str) -> bool {
    value.contains("/json") || value.contains("+json")
}

fn type_wildcard(range: &str, content_type: &str) -> bool {
    range
        .strip_suffix("/*")
        .is_some_and(|group| group == content_type.split('/').next().unwrap_or_default())
}

fn parse_accept(headers: &HeaderMap) -> Vec<String> {
    let Some(raw) = headers.get(ACCEPT).and_then(|value| value.to_str().ok()) else {
        return Vec::new();
    };
    let items = split_header(raw, ',');
    let mut deduplicated = BTreeMap::new();
    for (index, item) in items.enumerate() {
        let mut segments = split_header(item, ';').map(str::trim);
        let value = segments.next().unwrap_or_default();
        if value.is_empty() {
            continue;
        }
        let mut params = BTreeMap::new();
        let mut quality = 1.0_f64;
        for segment in segments {
            let (name, value) = segment
                .split_once('=')
                .map_or((segment, None), |(name, value)| (name, Some(value)));
            let name = name.trim().to_ascii_lowercase();
            let value = value.map_or_else(|| "1".to_owned(), |value| unquote(value.trim()));
            if name == "q" {
                quality = php_float(&value);
            } else if !name.is_empty() {
                params.insert(name, value);
            }
        }
        let mut key = unquote(value);
        for (name, value) in params {
            key.push(';');
            key.push_str(&name);
            key.push('=');
            if value
                .chars()
                .any(|character| character.is_whitespace() || ";,=".contains(character))
            {
                key.push('"');
                for character in value.chars() {
                    if character == '"' || character == '\\' {
                        key.push('\\');
                    }
                    key.push(character);
                }
                key.push('"');
            } else {
                key.push_str(&value);
            }
        }
        deduplicated.insert(key, (quality, index));
    }
    let mut items: Vec<_> = deduplicated.into_iter().collect();
    items.sort_by(
        |(_, (left_quality, left_index)), (_, (right_quality, right_index))| {
            // Equal quality keeps the original position, including signed zero.
            right_quality
                .partial_cmp(left_quality)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left_index.cmp(right_index))
        },
    );
    items.into_iter().map(|(key, _)| key).collect()
}

fn unquote(value: &str) -> String {
    let Some(value) = value.strip_prefix('"') else {
        return value.to_owned();
    };
    let value = value.strip_suffix('"').unwrap_or(value);
    let mut result = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character == '\\' {
            result.push(characters.next().unwrap_or('\\'));
        } else {
            result.push(character);
        }
    }
    result
}

// PHP casts the numeric prefix and treats textual NaN and INF as zero.
fn php_float(value: &str) -> f64 {
    let value = value.trim_start();
    let bytes = value.as_bytes();
    let mut end = usize::from(
        bytes
            .first()
            .is_some_and(|byte| matches!(byte, b'+' | b'-')),
    );
    let mut digits = 0;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
        digits += 1;
    }
    if bytes.get(end) == Some(&b'.') {
        end += 1;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return 0.0;
    }
    if bytes
        .get(end)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        let exponent = end;
        end += 1;
        if bytes
            .get(end)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            end += 1;
        }
        let first_digit = end;
        while bytes.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == first_digit {
            end = exponent;
        }
    }
    value[..end].parse().unwrap_or(0.0)
}

fn split_header(value: &str, separator: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    let mut escaped = false;
    value.split(move |character| {
        if escaped {
            escaped = false;
        } else if character == '\\' && quoted {
            escaped = true;
        } else if character == '"' {
            quoted = !quoted;
        } else {
            return character == separator && !quoted;
        }
        false
    })
}

fn strip_params(value: &str) -> &str {
    value
        .split_once(';')
        .map_or(value, |(value, _)| value.trim())
}

fn matches_type(actual: &str, requested: &str) -> bool {
    actual == requested
        || actual.split_once('/').is_some_and(|(main, suffix)| {
            requested
                .split_once(&format!("{main}/"))
                .is_some_and(|(_, rest)| {
                    rest.rfind(&format!("+{suffix}"))
                        .is_some_and(|index| index > 0)
                })
        })
}

const FORMATS: &[(&str, &[&str])] = &[
    ("html", &["text/html", "application/xhtml+xml"]),
    ("txt", &["text/plain"]),
    (
        "js",
        &[
            "application/javascript",
            "application/x-javascript",
            "text/javascript",
        ],
    ),
    ("css", &["text/css"]),
    ("json", &["application/json", "application/x-json"]),
    ("jsonld", &["application/ld+json"]),
    ("xml", &["text/xml", "application/xml", "application/x-xml"]),
    ("rdf", &["application/rdf+xml"]),
    ("atom", &["application/atom+xml"]),
    ("rss", &["application/rss+xml"]),
    (
        "form",
        &["application/x-www-form-urlencoded", "multipart/form-data"],
    ),
    ("soap", &["application/soap+xml"]),
    ("problem", &["application/problem+json"]),
    ("hal", &["application/hal+json", "application/hal+xml"]),
    ("jsonapi", &["application/vnd.api+json"]),
    ("yaml", &["text/yaml", "application/x-yaml"]),
    ("wbxml", &["application/vnd.wap.wbxml"]),
    ("pdf", &["application/pdf"]),
    ("csv", &["text/csv"]),
];

fn mime_for_format(format: &str) -> Option<&'static str> {
    FORMATS
        .iter()
        .find(|(name, _)| *name == format)
        .map(|(_, types)| types[0])
}

fn format_for_mime(mime: &str) -> Option<&'static str> {
    let canonical = strip_params(mime);
    if let Some((format, _)) = FORMATS.iter().find(|(_, types)| types.contains(&canonical)) {
        return Some(format);
    }
    let suffix = canonical.strip_prefix("application/")?.rsplit_once('+')?.1;
    Some(match suffix {
        "json" => "json",
        "xml" | "wbxml" => "xml",
        "xhtml" => "html",
        "cbor" => "cbor",
        "zip" => "zip",
        "ber" | "der" => "asn1",
        "tlv" => "tlv",
        "yaml" => "yaml",
        _ => return None,
    })
}
