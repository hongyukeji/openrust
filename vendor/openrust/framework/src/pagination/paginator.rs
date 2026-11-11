use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::Links;

/// An offset-based page that checks for one extra row.
#[derive(Debug)]
pub struct Paginator<T> {
    pub(super) items: Vec<T>,
    pub(super) page: u64,
    pub(super) per_page: u64,
    pub(super) links: Links,
    pub(super) more: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Paginator API")]
impl<T> Paginator<T> {
    pub(crate) fn new(mut items: Vec<T>, page: u64, per_page: u64, links: Links) -> Self {
        let more = items.len() as u64 > per_page;
        if more {
            items.pop();
        }
        Self {
            items,
            page,
            per_page,
            links,
            more,
        }
    }

    pub(crate) fn items_mut(&mut self) -> &mut [T] {
        &mut self.items
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }
    pub fn perPage(&self) -> u64 {
        self.per_page
    }
    pub fn currentPage(&self) -> u64 {
        self.page
    }
    pub fn hasMorePages(&self) -> bool {
        self.more
    }
    pub fn hasPages(&self) -> bool {
        self.page > 1 || self.more
    }

    pub fn firstItem(&self) -> Option<u64> {
        (!self.items.is_empty()).then(|| (self.page - 1) * self.per_page + 1)
    }
    pub fn lastItem(&self) -> Option<u64> {
        self.firstItem()?.checked_add(self.items.len() as u64 - 1)
    }
    pub fn url(&self, page: u64) -> String {
        self.links.url(Some(&page.max(1).to_string()))
    }
    pub fn nextPageUrl(&self) -> Option<String> {
        self.more
            .then(|| self.page.checked_add(1).map(|page| self.url(page)))
            .flatten()
    }
    pub fn previousPageUrl(&self) -> Option<String> {
        (self.page > 1).then(|| self.url(self.page - 1))
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
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> Paginator<U> {
        self.map_items(|items| items.into_iter().map(map).collect())
    }
    pub(crate) fn map_items<U>(self, map: impl FnOnce(Vec<T>) -> Vec<U>) -> Paginator<U> {
        Paginator {
            items: map(self.items),
            page: self.page,
            per_page: self.per_page,
            links: self.links,
            more: self.more,
        }
    }
    pub(crate) fn serialize_fields<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error>
    where
        T: Serialize,
    {
        fields.serialize_entry("current_page", &self.currentPage())?;
        fields.serialize_entry("data", &self.items)?;
        fields.serialize_entry("first_page_url", &self.url(1))?;
        fields.serialize_entry("from", &self.firstItem())?;
        fields.serialize_entry("next_page_url", &self.nextPageUrl())?;
        fields.serialize_entry("path", &self.links.path)?;
        fields.serialize_entry("per_page", &self.perPage())?;
        fields.serialize_entry("prev_page_url", &self.previousPageUrl())?;
        fields.serialize_entry("to", &self.lastItem())
    }
}

impl<T: Serialize> Serialize for Paginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(Some(9))?;
        self.serialize_fields(&mut fields)?;
        fields.end()
    }
}
