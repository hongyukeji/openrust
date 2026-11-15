use crate::view::{View, ViewData, ViewResult};

use serde::{Serialize, Serializer, ser::SerializeMap as _};

use super::{Links, Paginator};

/// An offset-based page with a total result count.
#[derive(Debug)]
pub struct LengthAwarePaginator<T> {
    inner: Paginator<T>,
    total: u64,
    on_each_side: u8,
}

#[allow(non_snake_case, reason = "Matches Laravel's LengthAwarePaginator API")]
impl<T> LengthAwarePaginator<T> {
    pub(crate) fn new(items: Vec<T>, total: u64, page: u64, per_page: u64, links: Links) -> Self {
        let mut inner = Paginator::new(items, page, per_page, links);
        inner.more = page < total.div_ceil(per_page);
        Self {
            inner,
            total,
            on_each_side: 3,
        }
    }

    pub fn onEachSide(&mut self, pages: u8) -> &mut Self {
        self.on_each_side = pages;
        self
    }

    pub fn links(&self) -> ViewResult<View> {
        self.render()
    }

    pub fn linksWith(&self, view: &str, data: ViewData) -> ViewResult<View> {
        self.renderWith(view, data)
    }

    pub fn render(&self) -> ViewResult<View> {
        self.renderWith(super::views::DEFAULT_VIEW, ViewData::new())
    }

    pub fn renderWith(&self, view: &str, data: ViewData) -> ViewResult<View> {
        let mut paginator = super::views::paginator_data(
            self.hasPages(),
            self.previousPageUrl(),
            self.nextPageUrl(),
        );
        paginator["first_item"] = serde_json::json!(self.firstItem());
        paginator["last_item"] = serde_json::json!(self.lastItem());
        paginator["total"] = serde_json::json!(self.total());
        paginator["current_page"] = serde_json::json!(self.currentPage());
        paginator["last_page"] = serde_json::json!(self.lastPage());
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
    pub fn lastPage(&self) -> u64 {
        self.total.div_ceil(self.inner.perPage()).max(1)
    }
    pub fn getUrlRange(&self, start: u64, end: u64) -> impl Iterator<Item = (u64, String)> + '_ {
        (start..=end).map(|page| (page, self.inner.url(page)))
    }
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> LengthAwarePaginator<U> {
        LengthAwarePaginator {
            inner: self.inner.map(map),
            total: self.total,
            on_each_side: self.on_each_side,
        }
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

impl<T: Serialize> Serialize for LengthAwarePaginator<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields = serializer.serialize_map(Some(13))?;
        self.serialize_metadata(&mut fields)?;
        fields.serialize_entry("data", self.items())?;
        fields.end()
    }
}

impl<T> LengthAwarePaginator<T> {
    pub(crate) fn serialize_metadata<S: serde::ser::SerializeMap>(
        &self,
        fields: &mut S,
    ) -> Result<(), S::Error> {
        self.inner.serialize_metadata(fields)?;
        let last = self.lastPage();
        fields.serialize_entry("last_page", &last)?;
        fields.serialize_entry("last_page_url", &self.url(last))?;
        fields.serialize_entry("links", &self.link_window())?;
        fields.serialize_entry("total", &self.total)
    }

    fn link_window(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let page = self.currentPage();
        let mut links = vec![json!({
            "url":self.previousPageUrl(),"label":super::views::navigation_label(true),
            "page":page.checked_sub(1).filter(|previous| *previous > 0),"active":false
        })];
        links.extend(self.elements());
        links.push(json!({
            "url":self.nextPageUrl(),"label":super::views::navigation_label(false),
            "page":self.hasMorePages().then(|| page + 1),"active":false
        }));
        links
    }

    fn elements(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let page = self.currentPage();
        let last = self.lastPage();
        let side = u64::from(self.on_each_side);
        let window = side + 4;
        let mut links = Vec::new();
        let pages = if last < side * 2 + 8 {
            vec![1..=last]
        } else if page <= window {
            vec![1..=window + side, last - 1..=last]
        } else if page > last - window {
            vec![1..=2, last - (window + side - 1)..=last]
        } else {
            vec![1..=2, page - side..=page + side, last - 1..=last]
        };
        for (index, pages) in pages.into_iter().enumerate() {
            if index > 0 {
                links.push(json!({"url":null,"label":"...","active":false}));
            }
            links.extend(pages.map(|current| json!({
                "url":self.url(current),"label":current.to_string(),"page":current,"active":current == page
            })));
        }
        links
    }
}
