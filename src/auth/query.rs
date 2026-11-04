//! 资源查询范围与安全组合入口。

use std::{marker::PhantomData, sync::Arc};

use crate::{
    auth::CurrentUser,
    core::{Error, Result},
    db::BatchReader,
    db::sea_orm::{
        self, ColumnTrait, Condition, ConnectionTrait, EntityTrait, Iterable, ModelTrait,
        PaginatorTrait, PrimaryKeyToColumn, QueryFilter, QueryOrder, QuerySelect, Related, Select,
        sea_query::SimpleExpr,
    },
    tenant::{TenantContext, TenantId},
};

/// 分页查询允许的最大页大小。
pub const MAX_AUTHORIZED_PAGE_SIZE: u64 = 500;

type RuleFn<E> = dyn Fn(&CurrentUser) -> Result<QueryScope<E>> + Send + Sync;

/// 为一个 SeaORM 实体定义按用户计算的资源行范围。
///
/// 规则只描述行范围，不授予 `Policy::authorize` 所检查的操作权限。
pub struct QueryRule<E: EntityTrait> {
    build: Arc<RuleFn<E>>,
    entity: PhantomData<fn() -> E>,
}

impl<E: EntityTrait> Clone for QueryRule<E> {
    fn clone(&self) -> Self {
        Self {
            build: Arc::clone(&self.build),
            entity: PhantomData,
        }
    }
}

impl<E: EntityTrait> QueryRule<E> {
    /// 创建一个同步、无 I/O 的资源范围规则。
    pub fn new(
        build: impl Fn(&CurrentUser) -> Result<QueryScope<E>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            build: Arc::new(build),
            entity: PhantomData,
        }
    }

    /// 将此资源规则与必需的当前租户列绑定。
    pub fn for_tenant(self, tenant_column: E::Column) -> TenantQueryRule<E> {
        TenantQueryRule {
            rule: self,
            tenant_column,
        }
    }

    /// 创建受保护的读取入口；用户筛选始终与授权范围做 AND。
    pub fn select(
        &self,
        user: &CurrentUser,
        user_filter: Condition,
    ) -> Result<AuthorizedSelect<E>> {
        let scope = self.scope(user)?;
        let condition = Condition::all().add(scope).add(user_filter);
        Ok(AuthorizedSelect {
            query: E::find().filter(condition),
        })
    }

    /// 创建受保护的关联读取入口，并同时应用当前实体与关联实体的规则。
    pub async fn related<M, R>(
        &self,
        user: &CurrentUser,
        model: &M,
        related_rule: &QueryRule<R>,
        user_filter: Condition,
        connection: &impl ConnectionTrait,
    ) -> Result<AuthorizedSelect<R>>
    where
        M: ModelTrait<Entity = E>,
        E: Related<R>,
        R: EntityTrait,
    {
        let parent_scope = self.scope(user)?;
        let mut parent_condition = Condition::all().add(parent_scope);
        let mut has_primary_key = false;
        for key in E::PrimaryKey::iter() {
            has_primary_key = true;
            parent_condition =
                parent_condition.add(key.into_column().eq(model.get(key.into_column())));
        }
        if !has_primary_key {
            return Err(forbidden_scope());
        }
        let parent = E::find()
            .filter(parent_condition)
            .one(connection)
            .await
            .map_err(Error::internal)?;
        let Some(parent) = parent else {
            return Err(forbidden_scope());
        };
        let child_scope = related_rule.scope(user)?;
        let query = parent
            .find_related(R::default())
            .filter(Condition::all().add(child_scope).add(user_filter));
        Ok(AuthorizedSelect { query })
    }

    /// 在实际批量 UPDATE 中应用授权范围和业务条件。
    pub async fn update_many<C>(
        &self,
        user: &CurrentUser,
        values: E::ActiveModel,
        user_filter: Condition,
        connection: &C,
    ) -> Result<sea_orm::UpdateResult>
    where
        C: ConnectionTrait,
        E::ActiveModel: Send,
    {
        let scope = self.scope(user)?;
        E::update_many()
            .set(values)
            .filter(Condition::all().add(scope).add(user_filter))
            .exec(connection)
            .await
            .map_err(Error::internal)
    }

    /// 在实际批量 DELETE 中应用授权范围和业务条件。
    pub async fn delete_many<C>(
        &self,
        user: &CurrentUser,
        user_filter: Condition,
        connection: &C,
    ) -> Result<sea_orm::DeleteResult>
    where
        C: ConnectionTrait,
    {
        let scope = self.scope(user)?;
        E::delete_many()
            .filter(Condition::all().add(scope).add(user_filter))
            .exec(connection)
            .await
            .map_err(Error::internal)
    }

    fn scope(&self, user: &CurrentUser) -> Result<Condition> {
        (self.build)(user)
            .map(QueryScope::into_condition)
            .map_err(|_| forbidden_scope())
    }
}

