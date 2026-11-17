use crate::database::{DatabaseError, DatabaseResult};

use super::{Builder, Column, Expression};
use crate::database::query::vector;

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
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
