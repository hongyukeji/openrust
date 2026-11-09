//! 通用比较前置；动态PHP猜参、Closure列与多列条件另行对应。

use super::{
    ColumnOperand, DbBackend, Error, Expr, ExprTrait, PredicateChain, QueryBuilder, QueryContext,
    QueryState, RawSql, Result, Value, WhereGroup, unsupported_backend,
};
use sea_orm::sea_query::BinOper;
use std::str::FromStr;

macro_rules! operators {
    ($(($name:ident, $sql:literal)),+ $(,)?) => {
        /// 查询构造器和目标语法支持的操作符集合。拒绝其他 SQL token。
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum WhereOperator { $(#[doc = concat!("SQL 操作符 `", $sql, "`。")] $name),+ }
        impl WhereOperator {
            fn sql(self) -> &'static str { match self { $(Self::$name => $sql),+ } }
            pub(super) fn builder_operator(self) -> bool {
                !matches!(self, Self::SoundsLike | Self::Between | Self::BitXorPg |
                    Self::SubnetContained | Self::SubnetContains | Self::Overlap | Self::Contains |
                    Self::Contained | Self::HasKey | Self::HasAnyKey | Self::HasAllKeys |
                    Self::Concat | Self::Subtract | Self::JsonPathExists | Self::TextSearch |
                    Self::JsonDeletePath | Self::DistinctFrom | Self::NotDistinctFrom)
            }
        }
        impl FromStr for WhereOperator {
            type Err = Error;
            fn from_str(value: &str) -> Result<Self> {
                match value.to_ascii_lowercase().as_str() { $($sql => Ok(Self::$name)),+, _ => Err(invalid_operator()) }
            }
        }
    };
}
operators! {
    (Eq, "="), (Lt, "<"), (Gt, ">"), (Le, "<="), (Ge, ">="),
    (NotEq, "<>"), (NotEqBang, "!="), (NullSafeEq, "<=>"),
    (Like, "like"), (LikeBinary, "like binary"), (NotLike, "not like"), (ILike, "ilike"),
    (BitAnd, "&"), (BitOr, "|"), (BitXor, "^"), (ShiftLeft, "<<"), (ShiftRight, ">>"), (BitAndNot, "&~"),
    (Is, "is"), (IsNot, "is not"), (RLike, "rlike"), (NotRLike, "not rlike"),
    (Regexp, "regexp"), (NotRegexp, "not regexp"), (Regex, "~"), (RegexInsensitive, "~*"),
    (NotRegex, "!~"), (NotRegexInsensitive, "!~*"), (SimilarTo, "similar to"),
    (NotSimilarTo, "not similar to"), (NotILike, "not ilike"), (PgILike, "~~*"), (PgNotILike, "!~~*"),
    (SoundsLike, "sounds like"), (Between, "between"), (BitXorPg, "#"),
    (SubnetContained, "<<="), (SubnetContains, ">>="), (Overlap, "&&"), (Contains, "@>"),
    (Contained, "<@"), (HasKey, "?"), (HasAnyKey, "?|"), (HasAllKeys, "?&"),
    (Concat, "||"), (Subtract, "-"), (JsonPathExists, "@?"), (TextSearch, "@@"),
    (JsonDeletePath, "#-"), (DistinctFrom, "is distinct from"), (NotDistinctFrom, "is not distinct from"),
}

/// 比较右值：普通值绑定，显式SQL表达式原样使用。
///
/// 已有标量查询保留实时SQL与挂接时绑定，父查询不主动消费其查询前回调。
pub struct WhereValue<'a>(ValueOperand<'a>);
enum ValueOperand<'a> {
    Bound(Value),
    Expression(ColumnOperand<'a>),
    Sub(Box<dyn FnOnce() -> Result<super::subqueries::DeferredChild<'a>> + Send + 'a>),
}
impl From<Value> for WhereValue<'_> {
    fn from(value: Value) -> Self {
        Self(ValueOperand::Bound(value))
    }
}
macro_rules! bound_values {
    ($($ty:ty),+ $(,)?) => { $(impl From<$ty> for WhereValue<'_> {
        fn from(value: $ty) -> Self { Self::from(Value::from(value)) }
    })+ };
}
bound_values!(
    String,
    &str,
    bool,
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    f32,
    f64,
    Vec<u8>
);
impl<T: Into<Value>> From<Option<T>> for WhereValue<'_> {
    fn from(value: Option<T>) -> Self {
        Self::from(value.map_or(Value::String(None), Into::into))
    }
}
impl From<RawSql> for WhereValue<'_> {
    fn from(value: RawSql) -> Self {
        Self(ValueOperand::Expression(value.into()))
    }
}
impl<'src: 'capture, 'capture, 'a> From<&QueryBuilder<'src, 'capture>> for WhereValue<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(value: &QueryBuilder<'src, 'capture>) -> Self {
        let query = value.alias();
        Self(ValueOperand::Sub(Box::new(move || {
            super::subqueries::DeferredChild::capture(&query, false)
        })))
    }
}
impl<'src: 'capture, 'capture, 'a> From<QueryBuilder<'src, 'capture>> for WhereValue<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(value: QueryBuilder<'src, 'capture>) -> Self {
        Self::from(&value)
    }
}

