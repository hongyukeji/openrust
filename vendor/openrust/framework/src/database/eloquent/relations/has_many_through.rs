use std::{collections::HashMap, marker::PhantomData, sync::Arc};

use crate::database::{
    Connection, DatabaseError, DatabaseFuture, DatabaseResult,
    eloquent::{
        Builder, Model, ModelState,
        concerns::{has_relationships::load_eager, queries_relationships::cast_existence},
    },
    query,
};

use super::{
    ExistenceRelation, HasOneThrough, Relation,
    one_of_many::OneOfMany,
    relation::{ParentKeys, RelationKeys, dictionary_key},
};

/// Links a parent to related models through an intermediate model.
pub struct HasManyThrough<P: Model, I: Model, R: Model> {
    pub(super) keys: RelationKeys<P, R>,
    first_key: &'static str,
    second_local_key: &'static str,
    include_trashed_parents: bool,
    pub(super) selection: Option<OneOfMany>,
    intermediate: PhantomData<fn() -> I>,
}

#[allow(non_snake_case, reason = "Matches Laravel's through relation API")]
impl<P: Model, I: Model, R: Model> HasManyThrough<P, I, R> {
    pub const fn new(
        first_key: &'static str,
        second_key: &'static str,
        local_key: &'static str,
        second_local_key: &'static str,
    ) -> Self {
        Self {
            keys: RelationKeys::new(local_key, second_key),
            first_key,
            second_local_key,
            include_trashed_parents: false,
            selection: None,
            intermediate: PhantomData,
        }
    }

    #[must_use]
    pub fn one(self) -> HasOneThrough<P, I, R> {
        HasOneThrough::from_many(self)
    }

    #[must_use]
    pub fn withTrashedParents(mut self) -> Self {
        self.include_trashed_parents = true;
        self
    }

    #[must_use]
    pub fn constrain(
        mut self,
        configure: impl Fn(Builder<'static, R>) -> Builder<'static, R> + Send + Sync + 'static,
    ) -> Self {
        self.keys.constrain(configure);
        self
    }

    fn join(
        &self,
        related: &Builder<'_, R>,
        parent_table: Option<&str>,
        connection: &Connection,
    ) -> DatabaseResult<ThroughJoin> {
        let intermediate = I::metadata();
        if intermediate
            .connection
            .is_some_and(|name| name != connection.getName())
        {
            return Err(DatabaseError::InvalidQuery(
                "through models require one joined connection",
            ));
        }
        let related_table = related.table_name();
        let table = intermediate.table;
        let mut alias = table;
        if table == related_table || parent_table == Some(table) {
            alias = [
                "laravel_reserved_1",
                "laravel_reserved_2",
                "laravel_reserved_3",
            ]
            .into_iter()
            .find(|alias| *alias != related_table && Some(*alias) != parent_table)
            .expect("three aliases for two tables");
        }
        let source = if alias == table {
            table.into()
        } else {
            format!("{table} as {alias}")
        };
        Ok(ThroughJoin {
            source,
            alias,
            match_key: format!("{alias}.{}", self.first_key),
            intermediate_key: format!("{alias}.{}", self.second_local_key),
            related_key: related.qualifyColumn(self.keys.related),
            deleted_at: intermediate
                .deleted_at
                .filter(|_| !self.include_trashed_parents)
                .map(|column| format!("{alias}.{column}")),
        })
    }

    fn select_one(
        &self,
        related: Builder<'static, R>,
        outer: &ThroughJoin,
        connection: &Connection,
    ) -> DatabaseResult<Builder<'static, R>> {
        let Some(selection) = &self.selection else {
            return Ok(related);
        };
        selection.apply_grouped(related, Some(outer.alias), |candidates| {
            let join = self.join(&candidates, Some(outer.alias), connection)?;
            let candidates = candidates.scoped()?.0;
            // Correlate through the intermediate key so ranking spans every
            // intermediate row of this parent. Candidate filters see R alone.
            let middle = candidates
                .subquery(&join.source)
                .select([query::Expression::new("1")])
                .whereColumn(&join.intermediate_key, "=", &join.related_key)
                .whereColumn(&join.match_key, "=", &outer.match_key);
            candidates.whereExists(&join.filter_deleted(middle))
        })
    }

