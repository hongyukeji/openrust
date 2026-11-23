use crate::{
    database::{Value, eloquent::Model},
    view::{View, ViewData, ViewResult},
};

use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::{Cursor, Links, PaginationResult, PaginatorOptions, page_size};

/// A page positioned by ordered column values.
#[derive(Debug)]
pub struct CursorPaginator<T> {
    items: Vec<T>,
    per_page: u64,
    links: Links,
    cursor: Option<Cursor>,
    next: Option<Cursor>,
    previous: Option<Cursor>,
    more: bool,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<T> CursorPaginator<T> {
    /// Creates a page from ordered items and one optional look-ahead item.
    /// Supply items in reverse fetch order for a previous-page cursor.
    pub fn new<K: AsRef<str>, I: IntoIterator<Item = (K, Value)>>(
        items: Vec<T>,
        per_page: u64,
        cursor: Option<&Cursor>,
        options: PaginatorOptions<'_>,
        mut keys: impl FnMut(&T) -> I,
    ) -> PaginationResult<Self> {
        let size = page_size(Some(per_page))?;
        let links = Links::manual(options.cursorName.unwrap_or("cursor"), options);
        let cursor = links.current_cursor(cursor);
        Self::from_ordered(items, size, links, cursor, |item, next| {
            Cursor::new(keys(item), next)
        })
    }

    pub(crate) fn from_ordered(
        mut items: Vec<T>,
        size: u64,
        links: Links,
        cursor: Option<Cursor>,
        mut key: impl FnMut(&T, bool) -> PaginationResult<Cursor>,
    ) -> PaginationResult<Self> {
        let more = items.len() as u64 > size;
        let backwards = cursor.as_ref().is_some_and(Cursor::pointsToPreviousItems);
        items.truncate(usize::try_from(size).unwrap_or(usize::MAX));
        if backwards {
            items.reverse();
        }
        let next = if backwards || more {
            items.last().map(|item| key(item, true)).transpose()?
        } else {
            None
        };
        let has_previous = if backwards { more } else { cursor.is_some() };
        let previous = if has_previous {
            items.first().map(|item| key(item, false)).transpose()?
        } else {
            None
        };
        Ok(Self {
            items,
            per_page: size,
            links,
            cursor,
            next,
            previous,
            more,
        })
    }

    pub(crate) fn items_mut(&mut self) -> &mut [T] {
        &mut self.items
    }

    pub fn links(&self) -> ViewResult<View> {
        self.render()
    }

    pub fn linksWith(&self, view: &str, data: ViewData) -> ViewResult<View> {
        self.renderWith(view, data)
    }

    pub fn render(&self) -> ViewResult<View> {
        self.renderWith(&super::views::DefaultViews::get(true)?, ViewData::new())
    }

    pub fn renderWith(&self, view: &str, data: ViewData) -> ViewResult<View> {
        let paginator = super::views::paginator_data(
            self.hasPages(),
            self.previousPageUrl()
                .map_err(|error| crate::view::ViewError::RenderFailed(Box::new(error)))?,
            self.nextPageUrl()
                .map_err(|error| crate::view::ViewError::RenderFailed(Box::new(error)))?,
        );
        super::views::make(view, data, paginator, None)
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }
    pub fn perPage(&self) -> u64 {
        self.per_page
    }
    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }
    pub fn nextCursor(&self) -> Option<&Cursor> {
        self.next.as_ref()
    }
    pub fn previousCursor(&self) -> Option<&Cursor> {
        self.previous.as_ref()
    }
    pub fn hasMorePages(&self) -> bool {
        self.more
            || self
                .cursor
                .as_ref()
                .is_some_and(Cursor::pointsToPreviousItems)
    }
    pub fn hasPages(&self) -> bool {
        !self.onFirstPage() || self.hasMorePages()
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn isEmpty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn isNotEmpty(&self) -> bool {
        !self.isEmpty()
    }

    pub fn onFirstPage(&self) -> bool {
        self.cursor
            .as_ref()
            .is_none_or(|cursor| cursor.pointsToPreviousItems() && !self.more)
    }

    pub fn onLastPage(&self) -> bool {
        !self.hasMorePages()
    }

    pub fn getOptions(&self) -> PaginatorOptions<'_> {
        self.links.options(true)
    }

    pub fn getCursorName(&self) -> &str {
        &self.links.name
    }

    pub fn setCursorName(&mut self, name: impl Into<String>) -> &mut Self {
        self.links.name = name.into();
        self
    }

    pub fn path(&self) -> &str {
        &self.links.path
    }

    pub fn withPath(&mut self, path: impl Into<String>) -> &mut Self {
        self.setPath(path)
    }

    pub fn setPath(&mut self, path: impl Into<String>) -> &mut Self {
        self.links.path = path.into();
        self
    }

    pub fn url(&self, cursor: Option<&Cursor>) -> PaginationResult<String> {
        let encoded = cursor.map(Cursor::encode).transpose()?;
        Ok(self.links.url(encoded.as_deref()))
    }
    pub fn nextPageUrl(&self) -> PaginationResult<Option<String>> {
        self.nextCursor()
            .map(|cursor| self.url(Some(cursor)))
            .transpose()
    }
    pub fn previousPageUrl(&self) -> PaginationResult<Option<String>> {
        self.previousCursor()
            .map(|cursor| self.url(Some(cursor)))
            .transpose()
    }
    pub fn appends(&mut self, query: &[(String, String)]) -> &mut Self {
        self.links.appends(query);
        self
    }
    pub fn fragment(&mut self, fragment: Option<&str>) -> &mut Self {
        self.links.fragment = fragment.map(str::to_owned);
        self
    }
    pub fn withQueryString(&mut self) -> &mut Self {
        self.links.with_query_string();
        self
    }
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> CursorPaginator<U> {
        self.map_items(|items| items.into_iter().map(map).collect())
    }
    pub fn through<U>(self, transform: impl FnMut(T) -> U) -> CursorPaginator<U> {
        self.map(transform)
    }

    pub(crate) fn map_items<U>(self, map: impl FnOnce(Vec<T>) -> Vec<U>) -> CursorPaginator<U> {
        CursorPaginator {
            items: map(self.items),
            per_page: self.per_page,
            links: self.links,
            cursor: self.cursor,
            next: self.next,
            previous: self.previous,
            more: self.more,
        }
    }
}

impl<M: Model> CursorPaginator<M> {
    pub(crate) fn apply_model_collection(&mut self) {
        // Cursor results are indexed sequentially, even for explicitly keyed model collections.
        let (items, _) = M::newCollection(std::mem::take(&mut self.items)).into_parts();
        self.items = items;
    }
}

super::paginator_json!(CursorPaginator);

impl<T: Serialize> Serialize for CursorPaginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(Some(7))?;
        self.serialize_metadata(&mut fields)?;
        fields.serialize_entry("data", &self.items)?;
        fields.end()
    }
}

impl<T> CursorPaginator<T> {
    pub(crate) fn serialize_metadata<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error> {
        use serde::ser::Error as _;
        fields.serialize_entry("path", &self.links.path)?;
        fields.serialize_entry("per_page", &self.per_page)?;
        fields.serialize_entry(
            "next_cursor",
            &self
                .nextCursor()
                .map(Cursor::encode)
                .transpose()
                .map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "prev_cursor",
            &self
                .previousCursor()
                .map(Cursor::encode)
                .transpose()
                .map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "next_page_url",
            &self.nextPageUrl().map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "prev_page_url",
            &self.previousPageUrl().map_err(S::Error::custom)?,
        )
    }
}
