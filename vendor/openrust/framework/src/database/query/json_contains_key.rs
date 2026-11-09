//! JSON 路径存在条件，复用查询与写入的条件树。

use super::comparison::{
    json_field_and_path, native_json_path_literal, parse_pg_integer_attribute,
    pg_json_path_template, sql_path_quote,
};
use super::{
    DbBackend, Error, Expr, ExprTrait, PredicateChain, QueryBuilder, QueryContext, Result,
    WhereGroup, unsupported_backend,
};

fn predicate(backend: DbBackend, field: Expr, path: Option<&str>, not: bool) -> Result<Expr> {
    if path.is_some_and(|path| path.contains('\0')) {
        return Err(Error::custom(
            "query_json_path_invalid",
            "JSON 查询路径不能包含 NUL",
        ));
    }
    let template = match (backend, path) {
        (DbBackend::MySql, Some(path)) => format!(
            "IFNULL(JSON_CONTAINS_PATH(?, 'one', {}), 0)",
            native_json_path_literal(Some(path))
        ),
        (DbBackend::Postgres, Some(path)) => pg_key_template(path),
        (DbBackend::Sqlite, Some(path)) => {
            format!(
                "JSON_TYPE(?, {}) IS NOT NULL",
                native_json_path_literal(Some(path))
            )
        }
        (DbBackend::Sqlite, None) => "JSON_TYPE(?) IS NOT NULL".to_owned(),
        (DbBackend::MySql | DbBackend::Postgres, None) => {
            return Err(Error::custom(
                "query_json_key_path_required",
                "当前数据库的 JSON 键存在条件需要箭头路径",
            ));
        }
        _ => return Err(unsupported_backend()),
    };
    let expression = Expr::cust_with_expr(template, field);
    Ok(if not { expression.not() } else { expression })
}

fn pg_key_template(path: &str) -> String {
    let (parent, last) = path
        .rsplit_once("->")
        .map_or((None, path), |(parent, last)| (Some(parent), last));
    if let Some(index) = parse_pg_integer_attribute(last) {
        let threshold = index.unsigned_abs() + u64::from(index >= 0);
        return pg_array_key_template(parent, threshold);
    }
    if let Some((prefix_len, index)) = pg_last_bracket_index(last) {
        let parent = &path[..path.len() - last.len() + prefix_len];
        let digits = index.trim_start_matches('-');
        let magnitude = digits.parse::<u64>().unwrap_or(u64::MAX);
        let threshold =
            magnitude.saturating_add(u64::from(!index.starts_with('-') || magnitude == 0));
        return pg_array_key_template(Some(parent), threshold);
    }
    let column = pg_json_path_template(parent, false);
    let key = sql_path_quote(last);
    format!("COALESCE(({column})::jsonb ? {key}, FALSE)")
}

fn pg_last_bracket_index(segment: &str) -> Option<(usize, &str)> {
    // 上游末尾数字括号的正则也匹配末尾单个换行之前。
    let segment = segment
        .strip_suffix('\n')
        .unwrap_or(segment)
        .strip_suffix(']')?;
    let (prefix, index) = segment.rsplit_once('[')?;
    let digits = index.strip_prefix('-').unwrap_or(index);
    (!digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then_some((prefix.len(), index))
}

fn pg_array_key_template(parent: Option<&str>, threshold: u64) -> String {
    let column = pg_json_path_template(parent, false);
    // PostgreSQL 的数组长度是整数，任何更大的阈值都不可能匹配。
    let threshold = std::cmp::min(threshold, u64::from(i32::MAX.unsigned_abs()) + 1);
    format!(
        "CASE WHEN jsonb_typeof(({column})::jsonb) = 'array' \
         THEN jsonb_array_length(({column})::jsonb) >= {threshold} ELSE FALSE END"
    )
}

impl PredicateChain<'_> {
    fn json_contains_key(
        self,
        context: &QueryContext<'_>,
        selector: &str,
        or: bool,
        not: bool,
    ) -> Result<Self> {
        let (field, path) = json_field_and_path(context, selector)?;
        let expression = predicate(context.db.backend()?, field, path, not)?;
        Ok(self.add_comparison(expression.clone(), expression, Vec::new(), or))
    }
}

macro_rules! json_contains_key_methods {
    () => {
        /// 添加 JSON 路径存在条件，值为 JSON null 的成员也算存在。
        ///
        /// MySQL 和 PostgreSQL 要求使用箭头路径。SQLite 也接受裸列以检查根值。
        pub fn where_json_contains_key(self, selector: &str) -> Result<Self> {
            self.json_contains_key(selector, false, false)
        }

        /// 添加 OR JSON 路径存在条件。
        pub fn or_where_json_contains_key(self, selector: &str) -> Result<Self> {
            self.json_contains_key(selector, true, false)
        }

        /// 添加 JSON 路径不存在条件，缺失路径及 SQL NULL 均匹配。
        pub fn where_json_doesnt_contain_key(self, selector: &str) -> Result<Self> {
            self.json_contains_key(selector, false, true)
        }

        /// 添加 OR JSON 路径不存在条件。
        pub fn or_where_json_doesnt_contain_key(self, selector: &str) -> Result<Self> {
            self.json_contains_key(selector, true, true)
        }
    };
}

impl QueryBuilder<'_, '_> {
    json_contains_key_methods!();

    fn json_contains_key(self, selector: &str, or: bool, not: bool) -> Result<Self> {
        self.transform(|mut state| {
            let context = state.context();
            state.predicates = state
                .predicates
                .json_contains_key(&context, selector, or, not)?;
            Ok(state)
        })?;
        Ok(self)
    }
}

impl WhereGroup<'_, '_> {
    json_contains_key_methods!();

    fn json_contains_key(mut self, selector: &str, or: bool, not: bool) -> Result<Self> {
        self.condition = self
            .condition
            .json_contains_key(&self.context, selector, or, not)?;
        Ok(self)
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/database/query/json_contains_key.rs"]
mod tests;
