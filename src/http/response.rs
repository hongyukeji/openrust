//! 通用 HTTP 响应包装和已校验的分页参数。

use std::collections::HashMap;
use std::num::NonZeroU64;

use crate::config::HttpStatusMode;
use crate::core::error::current_http_status_mode;
use crate::core::{ApiResponse, Error, PageMeta, Result, ValidationErrors};
use axum::extract::{FromRequestParts, Query};
use axum::http::{HeaderValue, StatusCode, header, request::Parts};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// 在语义 HTTP 模式下，将成功响应标记为资源已创建。
///
/// ```
/// use openrust::http::{Created, Json};
///
/// async fn create() -> Created<Json<&'static str>> {
///     Created(Json("created"))
/// }
/// ```
pub struct Created<T>(pub T);

impl<T: IntoResponse> IntoResponse for Created<T> {
    fn into_response(self) -> Response {
        let mut response = self.0.into_response();
        if current_http_status_mode() == HttpStatusMode::Semantic {
            *response.status_mut() = StatusCode::CREATED;
        }
        response
    }
}

/// 带有已校验 `Location` 响应头的资源创建响应。
pub struct Located<T> {
    location: HeaderValue,
    body: T,
}

impl<T> Created<T> {
    /// 创建包含 `Location` 响应头的结果；响应头值无效时返回内部错误。
    pub fn with_location(location: &str, body: T) -> Result<Located<T>> {
        let location = HeaderValue::from_str(location).map_err(Error::internal)?;
        Ok(Located { location, body })
    }
}

impl<T: IntoResponse> IntoResponse for Located<T> {
    fn into_response(self) -> Response {
        let mut response = Created(self.body).into_response();
        response
            .headers_mut()
            .insert(header::LOCATION, self.location);
        response
    }
}

/// 返回不带业务载荷的成功响应封装。
pub struct NoContent;

impl IntoResponse for NoContent {
    fn into_response(self) -> Response {
        ApiResponse::empty().into_response()
    }
}

/// 从查询字符串中提取的分页参数，页码从 1 开始。
///
/// 直接提取时默认第 1 页、每页 20 项，并校验两个字段以及从 0 开始的偏移量是否处于数据库 `i64` 范围内。
/// `Query<PageParams>` 只负责反序列化字段，不执行这些校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct PageParams {
    /// 从 1 开始的页码，最小值为 1。
    #[serde(default = "default_page")]
    pub page: u64,
    /// 每页条目数，范围为 1 到 100。
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
    /// 从 0 开始的偏移量；乘积超过 `u64` 上限时会饱和。
    pub fn offset(self) -> u64 {
        self.page.saturating_sub(1).saturating_mul(self.per_page)
    }

    /// 最多获取的条目数。
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
        if page != 0
            && (1..=100).contains(&per_page)
            && (page - 1)
                .checked_mul(per_page)
                .is_none_or(|offset| offset > i64::MAX as u64)
        {
            errors.add("page", "is too large");
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

/// 包含分页元数据的 JSON 分页结果。
///
/// ```
/// use openrust::http::{PageParams, Paginated};
///
/// let page = Paginated::new(vec!["post"], PageParams::default(), 1);
/// ```
pub struct Paginated<T> {
    items: Vec<T>,
    params: PageParams,
    total: u64,
}

impl<T> Paginated<T> {
    /// 根据条目、已校验的分页参数和条目总数构建分页结果。
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
        let total_pages = NonZeroU64::new(self.params.per_page).map_or(0, |page_size| {
            self.total / page_size.get() + u64::from(!self.total.is_multiple_of(page_size.get()))
        });
        ApiResponse::success(self.items)
            .with_meta(PageMeta {
                page: self.params.page,
                per_page: self.params.per_page,
                total: self.total,
                total_pages,
            })
            .into_response()
    }
}
