use crate::{
    database::{Connection, DatabaseResult, Value},
    pagination::{LengthAwarePaginator, PageOptions, Paginator},
};

use super::{Builder, Collection, Model};

/// A database search strategy for an Eloquent column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchColumn {
    Contains(&'static str),
    Prefix(&'static str),
    FullText(&'static str),
}

impl SearchColumn {
    fn name(self) -> &'static str {
        match self {
            Self::Contains(column) | Self::Prefix(column) | Self::FullText(column) => column,
        }
    }
}

/// Opts an Eloquent model into searching its own database columns.
#[allow(non_snake_case, reason = "Matches Laravel's model search API")]
pub trait Searchable: Model {
    fn searchableColumns() -> &'static [SearchColumn];

    fn search(term: &str) -> DatabaseResult<SearchBuilder<Self>> {
        Ok(SearchBuilder::new(Self::query()?, term))
    }

    fn searchOn(connection: &Connection, term: &str) -> SearchBuilder<Self> {
        SearchBuilder::new(Self::queryOn(connection), term)
    }
}

/// A search against the same table and scopes as the model.
pub struct SearchBuilder<M: Searchable> {
    query: Builder<'static, M>,
    key_order: Option<String>,
}

#[allow(non_snake_case, reason = "Matches Laravel's model search builder API")]
impl<M: Searchable> SearchBuilder<M> {
    fn new(mut query: Builder<'static, M>, term: &str) -> Self {
        let columns = M::searchableColumns();
        let key = M::metadata().primary_key;
        let qualified_key = query.qualifyColumn(key);
        let full_text = columns
            .iter()
            .filter_map(|column| match column {
                SearchColumn::FullText(column) => Some(query.qualifyColumn(column)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !term.trim().is_empty() {
            let exact_key = if M::metadata().incrementing
                && columns.iter().any(|column| column.name() == key)
                && term.bytes().all(|byte| byte.is_ascii_digit())
            {
                term.parse::<i64>()
                    .map(Value::Integer)
                    .or_else(|_| term.parse::<u64>().map(Value::Unsigned))
                    .ok()
            } else {
                None
            };
            let alternatives = columns
                .iter()
                .filter_map(|column| {
                    if exact_key.is_some() && column.name() == key {
                        return None;
                    }
                    let pattern = match column {
                        SearchColumn::Contains(_) => format!("%{term}%"),
                        SearchColumn::Prefix(_) => format!("{term}%"),
                        SearchColumn::FullText(_) => return None,
                    };
                    Some((query.qualifyColumn(column.name()), pattern))
                })
                .collect::<Vec<_>>();
            query.whereNested(|mut group| {
                if let Some(value) = exact_key {
                    group = group.orWhere(&qualified_key, "=", value);
                }
                for (column, pattern) in alternatives {
                    group = group.orWhereLike(&column, &pattern, false);
                }
                if !full_text.is_empty() {
                    let columns = full_text.iter().map(String::as_str).collect::<Vec<_>>();
                    group = group.orWhereFullText(&columns, term);
                }
                group
            });
        }
        Self {
            query,
            key_order: full_text.is_empty().then_some(qualified_key),
        }
    }

    pub fn r#where(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.query.r#where(column, operator, value);
        self
    }

    pub fn orderBy(mut self, column: &str, direction: &str) -> Self {
        self.query = self.query.orderBy(column, direction);
        self
    }

    pub fn withTrashed(mut self) -> Self {
        self.query = self.query.withTrashed();
        self
    }

    pub fn onlyTrashed(mut self) -> Self {
        self.query = self.query.onlyTrashed();
        self
    }

    pub fn intoQuery(self) -> Builder<'static, M> {
        match self.key_order {
            Some(key) => self.query.orderBy(&key, "desc"),
            None => self.query,
        }
    }

    pub async fn get(self) -> DatabaseResult<Collection<M>> {
        self.intoQuery().get().await
    }

    pub async fn paginate(self, per_page: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.intoQuery().paginate(per_page).await
    }

    pub async fn paginateWith(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.intoQuery().paginateWith(options).await
    }

    pub async fn simplePaginate(self, per_page: u64) -> DatabaseResult<Paginator<M>> {
        self.intoQuery().simplePaginate(per_page).await
    }
}