pub(super) fn validate_operator_backend(operator: WhereOperator, backend: DbBackend) -> Result<()> {
    let supported = match backend {
        DbBackend::MySql => operator.builder_operator() || operator == WhereOperator::SoundsLike,
        DbBackend::Postgres => operator != WhereOperator::SoundsLike,
        DbBackend::Sqlite => operator.builder_operator(),
        _ => return Err(unsupported_backend()),
    };
    if supported {
        Ok(())
    } else {
        Err(invalid_operator())
    }
}
fn invalid_operator() -> Error {
    Error::custom("query_operator_invalid", "查询操作符不在固定支持列表中")
}
pub(super) fn null_operator() -> Error {
    Error::custom("query_operator_null_invalid", "查询操作符不能与NULL组合")
}

fn comparison_expr(
    left: Expr,
    operator: WhereOperator,
    right: Expr,
    backend: DbBackend,
) -> Result<Expr> {
    let token = if operator == WhereOperator::NullSafeEq {
        match backend {
            DbBackend::MySql => "<=>",
            DbBackend::Sqlite => "is",
            DbBackend::Postgres => "is not distinct from",
            _ => return Err(unsupported_backend()),
        }
    } else {
        operator.sql()
    };
    let bitwise = bitwise_operator(operator, backend);
    let left = if backend == DbBackend::Postgres && !bitwise && token.contains("like") {
        Expr::cust_with_expr("$1::text", left)
    } else {
        left
    };
    let expression = left.binary(BinOper::Custom(token), right);
    if backend == DbBackend::Postgres && bitwise {
        Ok(Expr::cust_with_expr("($1)::bool", expression))
    } else {
        Ok(expression)
    }
}
fn bitwise_operator(operator: WhereOperator, backend: DbBackend) -> bool {
    matches!(
        operator,
        WhereOperator::BitAnd
            | WhereOperator::BitOr
            | WhereOperator::BitXor
            | WhereOperator::ShiftLeft
            | WhereOperator::ShiftRight
            | WhereOperator::BitAndNot
    ) || (backend == DbBackend::Postgres
        && matches!(
            operator,
            WhereOperator::Regex
                | WhereOperator::BitXorPg
                | WhereOperator::SubnetContained
                | WhereOperator::SubnetContains
        ))
}

fn null_comparison(left_sql: Expr, left_bind: Expr, not: bool, mysql_json: bool) -> (Expr, Expr) {
    if mysql_json {
        let template = if not {
            "(? is not null AND json_type(?) != 'NULL')"
        } else {
            "(? is null OR json_type(?) = 'NULL')"
        };
        let expression = Expr::cust_with_exprs(template, [left_sql.clone(), left_sql]);
        (expression.clone(), expression)
    } else if not {
        (left_sql.is_not_null(), left_bind.is_not_null())
    } else {
        (left_sql.is_null(), left_bind.is_null())
    }
}

