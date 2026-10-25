//! Common HTTP response wrappers and validated pagination parameters.

use std::collections::HashMap;
use std::num::NonZeroU64;

use axum::Json;
use axum::extract::{FromRequestParts, Query};
use axum::http::{StatusCode, request::Parts};
use axum::response::{IntoResponse, Response};
use openrust_core::{Error, Result, ValidationErrors};
use serde::{Deserialize, Serialize};

/// Wraps a response with HTTP 201 Created.
///
/// ```
/// use openrust_http::{Created, axum::Json};
///
/// async fn store() -> Created<Json<&'static str>> {
///     Created(Json("created"))
/// }
/// ```
pub struct Created<T>(pub T);

impl<T: IntoResponse> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        (StatusCode::CREATED, self.0).into_response()
    }
}

/// Responds with HTTP 204 and an empty body.
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        StatusCode::NO_CONTENT.into_response()
    }
}

/// One-based pagination parameters extracted from a query string.
///
/// Extraction defaults to page 1 and 20 items per page, and validates both fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct PageParams {
    /// One-based page number, at least 1.
    #[serde(default = "default_page")]
    pub page: u64,
    /// Page size, between 1 and 100.
    #[serde(default = "default_per_page")]
    pub per_page: u64,
}

const fn default_page() -> u64 {
    1
}

const fn default_per_page() -> u64 {
    20
}

impl Default for PageParams {
    fn default() -> Self {
        Self {
            page: default_page(),
            per_page: default_per_page(),
        }
    }
}

impl PageParams {
    /// Zero-based offset; saturates if the product exceeds `u64`.
    pub fn offset(self) -> u64 {
        self.page.saturating_sub(1).saturating_mul(self.per_page)
    }

    /// Maximum number of items to fetch.
    pub fn limit(self) -> u64 {
        self.per_page
    }
}

impl<S: Send + Sync> FromRequestParts<S> for PageParams {
    type Rejection = Error;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self> {
        let Query(values) = Query::<HashMap<String, String>>::from_request_parts(parts, state)
            .await
            .map_err(|rejection| {
                let mut errors = ValidationErrors::new();
                errors.add("query", rejection.body_text());
                Error::Validation(errors)
            })?;
        let mut errors = ValidationErrors::new();
        let page = parse_param(&values, "page", default_page(), &mut errors);
        let per_page = parse_param(&values, "per_page", default_per_page(), &mut errors);
        if page == 0 {
            errors.add("page", "must be at least 1");
        }
        if !(1..=100).contains(&per_page) {
            errors.add("per_page", "must be between 1 and 100");
        }
        if !errors.is_empty() {
            return Err(Error::Validation(errors));
        }
        Ok(Self { page, per_page })
    }
}

fn parse_param(
    values: &HashMap<String, String>,
    field: &str,
    default: u64,
    errors: &mut ValidationErrors,
) -> u64 {
    values.get(field).map_or(default, |value| {
        value.parse::<u64>().unwrap_or_else(|_| {
            errors.add(field, "must be a non-negative integer");
            default
        })
    })
}

/// A JSON page with its pagination metadata.
///
/// ```
/// use openrust_http::{PageParams, Paginated};
///
/// let page = Paginated::new(vec!["post"], PageParams::default(), 1);
/// ```
pub struct Paginated<T> {
    items: Vec<T>,
    params: PageParams,
    total: u64,
}

impl<T> Paginated<T> {
    /// Builds a page from its items, validated parameters and total item count.
    pub fn new(items: Vec<T>, params: PageParams, total: u64) -> Self {
        Self {
            items,
            params,
            total,
        }
    }
}

impl<T: Serialize> IntoResponse for Paginated<T> {
    fn into_response(self) -> Response {
        #[derive(Serialize)]
        struct Meta {
            page: u64,
            per_page: u64,
            total: u64,
            total_pages: u64,
        }

        #[derive(Serialize)]
        struct Page<T> {
            data: Vec<T>,
            meta: Meta,
        }

        let total_pages = NonZeroU64::new(self.params.per_page).map_or(0, |page_size| {
            self.total / page_size.get() + u64::from(!self.total.is_multiple_of(page_size.get()))
        });
        Json(Page {
            data: self.items,
            meta: Meta {
                page: self.params.page,
                per_page: self.params.per_page,
                total: self.total,
                total_pages,
            },
        })
        .into_response()
    }
}
