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
        let links = Links::manual(options.cursor_name.unwrap_or("cursor"), options);
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
        let backwards = cursor
            .as_ref()
            .is_some_and(Cursor::points_to_previous_items);
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

    pub fn links_with(&self, view: &str, data: ViewData) -> ViewResult<View> {
        self.render_with(view, data)
    }

    pub fn render(&self) -> ViewResult<View> {
        self.render_with(&super::views::DefaultViews::get(true)?, ViewData::new())
    }

    pub fn render_with(&self, view: &str, data: ViewData) -> ViewResult<View> {
        let paginator = super::views::paginator_data(
            self.has_pages(),
            self.previous_page_url()
                .map_err(|error| crate::view::ViewError::RenderFailed(Box::new(error)))?,
            self.next_page_url()
                .map_err(|error| crate::view::ViewError::RenderFailed(Box::new(error)))?,
        );
        super::views::make(view, data, paginator, None)
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }
    pub fn per_page(&self) -> u64 {
        self.per_page
    }
    pub fn cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }
    pub fn next_cursor(&self) -> Option<&Cursor> {
        self.next.as_ref()
    }
    pub fn previous_cursor(&self) -> Option<&Cursor> {
        self.previous.as_ref()
    }
    pub fn has_more_pages(&self) -> bool {
        self.more
            || self
                .cursor
                .as_ref()
                .is_some_and(Cursor::points_to_previous_items)
    }
    pub fn has_pages(&self) -> bool {
        !self.on_first_page() || self.has_more_pages()
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn is_not_empty(&self) -> bool {
        !self.is_empty()
    }

    pub fn on_first_page(&self) -> bool {
        self.cursor
            .as_ref()
            .is_none_or(|cursor| cursor.points_to_previous_items() && !self.more)
    }

    pub fn on_last_page(&self) -> bool {
        !self.has_more_pages()
    }

    pub fn get_options(&self) -> PaginatorOptions<'_> {
        self.links.options(true)
    }

    pub fn get_cursor_name(&self) -> &str {
        &self.links.name
    }

    pub fn set_cursor_name(&mut self, name: impl Into<String>) -> &mut Self {
        self.links.name = name.into();
        self
    }

    pub fn path(&self) -> &str {
        &self.links.path
    }

    pub fn with_path(&mut self, path: impl Into<String>) -> &mut Self {
        self.set_path(path)
    }

    pub fn set_path(&mut self, path: impl Into<String>) -> &mut Self {
        self.links.path = path.into();
        self
    }

    pub fn url(&self, cursor: Option<&Cursor>) -> PaginationResult<String> {
        let encoded = cursor.map(Cursor::encode).transpose()?;
        Ok(self.links.url(encoded.as_deref()))
    }
    pub fn next_page_url(&self) -> PaginationResult<Option<String>> {
        self.next_cursor()
            .map(|cursor| self.url(Some(cursor)))
            .transpose()
    }
    pub fn previous_page_url(&self) -> PaginationResult<Option<String>> {
        self.previous_cursor()
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
    pub fn with_query_string(&mut self) -> &mut Self {
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
        let (items, _) = M::new_collection(std::mem::take(&mut self.items)).into_parts();
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
                .next_cursor()
                .map(Cursor::encode)
                .transpose()
                .map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "prev_cursor",
            &self
                .previous_cursor()
                .map(Cursor::encode)
                .transpose()
                .map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "next_page_url",
            &self.next_page_url().map_err(S::Error::custom)?,
        )?;
        fields.serialize_entry(
            "prev_page_url",
            &self.previous_page_url().map_err(S::Error::custom)?,
        )
    }
}
