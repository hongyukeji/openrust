# `openrust::view`

> 受众：AI Agent
> 更新：2026-10-27
> 摘要：可选服务端 HTML 模板能力。

## 当前实现

- Cargo feature `view` 启用 MiniJinja 文件 loader；默认构建不增加模板加载能力。
- `ViewPlugin` 从 `[view]` 读取 `directory`（默认应用工作目录的 `views/`）和 `auto_reload`（默认关闭），构建 `Views` 服务。
- `Views::render(name, context)` 返回 Axum `Html<String>`，仅接受 `.html` 模板。模板名拒绝绝对路径和遍历；文件 canonical path 必须位于模板根内，阻止符号链接越界。
- `.html` 自动 HTML 转义；strict undefined、每次渲染 100,000 fuel；模板读写/语法/执行错误以内部错误处理，错误来源不作为响应正文返回。
- `server.root_target = "view:index.html"` 会在 router 组装时检查插件与模板，任何缺失均启动配置失败；默认宿主仍使用 `file:/index.html`。
- `openrust new` 和 `examples/example` 默认启用该 feature；生成器提供应用首页与独立静态目录。

## 边界

模板同步运行，不提供异步模板函数、数据库访问、组件系统、用户上传模板或远程 loader。开发期 `auto_reload` 会在每次渲染时清空模板缓存并重读文件，适合本地使用，不应在生产开启。视图不负责授权或业务数据查询，应用必须先授权并显式构造 context。邮件模板继续使用独立 `mail-templates` feature。

公共契约与决策见 [ADR-069](../decisions/adr-069-server-rendered-views.md)，用户说明见 [视图指南](../../../framework/src/view/README.md)。
