//! 单一查询状态、回调快照与不持锁跨 I/O 的操作租约。

use super::{
    Alias, Arc, Asterisk, CURRENT_PAGE_CONTEXT, ColumnOperand, ConnectionTrait,
    DatabaseTransaction, DbBackend, Error, Expr, Func, Future, JoinKind, Mutex, PageContext, Query,
    QueryBuilder, QueryContext, QueryInner, QueryPage, QueryResult, QueryState, RawSql, Result,
    SelectStatement, SimplePage, SortDirection, TryGetable, Value, WhereGroup, empty_values,
    subqueries, validate_identifier,
};
use std::cell::RefCell;
use std::sync::{
    MutexGuard, TryLockError,
    atomic::{AtomicBool, Ordering},
};

static GRAPH_COMMIT: Mutex<()> = Mutex::new(());
static CALLBACK_SLOTS: std::sync::LazyLock<Mutex<std::collections::HashSet<usize>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
thread_local! {
    static EDIT_SCOPES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

struct CallbackSlot(usize);
impl CallbackSlot {
    fn enter(address: usize) -> Result<Self> {
        let mut slots = CALLBACK_SLOTS
            .lock()
            .map_err(|_| subqueries::busy_error())?;
        if !slots.insert(address) {
            return Err(subqueries::busy_error());
        }
        Ok(Self(address))
    }
    fn active(address: usize) -> Result<bool> {
        Ok(CALLBACK_SLOTS
            .lock()
            .map_err(|_| subqueries::busy_error())?
            .contains(&address))
    }
}
impl Drop for CallbackSlot {
    fn drop(&mut self) {
        // 表内仅有整数；清理短锁内无用户对象、回调或 I/O。
        CALLBACK_SLOTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

struct EditScope(usize);
impl EditScope {
    fn enter(identity: usize) -> Self {
        EDIT_SCOPES.with(|scopes| scopes.borrow_mut().push(identity));
        Self(identity)
    }
    fn permits(identity: usize) -> bool {
        EDIT_SCOPES.with(|scopes| scopes.borrow().contains(&identity))
    }
}
impl Drop for EditScope {
    fn drop(&mut self) {
        EDIT_SCOPES.with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            if let Some(index) = scopes.iter().rposition(|identity| *identity == self.0) {
                scopes.remove(index);
            }
        });
    }
}

pub(super) struct OperationLease<'tx: 'cap, 'cap> {
    inner: Arc<QueryInner<'tx, 'cap>>,
}
impl Drop for OperationLease<'_, '_> {
    fn drop(&mut self) {
        self.inner.busy.store(false, Ordering::Release);
    }
}

impl Clone for QueryBuilder<'_, '_> {
    fn clone(&self) -> Self {
        // 此锁内只复制框架状态，不调用用户代码或执行 I/O。
        let state = match self.inner.state.lock() {
            Ok(state) => state.clone(),
            Err(poisoned) => {
                let mut state = poisoned.into_inner().clone();
                state.failure = Some((
                    "query_state_poisoned".to_owned(),
                    "查询状态锁已毒化".to_owned(),
                ));
                state
            }
        };
        Self::from_state(state)
    }
}