/// 将当前租户条件与资源授权规则合并的查询入口。
pub struct TenantQueryRule<E: EntityTrait> {
    rule: QueryRule<E>,
    tenant_column: E::Column,
}

impl<E: EntityTrait> TenantQueryRule<E> {
    fn scope(&self, user: &CurrentUser, tenant: &TenantId) -> Result<QueryScope<E>> {
        let scope = self.rule.scope(user)?;
        Ok(QueryScope {
            condition: Condition::all()
                .add(scope)
                .add(self.tenant_column.eq(tenant.as_str())),
            entity: PhantomData,
        })
    }

    /// 创建保留当前租户与用户授权范围的查询；缺少租户上下文时拒绝。
    pub fn select(
        &self,
        user: &CurrentUser,
        user_filter: Condition,
    ) -> Result<AuthorizedSelect<E>> {
        let tenant = TenantContext::require_current()?;
        let scope = self.scope(user, &tenant)?;
        Ok(AuthorizedSelect {
            query: E::find().filter(
                Condition::all()
                    .add(scope.into_condition())
                    .add(user_filter),
            ),
        })
    }

    /// 读取同时满足父子资源授权范围和同一租户条件的关联记录。
    pub async fn related<M, R>(
        &self,
        user: &CurrentUser,
        model: &M,
        related_rule: &TenantQueryRule<R>,
        user_filter: Condition,
        connection: &impl ConnectionTrait,
    ) -> Result<AuthorizedSelect<R>>
    where
        M: ModelTrait<Entity = E>,
        E: Related<R>,
        R: EntityTrait,
    {
        let tenant = TenantContext::require_current()?;
        let parent_scope = self.scope(user, &tenant)?;
        let mut parent_condition = Condition::all().add(parent_scope.into_condition());
        let mut has_primary_key = false;
        for key in E::PrimaryKey::iter() {
            has_primary_key = true;
            parent_condition =
                parent_condition.add(key.into_column().eq(model.get(key.into_column())));
        }
        if !has_primary_key {
            return Err(forbidden_scope());
        }
        let parent = E::find()
            .filter(parent_condition)
            .one(connection)
            .await
            .map_err(Error::internal)?;
        let Some(parent) = parent else {
            return Err(forbidden_scope());
        };
        let child_scope = related_rule.scope(user, &tenant)?;
        let query = parent.find_related(R::default()).filter(
            Condition::all()
                .add(child_scope.into_condition())
                .add(user_filter),
        );
        Ok(AuthorizedSelect { query })
    }

    /// 在实际批量 UPDATE 中保留当前租户与授权范围。
    pub async fn update_many<C>(
        &self,
        user: &CurrentUser,
        values: E::ActiveModel,
        user_filter: Condition,
        connection: &C,
    ) -> Result<sea_orm::UpdateResult>
    where
        C: ConnectionTrait,
        E::ActiveModel: Send,
    {
        let tenant = TenantContext::require_current()?;
        let scope = self.scope(user, &tenant)?;
        E::update_many()
            .set(values)
            .filter(
                Condition::all()
                    .add(scope.into_condition())
                    .add(user_filter),
            )
            .exec(connection)
            .await
            .map_err(Error::internal)
    }

