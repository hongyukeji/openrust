// Keep the public forwarding methods identical on relationships with the same chunk contract.
macro_rules! key_chunks {
    ($collection:ty) => {
        #[track_caller]
        pub fn chunk_by_id<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut($collection, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> impl Future<Output = crate::database::DatabaseResult<bool>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.ordered_chunk_by_id(parent, count, callback, column, alias, false)
        }

        #[track_caller]
        pub fn chunk_by_id_desc<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut($collection, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> impl Future<Output = crate::database::DatabaseResult<bool>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.ordered_chunk_by_id(parent, count, callback, column, alias, true)
        }
    };
    (forward $field:tt) => {
        super::key_chunks::key_chunks!(crate::database::eloquent::Collection<R>);

        #[track_caller]
        pub fn each<Fut>(
            &self,
            parent: &P,
            callback: impl FnMut(R, crate::support::ArrayKey) -> Fut,
            count: u64,
        ) -> impl Future<Output = crate::database::DatabaseResult<bool>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
                self.$field.each(parent, callback, count).await
            })
        }

        #[track_caller]
        pub fn each_by_id<Fut>(
            &self,
            parent: &P,
            callback: impl FnMut(R, i64) -> Fut,
            count: u64,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> impl Future<Output = crate::database::DatabaseResult<bool>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
                self.$field
                    .each_by_id(parent, callback, count, column, alias)
                    .await
            })
        }

        #[track_caller]
        pub fn ordered_chunk_by_id<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
            descending: bool,
        ) -> impl Future<Output = crate::database::DatabaseResult<bool>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
                self.$field
                    .ordered_chunk_by_id(parent, count, callback, column, alias, descending)
                    .await
            })
        }
    };
}
pub(super) use key_chunks;

macro_rules! lazy_chunks {
    ($target:ty) => {
        #[track_caller]
        pub fn lazy<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            let caller = std::panic::Location::caller();
            let stream = self.lazy_relation(parent, chunk_size, None);
            Box::pin(crate::database::events::stream_with_query_caller(
                caller, stream,
            ))
        }

        #[track_caller]
        pub fn lazy_by_id<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
            column: Option<&'a str>,
            alias: Option<&'a str>,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            let caller = std::panic::Location::caller();
            let stream = self.lazy_relation(parent, chunk_size, Some((column, alias, false)));
            Box::pin(crate::database::events::stream_with_query_caller(
                caller, stream,
            ))
        }

        #[track_caller]
        pub fn lazy_by_id_desc<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
            column: Option<&'a str>,
            alias: Option<&'a str>,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            let caller = std::panic::Location::caller();
            let stream = self.lazy_relation(parent, chunk_size, Some((column, alias, true)));
            Box::pin(crate::database::events::stream_with_query_caller(
                caller, stream,
            ))
        }

        #[track_caller]
        pub fn chunk_map<T, Fut>(
            &self,
            parent: &P,
            callback: impl FnMut($target) -> Fut,
            count: u64,
        ) -> impl Future<Output = crate::database::DatabaseResult<crate::support::Collection<T>>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<T>>,
        {
            crate::database::events::with_query_caller(std::panic::Location::caller(), async move {
                crate::database::query::map_chunk_rows(self.lazy(parent, count), callback).await
            })
        }
    };
    (forward $field:tt) => {
        super::key_chunks::lazy_chunks!(R);

        pub(super) fn lazy_relation<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
            key: Option<(Option<&'a str>, Option<&'a str>, bool)>,
        ) -> crate::database::DatabaseCursor<'a, R> {
            self.$field.lazy_relation(parent, chunk_size, key)
        }
    };
}

pub(super) use lazy_chunks;
