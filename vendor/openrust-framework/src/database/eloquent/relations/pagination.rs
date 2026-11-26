use crate::{
    database::DatabaseResult,
    pagination::{
        CursorOptions, CursorPaginator, LengthAwarePaginator, Links, PageOptions, PaginationError,
        Paginator, page_offset, page_size,
    },
};

pub(super) fn empty_length<T>(
    mut options: PageOptions<'_>,
) -> DatabaseResult<LengthAwarePaginator<T>> {
    let total = options.resolve_total()?.unwrap_or(0);
    options.per_page = options.resolve_per_page(total)?;
    let (links, page, size) = empty_offset(&options)?;
    Ok(LengthAwarePaginator::from_query(
        Vec::new(),
        total,
        page,
        size,
        links,
    ))
}

pub(super) fn empty_simple<T>(options: PageOptions<'_>) -> DatabaseResult<Paginator<T>> {
    let (links, page, size) = empty_offset(&options)?;
    Ok(Paginator::from_query(Vec::new(), page, size, links))
}

pub(super) fn empty_cursor<T>(options: CursorOptions<'_>) -> DatabaseResult<CursorPaginator<T>> {
    let size = page_size(options.per_page)?;
    let links = Links::current(options.cursor_name);
    let cursor = links.current_cursor(options.cursor);
    Ok(CursorPaginator::from_ordered(
        Vec::new(),
        size,
        links,
        cursor,
        |_, _| Err(PaginationError::InvalidCursor),
    )?)
}

fn empty_offset(options: &PageOptions<'_>) -> DatabaseResult<(Links, u64, u64)> {
    let links = Links::current(options.page_name);
    let page = links.current_page(options.page);
    let size = page_size(options.per_page)?;
    page_offset(page, size)?;
    Ok((links, page, size))
}

macro_rules! relation_pages {
    () => {
        #[track_caller]
        pub fn paginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::LengthAwarePaginator<R>>> {
            self.paginate_with(
                parent,
                crate::pagination::PageOptions {
                    per_page: Some(per_page),
                    ..Default::default()
                },
            )
        }

        #[track_caller]
        pub fn paginate_with(
            &self,
            parent: &P,
            options: crate::pagination::PageOptions<'_>,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::LengthAwarePaginator<R>>> {
            let caller = std::panic::Location::caller();
            $crate::database::events::with_query_caller(caller, async move {
                match self.pagination_target(parent)? {
                    Some(query) => query.paginate_with(options).await,
                    None => super::pagination::empty_length(crate::pagination::PageOptions {
                        per_page: options.per_page.or(Some(R::metadata().per_page)),
                        ..options
                    }),
                }
            })
        }

        #[track_caller]
        pub fn simple_paginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::Paginator<R>>> {
            self.simple_paginate_with(
                parent,
                crate::pagination::PageOptions {
                    per_page: Some(per_page),
                    ..Default::default()
                },
            )
        }

        #[track_caller]
        pub fn simple_paginate_with(
            &self,
            parent: &P,
            options: crate::pagination::PageOptions<'_>,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::Paginator<R>>> {
            let caller = std::panic::Location::caller();
            $crate::database::events::with_query_caller(caller, async move {
                match self.pagination_target(parent)? {
                    Some(query) => query.simple_paginate_with(options).await,
                    None => super::pagination::empty_simple(crate::pagination::PageOptions {
                        per_page: options.per_page.or(Some(R::metadata().per_page)),
                        ..options
                    }),
                }
            })
        }

        #[track_caller]
        pub fn cursor_paginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::CursorPaginator<R>>> {
            self.cursor_paginate_with(
                parent,
                crate::pagination::CursorOptions {
                    per_page: Some(per_page),
                    ..Default::default()
                },
            )
        }

        #[track_caller]
        pub fn cursor_paginate_with(
            &self,
            parent: &P,
            options: crate::pagination::CursorOptions<'_>,
        ) -> impl Future<Output = DatabaseResult<crate::pagination::CursorPaginator<R>>> {
            let caller = std::panic::Location::caller();
            $crate::database::events::with_query_caller(caller, async move {
                match self.pagination_target(parent)? {
                    Some(query) => query.cursor_paginate_with(options).await,
                    None => super::pagination::empty_cursor(crate::pagination::CursorOptions {
                        per_page: options.per_page.or(Some(R::metadata().per_page)),
                        ..options
                    }),
                }
            })
        }
    };
}
pub(super) use relation_pages;