impl<'tx: 'cap, 'cap> QueryBuilder<'tx, 'cap> {
    pub(super) fn from_state(state: QueryState<'tx, 'cap>) -> Self {
        Self {
            inner: Arc::new(QueryInner {
                state: Mutex::new(state),
                busy: AtomicBool::new(false),
            }),
        }
    }
    pub(super) fn alias(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
    fn lock_state(&self) -> Result<MutexGuard<'_, QueryState<'tx, 'cap>>> {
        match self.inner.state.try_lock() {
            Ok(state) => Ok(state),
            Err(TryLockError::WouldBlock) => Err(subqueries::busy_error()),
            Err(TryLockError::Poisoned(_)) => {
                Err(Error::custom("query_state_poisoned", "查询状态锁已毒化"))
            }
        }
    }
    pub(super) fn snapshot(&self) -> Result<QueryState<'tx, 'cap>> {
        Ok(self.lock_state()?.clone())
    }
    fn publish(&self, state: QueryState<'tx, 'cap>) -> Result<()> {
        // 图的最终检查和发布串行；旧状态及捕获析构一律移到两个锁之外。
        let retired = {
            let _graph = GRAPH_COMMIT
                .lock()
                .map_err(|_| Error::custom("query_graph_poisoned", "查询图提交锁已毒化"))?;
            self.reject_cycle(&state)?;
            let mut current = self.lock_state()?;
            std::mem::replace(&mut *current, state)
        };
        drop(retired);
        Ok(())
    }
    fn edit_state(
        &self,
        change: impl FnOnce(QueryState<'tx, 'cap>) -> Result<QueryState<'tx, 'cap>>,
    ) -> Result<()> {
        let state = change(self.snapshot()?)?;
        self.publish(state)
    }
    fn write_lease(&self) -> Result<Option<OperationLease<'tx, 'cap>>> {
        if EditScope::permits(self.identity()) {
            Ok(None)
        } else {
            self.lease().map(Some)
        }
    }
    pub(super) fn transform(
        &self,
        change: impl FnOnce(QueryState<'tx, 'cap>) -> Result<QueryState<'tx, 'cap>>,
    ) -> Result<()> {
        let _lease = self.write_lease()?;
        self.edit_state(change)
    }
    fn failed_copy(self, error: &Error) -> Self {
        let mut state = match self.snapshot() {
            Ok(state) => state,
            Err(_) => self
                .inner
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        };
        state.failure = Some((error.code().to_owned(), error.to_string()));
        Self::from_state(state)
    }
    fn remember_failure(&self, error: &Error) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.failure = Some((error.code().to_owned(), error.to_string()));
    }
    pub(super) fn lease(&self) -> Result<OperationLease<'tx, 'cap>> {
        if CallbackSlot::active(std::ptr::from_ref(self) as usize)? {
            return Err(subqueries::busy_error());
        }
        self.inner
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| subqueries::busy_error())?;
        Ok(OperationLease {
            inner: self.inner.clone(),
        })
    }
    pub(super) fn validate_current(&self) -> Result<()> {
        self.snapshot()?.validate()
    }
    pub(super) fn query_context(&self) -> Result<QueryContext<'tx>> {
        Ok(self.snapshot()?.context())
    }

    /// 登记有序同步查询前回调；捕获可以借用当前查询生命期内的资源。
    #[must_use]
    pub fn before_query<F>(self, callback: F) -> Self
    where
        F: for<'q> Fn(&mut QueryBuilder<'q, 'cap>) -> Result<()> + Send + Sync + 'cap,
    {
        if let Err(error) = self.transform(|mut state| {
            state.callbacks.push(Arc::new(callback));
            Ok(state)
        }) {
            return self.failed_copy(&error);
        }
        self
    }

    /// 对当前原对象执行回调快照；全部成功才清空当前列表，失败保留部分修改及列表。
    pub fn apply_before_query_callbacks(&self) -> Result<()> {
        let _lease = self.lease()?;
        self.apply_callbacks()
    }
    pub(super) fn apply_callbacks(&self) -> Result<()> {
        let original = self.snapshot()?;
        original.validate()?;
        let context = original.context();
        for callback in original.callbacks {
            let before = self.snapshot()?;
            let mut alias = self.alias();
            let result = {
                // 槽位在借用回调期间不移动；整体赋值仅替换内容，不能绕过当前准备操作。
                let _slot = CallbackSlot::enter(std::ptr::from_ref(&alias) as usize)?;
                let _edit = EditScope::enter(self.identity());
                callback(&mut alias)
            };
            let mut changed = alias.snapshot()?;
            changed.predicates.inherit(&before.predicates);
            let mut retained = before.callbacks.clone();
            for callback in std::mem::take(&mut changed.callbacks) {
                if !before
                    .callbacks
                    .iter()
                    .any(|old| Arc::ptr_eq(old, &callback))
                {
                    retained.push(callback);
                }
            }
            changed.callbacks = retained;
            if let Err(error) = context.validate_same(&changed.context()) {
                self.remember_failure(&Error::custom(
                    "query_callback_context_mismatch",
                    error.to_string(),
                ));
                return Err(Error::custom(
                    "query_callback_context_mismatch",
                    "查询前回调不能替换数据库、表或实际事务",
                ));
            }
            if before.failure.is_some() {
                changed.failure = before.failure;
            }
            if let Err(error) = self.publish(changed) {
                self.remember_failure(&Error::custom(error.code(), error.to_string()));
                return Err(error);
            }
            result?;
            self.snapshot()?.validate()?;
        }
        self.edit_state(|mut state| {
            state.callbacks = Vec::new();
            Ok(state)
        })
    }

    /// 编译 SQL 并消费当前查询前回调；绑定通过独立 `get_bindings` 读取。
    pub fn to_sql(&self) -> Result<String> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        let state = self.snapshot()?;
        Ok(state.build(&state.scoped_select(false)?)?.0)
    }

    /// 读取现有绑定计划；不执行本查询或子查询的回调，可在回调内调用。
    pub fn get_bindings(&self) -> Result<Vec<Value>> {
        let state = self.snapshot()?;
        Ok(state.build(&state.scoped_select(true)?)?.1.0)
    }
    pub(super) fn bindings_select(&self) -> Result<SelectStatement> {
        self.snapshot()?.scoped_select(true)
    }
    pub(super) fn prepared_select(&self) -> Result<SelectStatement> {
        self.snapshot()?.scoped_select(false)
    }
    pub(super) fn prepare_child(&self, callbacks: bool) -> Result<SelectStatement> {
        let _lease = self.lease()?;
        if callbacks {
            self.apply_callbacks()?;
        }
        self.prepared_select()
    }
    pub(super) fn prepare_scalar(&self) -> Result<(SelectStatement, SelectStatement)> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        let state = self.snapshot()?;
        Ok((state.scoped_select(false)?, state.scoped_select(true)?))
    }
    #[cfg(all(test, feature = "db-sqlite"))]
    pub(super) fn scoped_select(&self) -> Result<SelectStatement> {
        self.prepared_select()
    }
    #[cfg(all(test, feature = "db-sqlite"))]
    pub(super) fn statement(&self, select: &SelectStatement) -> Result<super::Statement> {
        self.snapshot()?.statement(select)
    }

    // 持久化生命周期从执行查询取得事务，避免 SQL 与事件使用不同连接。
    pub(crate) fn transaction_ref(&self) -> Result<Option<&'tx DatabaseTransaction>> {
        Ok(self.snapshot()?.transaction)
    }

    pub(crate) fn bind_model_date(&self, date: &crate::support::InputDate) -> Result<Value> {
        let db = &self.snapshot()?.db;
        if db.backend()? != DbBackend::Postgres {
            return date.format(db.query_date_format()?).map(Value::from);
        }
        // PostgreSQL 的驱动会把字符串声明为 TEXT。用无时区时间参数保留
        // grammar 的本地墙钟值，目标为 timestamptz 时仍由会话时区解释。
        let out_of_range = || {
            Error::custom(
                "model_postgres_date_binding_out_of_range",
                "模型日期超出 PostgreSQL 驱动的时间戳绑定范围",
            )
        };
        let datetime = date.try_chrono().map_err(|_| out_of_range())?;
        let local = datetime
            .naive_utc()
            .checked_add_offset(*datetime.offset())
            .ok_or_else(out_of_range)?;
        let whole_second = chrono::Timelike::with_nanosecond(&local, 0).ok_or_else(out_of_range)?;
        Ok(whole_second.into())
    }

    /// 借用事务并创建独立查询句柄；生命周期只能缩短，已接入父查询仍保留原句柄。
    pub fn using_transaction<'t>(
        self,
        transaction: &'t DatabaseTransaction,
    ) -> QueryBuilder<'t, 'cap>
    where
        'tx: 't,
        't: 'cap,
    {
        let permission = self.write_lease();
        let mut state = match self.snapshot() {
            Ok(state) => state,
            Err(error) => {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                state.failure = Some((error.code().to_owned(), error.to_string()));
                state
            }
        };
        let _permission = match permission {
            Ok(permission) => permission,
            Err(error) => {
                state.failure = Some((error.code().to_owned(), error.to_string()));
                None
            }
        };
        state
            .predicates
            .rebind(state.transaction, Some(transaction));
        QueryBuilder::from_state(QueryState {
            transaction: Some(transaction),
            db: state.db,
            table: state.table,
            select: state.select,
            has_joins: state.has_joins,
            predicates: state.predicates,
            callbacks: state.callbacks,
            unions: state.unions,
            projection: state.projection,
            limit: state.limit,
            offset: state.offset,
            orders: state.orders,
            union_orders: state.union_orders,
            union_limit: state.union_limit,
            union_offset: state.union_offset,
            grouped: state.grouped,
            having: state.having,
            failure: state.failure,
        })
    }

    async fn get_columns(&self, columns: Option<&[&str]>, first: bool) -> Result<Vec<QueryResult>> {
        let _lease = self.lease()?;
        self.get_columns_leased(columns, first).await
    }
    async fn get_columns_leased(
        &self,
        columns: Option<&[&str]>,
        first: bool,
    ) -> Result<Vec<QueryResult>> {
        let original = self.snapshot()?.projection;
        self.edit_state(|mut state| {
            if first {
                state = state.limit(1);
            }
            if state.projection.is_none()
                && let Some(columns) = columns
            {
                state = state.select(columns)?;
            }
            Ok(state)
        })?;
        self.apply_callbacks()?;
        let rows = self.snapshot()?.get().await?;
        self.edit_state(|mut state| {
            state.restore_projection(original);
            Ok(state)
        })?;
        Ok(rows)
    }

    /// 读取全部匹配行；成功后恢复执行前投影，失败与取消保留已发生的查询修改。
    pub async fn get(&self) -> Result<Vec<QueryResult>> {
        self.get_columns(None, false).await
    }
    /// 在当前原查询保留 limit 1，读取首行。
    pub async fn first(&self) -> Result<Option<QueryResult>> {
        Ok(self.get_columns(None, true).await?.into_iter().next())
    }
    /// 读取指定列首个值；当前原查询保留 limit 1。
    pub async fn value<T: TryGetable>(&self, column: &str) -> Result<Option<T>> {
        validate_identifier(column)?;
        self.get_columns(Some(&[column]), true)
            .await?
            .into_iter()
            .next()
            .map(|row| row.try_get_by_index(0).map_err(Error::internal))
            .transpose()
    }
    /// 读取指定列所有值；仅在缺省投影时临时选择该列。
    pub async fn pluck<T: TryGetable>(&self, column: &str) -> Result<Vec<T>> {
        validate_identifier(column)?;
        self.get_columns(Some(&[column]), false)
            .await?
            .into_iter()
            .map(|row| row.try_get("", column).map_err(Error::internal))
            .collect()
    }
    /// 在父状态克隆上执行聚合回调；原对象的回调列表不被消费。
    pub async fn count(&self) -> Result<u64> {
        let _source_lease = self.lease()?;
        let cloned = self.clone();
        let _lease = cloned.lease()?;
        cloned.edit_state(|mut state| {
            if !state.grouped {
                state.clear_orders();
            }
            Ok(state)
        })?;
        cloned.apply_callbacks()?;
        cloned.snapshot()?.count().await
    }
    /// 插入一行；执行前应用同一原查询的回调。
    pub async fn insert(&self, values: Vec<(&str, Value)>) -> Result<u64> {
        if values.is_empty() {
            return Err(empty_values());
        }
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        self.snapshot()?.insert(values).await
    }
    /// 插入一行并返回本次执行生成的 ID；空值列表插入默认行。
    ///
    /// PostgreSQL 的 sequence 指定 RETURNING 列（空值、空串及 0 默认 id）；
    /// MySQL/SQLite 忽略 sequence，并从本次执行结果读取原生自增 ID。
    /// 数值按 PHP 64 位整数规则转换，其余值保持原样。
    /// 浮点转换 warning 由默认处理器返回为错误；此前的写入不会隐式回滚。
    /// 使用同一原查询回调、主连接或已有事务，不另行查询连接级 last ID。
    pub async fn insert_get_id(
        &self,
        values: Vec<(&str, Value)>,
        sequence: Option<&str>,
    ) -> Result<Value> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        self.snapshot()?.insert_get_id(values, sequence).await
    }
    /// 按唯一键插入或更新；执行前应用同一原查询的回调。
    pub async fn upsert(
        &self,
        rows: Vec<Vec<(&str, Value)>>,
        unique_by: &[&str],
        update_columns: &[&str],
    ) -> Result<u64> {
        QueryState::validate_upsert(&rows, unique_by, update_columns)?;
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        self.snapshot()?
            .upsert(rows, unique_by, update_columns)
            .await
    }
    /// 更新匹配行；保留附着时的子查询绑定计划。
    pub async fn update(&self, values: Vec<(&str, Value)>) -> Result<u64> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        self.snapshot()?.update(values).await
    }
    /// 删除匹配行；保留附着时的子查询绑定计划。
    pub async fn delete(&self) -> Result<u64> {
        let _lease = self.lease()?;
        self.apply_callbacks()?;
        self.snapshot()?.delete().await
    }
}

