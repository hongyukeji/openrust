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

impl<'a, T> UrlWindow<'a, T> {
    pub fn new(paginator: &'a LengthAwarePaginator<T>) -> Self {
        Self { paginator }
    }

    pub fn make(paginator: &'a LengthAwarePaginator<T>) -> UrlWindowLinks {
        Self::new(paginator).get()
    }

    pub fn get(&self) -> UrlWindowLinks {
        let page = self.paginator.current_page();
        let last = self.paginator.last_page();
        let on_each_side = self.paginator.window_side_count();
        let side = u64::from(on_each_side);
        let window = side + 4;

        let (first, slider, last) = if last < side * 2 + 8 {
            (Some(self.range(1, last)), None, None)
        } else if !self.has_pages() {
            (None, None, None)
        } else if page <= window {
            (
                Some(self.range(1, window + side)),
                None,
                Some(self.get_finish()),
            )
        } else if page > last - window {
            (
                Some(self.get_start()),
                None,
                Some(self.range(last - (window + side - 1), last)),
            )
        } else {
            (
                Some(self.get_start()),
                Some(self.get_adjacent_url_range(on_each_side)),
                Some(self.get_finish()),
            )
        };
        UrlWindowLinks {
            first,
            slider,
            last,
        }
    }

    pub fn has_pages(&self) -> bool {
        self.paginator.has_pages()
    }

    pub fn get_adjacent_url_range(&self, on_each_side: u8) -> UrlRange {
        let page = self.paginator.current_page();
        let side = u64::from(on_each_side);
        self.range(page.saturating_sub(side), page.saturating_add(side))
    }

    pub fn get_start(&self) -> UrlRange {
        self.range(1, 2)
    }

    pub fn get_finish(&self) -> UrlRange {
        let last = self.paginator.last_page();
        self.range(last - 1, last)
    }

    fn range(&self, start: u64, end: u64) -> UrlRange {
        self.paginator.get_url_range(start, end).collect()
    }
}
