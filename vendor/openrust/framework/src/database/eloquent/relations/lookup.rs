use crate::database::{
    DatabaseError, DatabaseResult, Value,
    eloquent::{Attributes, Builder, Model},
};

#[derive(Clone, Copy)]
pub(super) enum LookupSelection<'a> {
    Limited {
        attributes: &'a Attributes,
        limit: u64,
    },
    Keys(&'a [Value]),
}

impl<'a> LookupSelection<'a> {
    pub(super) fn limited(attributes: &'a Attributes, limit: u64) -> Self {
        Self::Limited { attributes, limit }
    }

    pub(super) fn apply<M: Model>(self, query: Builder<'static, M>) -> Builder<'static, M> {
        match self {
            Self::Limited { attributes, limit } => {
                query.restrict_relation_attributes(attributes).limit(limit)
            }
            Self::Keys(keys) => {
                let key = query.qualify_column(M::metadata().primary_key);
                query.map_relation_query(|query| query.where_in(&key, keys.to_vec()))
            }
        }
    }
}

pub(super) fn primary_key<M: Model>(key: Value) -> Attributes {
    Attributes::from([(M::metadata().primary_key.into(), key)])
}

pub(in crate::database::eloquent) fn sole_match<M: Model>(mut models: Vec<M>) -> DatabaseResult<M> {
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
        pub async fn find(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<Option<R>> {
            let keys = super::lookup::primary_key::<R>(key.into());
            Ok(self$($delegate)*.lookup_models(parent, &keys, 1).await?.into_iter().next())
        }

        /// Finds the matching related primary keys in relationship query order.
        pub async fn find_many(
            &self, parent: &P, keys: &[crate::database::Value],
        ) -> crate::database::DatabaseResult<crate::database::eloquent::Collection<R>> {
            if keys.is_empty() {
                return Ok(R::new_collection(Vec::new()));
            }
            self$($delegate)*.lookup_many(parent, keys).await
        }

        /// Requires every requested primary key to match the relationship.
        pub async fn find_many_or_fail(
            &self,
            parent: &P,
            keys: &[crate::database::Value],
        ) -> crate::database::DatabaseResult<crate::database::eloquent::Collection<R>> {
            let models = self.find_many(parent, keys).await?;
            models.find_many_or_fail(keys)?;
            Ok(models)
        }

        /// Runs the fallback if any requested primary key is missing.
        pub async fn find_many_or<Fut>(
            &self,
            parent: &P,
            keys: &[crate::database::Value],
            missing: impl FnOnce() -> Fut + Send,
        ) -> crate::database::DatabaseResult<crate::database::eloquent::Collection<R>>
        where
            Fut: std::future::Future<Output = crate::database::DatabaseResult<crate::database::eloquent::Collection<R>>> + Send,
        {
            let models = self.find_many(parent, keys).await?;
            match models.find_many_or_fail(keys) {
                Ok(_) => Ok(models),
                Err(crate::database::DatabaseError::ModelNotFound(_)) => missing().await,
                Err(error) => Err(error),
            }
        }

        /// Requires a related model by its primary key.
        pub async fn find_or_fail(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<R> {
            self.find(parent, key).await?.ok_or(crate::database::DatabaseError::ModelNotFound(R::metadata().table))
        }

        /// Requires exactly one related match for the primary key.
        pub async fn find_sole(&self, parent: &P, key: impl Into<crate::database::Value> + Send) -> crate::database::DatabaseResult<R> {
            let keys = super::lookup::primary_key::<R>(key.into());
            super::lookup::sole_match(self$($delegate)*.lookup_models(parent, &keys, 2).await?)
        }

        /// Requires exactly one related match.
        pub async fn sole(&self, parent: &P) -> crate::database::DatabaseResult<R> {
            super::lookup::sole_match(self$($delegate)*.lookup_models(parent, &crate::database::eloquent::Attributes::new(), 2).await?)
        }

        /// Runs the fallback only when no row matches the related primary key.
        pub async fn find_or<Fut>(
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
        pub async fn first_or<Fut>(
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