impl<'tx: 'cap, 'cap> QueryBuilder<'tx, 'cap> {
    /// 只选择指定受信列。
    pub fn select(self, columns: &[&str]) -> Result<Self> {
        self.transform(|state| state.select(columns))?;
        Ok(self)
    }
    /// 返回去重结果。
    #[must_use]
    pub fn distinct(self) -> Self {
        if let Err(error) = self.transform(|state| Ok(state.distinct())) {
            return self.failed_copy(&error);
        }
        self
    }
    /// 添加参数绑定的相等条件。
    pub fn where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.transform(|state| state.where_eq(column, value))?;
        Ok(self)
    }
    /// 添加大于条件。
    pub fn where_gt(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.transform(|state| state.where_gt(column, value))?;
        Ok(self)
    }
    /// 添加小于条件。
    pub fn where_lt(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.transform(|state| state.where_lt(column, value))?;
        Ok(self)
    }
    /// 添加绑定值的LIKE条件。
    pub fn where_like(self, column: &str, pattern: impl Into<String>) -> Result<Self> {
        self.transform(|state| state.where_like(column, pattern))?;
        Ok(self)
    }
    /// 添加NULL条件。
    pub fn where_null(self, column: &str) -> Result<Self> {
        self.transform(|state| state.where_null(column))?;
        Ok(self)
    }
    /// 添加非NULL条件。
    pub fn where_not_null(self, column: &str) -> Result<Self> {
        self.transform(|state| state.where_not_null(column))?;
        Ok(self)
    }
    /// 添加绑定值闭区间条件。
    pub fn where_between(
        self,
        column: &str,
        lower: impl Into<Value>,
        upper: impl Into<Value>,
    ) -> Result<Self> {
        self.transform(|state| state.where_between(column, lower, upper))?;
        Ok(self)
    }
    /// 添加OR相等条件，保留AND优先级。
    pub fn or_where_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.transform(|state| state.or_where_eq(column, value))?;
        Ok(self)
    }
    /// 添加受信 SQL 条件，参数按顺序绑定到代码区中的 `?`。
    ///
    /// 占位符规则与 [`Self::select_raw`] 相同，包括 PostgreSQL 的 `??` 操作符转义。
    /// 参数数目或模板无效时，返回的查询在读取绑定、编译或执行时报告错误。
    #[must_use]
    pub fn where_raw(self, sql: RawSql, bindings: Vec<Value>) -> Self {
        if let Err(error) = self.transform(|state| state.where_raw(sql, bindings)) {
            return self.failed_copy(&error);
        }
        self
    }
    /// 按受信列排序。
    pub fn order_by(self, column: &str, direction: SortDirection) -> Result<Self> {
        self.transform(|state| state.order_by(column, direction))?;
        Ok(self)
    }
    /// 按受信列分组。
    pub fn group_by(self, column: &str) -> Result<Self> {
        self.transform(|state| state.group_by(column))?;
        Ok(self)
    }
    /// 通过逻辑表名和受信列建立连接。
    pub fn join(
        self,
        logical_table: &str,
        own_column: &str,
        foreign_column: &str,
        kind: JoinKind,
    ) -> Result<Self> {
        self.transform(|state| state.join(logical_table, own_column, foreign_column, kind))?;
        Ok(self)
    }
    /// 在显式事务中获取排他行锁。
    pub fn lock_for_update(self) -> Result<Self> {
        self.transform(QueryState::lock_for_update)?;
        Ok(self)
    }
    /// 添加聚合后相等条件。
    pub fn having_eq(self, column: &str, value: impl Into<Value>) -> Result<Self> {
        self.transform(|state| state.having_eq(column, value))?;
        Ok(self)
    }
    /// 限制返回行数。
    #[must_use]
    pub fn limit(self, rows: u64) -> Self {
        if let Err(error) = self.transform(|state| Ok(state.limit(rows))) {
            return self.failed_copy(&error);
        }
        self
    }
    /// 设置结果偏移。
    #[must_use]
    pub fn offset(self, rows: u64) -> Self {
        if let Err(error) = self.transform(|state| Ok(state.offset(rows))) {
            return self.failed_copy(&error);
        }
        self
    }
    /// 接入同一原对象的实时EXISTS子查询，绑定在接入时捕获。
    pub fn where_exists<'src, 'childcap>(
        self,
        subquery: &QueryBuilder<'src, 'childcap>,
    ) -> Result<Self>
    where
        'src: 'childcap + 'cap,
        'childcap: 'cap,
    {
        self.transform(|state| state.where_exists(subquery))?;
        Ok(self)
    }
    /// 接入实时UNION子查询，编译时消费子回调。
    pub fn union<'src, 'childcap>(
        self,
        other: &QueryBuilder<'src, 'childcap>,
        all: bool,
    ) -> Result<Self>
    where
        'src: 'childcap + 'cap,
        'childcap: 'cap,
    {
        if CallbackSlot::active(std::ptr::from_ref(other) as usize)? {
            return Err(subqueries::busy_error());
        }
        self.transform(|state| state.union(other, all))?;
        Ok(self)
    }
    /// 添加参数绑定的集合条件。
    pub fn where_in<V: Into<Value>, I: IntoIterator<Item = V>>(
        self,
        column: &str,
        values: I,
    ) -> Result<Self> {
        self.transform(|state| state.where_in(column, values))?;
        Ok(self)
    }
    /// 添加参数绑定的非集合条件。
    pub fn where_not_in<V: Into<Value>, I: IntoIterator<Item = V>>(
        self,
        column: &str,
        values: I,
    ) -> Result<Self> {
        self.transform(|state| state.where_not_in(column, values))?;
        Ok(self)
    }
    /// 以闭包构造显式括号组；返回组须保留同一来源上下文。
    pub fn where_nested<F>(self, configure: F) -> Result<Self>
    where
        F: FnOnce(WhereGroup<'tx, 'cap>) -> Result<WhereGroup<'tx, 'cap>>,
    {
        self.transform(|state| state.where_nested(configure))?;
        Ok(self)
    }
    /// 添加列间区间条件；标量子查询在接入时准备，边界不作为值绑定。
    pub fn where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        self.transform(|state| state.where_between_columns(column, bounds))?;
        Ok(self)
    }
    /// 添加列间区间条件；标量子查询在接入时准备，边界不作为值绑定。
    pub fn where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        self.transform(|state| state.where_not_between_columns(column, bounds))?;
        Ok(self)
    }
    /// 添加列间区间条件；标量子查询在接入时准备，边界不作为值绑定。
    pub fn or_where_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        self.transform(|state| state.or_where_between_columns(column, bounds))?;
        Ok(self)
    }
    /// 添加列间区间条件；标量子查询在接入时准备，边界不作为值绑定。
    pub fn or_where_not_between_columns<
        'operand,
        C: Into<ColumnOperand<'operand>>,
        B: Into<ColumnOperand<'operand>>,
    >(
        self,
        column: C,
        bounds: [B; 2],
    ) -> Result<Self>
    where
        'operand: 'cap,
    {
        self.transform(|state| state.or_where_not_between_columns(column, bounds))?;
        Ok(self)
    }
}

