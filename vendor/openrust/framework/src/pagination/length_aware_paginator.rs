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
        self.inner.serialize_fields(&mut fields)?;
        let last = self.lastPage();
        fields.serialize_entry("last_page", &last)?;
        fields.serialize_entry("last_page_url", &self.url(last))?;
        fields.serialize_entry("links", &self.link_window())?;
        fields.serialize_entry("total", &self.total)?;
        fields.end()
    }
}

impl<T> LengthAwarePaginator<T> {
    fn link_window(&self) -> Vec<serde_json::Value> {
        use serde_json::json;
        let page = self.currentPage();
        let last = self.lastPage();
        let side = u64::from(self.on_each_side);
        let window = side + 4;
        let mut links = vec![json!({
            "url":self.previousPageUrl(),"label":"Previous",
            "page":page.checked_sub(1).filter(|previous| *previous > 0),"active":false
        })];
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
        links.push(json!({
            "url":self.nextPageUrl(),"label":"Next",
            "page":self.hasMorePages().then(|| page + 1),"active":false
        }));
        links
    }
}
