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