fn bound_comparison(
    value: Value,
    boolean: Option<bool>,
    json_boolean: bool,
    backend: DbBackend,
) -> (Expr, Expr) {
    if let Some(boolean) = boolean {
        let text = if boolean { "true" } else { "false" };
        let expression = if json_boolean && backend == DbBackend::Postgres {
            Expr::cust(format!("'{text}'::jsonb"))
        } else {
            Expr::cust(text)
        };
        (expression.clone(), expression)
    } else {
        (Expr::val(value.clone()), Expr::val(value))
    }
}

impl<'a> PredicateChain<'a> {
    pub(super) fn compare<'operand: 'a>(
        self,
        context: &QueryContext<'_>,
        column: impl Into<ColumnOperand<'operand>>,
        operator: WhereOperator,
        value: WhereValue<'operand>,
        or: bool,
    ) -> Result<Self> {
        let backend = context.db.backend()?;
        validate_operator_backend(operator, backend)?;
        let null = matches!(&value.0, ValueOperand::Bound(value) if *value == value.as_null());
        if null
            && operator.builder_operator()
            && !matches!(
                operator,
                WhereOperator::Eq
                    | WhereOperator::NullSafeEq
                    | WhereOperator::NotEq
                    | WhereOperator::NotEqBang
            )
        {
            return Err(null_operator());
        }
        // 固定prepareValueAndOperator先于createSub；拒绝参数不得消费LHS回调。
        let column = column.into();
        let value = match value.0 {
            ValueOperand::Sub(child) => {
                let (sql, bindings, origin) = if let Some(column) =
                    column.column_name().filter(|column| column.contains("->"))
                {
                    let expression = json_selector(context, column, backend, false, false)?;
                    (expression.clone(), expression, None)
                } else {
                    column.expressions(context)?
                };
                let child = child()?;
                child.validate(context)?;
                return Ok(self.add_comparison_sub(
                    sql,
                    bindings,
                    operator.sql(),
                    child,
                    origin.into_iter().collect(),
                    or,
                ));
            }
            other => WhereValue(other),
        };
        let has_json_arrow = column.contains_json_arrow(context)?;
        let json_column = column
            .column_name()
            .or_else(|| {
                if null && backend == DbBackend::MySql {
                    column.raw_text()
                } else {
                    None
                }
            })
            .filter(|name| name.contains("->"))
            .map(str::to_owned);
        let boolean = match &value.0 {
            ValueOperand::Bound(Value::Bool(Some(value))) if has_json_arrow => Some(*value),
            _ => None,
        };
        let json_boolean = boolean.is_some()
            && json_column.is_some()
            && !bitwise_operator(operator, backend)
            && operator != WhereOperator::NullSafeEq;
        let (left_sql, left_bind, left_origin) = if let Some(selector) = &json_column {
            let expression = json_selector(context, selector, backend, json_boolean, null)?;
            (expression.clone(), expression, None)
        } else {
            column.expressions(context)?
        };
        let mut origins = left_origin.into_iter().collect::<Vec<_>>();
        let (sql, bindings) = if null {
            let not = !matches!(operator, WhereOperator::Eq | WhereOperator::NullSafeEq);
            null_comparison(
                left_sql,
                left_bind,
                not,
                backend == DbBackend::MySql && json_column.is_some(),
            )
        } else {
            let (right_sql, right_bind, right_origin) = match value.0 {
                ValueOperand::Bound(value) => {
                    let (sql, bindings) = bound_comparison(value, boolean, json_boolean, backend);
                    (sql, bindings, None)
                }
                ValueOperand::Expression(value) => value.expressions(context)?,
                ValueOperand::Sub(_) => return Err(invalid_operator()),
            };
            origins.extend(right_origin);
            if json_boolean {
                (
                    left_sql.binary(BinOper::Custom(operator.sql()), right_sql),
                    left_bind.binary(BinOper::Custom(operator.sql()), right_bind),
                )
            } else {
                (
                    comparison_expr(left_sql, operator, right_sql, backend)?,
                    comparison_expr(left_bind, operator, right_bind, backend)?,
                )
            }
        };
        Ok(self.add_comparison(sql, bindings, origins, or))
    }
}

