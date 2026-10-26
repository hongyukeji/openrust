//! 基于实体主键的有界分批读取。

use std::future::Future;
use std::marker::PhantomData;

use sea_orm::entity::Iterable;
use sea_orm::sea_query::ValueTuple;
use sea_orm::{
    ConnectionTrait, EntityTrait, Identity, IntoIdentity, ModelTrait, PrimaryKeyToColumn, Select,
};

use crate::core::{Cancellation, Error, Result};

use super::Db;

/// 单批读取的最大记录数。
pub const MAX_BATCH_SIZE: usize = 1000;

/// 每批读取的记录数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchOptions {
    size: usize,
}

impl BatchOptions {
    /// 创建批次选项；大小必须为 `1..=1000`。
    pub fn new(size: usize) -> Result<Self> {
        if !(1..=MAX_BATCH_SIZE).contains(&size) {
            return Err(Error::BadRequest(format!(
                "batch size must be between 1 and {MAX_BATCH_SIZE}"
            )));
        }
        Ok(Self { size })
    }

    /// 返回每批最大记录数。
    pub const fn size(self) -> usize {
        self.size
    }
}

impl Default for BatchOptions {
    fn default() -> Self {
        Self { size: 100 }
    }
}

/// 主键游标遍历方向。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BatchDirection {
    /// 按主键元组升序。
    #[default]
    Ascending,
    /// 按主键元组降序。
    Descending,
}

/// 可用于继续遍历同一实体查询的主键游标。
pub struct BatchCursor<E: EntityTrait> {
    value: ValueTuple,
    marker: PhantomData<fn() -> E>,
}

impl<E: EntityTrait> Clone for BatchCursor<E> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            marker: PhantomData,
        }
    }
}

/// 已成功处理的批次数、记录数及可恢复位置。
pub struct BatchReport<E: EntityTrait> {
    batches: usize,
    rows: usize,
    cancelled: bool,
    cursor: Option<BatchCursor<E>>,
}

impl<E: EntityTrait> BatchReport<E> {
    /// 返回 callback 成功处理的批次数。
    pub const fn batches(&self) -> usize {
        self.batches
    }
    /// 返回 callback 成功处理的记录数。
    pub const fn rows(&self) -> usize {
        self.rows
    }
    /// 返回是否因取消信号而在批次边界停止。
    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    /// 返回最后一个完整处理批次的游标。
    pub fn cursor(&self) -> Option<BatchCursor<E>> {
        self.cursor.clone()
    }
}

/// 以实体所有主键列进行升序或降序 keyset 分批读取。
pub struct BatchReader<E: EntityTrait> {
    query: Select<E>,
    after: Option<BatchCursor<E>>,
}

impl<E: EntityTrait> BatchReader<E> {
    /// 使用完整实体查询创建 reader；查询应保留实体主键列。
    pub fn new(query: Select<E>) -> Self {
        Self { query, after: None }
    }

    /// 从先前报告的最后完整游标继续读取。
    #[must_use]
    pub fn after(mut self, cursor: BatchCursor<E>) -> Self {
        self.after = Some(cursor);
        self
    }

    /// 逐批执行 callback，并通过 `Db` 的主库连接读取。
    ///
    /// 每次最多物化 `options.size()` 个模型。取消仅在批次间检查；callback
    /// 失败时返回错误，报告游标不会推进到失败批次。
    pub async fn run<F, Fut>(
        self,
        db: &Db,
        options: BatchOptions,
        direction: BatchDirection,
        cancellation: &Cancellation,
        mut process: F,
    ) -> Result<BatchReport<E>>
    where
        E::Model: ModelTrait<Entity = E> + Send + Sync,
        F: FnMut(Vec<E::Model>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let connection = db.primary_connection()?;
        self.run_on(&connection, options, direction, cancellation, &mut process)
            .await
    }

    async fn run_on<C, F, Fut>(
        self,
        connection: &C,
        options: BatchOptions,
        direction: BatchDirection,
        cancellation: &Cancellation,
        process: &mut F,
    ) -> Result<BatchReport<E>>
    where
        C: ConnectionTrait,
        E::Model: ModelTrait<Entity = E> + Send + Sync,
        F: FnMut(Vec<E::Model>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        if E::PrimaryKey::iter().next().is_none() {
            return Err(Error::custom(
                "batch_primary_key_required",
                "分批读取要求实体定义主键",
            ));
        }
        let identity = primary_key_identity::<E>()?;
        let mut cursor_value = self.after.map(|cursor| cursor.value);
        let mut report = BatchReport {
            batches: 0,
            rows: 0,
            cancelled: false,
            cursor: None,
        };

        loop {
            if cancellation.is_cancelled() {
                report.cancelled = true;
                return Ok(report);
            }

            let mut cursor = self.query.clone().cursor_by(identity.clone());
            cursor.first(options.size as u64);
            match direction {
                BatchDirection::Ascending => {
                    cursor.asc();
                }
                BatchDirection::Descending => {
                    cursor.desc();
                }
            }
            if let Some(value) = cursor_value.clone() {
                cursor.after(value);
            }
            let rows = cursor.all(connection).await.map_err(Error::internal)?;
            if rows.is_empty() {
                return Ok(report);
            }
            let next = rows
                .last()
                .map(ModelTrait::get_primary_key_value)
                .ok_or_else(|| {
                    Error::custom("batch_primary_key_missing", "分批查询结果缺少主键")
                })?;
            let row_count = rows.len();
            process(rows).await?;
            report.batches += 1;
            report.rows += row_count;
            report.cursor = Some(BatchCursor {
                value: next.clone(),
                marker: PhantomData,
            });
            cursor_value = Some(next);
        }
    }
}

fn primary_key_identity<E: EntityTrait>() -> Result<Identity> {
    let mut columns = Vec::new();
    for key in E::PrimaryKey::iter() {
        match key.into_column().into_identity() {
            Identity::Unary(column) => columns.push(column),
            _ => return Err(Error::custom("batch_primary_key_invalid", "实体主键列无效")),
        }
    }
    match columns.len() {
        1 => Ok(Identity::Unary(columns[0].clone())),
        2 => Ok(Identity::Binary(columns[0].clone(), columns[1].clone())),
        3 => Ok(Identity::Ternary(
            columns[0].clone(),
            columns[1].clone(),
            columns[2].clone(),
        )),
        4.. => Ok(Identity::Many(columns)),
        _ => Err(Error::custom(
            "batch_primary_key_required",
            "分批读取要求实体定义主键",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_options_enforce_the_documented_row_bound() {
        assert_eq!(BatchOptions::default().size(), 100);
        assert!(BatchOptions::new(1).is_ok_and(|options| options.size() == 1));
        assert!(
            BatchOptions::new(MAX_BATCH_SIZE).is_ok_and(|options| options.size() == MAX_BATCH_SIZE)
        );
        assert!(BatchOptions::new(0).is_err());
        assert!(BatchOptions::new(MAX_BATCH_SIZE + 1).is_err());
    }
}
