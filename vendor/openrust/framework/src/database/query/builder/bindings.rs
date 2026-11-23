use indexmap::IndexMap;

use super::{Builder, subqueries::Source};
use crate::database::{DatabaseError, DatabaseResult, Value};

const TYPES: [&str; 9] = [
    "select",
    "from",
    "join",
    "where",
    "groupBy",
    "having",
    "order",
    "union",
    "unionOrder",
];

/// 单个绑定值或一组绑定值，保留类型化参数而不拼接 SQL。
pub enum BindingValues {
    Value(Value),
    Values(Vec<Value>),
}
impl<T: Into<Value>> From<T> for BindingValues {
    fn from(value: T) -> Self {
        Self::Value(value.into())
    }
}
impl<T: Into<Value>> From<Vec<T>> for BindingValues {
    fn from(values: Vec<T>) -> Self {
        Self::Values(values.into_iter().map(Into::into).collect())
    }
}
impl<T: Into<Value>, const N: usize> From<[T; N]> for BindingValues {
    fn from(values: [T; N]) -> Self {
        Self::Values(values.into_iter().map(Into::into).collect())
    }
}

#[derive(Clone)]
pub(in crate::database::query) struct Change {
    values: Vec<Value>,
    // 手动修改时已有的自动绑定前缀；之后新增的条件继续追加自己的绑定。
    prefix: usize,
}
impl Change {
    pub(in crate::database::query) fn apply(&self, values: &mut Vec<Value>, start: usize) {
        let suffix = values.split_off((start + self.prefix).min(values.len()));
        values.truncate(start);
        values.extend_from_slice(&self.values);
        values.extend(suffix);
    }
}
pub(super) type Changes = [Option<Change>; 9];

fn index(kind: Option<&str>) -> DatabaseResult<usize> {
    TYPES
        .iter()
        .position(|name| *name == kind.unwrap_or("where"))
        .ok_or(DatabaseError::InvalidQuery("绑定类别无效"))
}

impl Builder<'_> {
    pub fn get_raw_bindings(&self) -> DatabaseResult<IndexMap<&'static str, Vec<Value>>> {
        TYPES
            .iter()
            .enumerate()
            .map(|(kind, name)| {
                let mut values = self.natural_bindings(kind)?;
                self.change_bindings(kind, &mut values, 0);
                Ok((*name, values))
            })
            .collect()
    }

    pub fn set_bindings(mut self, values: Vec<Value>, kind: Option<&str>) -> DatabaseResult<Self> {
        let kind = index(kind)?;
        let prefix = self.natural_bindings(kind)?.len();
        self.binding_changes.get_or_insert_default()[kind] = Some(Change { values, prefix });
        Ok(self)
    }

    pub fn add_binding(
        self,
        values: impl Into<BindingValues>,
        kind: Option<&str>,
    ) -> DatabaseResult<Self> {
        let kind_index = index(kind)?;
        let mut bindings = self.natural_bindings(kind_index)?;
        self.change_bindings(kind_index, &mut bindings, 0);
        match values.into() {
            BindingValues::Value(value) => bindings.push(value),
            BindingValues::Values(values) => bindings.extend(values),
        }
        self.set_bindings(bindings, kind)
    }

    pub fn merge_bindings(mut self, query: &Builder<'_>) -> DatabaseResult<Self> {
        let incoming = query.get_raw_bindings()?;
        for (kind, values) in incoming {
            self = self.add_binding(values, Some(kind))?;
        }
        Ok(self)
    }

    pub(super) fn natural_bindings(&self, kind: usize) -> DatabaseResult<Vec<Value>> {
        let mut sql = String::new();
        let mut values = Vec::new();
        match kind {
            0 => self.compile_columns(&self.columns, &mut sql, &mut values, true)?,
            1 => match &self.source {
                Source::Table(_) => {}
                Source::Query(expression, _) | Source::Raw(expression) => {
                    values.extend_from_slice(&expression.bindings)
                }
            },
            2 => {
                for join in &self.joins {
                    join.compile(&self.grammar, &mut sql, &mut values)?;
                }
            }
            3 => self
                .predicates
                .compile(&self.grammar, &mut sql, &mut values)?,
            4 => self.compile_columns(&self.groups, &mut sql, &mut values, false)?,
            5 => self.havings.compile(&self.grammar, &mut sql, &mut values)?,
            6 => self.compile_orders(&self.orders, &mut sql, &mut values)?,
            7 => {
                for (_, expression) in &self.unions {
                    values.extend_from_slice(&expression.bindings);
                }
            }
            8 => self.compile_orders(&self.union_orders, &mut sql, &mut values)?,
            _ => unreachable!("内部绑定类别"),
        }
        Ok(values)
    }

    pub(super) fn change_bindings(&self, kind: usize, values: &mut Vec<Value>, start: usize) {
        if let Some(change) = self
            .binding_changes
            .as_ref()
            .and_then(|changes| changes[kind].as_ref())
        {
            change.apply(values, start);
        }
    }

    pub(super) fn compile_bound_orders(
        &self,
        sql: &mut String,
        values: &mut Vec<Value>,
    ) -> DatabaseResult<()> {
        let start = values.len();
        self.compile_orders(&self.orders, sql, values)?;
        self.change_bindings(6, values, start);
        Ok(())
    }

    pub(super) fn clear_binding_changes(&mut self, kind: usize) {
        if let Some(changes) = &mut self.binding_changes {
            changes[kind] = None;
        }
    }

    // 来源可被替换，但已有from绑定按上游保留，新来源的绑定接在其后。
    pub(super) fn retain_source_bindings(&mut self) {
        let mut values = match &self.source {
            Source::Table(_) => Vec::new(),
            Source::Query(expression, _) | Source::Raw(expression) => expression.bindings.clone(),
        };
        self.change_bindings(1, &mut values, 0);
        if !values.is_empty()
            || self
                .binding_changes
                .as_ref()
                .is_some_and(|changes| changes[1].is_some())
        {
            self.binding_changes.get_or_insert_default()[1] = Some(Change { values, prefix: 0 });
        }
    }
}
