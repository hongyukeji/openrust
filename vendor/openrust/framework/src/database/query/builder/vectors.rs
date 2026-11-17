use crate::database::{DatabaseError, DatabaseResult, Value};

use super::{Builder, Column, Expression};

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
        if !(0.0..=1.0).contains(&min_similarity) {
            return Err(DatabaseError::InvalidQuery(
                "vector similarity must be between 0 and 1",
            ));
        }
        let distance = self.vector_distance(&column.into(), vector)?;
        let filter = distance_filter(distance.clone(), 1.0 - min_similarity)?;
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
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            return Err(DatabaseError::InvalidQuery(
                "vector must contain finite values",
            ));
        }
        let mut sql = String::new();
        let mut bindings = Vec::new();
        column.compile(&self.grammar, &mut sql, &mut bindings, false)?;
        let sql = self.grammar.vector_distance(&sql)?;
        let vector = serde_json::to_string(vector)
            .map_err(|_| DatabaseError::InvalidQuery("vector parameter"))?;
        bindings.push(Value::Text(vector));
        Ok(Expression::new(sql).with_bindings(bindings))
    }
}

fn distance_filter(mut distance: Expression, max_distance: f64) -> DatabaseResult<Expression> {
    if !max_distance.is_finite() {
        return Err(DatabaseError::InvalidQuery(
            "vector distance must be finite",
        ));
    }
    distance.sql.push_str(" <= ?");
    distance.bindings.push(Value::Real(max_distance));
    Ok(distance)
}
