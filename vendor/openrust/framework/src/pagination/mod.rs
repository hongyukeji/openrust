//! Offset and cursor pagination.

mod cursor;
mod cursor_paginator;
mod length_aware_paginator;
mod pagination_service_provider;
mod pagination_state;
mod paginator;
mod url_window;
mod views;

use std::fmt;

pub use cursor::Cursor;
pub use cursor_paginator::CursorPaginator;
pub use length_aware_paginator::LengthAwarePaginator;
pub(crate) use pagination_service_provider::PaginationServiceProvider;
pub use paginator::Paginator;
pub use url_window::{UrlWindow, UrlWindowLinks};
pub(crate) use views::VIEWS;

use crate::database::DatabaseResult;

const FRAGMENT_ENCODE: &percent_encoding::AsciiSet =
    &percent_encoding::CONTROLS.add(b' ').add(b'#').add(b'%');

/// Resolves the page size after the total is known. Returning `None` keeps the page default.
pub type PageSizeResolver<'a> = dyn Fn(u64) -> DatabaseResult<Option<u64>> + Send + Sync + 'a;

/// Supplies a precomputed total. Returning `None` allows the query to count rows.
pub type PageTotalResolver<'a> = dyn Fn() -> DatabaseResult<Option<u64>> + Send + Sync + 'a;

pub struct PageOptions<'a> {
    pub per_page: Option<u64>,
    pub columns: &'a [&'a str],
    pub page_name: &'a str,
    pub page: Option<u64>,
    /// Supplies the total for length-aware pagination and skips its count query.
    pub total: Option<u64>,
    /// Computes the length-aware page size using the resolved total.
    pub per_page_using: Option<&'a PageSizeResolver<'a>>,
    /// Computes the length-aware total when no numeric total was supplied.
    pub total_using: Option<&'a PageTotalResolver<'a>>,
}

impl Default for PageOptions<'_> {
    fn default() -> Self {
        Self {
            per_page: None,
            columns: &["*"],
            page_name: "page",
            page: None,
            total: None,
            per_page_using: None,
            total_using: None,
        }
    }
}

impl PageOptions<'_> {
    pub(crate) fn resolve_total(&self) -> DatabaseResult<Option<u64>> {
        match (self.total, self.total_using) {
            (None, Some(resolve)) => resolve(),
            (total, _) => Ok(total),
        }
    }

    pub(crate) fn resolve_per_page(&self, total: u64) -> DatabaseResult<Option<u64>> {
        match self.per_page_using {
            Some(resolve) => Ok(resolve(total)?.or(self.per_page)),
            None => Ok(self.per_page),
        }
    }
}

pub struct CursorOptions<'a> {
    pub per_page: Option<u64>,
    pub columns: &'a [&'a str],
    pub cursor_name: &'a str,
    pub cursor: Option<&'a Cursor>,
}

impl Default for CursorOptions<'_> {
    fn default() -> Self {
        Self {
            per_page: None,
            columns: &["*"],
            cursor_name: "cursor",
            cursor: None,
        }
    }
}

/// Link options for in-memory pagination. Omitted values use the current request.
#[derive(Clone, Copy, Debug, Default)]
pub struct PaginatorOptions<'a> {
    pub path: Option<&'a str>,
    pub query: &'a [(String, String)],
    pub fragment: Option<&'a str>,
    pub page_name: Option<&'a str>,
    pub cursor_name: Option<&'a str>,
}

pub(crate) const DEFAULT_PER_PAGE: u64 = 15;

pub(crate) fn page_size(size: Option<u64>) -> PaginationResult<u64> {
    let size = size.unwrap_or(DEFAULT_PER_PAGE);
    if size == 0 || size >= i64::MAX as u64 {
        return Err(PaginationError::InvalidPerPage);
    }
    Ok(size)
}

pub(crate) fn page_offset(page: u64, size: u64) -> PaginationResult<u64> {
    (page - 1)
        .checked_mul(size)
        .filter(|offset| *offset <= i64::MAX as u64)
        .ok_or(PaginationError::OffsetOverflow)
}