    async fn match_many(&self, parents: &[&P]) -> DatabaseResult<Vec<Vec<Arc<R>>>> {
        let keys = ParentKeys::new(parents, self.keys.parent)?;
        let mut dictionary: HashMap<_, Vec<_>> = HashMap::new();
        if !keys.bindings.is_empty() {
            let connection = self.keys.batch_connection(parents)?;
            let projection = format!("{}.*", R::metadata().table);
            for batch in keys.bindings.chunks(500) {
                let query = self
                    .keys
                    .configure(
                        R::queryOn(&connection).select_relation_columns([projection.as_str()]),
                    )?
                    .apply_default_counts()?;
                let join = self.join(&query, None, &connection)?;
                let query = self.select_one(query, &join, &connection)?;
                let mut query = query.restrict_relation(&join.match_key, batch);
                let eager_load = query.take_eager_load();
                let existence = query.take_existence_attributes();
                let casts = query.casts.clone();
                let match_column = format!("{} as laravel_through_key", join.match_key);
                let rows = join
                    .apply(query.scoped()?.0)
                    .addSelect([match_column.as_str()])
                    .get()
                    .await?;
                let mut matches = Vec::with_capacity(rows.len());
                let mut related = Vec::with_capacity(rows.len());
                for row in rows {
                    let mut columns = row.into_columns();
                    let (_, key) = columns
                        .pop()
                        .ok_or(DatabaseError::InvalidQuery("through match key"))?;
                    if let Some(key) = dictionary_key(Some(&key))? {
                        matches.push(key);
                        let mut attributes = columns.into_iter().collect();
                        cast_existence(&mut attributes, &existence);
                        related.push(R::from_state(ModelState::hydrated_attributes(
                            attributes,
                            connection.clone(),
                            casts.clone(),
                        )));
                    }
                }
                load_eager(eager_load, &mut related).await?;
                for (key, model) in matches.into_iter().zip(related) {
                    dictionary.entry(key).or_default().push(Arc::new(model));
                }
            }
        }
        Ok(keys.match_models(&dictionary))
    }
}

struct ThroughJoin {
    source: String,
    alias: &'static str,
    match_key: String,
    intermediate_key: String,
    related_key: String,
    deleted_at: Option<String>,
}

impl ThroughJoin {
    fn apply(&self, query: query::Builder<'static>) -> query::Builder<'static> {
        let query = query.join(&self.source, &self.intermediate_key, "=", &self.related_key);
        self.filter_deleted(query)
    }

    fn filter_deleted(&self, query: query::Builder<'static>) -> query::Builder<'static> {
        match &self.deleted_at {
            Some(column) => query.whereNull(column),
            None => query,
        }
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation API")]
impl<P: Model, I: Model, R: Model> Relation for HasManyThrough<P, I, R> {
    type Parent = P;
    type Output = Vec<Arc<R>>;

    fn serialize(output: &Self::Output) -> DatabaseResult<Option<serde_json::Value>> {
        super::super::serialization::many(output)
    }

    fn matchResults<'a>(
        &'a self,
        parents: &'a [&'a P],
    ) -> DatabaseFuture<'a, DatabaseResult<Vec<Self::Output>>> {
        Box::pin(self.match_many(parents))
    }
}

#[allow(non_snake_case, reason = "Matches Laravel's relation query contract")]
impl<P: Model, I: Model, R: Model> ExistenceRelation for HasManyThrough<P, I, R> {
    fn getRelationExistenceQuery(
        &self,
        connection: &Connection,
        parent_table: &str,
    ) -> DatabaseResult<query::Builder<'static>> {
        let related = self.keys.existence_builder(connection, parent_table)?;
        let join = self.join(&related, Some(parent_table), connection)?;
        let related = self.select_one(related, &join, connection)?;
        let parent = format!("{parent_table}.{}", self.keys.parent);
        Ok(join
            .apply(related.scoped()?.0)
            .whereColumn(&join.match_key, "=", &parent))
    }
}
