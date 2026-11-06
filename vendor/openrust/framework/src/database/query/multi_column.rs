//! 多列谓词组；区分省略参数、明确NULL及可重复的右侧查询来源。

use super::{
    Alias, ColumnOperand, Error, PredicateChain, QueryBuilder, QueryContext, QueryState, RawSql,
    Result, Value, WhereGroup, WhereOperator, WhereValue,
};

type ColumnCallback<'a> =
    Box<dyn FnOnce(QueryBuilder<'a, 'a>) -> Result<QueryBuilder<'a, 'a>> + Send + 'a>;
type ValueCallback<'a> =
    Box<dyn FnMut(QueryBuilder<'a, 'a>) -> Result<QueryBuilder<'a, 'a>> + Send + 'a>;

/// 多列条件的列来源；查询及闭包按声明顺序准备，不在转换时提前消费回调。
pub struct MultiColumn<'a>(ColumnSource<'a>);
enum ColumnSource<'a> {
    Name(String),
    Raw(RawSql),
    Query(Box<dyn FnOnce() -> ColumnOperand<'a> + Send + 'a>),
    Callback(ColumnCallback<'a>),
}
impl<'a> MultiColumn<'a> {
    /// 省略比较参数时配置nested组；明确操作符时配置真正空FROM的左标量查询。
    pub fn callback<F>(callback: F) -> Self
    where
        F: FnOnce(QueryBuilder<'a, 'a>) -> Result<QueryBuilder<'a, 'a>> + Send + 'a,
    {
        Self(ColumnSource::Callback(Box::new(callback)))
    }
}
impl From<&str> for MultiColumn<'_> {
    fn from(value: &str) -> Self {
        Self(ColumnSource::Name(value.into()))
    }
}
impl From<String> for MultiColumn<'_> {
    fn from(value: String) -> Self {
        Self(ColumnSource::Name(value))
    }
}
impl From<RawSql> for MultiColumn<'_> {
    fn from(value: RawSql) -> Self {
        Self(ColumnSource::Raw(value))
    }
}
impl<'src: 'capture, 'capture, 'a> From<&QueryBuilder<'src, 'capture>> for MultiColumn<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(query: &QueryBuilder<'src, 'capture>) -> Self {
        let query = query.alias();
        Self(ColumnSource::Query(Box::new(move || {
            ColumnOperand::from(query)
        })))
    }
}
impl<'src: 'capture, 'capture, 'a> From<QueryBuilder<'src, 'capture>> for MultiColumn<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(query: QueryBuilder<'src, 'capture>) -> Self {
        Self::from(&query)
    }
}

/// 每列重新取得的比较右值；不克隆一次性`WhereValue`工厂。
pub struct MultiValue<'a>(RepeatedValue<'a>);
enum RepeatedValue<'a> {
    Bound(Value),
    Raw(RawSql),
    Query(Box<dyn FnMut() -> WhereValue<'a> + Send + 'a>),
    Callback(ValueCallback<'a>),
}
impl<'a> MultiValue<'a> {
    /// 按列顺序对新的无FROM查询调用一次闭包；不主动执行其查询前回调。
    pub fn callback<F>(callback: F) -> Self
    where
        F: FnMut(QueryBuilder<'a, 'a>) -> Result<QueryBuilder<'a, 'a>> + Send + 'a,
    {
        Self(RepeatedValue::Callback(Box::new(callback)))
    }
    fn is_null(&self) -> bool {
        matches!(&self.0,RepeatedValue::Bound(value) if *value==value.as_null())
    }
    fn materialize(&mut self, context: &QueryContext<'a>) -> Result<WhereValue<'a>> {
        Ok(match &mut self.0 {
            RepeatedValue::Bound(value) => WhereValue::from(value.clone()),
            RepeatedValue::Raw(value) => WhereValue::from(value.clone()),
            RepeatedValue::Query(factory) => factory(),
            RepeatedValue::Callback(callback) => {
                let query = callback(fresh(context, false))?;
                context.validate_connection(&query.query_context()?)?;
                WhereValue::from(query)
            }
        })
    }
}
impl From<Value> for MultiValue<'_> {
    fn from(value: Value) -> Self {
        Self(RepeatedValue::Bound(value))
    }
}
impl From<RawSql> for MultiValue<'_> {
    fn from(value: RawSql) -> Self {
        Self(RepeatedValue::Raw(value))
    }
}
impl<T: Into<Value>> From<Option<T>> for MultiValue<'_> {
    fn from(value: Option<T>) -> Self {
        Self::from(value.map_or(Value::String(None), Into::into))
    }
}
impl<'src: 'capture, 'capture, 'a> From<&QueryBuilder<'src, 'capture>> for MultiValue<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(query: &QueryBuilder<'src, 'capture>) -> Self {
        let query = query.alias();
        Self(RepeatedValue::Query(Box::new(move || {
            WhereValue::from(&query)
        })))
    }
}
impl<'src: 'capture, 'capture, 'a> From<QueryBuilder<'src, 'capture>> for MultiValue<'a>
where
    'src: 'a,
    'capture: 'a,
{
    fn from(query: QueryBuilder<'src, 'capture>) -> Self {
        Self::from(&query)
    }
}