macro_rules! paginator_json {
    ($type:ident) => {
        impl<T: serde::Serialize> $type<T> {
            pub fn to_array(&self) -> serde_json::Result<serde_json::Value> {
                serde_json::to_value(self)
            }

            pub fn to_json(&self) -> serde_json::Result<String> {
                serde_json::to_string(self)
            }

            pub fn to_pretty_json(&self) -> serde_json::Result<String> {
                serde_json::to_string_pretty(self)
            }
        }
    };
}

pub(super) use paginator_json;

macro_rules! paginator_morph {
    ($paginator:ident) => {
        impl<M: crate::database::eloquent::Model> $paginator<M> {
            /// Loads a nested relation for a typed morph target on this page.
            pub async fn load_morph<T: crate::database::eloquent::Model>(
                &mut self,
                name: &str,
                morph: &crate::database::eloquent::relations::MorphTo<M>,
                alias: &str,
                related_name: &str,
                relation: &impl crate::database::eloquent::relations::Relation<Parent = T>,
            ) -> crate::database::DatabaseResult<&mut Self> {
                crate::database::eloquent::load_morph_relation(
                    self.items_mut(),
                    name,
                    morph,
                    alias,
                    related_name,
                    relation,
                )
                .await?;
                Ok(self)
            }

            /// Loads a nested relation count for a typed morph target on this page.
            pub async fn load_morph_count<T: crate::database::eloquent::Model>(
                &mut self,
                name: &str,
                morph: &crate::database::eloquent::relations::MorphTo<M>,
                alias: &str,
                related_name: &str,
                relation: &impl crate::database::eloquent::relations::ExistenceRelation<Parent = T>,
            ) -> crate::database::DatabaseResult<&mut Self> {
                use crate::database::eloquent::{
                    RelationAggregate, RelationAggregateQuery, load_morph_aggregates,
                };

                let count =
                    RelationAggregateQuery::new(related_name, relation, RelationAggregate::Count);
                load_morph_aggregates(self.items_mut(), name, morph, alias, &[count]).await?;
                Ok(self)
            }
        }
    };
}

paginator_morph!(Paginator);
paginator_morph!(LengthAwarePaginator);
paginator_morph!(CursorPaginator);

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
    pub(super) path: String,
    pub(super) name: String,
    query: Vec<(String, String)>,
    fragment: Option<String>,
}

impl Links {
    pub(crate) fn current(name: &str) -> Self {
        Self {
            path: Paginator::resolve_current_path("/"),
            name: name.into(),
            query: Vec::new(),
            fragment: None,
        }
    }

    fn manual(name: &str, options: PaginatorOptions<'_>) -> Self {
        let mut links = if let Some(path) = options.path {
            let path = path.trim_end_matches('/');
            Self {
                path: if path.is_empty() { "/" } else { path }.into(),
                name: name.into(),
                query: Vec::new(),
                fragment: None,
            }
        } else {
            Self::current(name)
        };
        links.appends(options.query);
        links.fragment = options.fragment.map(str::to_owned);
        links
    }

    pub(crate) fn current_page(&self, page: Option<u64>) -> u64 {
        page.unwrap_or_else(|| Paginator::resolve_current_page(&self.name, 1))
            .max(1)
    }

    pub(crate) fn current_cursor(&self, cursor: Option<&Cursor>) -> Option<Cursor> {
        cursor
            .cloned()
            .or_else(|| CursorPaginator::resolve_current_cursor(&self.name, None))
    }

    fn options(&self, cursor: bool) -> PaginatorOptions<'_> {
        PaginatorOptions {
            path: Some(&self.path),
            query: &self.query,
            fragment: self.fragment.as_deref(),
            page_name: (!cursor).then_some(self.name.as_str()),
            cursor_name: cursor.then_some(self.name.as_str()),
        }
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
            url.push(if url.contains('?') { '&' } else { '?' });
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
        if let Some(query) = Paginator::resolve_query_string(None) {
            self.appends(&query);
        }
    }
}
