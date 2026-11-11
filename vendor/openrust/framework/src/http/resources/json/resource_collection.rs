use serde_json::{Map, Value};

use crate::{
    http::Request,
    pagination::{CursorPaginator, LengthAwarePaginator, Paginator},
};

use super::{JsonResource, PaginatedResourceResponse, PaginationInformation, ResourceResult};

/// A collection source whose pagination survives resource transformation.
pub enum ResourceItems<T> {
    Collection(Vec<T>),
    Paginator(Paginator<T>),
    LengthAwarePaginator(LengthAwarePaginator<T>),
    CursorPaginator(Box<CursorPaginator<T>>),
}

impl<T> ResourceItems<T> {
    pub fn items(&self) -> &[T] {
        match self {
            Self::Collection(items) => items,
            Self::Paginator(page) => page.items(),
            Self::LengthAwarePaginator(page) => page.items(),
            Self::CursorPaginator(page) => page.items(),
        }
    }

    pub fn map<U>(self, map: impl FnMut(T) -> U) -> ResourceItems<U> {
        match self {
            Self::Collection(items) => {
                ResourceItems::Collection(items.into_iter().map(map).collect())
            }
            Self::Paginator(page) => ResourceItems::Paginator(page.map(map)),
            Self::LengthAwarePaginator(page) => ResourceItems::LengthAwarePaginator(page.map(map)),
            Self::CursorPaginator(page) => ResourceItems::from(page.map(map)),
        }
    }
}

impl<T> From<Vec<T>> for ResourceItems<T> {
    fn from(items: Vec<T>) -> Self {
        Self::Collection(items)
    }
}

impl<T> From<Paginator<T>> for ResourceItems<T> {
    fn from(page: Paginator<T>) -> Self {
        Self::Paginator(page)
    }
}

impl<T> From<LengthAwarePaginator<T>> for ResourceItems<T> {
    fn from(page: LengthAwarePaginator<T>) -> Self {
        Self::LengthAwarePaginator(page)
    }
}

impl<T> From<CursorPaginator<T>> for ResourceItems<T> {
    fn from(page: CursorPaginator<T>) -> Self {
        Self::CursorPaginator(Box::new(page))
    }
}

type PaginationHook =
    dyn Fn(&Request, PaginationInformation) -> ResourceResult<PaginationInformation> + Send + Sync;

/// Transforms a collection for an HTTP response and retains its paginator.
pub struct ResourceCollection<T> {
    source: ResourceItems<T>,
    pagination: Option<Box<PaginationHook>>,
}

#[allow(non_snake_case, reason = "Matches Laravel's ResourceCollection API")]
impl<T> ResourceCollection<T> {
    pub fn new(source: impl Into<ResourceItems<T>>) -> Self {
        Self {
            source: source.into(),
            pagination: None,
        }
    }

    pub fn items(&self) -> &[T] {
        self.source.items()
    }

    #[must_use]
    pub fn preserveQuery(mut self) -> Self {
        match &mut self.source {
            ResourceItems::Collection(_) => {}
            ResourceItems::Paginator(page) => {
                page.withQueryString();
            }
            ResourceItems::LengthAwarePaginator(page) => {
                page.withQueryString();
            }
            ResourceItems::CursorPaginator(page) => {
                page.withQueryString();
            }
        }
        self
    }

    #[must_use]
    pub fn withQuery(mut self, query: &[(String, String)]) -> Self {
        match &mut self.source {
            ResourceItems::Collection(_) => {}
            ResourceItems::Paginator(page) => {
                page.appends(query);
            }
            ResourceItems::LengthAwarePaginator(page) => {
                page.appends(query);
            }
            ResourceItems::CursorPaginator(page) => {
                page.appends(query);
            }
        }
        self
    }

    #[must_use]
    pub fn withPaginationInformation(
        mut self,
        customize: impl Fn(&Request, PaginationInformation) -> ResourceResult<PaginationInformation>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.pagination = Some(Box::new(customize));
        self
    }
}

impl<T: JsonResource> JsonResource for ResourceCollection<T> {
    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        self.items()
            .iter()
            .map(|resource| resource.toArray(request))
            .collect()
    }

    fn with(&self, request: &Request) -> ResourceResult<Map<String, Value>> {
        let Some(info) = PaginatedResourceResponse::information(&self.source)? else {
            return Ok(Map::new());
        };
        let info = match &self.pagination {
            Some(customize) => customize(request, info)?,
            None => info,
        };
        PaginatedResourceResponse::fields(info)
    }
}
