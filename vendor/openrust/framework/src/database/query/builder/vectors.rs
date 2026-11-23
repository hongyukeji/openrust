use crate::database::{DatabaseError, DatabaseFuture, DatabaseResult};

use super::{Builder, Column, Expression};
use crate::database::query::{embeddings, vector};

#[allow(non_snake_case, reason = "保留既定公开接口及命名")]
impl Builder<'_> {
    /// Adds the cosine distance to the selected columns.
    pub fn selectVectorDistance(
        self,
        column: impl Into<Column>,
        vector: &[f64],
        alias: Option<&str>,
    ) -> DatabaseResult<Self> {
        let column = column.into();
        let alias = match (alias, &column) {
            (Some(alias), _) => self.grammar.quote(alias)?,
            (None, Column::Identifier(name)) => {
                let name = name.rsplit('.').next().unwrap_or(name);
                self.grammar.quote(&format!("{name}_distance"))?
            }
            (None, Column::Raw(_)) => {
                return Err(DatabaseError::InvalidQuery(
                    "vector projection alias required",
                ));
            }
        };
        let Expression { sql, bindings } = self.vector_distance(&column, vector)?;
        Ok(self.selectRaw(&format!("{sql} as {alias}"), bindings))
    }

    /// Filters by an inclusive cosine similarity threshold and optionally orders closest first.
    pub fn whereVectorSimilarTo(
        self,
        column: impl Into<Column>,
        vector: &[f64],
        min_similarity: f64,
        order: bool,
    ) -> DatabaseResult<Self> {
        let max_distance = vector::similarity_max_distance(min_similarity)?;
        let distance = self.vector_distance(&column.into(), vector)?;
        let filter = distance_filter(distance.clone(), max_distance)?;
        let query = self.whereRaw(&filter.sql, filter.bindings);
        if order {
            Ok(query.orderByRaw(&format!("{} asc", distance.sql), distance.bindings))
        } else {
            Ok(query)
        }
    }

    /// Keeps rows whose cosine distance is at most the given value.
    pub fn whereVectorDistanceLessThan(
        self,
        column: impl Into<Column>,
        vector: &[f64],
        max_distance: f64,
    ) -> DatabaseResult<Self> {
        let filter = distance_filter(self.vector_distance(&column.into(), vector)?, max_distance)?;
        Ok(self.whereRaw(&filter.sql, filter.bindings))
    }

    pub fn orWhereVectorDistanceLessThan(
        self,
        column: impl Into<Column>,
        vector: &[f64],
        max_distance: f64,
    ) -> DatabaseResult<Self> {
        let filter = distance_filter(self.vector_distance(&column.into(), vector)?, max_distance)?;
        Ok(self.orWhereRaw(&filter.sql, filter.bindings))
    }

    pub fn orderByVectorDistance(
        self,
        column: impl Into<Column>,
        vector: &[f64],
    ) -> DatabaseResult<Self> {
        let Expression { sql, bindings } = self.vector_distance(&column.into(), vector)?;
        Ok(self.orderByRaw(&format!("{sql} asc"), bindings))
    }

    pub async fn selectVectorDistanceText(
        self,
        column: impl Into<Column> + Send,
        text: &str,
        alias: Option<&str>,
    ) -> DatabaseResult<Self> {
        let vector = self.text_vector(text).await?;
        self.selectVectorDistance(column, &vector, alias)
    }

    pub async fn whereVectorSimilarToText(
        self,
        column: impl Into<Column> + Send,
        text: &str,
        min_similarity: f64,
        order: bool,
    ) -> DatabaseResult<Self> {
        vector::similarity_max_distance(min_similarity)?;
        let vector = self.text_vector(text).await?;
        self.whereVectorSimilarTo(column, &vector, min_similarity, order)
    }

    pub async fn whereVectorDistanceLessThanText(
        self,
        column: impl Into<Column> + Send,
        text: &str,
        max_distance: f64,
    ) -> DatabaseResult<Self> {
        vector::validate_max_distance(max_distance)?;
        let vector = self.text_vector(text).await?;
        self.whereVectorDistanceLessThan(column, &vector, max_distance)
    }

    pub async fn orWhereVectorDistanceLessThanText(
        self,
        column: impl Into<Column> + Send,
        text: &str,
        max_distance: f64,
    ) -> DatabaseResult<Self> {
        vector::validate_max_distance(max_distance)?;
        let vector = self.text_vector(text).await?;
        self.orWhereVectorDistanceLessThan(column, &vector, max_distance)
    }

    pub async fn orderByVectorDistanceText(
        self,
        column: impl Into<Column> + Send,
        text: &str,
    ) -> DatabaseResult<Self> {
        let vector = self.text_vector(text).await?;
        self.orderByVectorDistance(column, &vector)
    }

    pub(in crate::database) fn text_vector<'a>(
        &self,
        text: &'a str,
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<f64>>> {
        let supported = self.grammar.is_postgres() || self.grammar.is_mariadb();
        let connection = self.executor.source().clone();
        Box::pin(async move {
            if !supported {
                return Err(DatabaseError::UnsupportedOperation(
                    "vector distance queries",
                ));
            }
            embeddings::cached_for(connection, text).await
        })
    }

    fn vector_distance(&self, column: &Column, vector: &[f64]) -> DatabaseResult<Expression> {
        let mut bindings = Vec::new();
        let sql = vector::distance(&self.grammar, column, vector, &mut bindings)?;
        Ok(Expression::new(sql).with_bindings(bindings))
    }
}

fn distance_filter(mut distance: Expression, max_distance: f64) -> DatabaseResult<Expression> {
    vector::append_max_distance(&mut distance.sql, &mut distance.bindings, max_distance)?;
    Ok(distance)
}
