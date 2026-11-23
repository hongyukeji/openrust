use crate::{
    database::{Connection, DatabaseResult, Value, query::FullTextOptions},
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
pub trait Searchable: Model {
    fn searchable_columns() -> &'static [SearchColumn];

    fn full_text_options() -> FullTextOptions {
        FullTextOptions::default()
    }

    fn search(term: &str) -> DatabaseResult<SearchBuilder<Self>> {
        Ok(SearchBuilder::new(Self::query()?, term))
    }

    fn search_on(connection: &Connection, term: &str) -> SearchBuilder<Self> {
        SearchBuilder::new(Self::query_on(connection), term)
    }
}

/// A search against the same table and scopes as the model.
pub struct SearchBuilder<M: Searchable> {
    query: Builder<'static, M>,
    key_order: Option<String>,
    relevance: Option<(Vec<String>, String, String)>,
}

impl<M: Searchable> SearchBuilder<M> {
    fn new(mut query: Builder<'static, M>, term: &str) -> Self {
        let columns = M::searchable_columns();
        let key = M::metadata().primary_key;
        let qualified_key = query.qualify_column(key);
        let full_text = columns
            .iter()
            .filter_map(|column| match column {
                SearchColumn::FullText(column) => Some(query.qualify_column(column)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let postgres = query.connection.grammar.is_postgres();
        let has_term = !term.trim().is_empty();
        if has_term {
            let exact_key = if M::get_incrementing()
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
                    Some((query.qualify_column(column.name()), pattern))
                })
                .collect::<Vec<_>>();
            query.where_nested(|mut group| {
                if let Some(value) = exact_key {
                    group = group.or_where(&qualified_key, "=", value);
                }
                for (column, pattern) in alternatives {
                    group = group.or_where_like(&column, &pattern, false);
                }
                if !full_text.is_empty() {
                    let columns = full_text.iter().map(String::as_str).collect::<Vec<_>>();
                    group = group.or_where_full_text_with(&columns, term, M::full_text_options());
                }
                group
            });
        }
        let key_order =
            (full_text.is_empty() || (postgres && !has_term)).then(|| qualified_key.clone());
        let relevance = (postgres && has_term && !full_text.is_empty())
            .then(|| (full_text, term.to_owned(), qualified_key));
        Self {
            query,
            key_order,
            relevance,
        }
    }

    pub fn r#where(mut self, column: &str, operator: &str, value: impl Into<Value>) -> Self {
        self.query.r#where(column, operator, value);
        self
    }

    pub fn order_by(mut self, column: &str, direction: &str) -> Self {
        self.query = self.query.order_by(column, direction);
        self.relevance = None;
        self
    }

    pub fn with_trashed(mut self) -> Self {
        self.query = self.query.with_trashed();
        self
    }

    pub fn only_trashed(mut self) -> Self {
        self.query = self.query.only_trashed();
        self
    }

    pub fn into_query(self) -> Builder<'static, M> {
        if let Some((columns, term, key)) = self.relevance {
            return self
                .query
                .order_by_full_text(columns, term, M::full_text_options())
                .order_by(&key, "desc");
        }
        match self.key_order {
            Some(key) => self.query.order_by(&key, "desc"),
            None => self.query,
        }
    }

    pub async fn get(self) -> DatabaseResult<Collection<M>> {
        self.into_query().get().await
    }

    pub async fn paginate(self, per_page: u64) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.into_query().paginate(per_page).await
    }

    pub async fn paginate_with(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<M>> {
        self.into_query().paginate_with(options).await
    }

    pub async fn simple_paginate(self, per_page: u64) -> DatabaseResult<Paginator<M>> {
        self.into_query().simple_paginate(per_page).await
    }
}