/// 明确区分PHP省略实参、默认Eq值与显式操作符；不猜测字符串是否为token。
pub enum MultiComparison<'a> {
    /// 省略比较实参；列闭包为nested组，普通列按NULL条件处理。
    Omitted,
    /// 明确的默认相等值；即使值为NULL，列闭包仍为标量查询。
    EqValue(MultiValue<'a>),
    /// 明确的操作符与可重复右值。
    WithOperator(WhereOperator, MultiValue<'a>),
}
impl<'a> MultiComparison<'a> {
    /// 使用默认相等操作符，NULL不会误作省略参数。
    pub fn eq(value: impl Into<MultiValue<'a>>) -> Self {
        Self::EqValue(value.into())
    }
    /// 使用固定操作符；每列重复取得右值。
    pub fn compare(operator: WhereOperator, value: impl Into<MultiValue<'a>>) -> Self {
        Self::WithOperator(operator, value.into())
    }
}
impl From<Value> for MultiComparison<'_> {
    fn from(value: Value) -> Self {
        Self::eq(value)
    }
}
impl<'a> From<MultiValue<'a>> for MultiComparison<'a> {
    fn from(value: MultiValue<'a>) -> Self {
        Self::eq(value)
    }
}
impl<T: Into<Value>> From<Option<T>> for MultiComparison<'_> {
    fn from(value: Option<T>) -> Self {
        Self::eq(MultiValue::from(value))
    }
}
macro_rules! values {
    ($($ty:ty),+ $(,)?)=>{$(
        impl From<$ty> for MultiValue<'_> {fn from(value:$ty)->Self {Self::from(Value::from(value))}}
        impl From<$ty> for MultiComparison<'_> {fn from(value:$ty)->Self {Self::eq(value)}}
    )+};
}
values!(
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

fn fresh<'a>(context: &QueryContext<'a>, nested: bool) -> QueryBuilder<'a, 'a> {
    let mut state = QueryState::empty(context.db.clone(), context.transaction);
    if nested && !context.table.is_empty() {
        state.table = Some(context.table.clone());
        state.select.from(Alias::new(&context.table));
    }
    QueryBuilder::from_state(state)
}
#[derive(Clone, Copy)]
enum Mode {
    Any,
    All,
    None,
}
fn build_group<'a, I, C>(
    context: &QueryContext<'a>,
    columns: I,
    comparison: MultiComparison<'a>,
    mode: Mode,
) -> Result<PredicateChain<'a>>
where
    I: IntoIterator<Item = C>,
    C: Into<MultiColumn<'a>>,
{
    let (operator, mut value) = match comparison {
        MultiComparison::Omitted => (None, None),
        MultiComparison::EqValue(value) => (Some(WhereOperator::Eq), Some(value)),
        MultiComparison::WithOperator(operator, value) => (Some(operator), Some(value)),
    };
    // 只做prepareValueAndOperator先验；backend/identifier等leaf校验不能扩大到空columns。
    if let (Some(operator), Some(value)) = (operator, value.as_ref())
        && value.is_null()
        && operator.builder_operator()
        && !matches!(
            operator,
            WhereOperator::Eq
                | WhereOperator::NullSafeEq
                | WhereOperator::NotEq
                | WhereOperator::NotEqBang
        )
    {
        return Err(super::comparison::null_operator());
    }
    let mut tree = PredicateChain::all();
    let or = !matches!(mode, Mode::All);
    for column in columns {
        if let Some(operator) = operator {
            super::comparison::validate_operator_backend(operator, context.db.backend()?)?;
        }
        let column = match column.into().0 {
            ColumnSource::Name(name) => ColumnOperand::from(name),
            ColumnSource::Raw(sql) => ColumnOperand::from(sql),
            ColumnSource::Query(factory) => {
                if operator.is_none() {
                    return Err(Error::custom(
                        "query_multi_column_operator_required",
                        "已有标量查询列需要明确比较参数",
                    ));
                }
                factory().ready(context)?
            }
            ColumnSource::Callback(callback) => {
                let query = callback(fresh(context, operator.is_none()))?;
                if operator.is_none() {
                    let snapshot = query.snapshot()?;
                    snapshot.validate()?;
                    context.validate_same(&snapshot.context())?;
                    tree = tree.add_group(snapshot.predicates, context.clone(), or, false);
                    continue;
                }
                context.validate_connection(&query.query_context()?)?;
                ColumnOperand::from(query).ready(context)?
            }
        };
        let column = column.ready(context)?;
        let rhs = if let Some(value) = value.as_mut() {
            value.materialize(context)?
        } else {
            WhereValue::from(Value::String(None))
        };
        tree = tree.compare(
            context,
            column,
            operator.unwrap_or(WhereOperator::Eq),
            rhs,
            or,
        )?;
    }
    Ok(tree)
}

