use std::{collections::HashSet, mem};

use crate::{
    database::{DatabaseError, DatabaseResult, Row, Value},
    pagination::{
        Cursor, CursorOptions, CursorPaginator, LengthAwarePaginator, Links, PageOptions,
        PaginationError, Paginator, page_offset, page_size,
    },
};

use super::{Builder, Column, Conditions, Order};

impl Builder<'_> {
    pub async fn paginate(self, per_page: u64) -> DatabaseResult<LengthAwarePaginator<Row>> {
        self.paginate_with(PageOptions {
            per_page: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn paginate_with(
        mut self,
        mut options: PageOptions<'_>,
    ) -> DatabaseResult<LengthAwarePaginator<Row>> {
        let resolved_page = if options.per_page_using.is_none() {
            Some(offset_page(&options)?)
        } else {
            None
        };
        self = self.select_page_columns(options.columns);
        let supplied_total = options.resolve_total()?;
        let total = match supplied_total {
            Some(total) => total,
            None => self.pagination_count().await?,
        };
        let (links, page, size, offset) = match resolved_page {
            Some(page) => page,
            None => {
                options.per_page = options.resolve_per_page(total)?;
                offset_page(&options)?
            }
        };
        let items = if total == 0 || (supplied_total.is_none() && offset >= total) {
            Vec::new()
        } else {
            self.limit(size).offset(offset).get().await?
        };
        Ok(LengthAwarePaginator::from_query(
            items, total, page, size, links,
        ))
    }

    pub async fn simple_paginate(self, per_page: u64) -> DatabaseResult<Paginator<Row>> {
        self.simple_paginate_with(PageOptions {
            per_page: Some(per_page),
            ..PageOptions::default()
        })
        .await
    }

    pub async fn simple_paginate_with(
        self,
        options: PageOptions<'_>,
    ) -> DatabaseResult<Paginator<Row>> {
        let (links, page, size, offset) = offset_page(&options)?;
        let items = self
            .select_page_columns(options.columns)
            .limit(size + 1)
            .offset(offset)
            .get()
            .await?;
        Ok(Paginator::from_query(items, page, size, links))
    }

    pub async fn cursor_paginate(self, per_page: u64) -> DatabaseResult<CursorPaginator<Row>> {
        self.cursor_paginate_with(CursorOptions {
            per_page: Some(per_page),
            ..CursorOptions::default()
        })
        .await
    }

    pub async fn cursor_paginate_with(
        mut self,
        options: CursorOptions<'_>,
    ) -> DatabaseResult<CursorPaginator<Row>> {
        let size = page_size(options.per_page)?;
        let links = Links::current(options.cursor_name);
        // the framework starts at the first page when an incoming token cannot be decoded.
        let cursor = links.current_cursor(options.cursor);
        if !self.unions.is_empty()
            || !self.groups.is_empty()
            || !self.havings.is_empty()
            || self.group_limit.is_some()
        {
            return Err(PaginationError::MissingCursorOrder.into());
        }
        self = self.select_page_columns(options.columns);
        let order = self.cursor_order()?;
        let names: Vec<_> = order.iter().map(|key| key.output.clone()).collect();
        let backwards = cursor
            .as_ref()
            .is_some_and(Cursor::points_to_previous_items);
        if let Some(cursor) = &cursor {
            let keys = cursor.keys(&names)?;
            let predicates = mem::take(&mut self.predicates);
            self.predicates = Conditions::default()
                .where_nested(|_| predicates)
                .where_nested(|group| seek(group, &order, &keys, backwards));
        }
        if backwards {
            for order in &mut self.orders {
                if let Order::Column(_, direction) = order {
                    *direction = if direction.eq_ignore_ascii_case("asc") {
                        "desc"
                    } else {
                        "asc"
                    }
                    .into();
                }
            }
        }
        let items = self.limit(size + 1).offset(0).get().await?;

        // Validate returned cursor columns even when the page has no next link.
        if items
            .iter()
            .take(usize::try_from(size).unwrap_or(usize::MAX))
            .any(|row| {
                names
                    .iter()
                    .any(|name| !row.get(name).is_some_and(valid_key))
            })
        {
            return Err(PaginationError::InvalidCursor.into());
        }
        Ok(CursorPaginator::from_ordered(
            items,
            size,
            links,
            cursor,
            |row, next| make_cursor(row, &names, next),
        )?)
    }

    pub(in crate::database) fn order_for_pagination(self, column: &str) -> Self {
        if self.orders.is_empty() && self.union_orders.is_empty() {
            self.order_by(column, "asc")
        } else {
            self
        }
    }

    fn select_page_columns(self, columns: &[&str]) -> Self {
        if self.columns.is_empty() && columns != ["*"] && !columns.is_empty() {
            self.select(columns.iter().copied())
        } else {
            self
        }
    }

    async fn pagination_count(&mut self) -> DatabaseResult<u64> {
        self.apply_before_query_callbacks()?;
        let orders = mem::take(&mut self.orders);
        let union_orders = mem::take(&mut self.union_orders);
        let limits = (
            self.limit.take(),
            self.offset.take(),
            self.union_limit.take(),
            self.union_offset.take(),
        );
        let lock = self.lock.take();
        let order_changes = self
            .binding_changes
            .as_mut()
            .map(|changes| (changes[6].take(), changes[8].take()));
        let compiled = if self.groups.is_empty()
            && self.havings.is_empty()
            && self.unions.is_empty()
            && !self.distinct
        {
            self.compile(Some(("count", "*")))
        } else {
            self.compile(None).map(|(sql, bindings)| {
                (
                    format!("select count(*) from ({sql}) as aggregate_table"),
                    bindings,
                )
            })
        };
        self.orders = orders;
        self.union_orders = union_orders;
        (self.limit, self.offset, self.union_limit, self.union_offset) = limits;
        self.lock = lock;
        if let Some((order, union_order)) = order_changes {
            let changes = self.binding_changes.as_mut().expect("绑定更改仍存在");
            changes[6] = order;
            changes[8] = union_order;
        }
        let (sql, bindings) = compiled?;
        match super::first_value(self.execute(&sql, &bindings).await?)? {
            Value::Unsigned(value) => Ok(value),
            Value::Integer(value) => {
                u64::try_from(value).map_err(|_| DatabaseError::InvalidQuery("pagination count"))
            }
            _ => Err(DatabaseError::InvalidQuery("pagination count")),
        }
    }

    fn cursor_order(&self) -> DatabaseResult<Vec<CursorOrder>> {
        if self.orders.is_empty() || self.orders.len() > 32 {
            return Err(PaginationError::MissingCursorOrder.into());
        }
        let mut outputs = HashSet::new();
        self.orders
            .iter()
            .map(|order| {
                let Order::Column(name, direction) = order else {
                    return Err(PaginationError::MissingCursorOrder.into());
                };
                let ascending = if direction.eq_ignore_ascii_case("asc") {
                    true
                } else if direction.eq_ignore_ascii_case("desc") {
                    false
                } else {
                    return Err(PaginationError::MissingCursorOrder.into());
                };
                let (sql_column, output) = selected_cursor_column(&self.columns, name);
                self.grammar.column(sql_column)?;
                if !outputs.insert(output) {
                    return Err(PaginationError::MissingCursorOrder.into());
                }
                Ok(CursorOrder {
                    column: sql_column.to_owned(),
                    output: output.to_owned(),
                    ascending,
                })
            })
            .collect()
    }
}

fn offset_page(options: &PageOptions<'_>) -> DatabaseResult<(Links, u64, u64, u64)> {
    let links = Links::current(options.page_name);
    let page = links.current_page(options.page);
    let size = page_size(options.per_page)?;
    let offset = page_offset(page, size)?;
    Ok((links, page, size, offset))
}

struct CursorOrder {
    column: String,
    output: String,
    ascending: bool,
}

fn selected_cursor_column<'a>(columns: &'a [Column], order: &'a str) -> (&'a str, &'a str) {
    for column in columns {
        let Column::Identifier(column) = column else {
            continue;
        };
        if let Some((source, alias)) = column.split_once(" as ")
            && (alias == order || source == order)
        {
            return (source, alias);
        }
    }
    (order, order.rsplit('.').next().unwrap_or(order))
}

fn seek(
    mut group: Conditions,
    order: &[CursorOrder],
    keys: &[Value],
    backwards: bool,
) -> Conditions {
    for (index, key) in order.iter().enumerate() {
        let operator = if key.ascending != backwards { ">" } else { "<" };
        group = group.or_where_nested(|mut branch| {
            for (previous, value) in order[..index].iter().zip(keys) {
                branch = branch.r#where(&previous.column, "=", value.clone());
            }
            branch.r#where(&key.column, operator, keys[index].clone())
        });
    }
    group
}

fn valid_key(value: &Value) -> bool {
    match value {
        Value::Bool(_) | Value::Integer(_) | Value::Unsigned(_) | Value::Text(_) => true,
        Value::Real(value) => value.is_finite(),
        _ => false,
    }
}

fn make_cursor(row: &Row, names: &[String], next: bool) -> Result<Cursor, PaginationError> {
    Cursor::from_keys(
        names.iter().map(|name| {
            (
                name.as_str(),
                row.get(name).expect("validated cursor column"),
            )
        }),
        next,
    )
}
