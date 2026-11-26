use openrust::database::{
    DatabaseError, DatabaseFuture, DatabaseResult, query::embeddings::EmbeddingProvider,
};

use crate::ToEmbeddings;

pub(crate) struct DatabaseEmbeddingProvider;

impl EmbeddingProvider for DatabaseEmbeddingProvider {
    fn cache_identity(&self) -> &str {
        "openrust-ai"
    }

    fn embed<'a>(&'a self, text: &'a str) -> DatabaseFuture<'a, DatabaseResult<Vec<f64>>> {
        Box::pin(async move {
            let vector = text
                .to_embeddings()
                .await
                .map_err(|error| DatabaseError::Embedding(Box::new(error)))?;
            Ok(vector.into_iter().map(f64::from).collect())
        })
    }
}