impl QueryBuilder<'_, '_> {
    /// 默认每页15行，页号由当前请求解析。
    pub async fn paginate_default(&self) -> Result<QueryPage> {
        self.paginate(15).await
    }
    /// 按当前请求page参数分页；总数为零不读取原查询或消费其回调。
    pub async fn paginate(&self, per_page: u64) -> Result<QueryPage> {
        self.paginate_named(per_page, "page").await
    }
    /// 使用独立页参数名分页。
    pub async fn paginate_named(&self, per_page: u64, page_name: &str) -> Result<QueryPage> {
        validate_identifier(page_name)?;
        let context = CURRENT_PAGE_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default();
        self.paginate_at(
            per_page,
            page_name,
            context.current_page(page_name),
            context,
        )
        .await
    }
    /// 显式指定页号并保留当前请求链接上下文。
    pub async fn paginate_on(
        &self,
        per_page: u64,
        page_name: &str,
        page: u64,
    ) -> Result<QueryPage> {
        validate_identifier(page_name)?;
        let context = CURRENT_PAGE_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default();
        self.paginate_at(per_page, page_name, std::cmp::Ord::max(page, 1), context)
            .await
    }
    async fn pagination_count(&self) -> Result<u64> {
        let cloned = self.clone();
        cloned.edit_state(|mut state| {
            if state.grouped || state.having || state.unions.is_empty() {
                state.clear_orders();
                state.select.reset_limit().reset_offset();
                state.limit = None;
                state.offset = 0;
            }
            state.union_orders.clear();
            state.union_limit = None;
            state.union_offset = None;
            Ok(state)
        })?;
        let _lease = cloned.lease()?;
        cloned.apply_callbacks()?;
        let state = cloned.snapshot()?;
        if state.grouped || state.having {
            let wrap = |bindings: bool| -> Result<SelectStatement> {
                Ok(Query::select()
                    .expr_as(Func::count(Expr::col(Asterisk)), Alias::new("total"))
                    .from_subquery(
                        state.scoped_select(bindings)?,
                        Alias::new("aggregate_table"),
                    )
                    .to_owned())
            };
            let statement = state.statement_pair(&wrap(false)?, &wrap(true)?)?;
            let row = match state.transaction {
                Some(tx) => tx.query_one_raw(statement).await,
                None => state.db.read_connection()?.query_one_raw(statement).await,
            }
            .map_err(Error::internal)?;
            let Some(row) = row else { return Ok(0) };
            u64::try_from(row.try_get::<i64>("", "total").map_err(Error::internal)?)
                .map_err(Error::internal)
        } else {
            state.count().await
        }
    }
    async fn paginate_at(
        &self,
        per_page: u64,
        page_name: &str,
        page: u64,
        context: PageContext,
    ) -> Result<QueryPage> {
        let _lease = self.lease()?;
        if per_page == 0 {
            return Err(Error::custom(
                "query_per_page_invalid",
                "per page must be positive",
            ));
        }
        let offset = page
            .saturating_sub(1)
            .checked_mul(per_page)
            .ok_or_else(|| {
                Error::custom("query_page_overflow", "page offset exceeds supported range")
            })?;
        let total = self.pagination_count().await?;
        let items = if total == 0 {
            Vec::new()
        } else {
            self.edit_state(|state| Ok(state.limit(per_page).offset(offset)))?;
            self.get_columns_leased(None, false).await?
        };
        Ok(QueryPage {
            items,
            current_page: page,
            per_page,
            total,
            last_page: std::cmp::Ord::max(total.div_ceil(per_page), 1),
            path: context.resolved_path()?,
            query: Vec::new(),
            page_name: page_name.to_owned(),
            fragment: None,
        })
    }
    /// 不执行总数查询，每页多读一行判断下一页。
    pub async fn simple_paginate(&self, per_page: u64) -> Result<SimplePage> {
        self.simple_paginate_named(per_page, "page").await
    }
    /// 以独立页参数进行无总数查询的分页。
    pub async fn simple_paginate_named(
        &self,
        per_page: u64,
        page_name: &str,
    ) -> Result<SimplePage> {
        let _lease = self.lease()?;
        validate_identifier(page_name)?;
        if per_page == 0 {
            return Err(Error::custom(
                "query_per_page_invalid",
                "per page must be positive",
            ));
        }
        let context = CURRENT_PAGE_CONTEXT
            .try_with(Clone::clone)
            .unwrap_or_default();
        let page = context.current_page(page_name);
        let offset = page
            .saturating_sub(1)
            .checked_mul(per_page)
            .ok_or_else(|| {
                Error::custom("query_page_overflow", "page offset exceeds supported range")
            })?;
        let fetch = per_page.checked_add(1).ok_or_else(|| {
            Error::custom(
                "query_per_page_overflow",
                "per page exceeds supported range",
            )
        })?;
        self.edit_state(|state| Ok(state.limit(fetch).offset(offset)))?;
        let mut items = self.get_columns_leased(None, false).await?;
        let has_more = items.len() as u64 > per_page;
        if has_more {
            items.pop();
        }
        Ok(SimplePage {
            items,
            current_page: page,
            per_page,
            has_more,
            path: context.resolved_path()?,
            query: Vec::new(),
            page_name: page_name.to_owned(),
            fragment: None,
        })
    }
    /// 按唯一递增列逐页克隆读取；尊重总limit、首块offset与零limit零查询。
    pub async fn chunk_by_id<F, Fut>(
        &self,
        size: u64,
        id_column: &str,
        mut callback: F,
    ) -> Result<()>
    where
        F: FnMut(Vec<QueryResult>) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let _source_lease = self.lease()?;
        validate_identifier(id_column)?;
        if size == 0 {
            return Err(Error::custom(
                "query_chunk_size_invalid",
                "chunk size must be positive",
            ));
        }
        let initial = self.snapshot()?;
        let mut remaining = initial.union_limit.or(initial.limit);
        let mut last = None;
        let mut first = true;
        loop {
            let limit = remaining.map_or(size, |remaining| std::cmp::Ord::min(remaining, size));
            if limit == 0 {
                return Ok(());
            }
            let page = self.clone();
            page.transform(|mut state| {
                state.remove_orders_for(id_column);
                if !first && initial.union_offset.unwrap_or(initial.offset) > 0 {
                    state = state.offset(0);
                }
                state = state.order_by(id_column, SortDirection::Asc)?.limit(limit);
                if let Some(id) = last {
                    state = state.where_gt(id_column, id)?;
                }
                Ok(state)
            })?;
            let rows = page.get().await?;
            let count = rows.len() as u64;
            if count == 0 {
                return Ok(());
            }
            let next = rows
                .last()
                .ok_or_else(|| Error::custom("query_chunk_empty", "chunk unexpectedly empty"))?
                .try_get::<i64>("", id_column)
                .map_err(Error::internal)?;
            if last.is_some_and(|previous| next <= previous) {
                return Err(Error::custom(
                    "query_chunk_not_advancing",
                    "chunk key must be unique and increasing",
                ));
            }
            callback(rows).await?;
            if let Some(left) = &mut remaining {
                *left = left.saturating_sub(count);
            }
            last = Some(next);
            first = false;
            if count < size || remaining == Some(0) {
                return Ok(());
            }
        }
    }
}

impl QueryBuilder<'_, '_> {
    pub(super) fn identity(&self) -> usize {
        Arc::as_ptr(&self.inner) as usize
    }
    pub(super) fn reaches(
        &self,
        target: usize,
        visited: &mut std::collections::HashSet<usize>,
    ) -> Result<bool> {
        if self.identity() == target {
            return Ok(true);
        }
        if !visited.insert(self.identity()) {
            return Ok(false);
        }
        // 图锁内只复制边，避免并发消费回调时在此释放最后一个用户捕获物。
        let (predicates, unions) = {
            let state = self.lock_state()?;
            (state.predicates.clone(), state.unions.clone())
        };
        if predicates.reaches(target, visited)? {
            return Ok(true);
        }
        for union in unions {
            if union.child.reaches(target, visited)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn reject_cycle(&self, state: &QueryState<'_, '_>) -> Result<()> {
        if state.reaches(self.identity(), &mut std::collections::HashSet::new())? {
            Err(Error::custom(
                "query_subquery_cycle",
                "子查询不能引用自身或其祖先查询",
            ))
        } else {
            Ok(())
        }
    }
}
