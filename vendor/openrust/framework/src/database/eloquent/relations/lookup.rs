use crate::database::{
    DatabaseError, DatabaseResult, Value,
    eloquent::{Attributes, Model},
};

pub(super) fn primary_key<M: Model>(key: Value) -> Attributes {
    Attributes::from([(M::metadata().primary_key.into(), key)])
}

pub(super) fn sole_match<M: Model>(mut models: Vec<M>) -> DatabaseResult<M> {
    match models.len() {
        0 => Err(DatabaseError::ModelNotFound(M::metadata().table)),
        1 => Ok(models.pop().expect("one matching model")),
        count => Err(DatabaseError::MultipleModelsFound {
            table: M::metadata().table,
            count,
        }),
    }
}

macro_rules! relation_lookup {
    ($($delegate:tt)*) => {
        /// Finds one related model by its primary key.
        #[allow(non_snake_case, reason = "Matches Laravel's relation API")]
        pub async fn find(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<Option<R>> {
            let keys = super::lookup::primary_key::<R>(key.into());
            Ok(self$($delegate)*.lookup_models(parent, &keys, 1).await?.into_iter().next())
        }

        /// Requires a related model by its primary key.
        #[allow(non_snake_case, reason = "Matches Laravel's relation API")]
        pub async fn findOrFail(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<R> {
            self.find(parent, key).await?.ok_or(crate::database::DatabaseError::ModelNotFound(R::metadata().table))
        }

        /// Requires exactly one related match for the primary key.
        #[allow(non_snake_case, reason = "Matches Laravel's relation API")]
        pub async fn findSole(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<R> {
            let keys = super::lookup::primary_key::<R>(key.into());
            super::lookup::sole_match(self$($delegate)*.lookup_models(parent, &keys, 2).await?)
        }

        /// Requires exactly one related match.
        pub async fn sole(&self, parent: &P) -> crate::database::DatabaseResult<R> {
            super::lookup::sole_match(self$($delegate)*.lookup_models(parent, &crate::database::eloquent::Attributes::new(), 2).await?)
        }

        /// Runs the fallback only when no row matches the related primary key.
        #[allow(non_snake_case, reason = "Matches Laravel's relation API")]
        pub async fn findOr<Fut>(
            &self,
            parent: &P,
            key: impl Into<crate::database::Value> + Send,
            missing: impl FnOnce() -> Fut + Send,
        ) -> crate::database::DatabaseResult<R>
        where
            Fut: std::future::Future<Output = crate::database::DatabaseResult<R>> + Send,
        {
            match self.find(parent, key).await? {
                Some(model) => Ok(model),
                None => missing().await,
            }
        }

        /// Runs the fallback only when the relationship has no matching rows.
        #[allow(non_snake_case, reason = "Matches Laravel's relation API")]
        pub async fn firstOr<Fut>(
            &self,
            parent: &P,
            missing: impl FnOnce() -> Fut + Send,
        ) -> crate::database::DatabaseResult<R>
        where
            Fut: std::future::Future<Output = crate::database::DatabaseResult<R>> + Send,
        {
            let models = self$($delegate)*.lookup_models(parent, &crate::database::eloquent::Attributes::new(), 1).await?;
            match models.into_iter().next() {
                Some(model) => Ok(model),
                None => missing().await,
            }
        }
    };
}

pub(super) use relation_lookup;
