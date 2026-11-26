use crate::view::{View, ViewData, ViewResult};

use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::{
    Links, PaginationResult, Paginator, PaginatorOptions, UrlWindow, page_offset, page_size,
};

/// An offset-based page with a total result count.
#[derive(Debug)]
pub struct LengthAwarePaginator<T> {
    inner: Paginator<T>,
    total: u64,
    on_each_side: u8,
}

impl<T> LengthAwarePaginator<T> {
    /// Creates a paginator from an already sliced page and the unsliced total.
    pub fn new(
        items: Vec<T>,
        total: u64,
        per_page: u64,
        page: Option<u64>,
        options: PaginatorOptions<'_>,
    ) -> PaginationResult<Self> {
        let size = if per_page == 0 {
            0
        } else {
            page_size(Some(per_page))?
        };
        let links = Links::manual(options.page_name.unwrap_or("page"), options);
        let page = links.current_page(page);
        page_offset(page, size)?;
        Ok(Self::from_query(items, total, page, size, links))
    }

    pub(crate) fn from_query(
        items: Vec<T>,
        total: u64,
        page: u64,
        per_page: u64,
        links: Links,
    ) -> Self {
        Self {
            inner: Paginator {
                items,
                keys: None,
                page,
                per_page,
                links,
                more: page < total.div_ceil(per_page.max(1)),
            },
            total,
            on_each_side: 3,
        }
    }

    pub fn on_each_side(&mut self, pages: u8) -> &mut Self {
        self.on_each_side = pages;
        self
    }

    pub(super) fn window_side_count(&self) -> u8 {
        self.on_each_side
    }

    pub fn links(&self) -> ViewResult<View> {
        self.render()
    }

    pub fn links_with(&self, view: &str, data: ViewData) -> ViewResult<View> {
        self.render_with(view, data)
    }

    pub fn render(&self) -> ViewResult<View> {
        self.render_with(&super::views::DefaultViews::get(false)?, ViewData::new())
    }

    pub fn render_with(&self, view: &str, data: ViewData) -> ViewResult<View> {
        let mut paginator = super::views::paginator_data(
            self.has_pages(),
            self.previous_page_url(),
            self.next_page_url(),
        );
        paginator["first_item"] = serde_json::json!(self.first_item());
        paginator["last_item"] = serde_json::json!(self.last_item());
        paginator["total"] = serde_json::json!(self.total());
        paginator["current_page"] = serde_json::json!(self.current_page());
        paginator["last_page"] = serde_json::json!(self.last_page());
        super::views::make(
            view,
            data,
            paginator,
            Some(serde_json::Value::Array(self.elements())),
        )
    }

    pub fn total(&self) -> u64 {
        self.total
    }
    pub fn last_page(&self) -> u64 {
        self.total.div_ceil(self.inner.per_page().max(1)).max(1)
    }
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> LengthAwarePaginator<U> {
        LengthAwarePaginator {
            inner: self.inner.map(map),
            total: self.total,
            on_each_side: self.on_each_side,
        }
    }
    pub fn through<U>(self, transform: impl FnMut(T) -> U) -> LengthAwarePaginator<U> {
        self.map(transform)
    }

    pub(crate) fn map_items<U>(
        self,
        map: impl FnOnce(Vec<T>) -> Vec<U>,
    ) -> LengthAwarePaginator<U> {
        LengthAwarePaginator {
            inner: self.inner.map_items(map),
            total: self.total,
            on_each_side: self.on_each_side,
        }
    }
}

impl<T> std::ops::Deref for LengthAwarePaginator<T> {
    type Target = Paginator<T>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> std::ops::DerefMut for LengthAwarePaginator<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

super::paginator_json!(LengthAwarePaginator);

impl<T: Serialize> Serialize for LengthAwarePaginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(Some(13))?;
        self.serialize_metadata(&mut fields)?;
        self.inner.serialize_items(&mut fields)?;
        fields.end()
    }
}

impl<T> LengthAwarePaginator<T> {
    pub(crate) fn serialize_metadata<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error> {
        self.inner.serialize_metadata(fields)?;
        let last = self.last_page();
        fields.serialize_entry("last_page", &last)?;
        fields.serialize_entry("last_page_url", &self.url(last))?;
        fields.serialize_entry("links", &self.link_collection())?;
        fields.serialize_entry("total", &self.total)
    }

    pub fn link_collection(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let page = self.current_page();
        let mut links = vec![json!({
            "url":self.previous_page_url(),"label":super::views::navigation_label(true),
            "page":page.checked_sub(1).filter(|previous| *previous > 0),"active":false
        })];
        links.extend(self.elements());
        links.push(json!({
            "url":self.next_page_url(),"label":super::views::navigation_label(false),
            "page":self.has_more_pages().then(|| page + 1),"active":false
        }));
        links
    }

    fn elements(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let page = self.current_page();
        let window = UrlWindow::make(self);
        let mut links = Vec::new();
        for (index, pages) in [window.first, window.slider, window.last]
            .into_iter()
            .flatten()
            .enumerate()
        {
            if index > 0 {
                links.push(json!({"url":null,"label":"...","active":false}));
            }
            links.extend(pages.into_iter().map(|(current, url)| {
                json!({
                    "url":url,"label":current.to_string(),"page":current,"active":current == page
                })
            }));
        }
        links
    }
}
