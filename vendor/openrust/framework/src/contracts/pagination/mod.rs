//! Pagination contracts used by query results and resources.

#![allow(non_snake_case, reason = "保留既定公开接口及命名")]

use crate::pagination::{self, Cursor, PaginationResult};

pub trait Paginator {
    type Item;

    fn items(&self) -> &[Self::Item];
    fn per_page(&self) -> u64;
    fn current_page(&self) -> u64;
    fn next_page_url(&self) -> Option<String>;
    fn previous_page_url(&self) -> Option<String>;
}

pub trait LengthAwarePaginator: Paginator {
    fn total(&self) -> u64;
    fn last_page(&self) -> u64;
}

pub trait CursorPaginator {
    type Item;

    fn items(&self) -> &[Self::Item];
    fn per_page(&self) -> u64;
    fn next_cursor(&self) -> Option<&Cursor>;
    fn previous_cursor(&self) -> Option<&Cursor>;
    fn next_page_url(&self) -> PaginationResult<Option<String>>;
    fn previous_page_url(&self) -> PaginationResult<Option<String>>;
}

macro_rules! offset_paginator {
    ($name:ident) => {
        impl<T> Paginator for pagination::$name<T> {
            type Item = T;
            fn items(&self) -> &[T] {
                pagination::Paginator::items(self)
            }
            fn per_page(&self) -> u64 {
                pagination::Paginator::per_page(self)
            }
            fn current_page(&self) -> u64 {
                pagination::Paginator::current_page(self)
            }
            fn next_page_url(&self) -> Option<String> {
                pagination::Paginator::next_page_url(self)
            }
            fn previous_page_url(&self) -> Option<String> {
                pagination::Paginator::previous_page_url(self)
            }
        }
    };
}

offset_paginator!(Paginator);
offset_paginator!(LengthAwarePaginator);

impl<T> LengthAwarePaginator for pagination::LengthAwarePaginator<T> {
    fn total(&self) -> u64 {
        self.total()
    }
    fn last_page(&self) -> u64 {
        self.last_page()
    }
}

impl<T> CursorPaginator for pagination::CursorPaginator<T> {
    type Item = T;
    fn items(&self) -> &[T] {
        self.items()
    }
    fn per_page(&self) -> u64 {
        self.per_page()
    }
    fn next_cursor(&self) -> Option<&Cursor> {
        self.next_cursor()
    }
    fn previous_cursor(&self) -> Option<&Cursor> {
        self.previous_cursor()
    }
    fn next_page_url(&self) -> PaginationResult<Option<String>> {
        self.next_page_url()
    }
    fn previous_page_url(&self) -> PaginationResult<Option<String>> {
        self.previous_page_url()
    }
}
