# ADR-068：API 命名空间与静态站点根路由

- 状态：Accepted
- 日期：2026-10-27
- 决策人：OpenRust maintainer

## 背景

框架 API、WebSocket 与浏览器站点需要稳定且容易识别的 URL 边界。API 路由应使用 JSON 契约；站点静态内容应能在不修改框架源码的情况下由应用目录提供，并允许部署方调整首页行为。

## 决策

- 业务 JSON API 使用 `/api` 根及 `/api/*` 子路径；WebSocket endpoint 使用 `/ws/*`。健康、就绪和 metrics 仍是独立运维端点。
- `[server].public_dir` 指定公开文件目录，相对路径以应用工作目录为基准，默认 `public`。公开文件映射到 URL 根，例如 `public/assets/site.css` 对应 `/assets/site.css`。
- `[server].root_target` 默认 `file:/index.html`，从公开目录提供首页。配置支持 `file:/relative-file`、`forward:/application/path` 和 `redirect:/path-or-http-url`。`forward:` 在应用内重派发请求；`redirect:` 发送临时 307。
- `APP_SERVER__PUBLIC_DIR` 与 `APP_SERVER__ROOT_TARGET` 遵循现有 ConfigLoader 层级，非 production `.env` 和进程环境覆盖 TOML；进程环境优先。配置在 router 构建时校验，路径穿越、无效 URI、非 HTTP(S) 外部 URL 和带用户信息的 URL 均拒绝。
- `/api/*`、`/ws/*` 未匹配的路径继续返回 OpenRust JSON 404，不回落到静态资源；静态 HTML/资源保留对应 MIME 与原始响应体。
- OpenSpring 参考 `openspring-core/openspring-core-view/.../ViewRootRouteResolver.java` 的 `file:` / `forward:` / `redirect:` 目标语义及内部路径/外部 URL 校验。OpenRust 只采用根目标的显式前缀模型，沿用自己的 Figment 配置、Axum/Tower 路由和静态服务。

## 后果

生成项目提供 `public/index.html` 与 TOML 默认值；应用可以用配置切换站点目录和首页行为，不需修改框架路由。默认 API 根为 `GET /api`，静态首页 `/` 不是 API。根路径由 `root_target` 接管；自定义首页 handler 应通过 `forward:` 指向应用路由，或将目标设置为对应静态文件。

## 验收

HTTP 集成测试验证默认首页、可配置站点目录、静态 URL 映射、forward/redirect、API/WebSocket 404 命名空间和无效配置；生成项目测试验证配置样例与默认首页文件。默认 workspace 与 all-features 门禁另按仓库工作循环执行。
