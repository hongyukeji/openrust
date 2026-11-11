//! Offset and cursor pagination.

mod cursor;
mod cursor_paginator;
mod length_aware_paginator;
mod paginator;

use std::fmt;

pub use cursor::Cursor;
pub use cursor_paginator::CursorPaginator;
pub use length_aware_paginator::LengthAwarePaginator;
pub use paginator::Paginator;

use crate::http::request_context;

const FRAGMENT_ENCODE: &percent_encoding::AsciiSet =
    &percent_encoding::CONTROLS.add(b' ').add(b'#').add(b'%');

#[allow(non_snake_case, reason = "Matches Laravel's pagination options")]
pub struct PageOptions<'a> {
    pub perPage: Option<u64>,
    pub columns: &'a [&'a str],
    pub pageName: &'a str,
    pub page: Option<u64>,
}

impl Default for PageOptions<'_> {
    fn default() -> Self {
        Self {
            perPage: None,
            columns: &["*"],
            pageName: "page",
            page: None,
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's pagination options")]
pub struct CursorOptions<'a> {
    pub perPage: Option<u64>,
    pub columns: &'a [&'a str],
    pub cursorName: &'a str,
    pub cursor: Option<&'a Cursor>,
}

impl Default for CursorOptions<'_> {
    fn default() -> Self {
        Self {
            perPage: None,
            columns: &["*"],
            cursorName: "cursor",
            cursor: None,
        }
    }
}

pub type PaginationResult<T> = Result<T, PaginationError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PaginationError {
    InvalidPerPage,
    OffsetOverflow,
    InvalidCursor,
    MissingCursorOrder,
}

impl fmt::Display for PaginationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPerPage => "invalid page size",
            Self::OffsetOverflow => "page offset exceeds the supported range",
            Self::InvalidCursor => "invalid pagination cursor or cursor column",
            Self::MissingCursorOrder => "cursor pagination requires supported ordered columns",
        })
    }
}

impl std::error::Error for PaginationError {}

#[derive(Debug)]
pub(crate) struct Links {
    path: String,
    name: String,
    incoming: Vec<(String, String)>,
    query: Vec<(String, String)>,
    fragment: Option<String>,
}

impl Links {
    pub(crate) fn current(name: &str) -> Self {
        request_context::with_uri(|uri| Self {
            // Use only the path from the request target. Avoid trusting its authority.
            path: format!("/{}", uri.path().trim_matches('/')),
            name: name.to_owned(),
            incoming: url::form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes())
                .into_owned()
                .collect(),
            query: Vec::new(),
            fragment: None,
        })
        .unwrap_or_else(|| Self {
            path: "/".into(),
            name: name.into(),
            incoming: Vec::new(),
            query: Vec::new(),
            fragment: None,
        })
    }

    pub(crate) fn incoming(&self) -> Option<&str> {
        self.incoming
            .iter()
            .rev()
            .find(|(key, _)| key == &self.name)
            .map(|(_, value)| value.as_str())
    }

    fn url(&self, position: Option<&str>) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.extend_pairs(self.query.iter().filter(|(key, _)| key != &self.name));
        if let Some(value) = position {
            query.append_pair(&self.name, value);
        }
        let query = query.finish();
        let mut url = self.path.clone();
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        if let Some(fragment) = &self.fragment {
            url.push('#');
            url.push_str(
                &percent_encoding::utf8_percent_encode(fragment, FRAGMENT_ENCODE).to_string(),
            );
        }
        url
    }

    fn appends(&mut self, query: &[(String, String)]) {
        for (key, value) in query.iter().filter(|(key, _)| key != &self.name) {
            self.query.retain(|(existing, _)| existing != key);
            self.query.push((key.clone(), value.clone()));
        }
    }

    fn with_query_string(&mut self) {
        let mut query: Vec<_> = self
            .incoming
            .iter()
            .filter(|(key, _)| {
                key != &self.name && !self.query.iter().any(|(other, _)| key == other)
            })
            .cloned()
            .collect();
        query.append(&mut self.query);
        self.query = query;
    }
}