    /// 在实际批量 DELETE 中保留当前租户与授权范围。
    pub async fn delete_many<C>(
        &self,
        user: &CurrentUser,
        user_filter: Condition,
        connection: &C,
    ) -> Result<sea_orm::DeleteResult>
    where
        C: ConnectionTrait,
    {
        let tenant = TenantContext::require_current()?;
        let scope = self.scope(user, &tenant)?;
        E::delete_many()
            .filter(
                Condition::all()
                    .add(scope.into_condition())
                    .add(user_filter),
            )
            .exec(connection)
            .await
            .map_err(Error::internal)
    }
}

/// 必须从一个条件表达式开始构造的资源范围。
pub struct QueryScope<E: EntityTrait> {
    condition: Condition,
    entity: PhantomData<fn() -> E>,
}

impl<E: EntityTrait> QueryScope<E> {
    /// 以一个必填条件表达式创建资源范围。
    #[must_use]
    pub fn new(first: SimpleExpr) -> Self {
        Self {
            condition: Condition::all().add(first),
            entity: PhantomData,
        }
    }

    /// 继续添加 AND 条件。
    #[must_use]
    pub fn and(mut self, condition: SimpleExpr) -> Self {
        self.condition = self.condition.add(condition);
        self
    }

    /// 添加一个非空的 OR 条件组，再与既有范围做 AND。
    pub fn and_any(mut self, choices: impl IntoIterator<Item = SimpleExpr>) -> Result<Self> {
        let mut alternatives = Condition::any();
        let mut count = 0;
        for choice in choices {
            alternatives = alternatives.add(choice);
            count += 1;
        }
        if count == 0 {
            return Err(forbidden_scope());
        }
        self.condition = self.condition.add(alternatives);
        Ok(self)
    }

    fn into_condition(self) -> Condition {
        self.condition
    }
}

/// 只允许继续添加 AND 筛选条件的受保护 SELECT。
pub struct AuthorizedSelect<E: EntityTrait> {
    query: Select<E>,
}

impl<E: EntityTrait> AuthorizedSelect<E> {
    /// 将受保护查询转换为逐页始终保留授权范围的主键分批读取。
    #[must_use]
    pub fn into_batches(self) -> BatchReader<E> {
        BatchReader::new(self.query)
    }

    /// 添加额外业务条件；该条件与授权范围通过 AND 组合。
    #[must_use]
    pub fn and_filter(mut self, condition: Condition) -> Self {
        self.query = self.query.filter(condition);
        self
    }

    /// 按实体列升序排列。
    #[must_use]
    pub fn order_by_asc(mut self, column: E::Column) -> Self {
        self.query = self.query.order_by_asc(column);
        self
    }

    /// 按实体列降序排列。
    #[must_use]
    pub fn order_by_desc(mut self, column: E::Column) -> Self {
        self.query = self.query.order_by_desc(column);
        self
    }

    /// 限制最多返回的行数。
    #[must_use]
    pub fn limit(mut self, limit: u64) -> Self {
        self.query = self.query.limit(limit);
        self
    }

    /// 跳过指定数量的行。
    #[must_use]
    pub fn offset(mut self, offset: u64) -> Self {
        self.query = self.query.offset(offset);
        self
    }

    /// 执行受保护列表查询。
    pub async fn all<C>(self, connection: &C) -> Result<Vec<E::Model>>
    where
        C: ConnectionTrait,
        E::Model: Send + Sync,
    {
        self.query.all(connection).await.map_err(Error::internal)
    }

