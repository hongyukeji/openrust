use indexmap::IndexMap;

use crate::database::{
    DatabaseResult,
    eloquent::{Builder, Model},
    query::{self, Conditions},
};

/// Selects the minimum or maximum value among a relation's candidates.
#[derive(Clone, Copy)]
pub enum OneOfManyAggregate {
    Min,
    Max,
}

impl OneOfManyAggregate {
    const fn function(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Max => "max",
        }
    }
}

type CandidateFilter = dyn Fn(Conditions) -> Conditions + Send + Sync;

pub(super) struct OneOfMany {
    columns: IndexMap<&'static str, OneOfManyAggregate>,
    filter: Option<Box<CandidateFilter>>,
}

impl OneOfMany {
    pub fn new<R: Model>(
        columns: impl IntoIterator<Item = (&'static str, OneOfManyAggregate)>,
        tie: OneOfManyAggregate,
    ) -> Self {
        let mut columns: IndexMap<_, _> = columns.into_iter().collect();
        columns.entry(R::metadata().primary_key).or_insert(tie);
        Self {
            columns,
            filter: None,
        }
    }

    pub fn filter(&mut self, filter: impl Fn(Conditions) -> Conditions + Send + Sync + 'static) {
        self.filter = Some(Box::new(filter));
    }

    pub fn apply<R: Model>(
        &self,
        related: Builder<'static, R>,
        foreign_key: &str,
        discriminator: Option<(&str, &str)>,
    ) -> DatabaseResult<Builder<'static, R>> {
        let outer_foreign = related.qualifyColumn(foreign_key);
        self.apply_grouped(related, None, |mut candidates| {
            if let Some((column, value)) = discriminator {
                let column = candidates.qualifyColumn(column);
                candidates = candidates.require_relation_value(&column, value);
            }
            let candidate_foreign = candidates.qualifyColumn(foreign_key);
            Ok(candidates
                .scoped()?
                .0
                .whereColumn(&candidate_foreign, "=", &outer_foreign))
        })
    }

    pub fn apply_grouped<R: Model>(
        &self,
        mut related: Builder<'static, R>,
        outer_group_table: Option<&str>,
        group: impl Fn(Builder<'static, R>) -> DatabaseResult<query::Builder<'static>>,
    ) -> DatabaseResult<Builder<'static, R>> {
        let alias = [
            "laravel_candidates",
            "laravel_candidates_1",
            "laravel_candidates_2",
        ]
        .into_iter()
        .find(|alias| *alias != related.table_name() && Some(*alias) != outer_group_table)
        .expect("three aliases for two tables");
        // Narrow the candidate group by each preceding aggregate before
        // calculating the next one, including the primary key tie breaker.
        for (index, (column, aggregate)) in self.columns.iter().enumerate() {
            let mut candidates = related.relation_candidate(alias);
            if let Some(filter) = &self.filter {
                candidates.whereNested(filter);
            }
            let candidate_column = candidates.qualifyColumn(column);
            let mut query = group(candidates)?;
            for previous in self.columns.keys().take(index) {
                query = query.whereColumn(
                    &format!("{alias}.{previous}"),
                    "=",
                    &related.qualifyColumn(previous),
                );
            }
            let query = query.select_related_aggregate(aggregate.function(), &candidate_column)?;
            let outer_column = related.qualifyColumn(column);
            related = related.require_relation_subquery(&outer_column, &query)?;
        }
        Ok(related)
    }
}
