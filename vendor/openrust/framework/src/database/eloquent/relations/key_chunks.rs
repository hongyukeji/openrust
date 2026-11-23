// Keep the public forwarding methods identical on relationships with the same chunk contract.
macro_rules! key_chunks {
    ($collection:ty) => {
        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub async fn chunkById<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut($collection, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> crate::database::DatabaseResult<bool>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.orderedChunkById(parent, count, callback, column, alias, false)
                .await
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub async fn chunkByIdDesc<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut($collection, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> crate::database::DatabaseResult<bool>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.orderedChunkById(parent, count, callback, column, alias, true)
                .await
        }
    };
    (forward $field:tt) => {
        super::key_chunks::key_chunks!(crate::database::eloquent::Collection<R>);

        pub async fn each<Fut>(
            &self,
            parent: &P,
            callback: impl FnMut(R, crate::support::ArrayKey) -> Fut,
            count: u64,
        ) -> crate::database::DatabaseResult<bool>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.$field.each(parent, callback, count).await
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub async fn eachById<Fut>(
            &self,
            parent: &P,
            callback: impl FnMut(R, i64) -> Fut,
            count: u64,
            column: Option<&str>,
            alias: Option<&str>,
        ) -> crate::database::DatabaseResult<bool>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.$field
                .eachById(parent, callback, count, column, alias)
                .await
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub async fn orderedChunkById<Fut>(
            &self,
            parent: &P,
            count: u64,
            callback: impl FnMut(crate::database::eloquent::Collection<R>, u64) -> Fut,
            column: Option<&str>,
            alias: Option<&str>,
            descending: bool,
        ) -> crate::database::DatabaseResult<bool>
        where
            Fut: Future<Output = crate::database::DatabaseResult<bool>>,
        {
            self.$field
                .orderedChunkById(parent, count, callback, column, alias, descending)
                .await
        }
    };
}
pub(super) use key_chunks;

macro_rules! lazy_chunks {
    ($target:ty) => {
        pub fn lazy<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            self.lazy_relation(parent, chunk_size, None)
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub fn lazyById<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
            column: Option<&'a str>,
            alias: Option<&'a str>,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            self.lazy_relation(parent, chunk_size, Some((column, alias, false)))
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub fn lazyByIdDesc<'a>(
            &'a self,
            parent: &'a P,
            chunk_size: u64,
            column: Option<&'a str>,
            alias: Option<&'a str>,
        ) -> crate::database::DatabaseCursor<'a, $target> {
            self.lazy_relation(parent, chunk_size, Some((column, alias, true)))
        }

        #[allow(non_snake_case, reason = "保留既定公开接口及命名")]
        pub async fn chunkMap<T, Fut>(
            &self,
            parent: &P,
            callback: impl FnMut($target) -> Fut,
            count: u64,
        ) -> crate::database::DatabaseResult<crate::support::Collection<T>>
        where
            Fut: Future<Output = crate::database::DatabaseResult<T>>,
        {
            crate::database::query::map_chunk_rows(self.lazy(parent, count), callback).await
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