    /// 执行受保护详情查询。
    pub async fn one<C>(self, connection: &C) -> Result<Option<E::Model>>
    where
        C: ConnectionTrait,
        E::Model: Send + Sync,
    {
        self.query.one(connection).await.map_err(Error::internal)
    }

    /// 按 0 起始页码分页，返回授权范围内的记录和总数。
    pub async fn page<C>(
        self,
        connection: &C,
        page: u64,
        page_size: u64,
    ) -> Result<(Vec<E::Model>, u64)>
    where
        C: ConnectionTrait,
        E::Model: Send + Sync,
    {
        validate_page(page, page_size)?;
        let paginator = self.query.paginate(connection, page_size);
        let total = paginator.num_items().await.map_err(Error::internal)?;
        let rows = paginator.fetch_page(page).await.map_err(Error::internal)?;
        Ok((rows, total))
    }
}

fn validate_page(page: u64, page_size: u64) -> Result<()> {
    if page_size == 0 {
        return Err(Error::BadRequest("page size must be positive".into()));
    }
    if page_size > MAX_AUTHORIZED_PAGE_SIZE {
        return Err(Error::BadRequest(format!(
            "page size must not exceed {MAX_AUTHORIZED_PAGE_SIZE}"
        )));
    }
    if page.checked_mul(page_size).is_none() {
        return Err(Error::BadRequest(
            "page offset exceeds the supported range".into(),
        ));
    }
    Ok(())
}

fn forbidden_scope() -> Error {
    Error::Forbidden("resource query is not authorized".into())
}

#[cfg(test)]
mod tests {
    use crate::db::sea_orm::{DbBackend, QueryTrait};

    use super::*;

    mod record {
        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "query_records")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i64,
            pub owner_id: i64,
            pub title: String,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    fn user() -> CurrentUser {
        CurrentUser {
            id: 7.into(),
            guard: None,
            provider: None,
            source: crate::auth::AuthSource::Jwt,
            token_abilities: None,
        }
    }

    #[test]
    fn nested_or_user_filters_remain_inside_authorized_and_scope() {
        let rule = QueryRule::<record::Entity>::new(|user| {
            Ok(QueryScope::new(
                record::Column::OwnerId.eq(user.integer_id()?),
            ))
        });
        let user_filter = Condition::any()
            .add(record::Column::Id.eq(10))
            .add(record::Column::Id.eq(11));
        let selected = rule.select(&user(), user_filter);
        assert!(selected.is_ok());
        if let Ok(query) = selected {
            let query = query.and_filter(
                Condition::any()
                    .add(record::Column::Title.contains("report"))
                    .add(record::Column::Title.contains("draft")),
            );
            let sql = query.query.build(DbBackend::Postgres).to_string();
            assert!(sql.contains("\"owner_id\" = 7 AND"), "{sql}");
            assert!(sql.matches(" OR ").count() >= 2, "{sql}");
            assert!(sql.contains(" AND "), "{sql}");
        }
    }

    #[test]
    fn empty_and_failed_rules_fail_closed() {
        let scope = QueryScope::<record::Entity>::new(record::Column::OwnerId.eq(7));
        assert!(matches!(
            scope.and_any(Vec::<SimpleExpr>::new()),
            Err(Error::Forbidden(_))
        ));

        let failed = QueryRule::<record::Entity>::new(|_| {
            Err(Error::Internal(
                std::io::Error::other("private rule detail").into(),
            ))
        });
        assert!(
            matches!(failed.select(&user(), Condition::all()), Err(Error::Forbidden(message)) if message == "resource query is not authorized")
        );
    }

    #[test]
    fn pagination_rejects_unbounded_and_overflowing_requests() {
        assert!(validate_page(0, 0).is_err());
        assert!(validate_page(0, MAX_AUTHORIZED_PAGE_SIZE + 1).is_err());
        assert!(validate_page(u64::MAX, 2).is_err());
        assert!(validate_page(1, MAX_AUTHORIZED_PAGE_SIZE).is_ok());
    }
}
