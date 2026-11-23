use std::sync::Arc;

use serde_json::{Map, Value};

use crate::{
    database::eloquent::{Collection as ModelCollection, Model, RelatedCollection},
    http::{HttpFuture, Request},
    pagination::{CursorPaginator, LengthAwarePaginator, Paginator},
    support::{ArrayKey, Collection},
};

use super::{
    JsonOptions, JsonResource, PaginatedResourceResponse, PaginationInformation, ResourceResult,
};

/// A collection source whose pagination survives resource transformation.
pub enum ResourceItems<T> {
    Collection(Vec<T>),
    KeyedCollection(KeyedResourceItems<T>),
    Paginator(Paginator<T>),
    LengthAwarePaginator(LengthAwarePaginator<T>),
    CursorPaginator(Box<CursorPaginator<T>>),
}

/// An ordered resource collection with a key for each item.
#[doc(hidden)]
pub struct KeyedResourceItems<T> {
    items: Vec<T>,
    keys: Vec<ArrayKey>,
}

impl<T> ResourceItems<T> {
    pub fn items(&self) -> &[T] {
        match self {
            Self::Collection(items) => items,
            Self::KeyedCollection(items) => &items.items,
            Self::Paginator(page) => page.items(),
            Self::LengthAwarePaginator(page) => page.items(),
            Self::CursorPaginator(page) => page.items(),
        }
    }

    fn items_mut(&mut self) -> &mut [T] {
        match self {
            Self::Collection(items) => items,
            Self::KeyedCollection(items) => &mut items.items,
            Self::Paginator(page) => page.items_mut(),
            Self::LengthAwarePaginator(page) => page.items_mut(),
            Self::CursorPaginator(page) => page.items_mut(),
        }
    }

    fn keys(&self) -> Option<&[ArrayKey]> {
        match self {
            Self::KeyedCollection(items) => Some(&items.keys),
            Self::Paginator(page) => page.collection_keys(),
            Self::LengthAwarePaginator(page) => page.collection_keys(),
            Self::Collection(_) | Self::CursorPaginator(_) => None,
        }
    }

    pub fn map<U>(self, map: impl FnMut(T) -> U) -> ResourceItems<U> {
        match self {
            Self::Collection(items) => {
                ResourceItems::Collection(items.into_iter().map(map).collect())
            }
            Self::KeyedCollection(items) => ResourceItems::KeyedCollection(KeyedResourceItems {
                items: items.items.into_iter().map(map).collect(),
                keys: items.keys,
            }),
            Self::Paginator(page) => ResourceItems::Paginator(page.map(map)),
            Self::LengthAwarePaginator(page) => ResourceItems::LengthAwarePaginator(page.map(map)),
            Self::CursorPaginator(page) => ResourceItems::from(page.map(map)),
        }
    }
}

impl<T> From<Collection<T>> for ResourceItems<T> {
    fn from(collection: Collection<T>) -> Self {
        let (keys, items) = collection.into_iter().unzip();
        Self::KeyedCollection(KeyedResourceItems { items, keys })
    }
}

impl<M: Model> From<ModelCollection<M>> for ResourceItems<M> {
    fn from(collection: ModelCollection<M>) -> Self {
        let (items, keys) = collection.into_parts();
        match keys {
            Some(keys) => Self::KeyedCollection(KeyedResourceItems { items, keys }),
            None => Self::Collection(items),
        }
    }
}

impl<M: Model> From<RelatedCollection<M>> for ResourceItems<Arc<M>> {
    fn from(collection: RelatedCollection<M>) -> Self {
        Self::from(Collection::from(collection))
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
    preserve_keys: Option<bool>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<T> ResourceCollection<T> {
    pub fn new(source: impl Into<ResourceItems<T>>) -> Self {
        Self {
            source: source.into(),
            pagination: None,
            preserve_keys: None,
        }
    }

    pub fn items(&self) -> &[T] {
        self.source.items()
    }

    /// Overrides the resource type's policy for retaining numeric keys.
    #[must_use]
    pub fn preserveKeys(mut self, preserve: bool) -> Self {
        self.preserve_keys = Some(preserve);
        self
    }

    #[must_use]
    pub fn preserveQuery(mut self) -> Self {
        match &mut self.source {
            ResourceItems::Collection(_) | ResourceItems::KeyedCollection(_) => {}
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
            ResourceItems::Collection(_) | ResourceItems::KeyedCollection(_) => {}
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
    const JSON_API: bool = T::JSON_API;

    fn prepareResponse(&mut self, request: &Request) -> Option<HttpFuture<'_, ResourceResult<()>>> {
        T::prepareCollection(self.source.items_mut(), request)
    }

    fn resolveResponseData(
        &self,
        request: &Request,
    ) -> ResourceResult<(Value, Map<String, Value>)> {
        if !Self::JSON_API {
            return Ok((self.toArray(request)?, self.with(request)?));
        }
        let mut primary = Vec::with_capacity(self.items().len());
        let mut included = Vec::new();
        for resource in self.items() {
            let (data, mut fields) = resource.resolveResponseData(request)?;
            primary.push(data);
            if let Some(Value::Array(objects)) = fields.shift_remove("included") {
                included.extend(objects);
            }
        }
        let primary = Value::Array(primary);
        let mut fields = self.with(request)?;
        super::super::json_api::add_included(&mut fields, &primary, included);
        Ok((primary, fields))
    }

    fn jsonOptions(&self) -> JsonOptions {
        self.items()
            .first()
            .map_or_else(T::defaultJsonOptions, JsonResource::jsonOptions)
    }

    fn defaultJsonOptions() -> JsonOptions {
        T::defaultJsonOptions()
    }

    fn toArray(&self, request: &Request) -> ResourceResult<Value> {
        let values = self
            .items()
            .iter()
            .map(|resource| resource.toArray(request));
        if !Self::JSON_API
            && let Some(keys) = self.source.keys()
            && (self.preserve_keys.unwrap_or_else(T::preservesKeys)
                || keys.iter().any(|key| !numeric_key(key)))
            && keys.iter().enumerate().any(|(index, key)| {
                !matches!(key, ArrayKey::Integer(key) if usize::try_from(*key) == Ok(index))
            })
        {
            return keys
                .iter()
                .zip(values)
                .map(|(key, value)| Ok((key.to_string(), value?)))
                .collect::<ResourceResult<Map<_, _>>>()
                .map(Value::Object);
        }
        values.collect()
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

fn numeric_key(key: &ArrayKey) -> bool {
    match key {
        ArrayKey::Integer(_) => true,
        ArrayKey::String(key) => {
            let text = key.trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}']);
            text.bytes().any(|byte| byte.is_ascii_digit()) && text.parse::<f64>().is_ok()
        }
    }
}
