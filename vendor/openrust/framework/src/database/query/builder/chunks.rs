use futures_util::{Stream, TryStreamExt as _};

use crate::{
    database::{DatabaseCursor, DatabaseError, DatabaseResult, Row, Value},
    support::ArrayKey,
};

use super::{Builder, Conditions, Order};

pub(in crate::database) struct ChunkKey {
    pub column: String,
    pub alias: String,
    pub descending: bool,
}

#[allow(non_snake_case, reason = "Matches Laravel's Query Builder API")]
impl Builder<'_> {
    /// Visits rows in offset batches, with an index local to each batch.
    pub async fn each<Fut>(
        mut self,
        callback: impl FnMut(Row, i64) -> Fut,
        count: u64,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let batches = self.chunk_pages(count).map_ok(index_rows);
        visit_batches(batches, |_, key| Ok(key), callback).await
    }

    /// Visits rows in ascending key order, with indices spanning all batches.
    pub async fn eachById<Fut>(
        mut self,
        callback: impl FnMut(Row, i64) -> Fut,
        count: u64,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let key = query_chunk_key(column, alias, false);
        let batches = self.chunk_id_pages(count, key).map_ok(index_rows);
        visit_batches(
            batches,
            |batch, key| chunk_index(batch, count, key),
            callback,
        )
        .await
    }

    /// Processes successive batches ordered by a unique, projected key.
    pub async fn chunkById<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Vec<Row>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.orderedChunkById(count, callback, column, alias, false)
            .await
    }

    pub async fn chunkByIdDesc<Fut>(
        self,
        count: u64,
        callback: impl FnMut(Vec<Row>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        self.orderedChunkById(count, callback, column, alias, true)
            .await
    }

    pub async fn orderedChunkById<Fut>(
        mut self,
        count: u64,
        mut callback: impl FnMut(Vec<Row>, u64) -> Fut,
        column: Option<&str>,
        alias: Option<&str>,
        descending: bool,
    ) -> DatabaseResult<bool>
    where
        Fut: Future<Output = DatabaseResult<bool>>,
    {
        let key = query_chunk_key(column, alias, descending);
        let mut batches = self.chunk_id_pages(count, key);
        let mut page = 1;
        while let Some(rows) = batches.try_next().await? {
            if !callback(rows, page).await? {
                return Ok(false);
            }
            page += 1;
        }
        Ok(true)
    }

    pub(in crate::database) fn chunk_id_pages(
        &mut self,
        count: u64,
        key: ChunkKey,
    ) -> DatabaseCursor<'_, Vec<Row>> {
        Box::pin(async_stream::try_stream! {
            if count == 0 {
                Err(DatabaseError::InvalidQuery("chunk size must be positive"))?;
            }
            if !self.unions.is_empty() {
                Err(DatabaseError::InvalidQuery("chunkById requires a single query source; wrap the union in a subquery"))?;
            }

            let (direction, operator) = if key.descending { ("desc", "<") } else { ("asc", ">") };
            let unqualified = key.column.rsplit('.').next().expect("column has one component");
            self.orders.retain(|order| {
                !matches!(order, Order::Column(column, _) if column == &key.column || column == unqualified)
            });
            self.orders.push(Order::Column(key.column.clone(), direction.into()));
            let predicates = std::mem::take(&mut self.predicates);
            let base = if predicates.is_empty() {
                Conditions::default()
            } else {
                Conditions::default().whereNested(|_| predicates)
            };
            let mut remaining = self.limit.unwrap_or(u64::MAX);
            self.predicates = base.clone().whereNotNull(&key.column);
            while remaining != 0 {
                let size = count.min(remaining);
                self.limit = Some(size);
                let (sql, bindings) = self.compile(None)?;
                let rows = self.execute(&sql, &bindings).await?;
                let length = rows.len() as u64;
                let Some(last) = rows.last() else { break };
                let last_id = last.get(&key.alias).filter(|value| !matches!(value, Value::Null)).cloned();
                yield rows;
                // The callback can stop before the missing-key check, including on the last page.
                let last_id = last_id.ok_or(DatabaseError::InvalidQuery("chunkById key is missing from the result"))?;
                if length < size {
                    break;
                }
                remaining -= length;
                self.offset = None;
                self.predicates = base.clone().r#where(&key.column, operator, last_id);
            }
        })
    }
}

fn query_chunk_key(column: Option<&str>, alias: Option<&str>, descending: bool) -> ChunkKey {
    let column = column.unwrap_or("id");
    ChunkKey {
        column: column.into(),
        alias: alias.unwrap_or(column).into(),
        descending,
    }
}

fn index_rows(rows: Vec<Row>) -> impl Iterator<Item = (i64, Row)> {
    (0..).zip(rows)
}

pub(in crate::database) async fn visit_batches<B, T, K, I, Fut>(
    mut batches: impl Stream<Item = DatabaseResult<B>> + Unpin,
    mut index: impl FnMut(u64, K) -> DatabaseResult<I>,
    mut callback: impl FnMut(T, I) -> Fut,
) -> DatabaseResult<bool>
where
    B: IntoIterator<Item = (K, T)>,
    Fut: Future<Output = DatabaseResult<bool>>,
{
    let mut batch = 0;
    while let Some(rows) = batches.try_next().await? {
        for (key, item) in rows {
            if !callback(item, index(batch, key)?).await? {
                return Ok(false);
            }
        }
        batch += 1;
    }
    Ok(true)
}

pub(in crate::database) fn chunk_index(batch: u64, count: u64, key: i64) -> DatabaseResult<i64> {
    let overflow = || DatabaseError::InvalidQuery("eachById index overflow");
    let base = i128::from(batch)
        .checked_mul(i128::from(count))
        .ok_or_else(overflow)?;
    let index = base.checked_add(i128::from(key)).ok_or_else(overflow)?;
    i64::try_from(index).map_err(|_| overflow())
}

pub(in crate::database) fn collection_chunk_index(
    batch: u64,
    count: u64,
    key: ArrayKey,
) -> DatabaseResult<i64> {
    let key = match key {
        ArrayKey::Integer(value) => value,
        ArrayKey::String(value) => value.trim().parse::<i64>().map_err(|_| {
            DatabaseError::InvalidQuery("eachById requires integer collection keys")
        })?,
    };
    chunk_index(batch, count, key)
}