// 对应CompilesJsonPaths尾数组分组：只有连续的非空[...]后缀拆成数组键。
fn path_segment(segment: &str) -> (&str, Vec<&str>) {
    for (start, character) in segment.char_indices() {
        if character != '[' {
            continue;
        }
        let mut rest = &segment[start..];
        let mut indices = Vec::new();
        while let Some(index) = rest.strip_prefix('[') {
            let Some(end) = index.find(']') else {
                break;
            };
            if end == 0 {
                break;
            }
            indices.push(&index[..end]);
            rest = &index[end + 1..];
        }
        // PCRE 的 $ 也匹配单个末尾换行前的位置。
        if rest.is_empty() || rest == "\n" {
            return (&segment[..start], indices);
        }
    }
    (segment, Vec::new())
}
fn pg_integer_attribute(value: &str) -> bool {
    // 固定FILTER_VALIDATE_INT：ASCII默认空白、可选符号、无多余前导零及PHP64范围。
    let value = value.trim_matches([' ', '\t', '\n', '\r', '\u{b}']);
    let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
    !digits.is_empty()
        && (digits == "0" || !digits.starts_with('0'))
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && value.parse::<i64>().is_ok()
}
// 相对上游普通引号的安全增强：E字面量明确反斜线语义，兼容PG两种字符串模式。
fn sql_path_quote(value: &str) -> String {
    format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
}
pub(super) fn json_field_and_path<'selector>(
    context: &QueryContext<'_>,
    selector: &'selector str,
) -> Result<(Expr, Option<&'selector str>)> {
    // 只拆字段与路径。路径中的空格及 " as " 仍属于 JSON 键。
    let (field, path) = selector
        .split_once("->")
        .map_or((selector, None), |(field, path)| (field, Some(path)));
    let (field, _, _) = ColumnOperand::from(field).expressions(context)?;
    Ok((field, path))
}

pub(super) fn pg_json_path_template(path: Option<&str>, terminal_text: bool) -> String {
    let mut attributes = Vec::new();
    for segment in path.into_iter().flat_map(|path| path.split("->")) {
        let (key, indices) = path_segment(segment);
        if indices.is_empty() {
            attributes.push(segment);
        } else {
            if !key.is_empty() {
                attributes.push(key);
            }
            attributes.extend(indices);
        }
    }
    let last = attributes.len().saturating_sub(1);
    let mut template = "$1".to_owned();
    for (index, attribute) in attributes.into_iter().enumerate() {
        template.push_str(if index == last && terminal_text {
            "->>"
        } else {
            "->"
        });
        template.push_str(&if pg_integer_attribute(attribute) {
            attribute.to_owned()
        } else {
            sql_path_quote(attribute)
        });
    }
    template
}

pub(super) fn native_json_path_literal(path: Option<&str>) -> String {
    let mut rendered = String::from("$");
    for (index, segment) in path
        .into_iter()
        .flat_map(|path| path.split("->"))
        .enumerate()
    {
        let (key, indices) = path_segment(segment);
        if (!key.is_empty() && key != "0") || indices.is_empty() {
            rendered.push('.');
            rendered.push('"');
            rendered.push_str(key);
            rendered.push('"');
        } else if index != 0 {
            rendered.push('.');
        }
        for index in indices {
            rendered.push('[');
            rendered.push_str(index);
            rendered.push(']');
        }
    }
    // 固定wrapJsonPath把紧贴单引号的反斜线一起替换，保留其它路径字符。
    let mut literal = String::from("'");
    let mut backslashes = 0;
    for character in rendered.chars() {
        if character == '\\' {
            backslashes += 1;
            continue;
        }
        if character == '\'' {
            literal.push_str("''");
        } else {
            for _ in 0..backslashes {
                literal.push('\\');
            }
            literal.push(character);
        }
        backslashes = 0;
    }
    for _ in 0..backslashes {
        literal.push('\\');
    }
    literal.push('\'');
    literal
}

fn json_selector(
    context: &QueryContext<'_>,
    selector: &str,
    backend: DbBackend,
    boolean: bool,
    null: bool,
) -> Result<Expr> {
    let (field, path) = json_field_and_path(context, selector)?;
    if backend == DbBackend::Postgres {
        let template = pg_json_path_template(path, !boolean);
        let template = if boolean {
            format!("({template})::jsonb")
        } else {
            template
        };
        Ok(Expr::cust_with_expr(template, field))
    } else {
        let path = native_json_path_literal(path);
        let extract = format!("json_extract(?, {path})");
        let template = if backend == DbBackend::MySql && !boolean && !null {
            format!("json_unquote({extract})")
        } else {
            extract
        };
        Ok(Expr::cust_with_expr(template, field))
    }
}