macro_rules! multi_methods {
    ($(($name:ident,$mode:ident,$or:literal)),+)=>{$(
        #[doc=concat!("添加多列",stringify!($mode),"条件组；空列不添加条件，保留参数准备错误。")]
        pub fn $name<I,C>(self,columns:I,comparison:impl Into<MultiComparison<'cap>>)->Result<Self>
        where I:IntoIterator<Item=C>,C:Into<MultiColumn<'cap>> {
            self.multi(columns,comparison.into(),Mode::$mode,$or)
        }
    )+};
}
impl<'tx: 'cap, 'cap> QueryBuilder<'tx, 'cap> {
    fn multi<I, C>(
        self,
        columns: I,
        comparison: MultiComparison<'cap>,
        mode: Mode,
        or: bool,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = C>,
        C: Into<MultiColumn<'cap>>,
    {
        self.transform(|mut state| {
            let context: QueryContext<'cap> = state.context();
            let tree = build_group(&context, columns, comparison, mode)?;
            state.predicates =
                state
                    .predicates
                    .add_group(tree, context, or, matches!(mode, Mode::None));
            Ok(state)
        })?;
        Ok(self)
    }
    multi_methods!(
        (where_any, Any, false),
        (where_all, All, false),
        (where_none, None, false),
        (or_where_any, Any, true),
        (or_where_all, All, true),
        (or_where_none, None, true)
    );
}
impl<'tx: 'cap, 'cap> WhereGroup<'tx, 'cap> {
    fn multi<I, C>(
        mut self,
        columns: I,
        comparison: MultiComparison<'cap>,
        mode: Mode,
        or: bool,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = C>,
        C: Into<MultiColumn<'cap>>,
    {
        let context: QueryContext<'cap> = self.context.clone();
        let tree = build_group(&context, columns, comparison, mode)?;
        self.condition = self
            .condition
            .add_group(tree, context, or, matches!(mode, Mode::None));
        Ok(self)
    }
    multi_methods!(
        (where_any, Any, false),
        (where_all, All, false),
        (where_none, None, false),
        (or_where_any, Any, true),
        (or_where_all, All, true),
        (or_where_none, None, true)
    );
}

#[cfg(all(test, feature = "db-sqlite"))]
#[path = "multi_column_tests.rs"]
mod tests;

#[cfg(all(test, any(feature = "db-mysql", feature = "db-postgres")))]
#[path = "multi_column_real_tests.rs"]
mod real_tests;
