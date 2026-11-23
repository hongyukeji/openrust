use super::LengthAwarePaginator;

type UrlRange = Vec<(u64, String)>;

/// The opening pages, optional sliding window, and closing pages.
#[derive(Debug, PartialEq, Eq)]
pub struct UrlWindowLinks {
    pub first: Option<UrlRange>,
    pub slider: Option<UrlRange>,
    pub last: Option<UrlRange>,
}

/// Generates the numbered URL segments shown by length-aware pagination.
pub struct UrlWindow<'a, T> {
    paginator: &'a LengthAwarePaginator<T>,
}

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl<'a, T> UrlWindow<'a, T> {
    pub fn new(paginator: &'a LengthAwarePaginator<T>) -> Self {
        Self { paginator }
    }

    pub fn make(paginator: &'a LengthAwarePaginator<T>) -> UrlWindowLinks {
        Self::new(paginator).get()
    }

    pub fn get(&self) -> UrlWindowLinks {
        let page = self.paginator.currentPage();
        let last = self.paginator.lastPage();
        let on_each_side = self.paginator.on_each_side();
        let side = u64::from(on_each_side);
        let window = side + 4;

        let (first, slider, last) = if last < side * 2 + 8 {
            (Some(self.range(1, last)), None, None)
        } else if !self.hasPages() {
            (None, None, None)
        } else if page <= window {
            (
                Some(self.range(1, window + side)),
                None,
                Some(self.getFinish()),
            )
        } else if page > last - window {
            (
                Some(self.getStart()),
                None,
                Some(self.range(last - (window + side - 1), last)),
            )
        } else {
            (
                Some(self.getStart()),
                Some(self.getAdjacentUrlRange(on_each_side)),
                Some(self.getFinish()),
            )
        };
        UrlWindowLinks {
            first,
            slider,
            last,
        }
    }

    pub fn hasPages(&self) -> bool {
        self.paginator.hasPages()
    }

    pub fn getAdjacentUrlRange(&self, on_each_side: u8) -> UrlRange {
        let page = self.paginator.currentPage();
        let side = u64::from(on_each_side);
        self.range(page.saturating_sub(side), page.saturating_add(side))
    }

    pub fn getStart(&self) -> UrlRange {
        self.range(1, 2)
    }

    pub fn getFinish(&self) -> UrlRange {
        let last = self.paginator.lastPage();
        self.range(last - 1, last)
    }

    fn range(&self, start: u64, end: u64) -> UrlRange {
        self.paginator.getUrlRange(start, end).collect()
    }
}
