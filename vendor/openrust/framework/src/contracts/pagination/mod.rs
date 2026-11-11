//! Pagination contracts used by query results and resources.

#![allow(non_snake_case, reason = "Matches Laravel's paginator contracts")]

use crate::pagination::{self, Cursor, PaginationResult};

pub trait Paginator {
    type Item;

    fn items(&self) -> &[Self::Item];
    fn perPage(&self) -> u64;
    fn currentPage(&self) -> u64;
    fn nextPageUrl(&self) -> Option<String>;
    fn previousPageUrl(&self) -> Option<String>;
}

pub trait LengthAwarePaginator: Paginator {
    fn total(&self) -> u64;
    fn lastPage(&self) -> u64;
}

pub trait CursorPaginator {
    type Item;

    fn items(&self) -> &[Self::Item];
    fn perPage(&self) -> u64;
    fn nextCursor(&self) -> Option<&Cursor>;
    fn previousCursor(&self) -> Option<&Cursor>;
    fn nextPageUrl(&self) -> PaginationResult<Option<String>>;
    fn previousPageUrl(&self) -> PaginationResult<Option<String>>;
}

macro_rules! offset_paginator {
    ($name:ident) => {
        impl<T> Paginator for pagination::$name<T> {
            type Item = T;
            fn items(&self) -> &[T] {
                pagination::Paginator::items(self)
            }
            fn perPage(&self) -> u64 {
                pagination::Paginator::perPage(self)
            }
            fn currentPage(&self) -> u64 {
                pagination::Paginator::currentPage(self)
            }
            fn nextPageUrl(&self) -> Option<String> {
                pagination::Paginator::nextPageUrl(self)
            }
            fn previousPageUrl(&self) -> Option<String> {
                pagination::Paginator::previousPageUrl(self)
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
    fn lastPage(&self) -> u64 {
        self.lastPage()
    }
}

impl<T> CursorPaginator for pagination::CursorPaginator<T> {
    type Item = T;
    fn items(&self) -> &[T] {
        self.items()
    }
    fn perPage(&self) -> u64 {
        self.perPage()
    }
    fn nextCursor(&self) -> Option<&Cursor> {
        self.nextCursor()
    }
    fn previousCursor(&self) -> Option<&Cursor> {
        self.previousCursor()
    }
    fn nextPageUrl(&self) -> PaginationResult<Option<String>> {
        self.nextPageUrl()
    }
    fn previousPageUrl(&self) -> PaginationResult<Option<String>> {
        self.previousPageUrl()
    }
}
