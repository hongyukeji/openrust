# 路由

根应用在 `routes/api.rs` 声明相对 API 路径，启动装配把它们放入 `api` 中间件组并添加前缀。默认前缀为 `/api`，因此相对 `/` 对应 `GET /api`。

```rust
pub(crate) fn register(routes: openrust::http::Routes) -> openrust::http::Routes {
    routes.get("/", crate::app::http::controllers::application_info::index)
}
```

在 `config/api.rs` 修改 `api.prefix`，或用部署变量选择前缀：

```dotenv
API_PREFIX=/v1
```

挂载路径和根应用的 JSON 异常分类从同一配置仓储读取此前缀。使用 `/v1` 后，根信息端点为 `/v1`；不要在 Controller 重复添加前缀。API 范围匹配路径段边界，不能把 `/v10` 当作 `/v1` 的子路径。

Web 路由在 `routes/web.rs` 声明。模块可以通过相同公开路由 API 提供业务路径，但启用模块不是根应用 API 存在的前提。

本章覆盖宿主 API 的配置与挂载；完整 Routing、中间件与请求行为仍在实现，不把此入口视为所有 Laravel 路由能力的等价证明。

对应固定 Laravel 13.x 官方章节：[routing](https://github.com/laravel/docs/blob/960eedf0911155ad16326ade49c0ea5d18eda0b8/routing.md)。本章仅说明已落地的当前入口及边界，尚未覆盖该官方章节全部能力。

前缀可使用 `v1`、`/v1/` 或普通 `{tenant}` 参数段；两端斜线按路由组归一化，空值挂载到根，同时错误范围覆盖应用根路径。末尾 catch-all 无法再挂载子路由，配置时明确拒绝。当前底层匹配器最多支持 25 个普通参数，前缀与子路由合计，catch-all 不计；超限返回安全启动错误。该上限是尚待解除的后端差距，不是 Laravel 限制，也不作为 Rust 语言必然差异关闭。
