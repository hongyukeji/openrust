use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::{Cursor, Links, PaginationResult};

/// A page positioned by ordered column values.
#[derive(Debug)]
pub struct CursorPaginator<T> {
    pub(crate) items: Vec<T>,
    pub(crate) per_page: u64,
    pub(crate) links: Links,
    pub(crate) cursor: Option<Cursor>,
    pub(crate) next: Option<Cursor>,
    pub(crate) previous: Option<Cursor>,
}

#[allow(non_snake_case, reason = "Matches Laravel's CursorPaginator API")]
impl<T> CursorPaginator<T> {
    pub(crate) fn items_mut(&mut self) -> &mut [T] {
        &mut self.items
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
        self.next.is_some()
    }
    pub fn hasPages(&self) -> bool {
        self.cursor.is_some() || self.hasMorePages()
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
    pub(crate) fn map_items<U>(self, map: impl FnOnce(Vec<T>) -> Vec<U>) -> CursorPaginator<U> {
        CursorPaginator {
            items: map(self.items),
            per_page: self.per_page,
            links: self.links,
            cursor: self.cursor,
            next: self.next,
            previous: self.previous,
        }
    }
}

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
