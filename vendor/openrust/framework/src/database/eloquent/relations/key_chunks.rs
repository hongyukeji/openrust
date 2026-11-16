// Keep the public forwarding methods identical on relationships with the same chunk contract.
macro_rules! key_chunks {
    ($collection:ty) => {
        #[allow(non_snake_case, reason = "Matches Laravel's relation query API")]
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

        #[allow(non_snake_case, reason = "Matches Laravel's relation query API")]
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

        #[allow(non_snake_case, reason = "Matches Laravel's relation query API")]
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

        #[allow(non_snake_case, reason = "Matches Laravel's relation query API")]
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