macro_rules! comparison_methods {
    ($state:ident, $context:expr) => {
        /// 以默认相等操作符添加绑定比较；NULL转换为IS NULL。
        pub fn r#where<'operand: 'cap>(
            self,
            column: impl Into<ColumnOperand<'operand>>,
            value: impl Into<WhereValue<'operand>>,
        ) -> Result<Self> {
            self.where_compare(column, WhereOperator::Eq, value)
        }
        /// 添加OR默认相等比较，保留SQL AND优先规则。
        pub fn or_where_value<'operand: 'cap>(
            self,
            column: impl Into<ColumnOperand<'operand>>,
            value: impl Into<WhereValue<'operand>>,
        ) -> Result<Self> {
            self.or_where_compare(column, WhereOperator::Eq, value)
        }
        /// 添加明确操作符的AND比较；值与标量查询通过同一安全编译链。
        pub fn where_compare<'operand: 'cap>(
            mut self,
            column: impl Into<ColumnOperand<'operand>>,
            operator: WhereOperator,
            value: impl Into<WhereValue<'operand>>,
        ) -> Result<Self> {
            let context = ($context)(&self);
            self.$state = self
                .$state
                .compare(&context, column, operator, value.into(), false)?;
            Ok(self)
        }
        /// 添加明确操作符的OR比较；不把OR误编成左结合布尔树。
        pub fn or_where_compare<'operand: 'cap>(
            mut self,
            column: impl Into<ColumnOperand<'operand>>,
            operator: WhereOperator,
            value: impl Into<WhereValue<'operand>>,
        ) -> Result<Self> {
            let context = ($context)(&self);
            self.$state = self
                .$state
                .compare(&context, column, operator, value.into(), true)?;
            Ok(self)
        }
    };
}
impl<'tx: 'cap, 'cap> QueryState<'tx, 'cap> {
    comparison_methods!(predicates, |state: &Self| state.context());
}
impl<'tx: 'cap, 'cap> WhereGroup<'tx, 'cap> {
    comparison_methods!(condition, |group: &Self| group.context.clone());
}
impl<'tx: 'cap, 'cap> QueryBuilder<'tx, 'cap> {
    /// 以默认相等操作符添加条件；Rust使用原始标识符调用`r#where`。
    pub fn r#where<'operand: 'cap>(
        self,
        column: impl Into<ColumnOperand<'operand>>,
        value: impl Into<WhereValue<'operand>>,
    ) -> Result<Self> {
        self.where_compare(column, WhereOperator::Eq, value)
    }
    /// 以默认相等操作符添加OR条件。
    pub fn or_where_value<'operand: 'cap>(
        self,
        column: impl Into<ColumnOperand<'operand>>,
        value: impl Into<WhereValue<'operand>>,
    ) -> Result<Self> {
        self.or_where_compare(column, WhereOperator::Eq, value)
    }
    /// 使用封闭操作符添加AND比较；不执行PHP动态operator猜参。
    pub fn where_compare<'operand: 'cap>(
        self,
        column: impl Into<ColumnOperand<'operand>>,
        operator: WhereOperator,
        value: impl Into<WhereValue<'operand>>,
    ) -> Result<Self> {
        self.transform(|state| state.where_compare(column, operator, value))?;
        Ok(self)
    }
    /// 使用封闭操作符添加OR比较，保留现有租约与来源末检。
    pub fn or_where_compare<'operand: 'cap>(
        self,
        column: impl Into<ColumnOperand<'operand>>,
        operator: WhereOperator,
        value: impl Into<WhereValue<'operand>>,
    ) -> Result<Self> {
        self.transform(|state| state.or_where_compare(column, operator, value))?;
        Ok(self)
    }
}

#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;

#[cfg(all(test, any(feature = "db-postgres", feature = "db-mysql")))]
#[path = "comparison_real_tests.rs"]
mod real_tests;
