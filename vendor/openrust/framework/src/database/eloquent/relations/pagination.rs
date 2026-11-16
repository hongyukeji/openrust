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
    options.perPage = options.resolve_per_page(total)?;
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
    let size = page_size(options.perPage)?;
    let links = Links::current(options.cursorName);
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
    let links = Links::current(options.pageName);
    let page = links.current_page(options.page);
    let size = page_size(options.perPage)?;
    page_offset(page, size)?;
    Ok((links, page, size))
}

macro_rules! relation_pages {
    () => {
        pub async fn paginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> DatabaseResult<crate::pagination::LengthAwarePaginator<R>> {
            self.paginateWith(
                parent,
                crate::pagination::PageOptions {
                    perPage: Some(per_page),
                    ..Default::default()
                },
            )
            .await
        }

        pub async fn paginateWith(
            &self,
            parent: &P,
            options: crate::pagination::PageOptions<'_>,
        ) -> DatabaseResult<crate::pagination::LengthAwarePaginator<R>> {
            match self.pagination_target(parent)? {
                Some(query) => query.paginateWith(options).await,
                None => super::pagination::empty_length(crate::pagination::PageOptions {
                    perPage: options.perPage.or(Some(R::metadata().per_page)),
                    ..options
                }),
            }
        }

        pub async fn simplePaginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> DatabaseResult<crate::pagination::Paginator<R>> {
            self.simplePaginateWith(
                parent,
                crate::pagination::PageOptions {
                    perPage: Some(per_page),
                    ..Default::default()
                },
            )
            .await
        }

        pub async fn simplePaginateWith(
            &self,
            parent: &P,
            options: crate::pagination::PageOptions<'_>,
        ) -> DatabaseResult<crate::pagination::Paginator<R>> {
            match self.pagination_target(parent)? {
                Some(query) => query.simplePaginateWith(options).await,
                None => super::pagination::empty_simple(crate::pagination::PageOptions {
                    perPage: options.perPage.or(Some(R::metadata().per_page)),
                    ..options
                }),
            }
        }

        pub async fn cursorPaginate(
            &self,
            parent: &P,
            per_page: u64,
        ) -> DatabaseResult<crate::pagination::CursorPaginator<R>> {
            self.cursorPaginateWith(
                parent,
                crate::pagination::CursorOptions {
                    perPage: Some(per_page),
                    ..Default::default()
                },
            )
            .await
        }

        pub async fn cursorPaginateWith(
            &self,
            parent: &P,
            options: crate::pagination::CursorOptions<'_>,
        ) -> DatabaseResult<crate::pagination::CursorPaginator<R>> {
            match self.pagination_target(parent)? {
                Some(query) => query.cursorPaginateWith(options).await,
                None => super::pagination::empty_cursor(crate::pagination::CursorOptions {
                    perPage: options.perPage.or(Some(R::metadata().per_page)),
                    ..options
                }),
            }
        }
    };
}
pub(super) use relation_pages;
