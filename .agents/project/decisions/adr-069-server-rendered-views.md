# ADR-069：服务端视图模板

> 替代范围见 [ADR-070](adr-070-laravel-first-rebuild.md#替代范围)；本文旧目录/命名/默认值不阻止全面重写。代码是否已迁移以当前实现和 todo.md 为准。

> 状态：Partially Superseded（目录与默认体验由 ADR-070 重开，现有实现/验收事实保留）
> 日期：2026-10-27

## 决策

- v1 提供可选 `view` feature，复用已采用的 MiniJinja，不建立第二套模板语言或 PHP Facade。
- `ViewPlugin` 从应用工作目录的 `[view].directory` 装载模板，默认 `views/`；`auto_reload` 默认关闭，可在本地开发环境显式开启。
- 应用通过 `Views::render(name, &context)` 返回 HTML。文件系统模板仅从配置目录读取；拒绝绝对路径、路径遍历与越出根目录的符号链接。HTML 模板自动转义、严格处理缺失变量，并使用渲染 fuel 限额。
- `[server].root_target = "view:index.html"` 可将 `/` 渲染为视图；启动 router 时要求 `view` feature、`ViewPlugin` 和有效首页模板均已存在。不会静默回退到静态文件。
- `public/` 继续承载 CSS、图片等静态资源；`/api/*` JSON API、`/ws/*` WebSocket 与运维路由的协议边界不变。HTML 不使用 JSON response envelope。
- `openrust new` 与 `apps/example` 默认启用 `view`，生成 `views/index.html`，并保留 `public/index.html` 作为独立静态示例/回退目标。
- 邮件模板仍使用独立的 `mail-templates` feature 与编译期资源注册；服务端视图不改变 Mailer 契约。

## 验收

以同一 `Views` API 覆盖具名视图、HTML 转义、路径拒绝、模板错误、热重载与根路由；生成项目实际 `cargo check`、`cargo test`，默认 feature 与 `view` feature 分别验证。模板只信任应用自身维护的文件，不接受用户上传模板；用户可见数据仍须由应用授权后传入 context。
