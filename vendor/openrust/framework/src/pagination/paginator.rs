use crate::{
    database::eloquent::Model,
    support::{ArrayKey, Collection},
    view::{View, ViewData, ViewResult},
};

use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::{Links, PaginationResult, PaginatorOptions, page_offset, page_size};

/// An offset-based page that checks for one extra row.
#[derive(Debug)]
pub struct Paginator<T> {
    pub(super) items: Vec<T>,
    pub(super) keys: Option<Vec<ArrayKey>>,
    pub(super) page: u64,
    pub(super) per_page: u64,
    pub(super) links: Links,
    pub(super) more: bool,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<T> Paginator<T> {
    /// Creates a paginator from the current slice plus one look-ahead item.
    pub fn new(
        items: Vec<T>,
        per_page: u64,
        page: Option<u64>,
        options: PaginatorOptions<'_>,
    ) -> PaginationResult<Self> {
        let size = page_size(Some(per_page))?;
        let links = Links::manual(options.pageName.unwrap_or("page"), options);
        let page = links.current_page(page);
        page_offset(page, size)?;
        Ok(Self::from_query(items, page, size, links))
    }

    pub(crate) fn from_query(mut items: Vec<T>, page: u64, per_page: u64, links: Links) -> Self {
        let more = items.len() as u64 > per_page;
        items.truncate(usize::try_from(per_page).unwrap_or(usize::MAX));
        Self {
            items,
            keys: None,
            page,
            per_page,
            links,
            more,
        }
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
            self.previousPageUrl(),
            self.nextPageUrl(),
        );
        super::views::make(view, data, paginator, None)
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn collection_keys(&self) -> Option<&[ArrayKey]> {
        self.keys.as_deref()
    }
    /// Looks up the collection key for an item on the current page.
    pub fn getByKey(&self, key: impl Into<ArrayKey>) -> Option<&T> {
        let key = key.into().normalized();
        if let Some(keys) = &self.keys {
            return keys
                .iter()
                .position(|candidate| *candidate == key)
                .map(|index| &self.items[index]);
        }
        match key {
            ArrayKey::Integer(index) => usize::try_from(index)
                .ok()
                .and_then(|index| self.items.get(index)),
            ArrayKey::String(_) => None,
        }
    }

    pub fn perPage(&self) -> u64 {
        self.per_page
    }
    pub fn currentPage(&self) -> u64 {
        self.page
    }
    /// Overrides the look-ahead result when a search engine provides a total.
    pub fn hasMorePagesWhen(&mut self, more: bool) -> &mut Self {
        self.more = more;
        self
    }

    pub fn hasMorePages(&self) -> bool {
        self.more
    }
    pub fn hasPages(&self) -> bool {
        self.page > 1 || self.more
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
        self.currentPage() == 1
    }

    pub fn onLastPage(&self) -> bool {
        !self.hasMorePages()
    }

    pub fn getOptions(&self) -> PaginatorOptions<'_> {
        self.links.options(false)
    }

    pub fn getPageName(&self) -> &str {
        &self.links.name
    }

    pub fn setPageName(&mut self, name: impl Into<String>) -> &mut Self {
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

    pub fn getUrlRange(&self, start: u64, end: u64) -> impl Iterator<Item = (u64, String)> + '_ {
        (start..=end).map(|page| (page, self.url(page)))
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
    pub fn through<U>(self, transform: impl FnMut(T) -> U) -> Paginator<U> {
        self.map(transform)
    }

    pub(crate) fn map_items<U>(self, map: impl FnOnce(Vec<T>) -> Vec<U>) -> Paginator<U> {
        Paginator {
            items: map(self.items),
            keys: self.keys,
            page: self.page,
            per_page: self.per_page,
            links: self.links,
            more: self.more,
        }
    }
    pub(crate) fn serialize_metadata<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error> {
        fields.serialize_entry("current_page", &self.currentPage())?;
        fields.serialize_entry("first_page_url", &self.url(1))?;
        fields.serialize_entry("from", &self.firstItem())?;
        fields.serialize_entry("next_page_url", &self.nextPageUrl())?;
        fields.serialize_entry("path", &self.links.path)?;
        fields.serialize_entry("per_page", &self.perPage())?;
        fields.serialize_entry("prev_page_url", &self.previousPageUrl())?;
        fields.serialize_entry("to", &self.lastItem())
    }
}

impl<M: Model> Paginator<M> {
    pub(crate) fn apply_model_collection(&mut self) {
        (self.items, self.keys) = M::newCollection(std::mem::take(&mut self.items)).into_parts();
    }
}

impl<T: Serialize> Paginator<T> {
    pub(super) fn serialize_items<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error> {
        match &self.keys {
            Some(keys) => fields.serialize_entry(
                "data",
                &Collection::fromEntries(keys.iter().zip(&self.items)),
            ),
            None => fields.serialize_entry("data", &self.items),
        }
    }
}

/// Application-wide view defaults shared by all three paginator types.
#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Paginator<()> {
    pub fn defaultView(view: impl Into<String>) -> ViewResult<()> {
        super::views::DefaultViews::configure(Some(view.into()), None)
    }

    pub fn defaultSimpleView(view: impl Into<String>) -> ViewResult<()> {
        super::views::DefaultViews::configure(None, Some(view.into()))
    }

    pub fn useTailwind() -> ViewResult<()> {
        Self::use_views(
            super::views::DEFAULT_VIEW,
            super::views::DEFAULT_SIMPLE_VIEW,
        )
    }

    pub fn useBootstrap() -> ViewResult<()> {
        Self::useBootstrapFour()
    }

    pub fn useBootstrapThree() -> ViewResult<()> {
        Self::use_views("pagination::bootstrap-3", "pagination::simple-bootstrap-3")
    }

    pub fn useBootstrapFour() -> ViewResult<()> {
        Self::use_views("pagination::bootstrap-4", "pagination::simple-bootstrap-4")
    }

    pub fn useBootstrapFive() -> ViewResult<()> {
        Self::use_views("pagination::bootstrap-5", "pagination::simple-bootstrap-5")
    }

    fn use_views(view: &str, simple: &str) -> ViewResult<()> {
        super::views::DefaultViews::configure(Some(view.into()), Some(simple.into()))
    }
}

super::paginator_json!(Paginator);

impl<T: Serialize> Serialize for Paginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(Some(9))?;
        self.serialize_metadata(&mut fields)?;
        self.serialize_items(&mut fields)?;
        fields.end()
    }
}
