# 可选租户隔离

`openrust::tenant` 提供经过校验的 `TenantId`、异步 `TenantContext` 和 HTTP `TenantLayer`。普通应用默认不启用租户隔离，也不需要增加数据库列。

## 从可信身份解析租户

应用必须从已验证身份、可信域名映射或明确受信的应用解析器取得租户。框架不会自动信任 `tenant_id` query、请求头或 body 字段。启用身份认证时，让 `AuthLayer` 位于外层，使其先验证凭据并将 `CurrentUser` 放入请求扩展：

```rust,ignore
routes
    .post("/records", list_records)
    .layer(TenantLayer::new(|request| {
        let user = request
            .extensions()
            .get::<CurrentUser>()
            .ok_or(Error::Unauthorized)?;
        tenant_for_user(user.id())
    }).layer())
    .layer(AuthLayer::required(AuthMode::Jwt));
```

Tower layer 按后加包裹先前 layer 的顺序执行；上例先认证，再解析租户。resolver 返回错误时请求失败关闭。resolver 必须自行验证租户成员关系；不能把未经验证的 `x-tenant-id` 原样转换成 `TenantId`。

非 HTTP 入口可用 `TenantContext::scope(tenant_id, future)` 显式建立异步作用域。`TenantContext::require_current()` 在租户未设置时返回拒绝错误；嵌套作用域不能切换到另一个租户。Tokio task-local 不会自动跨 `tokio::spawn`，跨任务边界须显式传递并重新校验 `TenantId`。

## 受保护查询

资源继续声明应用自己的 tenant column 与用户授权规则，再用 `for_tenant` 绑定：

```rust,ignore
let resources = QueryRule::<resource::Entity>::new(|user| {
    Ok(QueryScope::new(resource::Column::OwnerId.eq(user.id())))
})
.for_tenant(resource::Column::TenantId);

let rows = resources
    .select(&user, Condition::all())?
    .all(&connection)
    .await?;
```

`TenantQueryRule` 将用户授权、tenant column 和业务筛选以 AND 组合，并为关联读取、批量更新和删除重复应用相同范围。它不替代 `Policy` 操作权限，也不会改写直接 SeaORM、原生 SQL、未注册关联或自定义 driver。应用必须保护这些逃生入口，并为跨租户管理操作单独实现权限与审计。

F6 后续子簇会将同一可信上下文传播到缓存、持久任务、对象路径和审计。当前 tenant context 不自动隔离这些资源。
